use inspectah_core::traits::executor::Executor;
use inspectah_core::traits::inspector::{
    InspectionContext, Inspector, InspectorError, InspectorOutput, RpmState,
};
use inspectah_core::types::completeness::{InspectorId, SectionData, SourceSystemKind};
use inspectah_core::types::redaction::{Confidence, RedactionHint};
use inspectah_core::types::selinux::{CarryForwardFile, SelinuxPortLabel, SelinuxSection};
use inspectah_core::types::warnings::Warning;
use regex::Regex;
use std::path::Path;
use std::sync::LazyLock;

/// Patterns in audit rule or PAM config content that trigger redaction hints.
const SENSITIVE_PATTERNS: &[&str] = &["password", "secret", "token", "key", "credential"];

/// System-generated PAM configs to exclude from custom PAM detection.
/// These are base system files that appear in unowned scans but are not
/// user customizations. Matches Go's `unownedExcludeExact` PAM entries.
const EXCLUDED_PAM_CONFIGS: &[&str] = &[
    "/etc/pam.d/chfn",
    "/etc/pam.d/chsh",
    "/etc/pam.d/login",
    "/etc/pam.d/remote",
    "/etc/pam.d/runuser",
    "/etc/pam.d/runuser-l",
    "/etc/pam.d/su",
    "/etc/pam.d/su-l",
];

/// Regex for parsing `semanage boolean -l` output lines:
///   name  (current , default)  description
static BOOL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\S+)\s+\((\w+)\s*,\s*(\w+)\)\s+(.*)").unwrap());

/// Regex for parsing `semanage port -l -C` output lines:
///   type  protocol  port[,port...]
static PORT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(\S+)\s+(tcp|udp)\s+([\d,\-\s]+)").unwrap());

/// Inspects SELinux/security state: mode, custom modules, boolean overrides,
/// fcontext rules, port labels, audit rules, FIPS mode, PAM configs.
pub struct SelinuxInspector;

impl SelinuxInspector {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SelinuxInspector {
    fn default() -> Self {
        Self::new()
    }
}

impl Inspector for SelinuxInspector {
    fn id(&self) -> InspectorId {
        InspectorId::Selinux
    }

    fn applicable_to(&self) -> &[SourceSystemKind] {
        &[SourceSystemKind::PackageBased]
    }

    fn inspect(&self, ctx: &InspectionContext<'_>) -> Result<InspectorOutput, InspectorError> {
        let rpm_state = match ctx.rpm_state {
            None => {
                return Err(InspectorError::Failed {
                    reason: "RPM prerequisite unavailable".into(),
                });
            }
            Some(state) => state,
        };

        let exec = ctx.executor;
        let mut warnings: Vec<Warning> = Vec::new();
        let mut hints: Vec<RedactionHint> = Vec::new();
        let mut degraded_reasons: Vec<String> = Vec::new();

        let mut section = SelinuxSection::default();

        collect_selinux_mode(exec, &mut section, &mut degraded_reasons);
        let policy_type = read_policy_type(exec);
        collect_custom_modules(exec, &mut section, &policy_type);
        collect_boolean_overrides(exec, &mut section, &mut warnings, &mut degraded_reasons);
        collect_fcontext_rules(exec, &mut section, &policy_type);
        collect_port_labels(exec, &mut section);
        collect_audit_rules(
            exec,
            &mut section,
            rpm_state,
            &mut hints,
            &mut degraded_reasons,
        );
        collect_fips_mode(exec, &mut section);
        collect_pam_configs(
            exec,
            &mut section,
            rpm_state,
            &mut hints,
            &mut degraded_reasons,
        );

        let output = InspectorOutput {
            section: SectionData::Selinux(section),
            warnings,
            redaction_hints: hints,
        };

        if degraded_reasons.is_empty() {
            Ok(output)
        } else {
            Err(InspectorError::Degraded {
                partial: Box::new(output),
                reason: degraded_reasons.join("; "),
            })
        }
    }
}

// ---------------------------------------------------------------------------
// SELinux mode
// ---------------------------------------------------------------------------

/// Reads the SELinux mode via `getenforce` command, falling back to
/// `/sys/fs/selinux/enforce` sysfs file.
fn collect_selinux_mode(
    exec: &dyn Executor,
    section: &mut SelinuxSection,
    degraded_reasons: &mut Vec<String>,
) {
    // Try getenforce command first
    let res = exec.run("getenforce", &[]);
    if res.success() {
        let mode = res.stdout.trim().to_string();
        if !mode.is_empty() {
            section.mode = mode;
            return;
        }
    }

    // Fallback to sysfs
    match exec.read_file(Path::new("/sys/fs/selinux/enforce")) {
        Ok(content) => {
            section.mode = match content.trim() {
                "1" => "Enforcing".to_string(),
                "0" => "Permissive".to_string(),
                _ => content.trim().to_string(),
            };
        }
        Err(_) => {
            // Both methods failed
            degraded_reasons.push(
                "SELinux mode detection unavailable — getenforce failed and sysfs not accessible"
                    .into(),
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Policy type
// ---------------------------------------------------------------------------

/// Reads SELINUXTYPE from `/etc/selinux/config`, defaulting to "targeted".
fn read_policy_type(exec: &dyn Executor) -> String {
    let content = match exec.read_file(Path::new("/etc/selinux/config")) {
        Ok(c) => c,
        Err(_) => return "targeted".to_string(),
    };

    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(val) = trimmed.strip_prefix("SELINUXTYPE=") {
            if !val.is_empty() {
                return val.to_string();
            }
        }
    }

    "targeted".to_string()
}

// ---------------------------------------------------------------------------
// Custom modules (priority-400 store)
// ---------------------------------------------------------------------------

/// Discovers custom SELinux modules from the priority-400 module store.
/// Modules at priority 400 were installed locally via `semodule -i`.
fn collect_custom_modules(exec: &dyn Executor, section: &mut SelinuxSection, policy_type: &str) {
    let store_path = format!("/etc/selinux/{policy_type}/active/modules/400");
    let entries = match exec.read_dir(Path::new(&store_path)) {
        Ok(e) => e,
        Err(_) => return,
    };

    let mut names: Vec<String> = entries.into_iter().collect();
    names.sort();
    section.custom_modules = names;
}

// ---------------------------------------------------------------------------
// Boolean overrides
// ---------------------------------------------------------------------------

/// Parses `semanage boolean -l` output and returns booleans where current
/// state differs from default.
fn parse_semanage_booleans(text: &str) -> Vec<serde_json::Value> {
    let mut results = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("SELinux boolean") {
            continue;
        }
        let Some(caps) = BOOL_RE.captures(trimmed) else {
            continue;
        };
        let name = &caps[1];
        let current = &caps[2];
        let default_val = &caps[3];
        let desc = caps[4].trim();

        if current != default_val {
            results.push(serde_json::json!({
                "name": name,
                "current": current,
                "default": default_val,
                "non_default": true,
                "description": desc,
            }));
        }
    }
    results
}

/// Reads boolean runtime values from `/sys/fs/selinux/booleans/` as a
/// fallback when semanage is unavailable. Each sysfs boolean file contains
/// two integers: current and pending (policy-loaded) values.
fn read_bools_from_fs(exec: &dyn Executor) -> Option<Vec<serde_json::Value>> {
    let bool_dir = "/sys/fs/selinux/booleans";
    let entries = exec.read_dir(Path::new(bool_dir)).ok()?;

    let mut results = Vec::new();
    for entry_name in &entries {
        let file_path = format!("{bool_dir}/{entry_name}");
        let content = match exec.read_file(Path::new(&file_path)) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let parts: Vec<&str> = content.split_whitespace().collect();
        if parts.len() < 2 {
            continue;
        }

        let current = if parts[0] == "1" { "on" } else { "off" };
        let pending = if parts[1] == "1" { "on" } else { "off" };

        if current != pending {
            results.push(serde_json::json!({
                "name": entry_name,
                "current": current,
                "default": pending,
                "non_default": true,
                "description": "",
            }));
        }
    }

    Some(results)
}

/// Tries `semanage boolean -l` via chroot, then falls back to reading
/// `/sys/fs/selinux/booleans/`.
fn collect_boolean_overrides(
    exec: &dyn Executor,
    section: &mut SelinuxSection,
    warnings: &mut Vec<Warning>,
    degraded_reasons: &mut Vec<String>,
) {
    let host_root = exec.host_root().to_string_lossy().to_string();
    let res = exec.run("chroot", &[&host_root, "semanage", "boolean", "-l"]);
    if res.success() && !res.stdout.trim().is_empty() {
        section.boolean_overrides = parse_semanage_booleans(&res.stdout);
        return;
    }

    // Fallback to filesystem
    if let Some(fallback) = read_bools_from_fs(exec) {
        section.boolean_overrides = fallback;
        return;
    }

    // Neither method worked
    if !exec.file_exists(Path::new("/sys/fs/selinux/booleans")) {
        warnings.push(Warning {
            inspector: "selinux".into(),
            message: "SELinux boolean override detection unavailable — semanage failed and /sys/fs/selinux/booleans not accessible".into(),
            severity: None,
            extra: Default::default(),
        });
    }
    degraded_reasons.push("semanage boolean unavailable and sysfs fallback failed".into());
}

// ---------------------------------------------------------------------------
// Custom fcontext rules
// ---------------------------------------------------------------------------

/// Tries `semanage fcontext -l -C` via chroot, then falls back to reading
/// `file_contexts.local` from the policy store.
fn collect_fcontext_rules(exec: &dyn Executor, section: &mut SelinuxSection, policy_type: &str) {
    let host_root = exec.host_root().to_string_lossy().to_string();
    let res = exec.run("chroot", &[&host_root, "semanage", "fcontext", "-l", "-C"]);
    if res.success() && !res.stdout.trim().is_empty() {
        for line in res.stdout.trim().lines() {
            let trimmed = line.trim();
            if !trimmed.is_empty() && !trimmed.starts_with("SELinux") {
                section.fcontext_rules.push(trimmed.to_string());
            }
        }
        if !section.fcontext_rules.is_empty() {
            return;
        }
    }

    // Fallback: read file_contexts.local
    let fc_local = format!("/etc/selinux/{policy_type}/contexts/files/file_contexts.local");
    if let Ok(content) = exec.read_file(Path::new(&fc_local)) {
        for line in content.lines() {
            let trimmed = line.trim();
            if !trimmed.is_empty() && !trimmed.starts_with('#') {
                section.fcontext_rules.push(trimmed.to_string());
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Custom port labels
// ---------------------------------------------------------------------------

/// Parses `semanage port -l -C` output into port labels.
fn parse_semanage_ports(text: &str) -> Vec<SelinuxPortLabel> {
    let mut results = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("SELinux") {
            continue;
        }
        let Some(caps) = PORT_RE.captures(trimmed) else {
            continue;
        };
        let port_type = &caps[1];
        let protocol = caps[2].to_lowercase();
        let ports_raw = caps[3].trim();

        for port in ports_raw.split(',') {
            let port = port.trim();
            if !port.is_empty() {
                results.push(SelinuxPortLabel {
                    protocol: protocol.clone(),
                    port: port.to_string(),
                    label_type: port_type.to_string(),
                    include: true,
                    fleet: None,
                });
            }
        }
    }
    results
}

/// Runs `semanage port -l -C` via chroot.
fn collect_port_labels(exec: &dyn Executor, section: &mut SelinuxSection) {
    let host_root = exec.host_root().to_string_lossy().to_string();
    let res = exec.run("chroot", &[&host_root, "semanage", "port", "-l", "-C"]);
    if res.success() && !res.stdout.trim().is_empty() {
        section.port_labels = parse_semanage_ports(&res.stdout);
    }
}

// ---------------------------------------------------------------------------
// Audit rules
// ---------------------------------------------------------------------------

/// Scans `/etc/audit/rules.d/` for custom audit rule files, skipping files
/// owned by RPM.
fn collect_audit_rules(
    exec: &dyn Executor,
    section: &mut SelinuxSection,
    rpm_state: &RpmState,
    hints: &mut Vec<RedactionHint>,
    degraded_reasons: &mut Vec<String>,
) {
    let audit_dir = "/etc/audit/rules.d";
    let entries = match exec.read_dir(Path::new(audit_dir)) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            degraded_reasons.push("permission denied reading /etc/audit/rules.d".into());
            return;
        }
        Err(_) => return,
    };

    let mut rules: Vec<CarryForwardFile> = Vec::new();
    for entry_name in &entries {
        let abs_path = format!("{audit_dir}/{entry_name}");
        if rpm_state.is_rpm_owned(Path::new(&abs_path)) {
            continue;
        }

        let rel = format!("etc/audit/rules.d/{entry_name}");
        check_file_redaction(exec, &abs_path, hints);
        let content = exec.read_file(Path::new(&abs_path)).unwrap_or_default();
        rules.push(CarryForwardFile { path: rel, content });
    }
    rules.sort_by(|a, b| a.path.cmp(&b.path));
    section.audit_rules = rules;
}

// ---------------------------------------------------------------------------
// FIPS mode
// ---------------------------------------------------------------------------

/// Reads `/proc/sys/crypto/fips_enabled`.
fn collect_fips_mode(exec: &dyn Executor, section: &mut SelinuxSection) {
    if let Ok(content) = exec.read_file(Path::new("/proc/sys/crypto/fips_enabled")) {
        section.fips_mode = content.trim() == "1";
    }
}

// ---------------------------------------------------------------------------
// PAM configs
// ---------------------------------------------------------------------------

/// Scans `/etc/pam.d/` for custom PAM configuration files, skipping files
/// owned by RPM and system-generated exclusions.
fn collect_pam_configs(
    exec: &dyn Executor,
    section: &mut SelinuxSection,
    rpm_state: &RpmState,
    hints: &mut Vec<RedactionHint>,
    degraded_reasons: &mut Vec<String>,
) {
    let pam_dir = "/etc/pam.d";
    let entries = match exec.read_dir(Path::new(pam_dir)) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            degraded_reasons.push("permission denied reading /etc/pam.d".into());
            return;
        }
        Err(_) => return,
    };

    let mut configs: Vec<CarryForwardFile> = Vec::new();
    for entry_name in &entries {
        let abs_path = format!("{pam_dir}/{entry_name}");
        if rpm_state.is_rpm_owned(Path::new(&abs_path)) {
            continue;
        }
        if EXCLUDED_PAM_CONFIGS.contains(&abs_path.as_str()) {
            continue;
        }

        let rel = format!("etc/pam.d/{entry_name}");
        check_file_redaction(exec, &abs_path, hints);
        let content = exec.read_file(Path::new(&abs_path)).unwrap_or_default();
        configs.push(CarryForwardFile { path: rel, content });
    }
    configs.sort_by(|a, b| a.path.cmp(&b.path));
    section.pam_configs = configs;
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Checks file content for sensitive patterns and emits redaction hints.
fn check_file_redaction(exec: &dyn Executor, path: &str, hints: &mut Vec<RedactionHint>) {
    let content = match exec.read_file(Path::new(path)) {
        Ok(c) => c,
        Err(_) => return,
    };

    let lower = content.to_lowercase();
    for pattern in SENSITIVE_PATTERNS {
        if lower.contains(pattern) {
            hints.push(RedactionHint {
                path: path.to_string(),
                reason: format!("file content may contain credentials (matched '{pattern}')"),
                confidence: Some(Confidence::Medium),
            });
            return; // One hint per file
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::mock::MockExecutor;
    use inspectah_core::traits::executor::ExecResult;
    use inspectah_core::types::os::OsRelease;
    use inspectah_core::types::system::SourceSystem;
    use std::collections::HashSet;
    use std::path::PathBuf;

    fn test_source_system() -> SourceSystem {
        SourceSystem::PackageBased {
            os_release: OsRelease {
                name: "Red Hat Enterprise Linux".into(),
                version_id: "9.4".into(),
                id: "rhel".into(),
                ..Default::default()
            },
        }
    }

    fn empty_rpm_state() -> RpmState {
        RpmState::default()
    }

    fn rpm_state_with_owned(paths: Vec<&str>) -> RpmState {
        let mut owned = HashSet::new();
        for p in &paths {
            owned.insert(PathBuf::from(p));
        }
        RpmState {
            owned_paths: owned,
            ..Default::default()
        }
    }

    // ---- Test 1: test_selinux_mode_enforcing ----

    #[test]
    fn test_selinux_mode_enforcing() {
        let exec = MockExecutor::new().with_command(
            "getenforce",
            ExecResult {
                stdout: "Enforcing\n".into(),
                exit_code: 0,
                ..Default::default()
            },
        );

        let rpm_state = empty_rpm_state();
        let source = test_source_system();
        let inspector = SelinuxInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: Some(&rpm_state),
        };

        let result = inspector.inspect(&ctx);
        // May be Ok or Degraded (due to semanage unavailable)
        let section = extract_section(&result);
        assert_eq!(section.mode, "Enforcing");
    }

    // ---- Test 2: test_selinux_mode_permissive ----

    #[test]
    fn test_selinux_mode_permissive() {
        let exec = MockExecutor::new().with_command(
            "getenforce",
            ExecResult {
                stdout: "Permissive\n".into(),
                exit_code: 0,
                ..Default::default()
            },
        );

        let rpm_state = empty_rpm_state();
        let source = test_source_system();
        let inspector = SelinuxInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: Some(&rpm_state),
        };

        let result = inspector.inspect(&ctx);
        let section = extract_section(&result);
        assert_eq!(section.mode, "Permissive");
    }

    // ---- Test 3: test_selinux_mode_disabled ----

    #[test]
    fn test_selinux_mode_disabled() {
        let exec = MockExecutor::new().with_command(
            "getenforce",
            ExecResult {
                stdout: "Disabled\n".into(),
                exit_code: 0,
                ..Default::default()
            },
        );

        let rpm_state = empty_rpm_state();
        let source = test_source_system();
        let inspector = SelinuxInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: Some(&rpm_state),
        };

        let result = inspector.inspect(&ctx);
        let section = extract_section(&result);
        assert_eq!(section.mode, "Disabled");
    }

    // ---- Test 4: test_selinux_mode_fallback_sysfs ----

    #[test]
    fn test_selinux_mode_fallback_sysfs() {
        // getenforce fails (not found), sysfs reports "1" -> Enforcing
        let exec = MockExecutor::new().with_file("/sys/fs/selinux/enforce", "1\n");

        let rpm_state = empty_rpm_state();
        let source = test_source_system();
        let inspector = SelinuxInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: Some(&rpm_state),
        };

        let result = inspector.inspect(&ctx);
        let section = extract_section(&result);
        assert_eq!(section.mode, "Enforcing");
    }

    // ---- Test 5: test_policy_type_targeted ----

    #[test]
    fn test_policy_type_targeted() {
        let config_content = "# This file controls the state of SELinux on the system.\n\
            SELINUX=enforcing\n\
            SELINUXTYPE=targeted\n";

        let exec = MockExecutor::new()
            .with_file("/etc/selinux/config", config_content)
            .with_command(
                "getenforce",
                ExecResult {
                    stdout: "Enforcing\n".into(),
                    exit_code: 0,
                    ..Default::default()
                },
            );

        let policy_type = read_policy_type(&exec);
        assert_eq!(policy_type, "targeted");
    }

    // ---- Test 6: test_custom_modules_found ----

    #[test]
    fn test_custom_modules_found() {
        let exec = MockExecutor::new()
            .with_command(
                "getenforce",
                ExecResult {
                    stdout: "Enforcing\n".into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_file(
                "/etc/selinux/config",
                "SELINUX=enforcing\nSELINUXTYPE=targeted\n",
            )
            .with_dir(
                "/etc/selinux/targeted/active/modules/400",
                vec!["myapp", "custom_policy", "webapp"],
            );

        let rpm_state = empty_rpm_state();
        let source = test_source_system();
        let inspector = SelinuxInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: Some(&rpm_state),
        };

        let result = inspector.inspect(&ctx);
        let section = extract_section(&result);
        assert_eq!(section.custom_modules.len(), 3);
        // Should be sorted
        assert_eq!(section.custom_modules[0], "custom_policy");
        assert_eq!(section.custom_modules[1], "myapp");
        assert_eq!(section.custom_modules[2], "webapp");
    }

    // ---- Test 7: test_custom_modules_empty ----

    #[test]
    fn test_custom_modules_empty() {
        // No modules/400 directory -> empty custom modules list
        let exec = MockExecutor::new()
            .with_command(
                "getenforce",
                ExecResult {
                    stdout: "Enforcing\n".into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_file(
                "/etc/selinux/config",
                "SELINUX=enforcing\nSELINUXTYPE=targeted\n",
            );

        let rpm_state = empty_rpm_state();
        let source = test_source_system();
        let inspector = SelinuxInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: Some(&rpm_state),
        };

        let result = inspector.inspect(&ctx);
        let section = extract_section(&result);
        assert!(section.custom_modules.is_empty());
    }

    // ---- Test 8: test_parse_semanage_booleans ----

    #[test]
    fn test_parse_semanage_booleans() {
        let output = "\
SELinux boolean                State  Default Description\n\
\n\
httpd_can_network_connect      (on   ,   off)  Allow httpd to make outbound network connections\n\
virt_use_nfs                   (on   ,   off)  Allow virt to use nfs\n\
samba_enable_home_dirs          (on   ,   on)  Allow samba to enable home dirs\n\
container_manage_cgroup        (off  ,   on)  Allow container to manage cgroup\n\
httpd_enable_cgi               (on   ,   on)  Allow httpd to enable cgi\n\
ftpd_full_access               (off  ,   off)  Allow ftpd full access\n";

        let results = parse_semanage_booleans(output);
        // Only non-default: httpd_can_network_connect (on!=off),
        // virt_use_nfs (on!=off), container_manage_cgroup (off!=on)
        assert_eq!(results.len(), 3);

        assert_eq!(results[0]["name"], "httpd_can_network_connect");
        assert_eq!(results[0]["current"], "on");
        assert_eq!(results[0]["default"], "off");
        assert_eq!(results[0]["non_default"], true);

        assert_eq!(results[1]["name"], "virt_use_nfs");
        assert_eq!(results[1]["current"], "on");
        assert_eq!(results[1]["default"], "off");

        assert_eq!(results[2]["name"], "container_manage_cgroup");
        assert_eq!(results[2]["current"], "off");
        assert_eq!(results[2]["default"], "on");
    }

    // ---- Test 9: test_boolean_fallback_sysfs ----

    #[test]
    fn test_boolean_fallback_sysfs() {
        // semanage fails (command not found), sysfs booleans available.
        // sysfs boolean files contain "current pending" (e.g., "1 0" means
        // current=on, pending=off -> non-default).
        let exec = MockExecutor::new()
            .with_command(
                "getenforce",
                ExecResult {
                    stdout: "Enforcing\n".into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_dir(
                "/sys/fs/selinux/booleans",
                vec![
                    "httpd_can_network_connect",
                    "container_manage_cgroup",
                    "virt_use_nfs",
                ],
            )
            .with_file("/sys/fs/selinux/booleans/httpd_can_network_connect", "1 0")
            .with_file("/sys/fs/selinux/booleans/container_manage_cgroup", "0 1")
            .with_file("/sys/fs/selinux/booleans/virt_use_nfs", "1 1"); // same -> excluded

        let rpm_state = empty_rpm_state();
        let source = test_source_system();
        let inspector = SelinuxInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: Some(&rpm_state),
        };

        let result = inspector.inspect(&ctx);
        let section = extract_section(&result);
        // httpd_can_network_connect: current=on, default=off -> non-default
        // container_manage_cgroup: current=off, default=on -> non-default
        // virt_use_nfs: current=on, default=on -> excluded
        assert_eq!(section.boolean_overrides.len(), 2);

        let names: Vec<String> = section
            .boolean_overrides
            .iter()
            .map(|b| b["name"].as_str().unwrap_or("").to_string())
            .collect();
        assert!(names.contains(&"httpd_can_network_connect".to_string()));
        assert!(names.contains(&"container_manage_cgroup".to_string()));
    }

    // ---- Test 10: test_fcontext_rules_parsed ----

    #[test]
    fn test_fcontext_rules_parsed() {
        let fcontext_output = "\
SELinux fcontext                                   type               Context\n\
\n\
/opt/myapp(/.*)?                                   all files          system_u:object_r:httpd_sys_content_t:s0\n\
/srv/data(/.*)?                                    all files          system_u:object_r:nfs_t:s0\n\
/var/lib/mydb(/.*)?                                all files          system_u:object_r:mysqld_db_t:s0\n";

        let host_root = "/";
        let exec = MockExecutor::new()
            .with_command(
                "getenforce",
                ExecResult {
                    stdout: "Enforcing\n".into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_file(
                "/etc/selinux/config",
                "SELINUX=enforcing\nSELINUXTYPE=targeted\n",
            )
            .with_command(
                &format!("chroot {host_root} semanage fcontext -l -C"),
                ExecResult {
                    stdout: fcontext_output.into(),
                    exit_code: 0,
                    ..Default::default()
                },
            );

        let rpm_state = empty_rpm_state();
        let source = test_source_system();
        let inspector = SelinuxInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: Some(&rpm_state),
        };

        let result = inspector.inspect(&ctx);
        let section = extract_section(&result);
        assert_eq!(section.fcontext_rules.len(), 3);
        assert!(section.fcontext_rules[0].contains("/opt/myapp"));
        assert!(section.fcontext_rules[1].contains("/srv/data"));
        assert!(section.fcontext_rules[2].contains("/var/lib/mydb"));
    }

    // ---- Test 11: test_parse_semanage_ports ----

    #[test]
    fn test_parse_semanage_ports() {
        let port_output = "\
SELinux port     Type              Proto    Port Number\n\
\n\
ssh_port_t                      tcp      2222\n\
http_port_t                     tcp      8080, 8443\n\
redis_port_t                    tcp      6380\n";

        let results = parse_semanage_ports(port_output);
        // ssh_port_t: 1 entry (2222)
        // http_port_t: 2 entries (8080, 8443)
        // redis_port_t: 1 entry (6380)
        assert_eq!(results.len(), 4);

        assert_eq!(results[0].label_type, "ssh_port_t");
        assert_eq!(results[0].protocol, "tcp");
        assert_eq!(results[0].port, "2222");
        assert!(results[0].include);

        assert_eq!(results[1].label_type, "http_port_t");
        assert_eq!(results[1].port, "8080");

        assert_eq!(results[2].label_type, "http_port_t");
        assert_eq!(results[2].port, "8443");

        assert_eq!(results[3].label_type, "redis_port_t");
        assert_eq!(results[3].port, "6380");
    }

    // ---- Test 12: test_audit_rules_rpm_owned_filtered ----

    #[test]
    fn test_audit_rules_rpm_owned_filtered() {
        let exec = MockExecutor::new()
            .with_command(
                "getenforce",
                ExecResult {
                    stdout: "Enforcing\n".into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_dir(
                "/etc/audit/rules.d",
                vec!["audit.rules", "custom-compliance.rules"],
            )
            .with_file("/etc/audit/rules.d/audit.rules", "-w /var/log -p wa\n")
            .with_file(
                "/etc/audit/rules.d/custom-compliance.rules",
                "-w /etc/passwd -p wa -k identity\n",
            );

        // audit.rules is RPM-owned, custom-compliance.rules is not
        let rpm_state = rpm_state_with_owned(vec!["/etc/audit/rules.d/audit.rules"]);
        let source = test_source_system();
        let inspector = SelinuxInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: Some(&rpm_state),
        };

        let result = inspector.inspect(&ctx);
        let section = extract_section(&result);
        // Only custom-compliance.rules should be included (audit.rules is RPM-owned)
        assert_eq!(section.audit_rules.len(), 1);
        assert!(section.audit_rules[0]
            .path
            .contains("custom-compliance.rules"));
        assert!(section.audit_rules[0]
            .content
            .contains("-w /etc/passwd -p wa -k identity"));
    }

    // ---- Test 13: test_audit_rules_custom_included ----

    #[test]
    fn test_audit_rules_custom_included() {
        let exec = MockExecutor::new()
            .with_command(
                "getenforce",
                ExecResult {
                    stdout: "Enforcing\n".into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_dir(
                "/etc/audit/rules.d",
                vec!["custom-a.rules", "custom-b.rules"],
            )
            .with_file(
                "/etc/audit/rules.d/custom-a.rules",
                "-w /etc/shadow -p wa\n",
            )
            .with_file(
                "/etc/audit/rules.d/custom-b.rules",
                "-a always,exit -F arch=b64\n",
            );

        let rpm_state = empty_rpm_state();
        let source = test_source_system();
        let inspector = SelinuxInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: Some(&rpm_state),
        };

        let result = inspector.inspect(&ctx);
        let section = extract_section(&result);
        assert_eq!(section.audit_rules.len(), 2);
        // Sorted by path
        assert_eq!(
            section.audit_rules[0].path,
            "etc/audit/rules.d/custom-a.rules"
        );
        assert_eq!(
            section.audit_rules[1].path,
            "etc/audit/rules.d/custom-b.rules"
        );
        // Content persisted
        assert!(section.audit_rules[0].content.contains("-w /etc/shadow"));
        assert!(section.audit_rules[1].content.contains("-a always,exit"));
    }

    // ---- Test 14: test_pam_configs_rpm_owned_filtered ----

    #[test]
    fn test_pam_configs_rpm_owned_filtered() {
        let exec = MockExecutor::new()
            .with_command(
                "getenforce",
                ExecResult {
                    stdout: "Enforcing\n".into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_dir("/etc/pam.d", vec!["sshd", "custom-app"])
            .with_file("/etc/pam.d/sshd", "auth required pam_sepermit.so\n")
            .with_file("/etc/pam.d/custom-app", "auth required pam_unix.so\n");

        // sshd is RPM-owned
        let rpm_state = rpm_state_with_owned(vec!["/etc/pam.d/sshd"]);
        let source = test_source_system();
        let inspector = SelinuxInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: Some(&rpm_state),
        };

        let result = inspector.inspect(&ctx);
        let section = extract_section(&result);
        // Only custom-app should be included (sshd is RPM-owned)
        assert_eq!(section.pam_configs.len(), 1);
        assert!(section.pam_configs[0].path.contains("custom-app"));
        assert!(section.pam_configs[0]
            .content
            .contains("auth required pam_unix.so"));
    }

    // ---- Test 15: test_pam_configs_custom_included ----

    #[test]
    fn test_pam_configs_custom_included() {
        let exec = MockExecutor::new()
            .with_command(
                "getenforce",
                ExecResult {
                    stdout: "Enforcing\n".into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_dir("/etc/pam.d", vec!["custom-sshd", "myapp-auth"])
            .with_file("/etc/pam.d/custom-sshd", "auth required pam_unix.so\n")
            .with_file("/etc/pam.d/myapp-auth", "auth required pam_unix.so\n");

        let rpm_state = empty_rpm_state();
        let source = test_source_system();
        let inspector = SelinuxInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: Some(&rpm_state),
        };

        let result = inspector.inspect(&ctx);
        let section = extract_section(&result);
        // Both custom PAM configs included (neither RPM-owned nor excluded)
        assert_eq!(section.pam_configs.len(), 2);
        assert_eq!(section.pam_configs[0].path, "etc/pam.d/custom-sshd");
        assert_eq!(section.pam_configs[1].path, "etc/pam.d/myapp-auth");
        // Content persisted
        assert!(section.pam_configs[0]
            .content
            .contains("auth required pam_unix.so"));
    }

    // ---- Test 16: test_fips_mode_enabled ----

    #[test]
    fn test_fips_mode_enabled() {
        let exec = MockExecutor::new()
            .with_command(
                "getenforce",
                ExecResult {
                    stdout: "Enforcing\n".into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_file("/proc/sys/crypto/fips_enabled", "1\n");

        let rpm_state = empty_rpm_state();
        let source = test_source_system();
        let inspector = SelinuxInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: Some(&rpm_state),
        };

        let result = inspector.inspect(&ctx);
        let section = extract_section(&result);
        assert!(section.fips_mode);
    }

    // ---- Test 17: test_fips_mode_disabled ----

    #[test]
    fn test_fips_mode_disabled() {
        let exec = MockExecutor::new()
            .with_command(
                "getenforce",
                ExecResult {
                    stdout: "Enforcing\n".into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_file("/proc/sys/crypto/fips_enabled", "0\n");

        let rpm_state = empty_rpm_state();
        let source = test_source_system();
        let inspector = SelinuxInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: Some(&rpm_state),
        };

        let result = inspector.inspect(&ctx);
        let section = extract_section(&result);
        assert!(!section.fips_mode);
    }

    // ---- Test 18: test_selinux_empty_system ----

    #[test]
    fn test_selinux_empty_system() {
        // No SELinux, no audit, no PAM, no FIPS -> minimal section
        let exec = MockExecutor::new();

        let rpm_state = empty_rpm_state();
        let source = test_source_system();
        let inspector = SelinuxInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: Some(&rpm_state),
        };

        let result = inspector.inspect(&ctx);
        // Should be degraded since getenforce fails and sysfs unavailable,
        // plus semanage unavailable
        let section = extract_section(&result);
        assert!(section.mode.is_empty());
        assert!(section.custom_modules.is_empty());
        assert!(section.boolean_overrides.is_empty());
        assert!(section.fcontext_rules.is_empty());
        assert!(section.audit_rules.is_empty());
        assert!(!section.fips_mode);
        assert!(section.pam_configs.is_empty());
        assert!(section.port_labels.is_empty());
    }

    // ---- Additional: RPM state None -> Failed ----

    #[test]
    fn test_rpm_state_none_returns_failed() {
        let exec = MockExecutor::new();
        let source = test_source_system();
        let inspector = SelinuxInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
        };

        let result = inspector.inspect(&ctx);
        match result {
            Err(InspectorError::Failed { reason }) => {
                assert!(
                    reason.contains("RPM prerequisite"),
                    "expected RPM prerequisite message, got: {reason}"
                );
            }
            other => panic!("expected Failed error for None rpm_state, got: {other:?}"),
        }
    }

    // ---- Helper: extract section from Ok or Degraded ----

    fn extract_section(result: &Result<InspectorOutput, InspectorError>) -> &SelinuxSection {
        match result {
            Ok(output) => {
                if let SectionData::Selinux(ref section) = output.section {
                    section
                } else {
                    panic!("expected Selinux section in Ok result");
                }
            }
            Err(InspectorError::Degraded { partial, .. }) => {
                if let SectionData::Selinux(ref section) = partial.section {
                    section
                } else {
                    panic!("expected Selinux section in Degraded result");
                }
            }
            Err(e) => panic!("unexpected error: {e}"),
        }
    }
}
