use inspectah_core::traits::executor::Executor;
use inspectah_core::traits::inspector::{
    InspectionContext, Inspector, InspectorError, InspectorOutput,
};
use inspectah_core::traits::progress::ProgressSink;
use inspectah_core::types::completeness::{InspectorId, SectionData, SourceSystemKind};
use inspectah_core::types::kernelboot::{
    ConfigSnippet, KernelBootSection, KernelModule, SysctlOverride, is_stock_tuned_profile,
};
use inspectah_core::types::redaction::{Confidence, RedactionHint};
use std::collections::HashMap;
use std::path::Path;

/// Secret-like patterns in kernel cmdline and config snippets.
const CMDLINE_SECRET_PATTERNS: &[&str] = &["password=", "key=", "secret="];

/// Secret-like patterns in config snippet content.
const SNIPPET_SECRET_PATTERNS: &[&str] = &["password", "secret", "key", "credential", "token"];

/// Inspects kernel boot configuration: cmdline, loaded modules, sysctl overrides,
/// locale, timezone, tuned profiles, and config snippets from modprobe.d,
/// modules-load.d, and dracut.conf.d.
pub struct KernelbootInspector;

impl KernelbootInspector {
    pub fn new() -> Self {
        Self
    }
}

impl Default for KernelbootInspector {
    fn default() -> Self {
        Self::new()
    }
}

impl Inspector for KernelbootInspector {
    fn id(&self) -> InspectorId {
        InspectorId::KernelBoot
    }

    fn applicable_to(&self) -> &[SourceSystemKind] {
        &[SourceSystemKind::PackageBased]
    }

    fn inspect(
        &self,
        ctx: &InspectionContext<'_>,
        _progress: &dyn ProgressSink,
    ) -> Result<InspectorOutput, InspectorError> {
        let exec = ctx.executor;
        let mut redaction_hints = Vec::new();
        let mut primary_failure: Option<String> = None;

        // 1. Read /proc/cmdline — PRIMARY source
        let cmdline = match exec.read_file(Path::new("/proc/cmdline")) {
            Ok(c) => {
                let trimmed = c.trim().to_string();
                // Check cmdline for secrets
                for param in trimmed.split_whitespace() {
                    let lower = param.to_lowercase();
                    if CMDLINE_SECRET_PATTERNS.iter().any(|p| lower.contains(p)) {
                        redaction_hints.push(RedactionHint {
                            path: "/proc/cmdline".into(),
                            reason: format!(
                                "kernel cmdline contains sensitive parameter: {}",
                                param.split('=').next().unwrap_or(param)
                            ),
                            confidence: Some(Confidence::High),
                        });
                    }
                }
                trimmed
            }
            Err(e) => {
                primary_failure = Some(format!("/proc/cmdline unreadable: {e}"));
                String::new()
            }
        };

        // 2. Run lsmod — PRIMARY source
        let loaded_modules = match collect_lsmod(exec) {
            Ok(modules) => modules,
            Err(reason) => {
                primary_failure = Some(reason);
                Vec::new()
            }
        };

        // 3. Sysctl overrides — PRIMARY source (sysctl.d files + sysctl -a runtime)
        let sysctl_overrides = match collect_sysctl_overrides(exec) {
            Ok(overrides) => overrides,
            Err(reason) => {
                primary_failure = Some(reason);
                Vec::new()
            }
        };

        // 4. Read /etc/locale.conf — OPTIONAL
        let locale = exec
            .read_file(Path::new("/etc/locale.conf"))
            .ok()
            .and_then(|content| parse_locale(&content));

        // 5. Timezone — OPTIONAL
        let timezone = collect_timezone(exec);

        // 6. Tuned profile — OPTIONAL (tuned not installed is fine)
        let tuned_active = collect_tuned(exec);

        // 7. Config snippets from /etc/modprobe.d/, /etc/modules-load.d/, /etc/dracut.conf.d/
        let (modprobe_d, modprobe_failures) =
            collect_config_snippets(exec, "/etc/modprobe.d", &mut redaction_hints);
        let (modules_load_d, modules_load_failures) =
            collect_config_snippets(exec, "/etc/modules-load.d", &mut redaction_hints);

        // dracut.conf.d — failure is a degraded condition
        let dracut_result =
            collect_config_snippets_strict(exec, "/etc/dracut.conf.d", &mut redaction_hints);

        // Collect read failures from correctness-bearing config directories.
        // Individual file read failures (e.g., size cap) mean the snapshot
        // looks complete when collection was actually incomplete.
        let mut snippet_failures: Vec<String> = Vec::new();
        snippet_failures.extend(modprobe_failures);
        snippet_failures.extend(modules_load_failures);

        // Build section with what we have
        let section = KernelBootSection {
            cmdline,
            grub_defaults: String::new(),
            sysctl_overrides,
            modules_load_d,
            modprobe_d,
            dracut_conf: dracut_result
                .as_ref()
                .map_or_else(|_| Vec::new(), |v| v.clone()),
            loaded_modules,
            non_default_modules: Vec::new(),
            tuned_include: !tuned_active.is_empty()
                && !is_stock_tuned_profile(&tuned_active),
            tuned_active,
            tuned_custom_profiles: Vec::new(),
            locale,
            timezone,
            alternatives: Vec::new(),
        };

        // Check for primary failure → Degraded
        if let Some(reason) = primary_failure {
            return Err(InspectorError::Degraded {
                partial: Box::new(InspectorOutput {
                    section: SectionData::KernelBoot(section),
                    warnings: Vec::new(),
                    redaction_hints,
                }),
                reason,
            });
        }

        // Check for dracut failure → Degraded (materially reduces section correctness)
        if let Err(reason) = dracut_result {
            return Err(InspectorError::Degraded {
                partial: Box::new(InspectorOutput {
                    section: SectionData::KernelBoot(section),
                    warnings: Vec::new(),
                    redaction_hints,
                }),
                reason,
            });
        }

        // Config snippet read failures → Degraded (missing data would
        // make emitted artifacts look complete when collection was incomplete)
        if !snippet_failures.is_empty() {
            let reason = format!(
                "config snippet read failures (possible size cap): {}",
                snippet_failures.join("; ")
            );
            return Err(InspectorError::Degraded {
                partial: Box::new(InspectorOutput {
                    section: SectionData::KernelBoot(section),
                    warnings: Vec::new(),
                    redaction_hints,
                }),
                reason,
            });
        }

        Ok(InspectorOutput {
            section: SectionData::KernelBoot(section),
            warnings: Vec::new(),
            redaction_hints,
        })
    }
}

/// Parse `lsmod` output into KernelModule list.
fn collect_lsmod(exec: &dyn Executor) -> Result<Vec<KernelModule>, String> {
    let result = exec.run("lsmod", &[]);

    if !result.success() {
        return Err(format!("lsmod failed with exit code {}", result.exit_code));
    }

    Ok(parse_lsmod(&result.stdout))
}

/// Parse lsmod output lines. Skips the header line.
///
/// Format: `Module  Size  Used by`
/// The columns are variable-width and whitespace-padded, so we split
/// on whitespace runs, not individual characters.
fn parse_lsmod(stdout: &str) -> Vec<KernelModule> {
    let mut modules = Vec::new();
    for line in stdout.lines().skip(1) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 3 {
            // parts[0]=name, parts[1]=size, parts[2]=use_count, parts[3..]=dependents
            let used_by = if parts.len() > 3 {
                // e.g., ["1", "bridge"] or ["2", "stp,bridge"]
                parts[2..].join(" ")
            } else {
                parts[2].to_string()
            };

            modules.push(KernelModule {
                name: parts[0].to_string(),
                size: parts[1].to_string(),
                used_by,
                include: true,
                fleet: None,
            });
        }
    }
    modules
}

/// Collect sysctl overrides by comparing file-defined values with runtime values.
///
/// Three-way diff:
/// - Read sysctl config files from /etc/sysctl.d/ and /usr/lib/sysctl.d/
/// - Run `sysctl -a` for runtime values
/// - Where file-defined != runtime → record as override
fn collect_sysctl_overrides(exec: &dyn Executor) -> Result<Vec<SysctlOverride>, String> {
    // Read config files
    let (file_values, sysctl_read_failures) = read_sysctl_files(exec);

    // Sysctl config file read failures are correctness-bearing — missing
    // overrides make the snapshot look like defaults when they're not.
    if !sysctl_read_failures.is_empty() {
        return Err(format!(
            "sysctl config read failures: {}",
            sysctl_read_failures.join("; ")
        ));
    }

    if file_values.is_empty() {
        return Ok(Vec::new());
    }

    // Get runtime values
    let runtime_result = exec.run("sysctl", &["-a"]);
    if !runtime_result.success() {
        return Err(format!(
            "sysctl -a failed with exit code {}",
            runtime_result.exit_code
        ));
    }

    let runtime_values = parse_sysctl_output(&runtime_result.stdout);

    // Three-way diff: only report where file value != runtime value
    let mut overrides = Vec::new();
    for (key, (file_val, source)) in &file_values {
        if let Some(runtime_val) = runtime_values.get(key.as_str())
            && file_val != runtime_val
        {
            overrides.push(SysctlOverride {
                key: key.clone(),
                runtime: runtime_val.to_string(),
                default: file_val.clone(),
                source: source.clone(),
                include: true,
                fleet: None,
            });
        }
    }

    // Sort for deterministic output
    overrides.sort_by(|a, b| a.key.cmp(&b.key));

    Ok(overrides)
}

/// Read sysctl config files from standard directories.
/// Returns (map of key → (file_value, source_path), read_failures).
/// Read failures on individual sysctl config files are correctness-bearing.
fn read_sysctl_files(exec: &dyn Executor) -> (HashMap<String, (String, String)>, Vec<String>) {
    let mut values = HashMap::new();
    let mut read_failures = Vec::new();

    for dir_path in &["/etc/sysctl.d", "/usr/lib/sysctl.d"] {
        let dir = Path::new(dir_path);
        if let Ok(entries) = exec.read_dir(dir) {
            let mut sorted_entries = entries;
            sorted_entries.sort();

            for entry in &sorted_entries {
                if !entry.ends_with(".conf") {
                    continue;
                }
                let file_path = dir.join(entry);
                match exec.read_file(&file_path) {
                    Ok(content) => {
                        let source = file_path.to_string_lossy().to_string();
                        for (key, val) in parse_sysctl_conf(&content) {
                            values.insert(key, (val, source.clone()));
                        }
                    }
                    Err(e) => {
                        read_failures.push(format!("{}: {e}", file_path.to_string_lossy()));
                    }
                }
            }
        }
    }

    (values, read_failures)
}

/// Parse a sysctl.d config file into key-value pairs.
fn parse_sysctl_conf(content: &str) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some((key, val)) = line.split_once('=') {
            pairs.push((key.trim().to_string(), val.trim().to_string()));
        }
    }
    pairs
}

/// Parse `sysctl -a` output into key → value map.
fn parse_sysctl_output(stdout: &str) -> HashMap<&str, &str> {
    let mut map = HashMap::new();
    for line in stdout.lines() {
        let line = line.trim();
        if let Some((key, val)) = line.split_once('=') {
            map.insert(key.trim(), val.trim());
        }
    }
    map
}

/// Parse locale from /etc/locale.conf content.
fn parse_locale(content: &str) -> Option<String> {
    for line in content.lines() {
        let line = line.trim();
        if let Some(val) = line.strip_prefix("LANG=") {
            return Some(val.to_string());
        }
    }
    None
}

/// Collect timezone via timedatectl.
fn collect_timezone(exec: &dyn Executor) -> Option<String> {
    let result = exec.run("timedatectl", &["show", "--property=Timezone", "--value"]);
    if result.success() {
        let tz = result.stdout.trim().to_string();
        if !tz.is_empty() {
            return Some(tz);
        }
    }
    None
}

/// Collect tuned profile. Returns empty string if tuned is not installed.
fn collect_tuned(exec: &dyn Executor) -> String {
    let result = exec.run("tuned-adm", &["active"]);
    if !result.success() {
        return String::new();
    }

    // Parse "Current active profile: virtual-guest"
    for line in result.stdout.lines() {
        if let Some(profile) = line.strip_prefix("Current active profile:") {
            return profile.trim().to_string();
        }
    }

    String::new()
}

/// Collect config snippets from a directory. Tolerates unreadable dirs.
/// Returns (snippets, read_failures) — the caller decides whether failures
/// are correctness-bearing.
fn collect_config_snippets(
    exec: &dyn Executor,
    dir_path: &str,
    hints: &mut Vec<RedactionHint>,
) -> (Vec<ConfigSnippet>, Vec<String>) {
    let dir = Path::new(dir_path);
    let entries = match exec.read_dir(dir) {
        Ok(e) => e,
        Err(_) => return (Vec::new(), Vec::new()),
    };

    let mut snippets = Vec::new();
    let mut read_failures = Vec::new();
    for entry in &entries {
        let file_path = dir.join(entry);
        match exec.read_file(&file_path) {
            Ok(content) => {
                let path_str = file_path.to_string_lossy().to_string();

                // Check for secret-like content
                check_snippet_secrets(&path_str, &content, hints);

                snippets.push(ConfigSnippet {
                    path: path_str,
                    content,
                });
            }
            Err(e) => {
                read_failures.push(format!("{}: {e}", file_path.to_string_lossy()));
            }
        }
    }
    (snippets, read_failures)
}

/// Strict variant: returns Err if the directory is unreadable.
/// Individual file read failures within the directory also return Err
/// because dracut config is correctness-bearing — a missing file makes
/// the snapshot look complete when collection was actually incomplete.
fn collect_config_snippets_strict(
    exec: &dyn Executor,
    dir_path: &str,
    hints: &mut Vec<RedactionHint>,
) -> Result<Vec<ConfigSnippet>, String> {
    let dir = Path::new(dir_path);
    let entries = exec
        .read_dir(dir)
        .map_err(|_| format!("{dir_path} unreadable — dracut config missing"))?;

    let mut snippets = Vec::new();
    let mut read_failures = Vec::new();
    for entry in &entries {
        let file_path = dir.join(entry);
        match exec.read_file(&file_path) {
            Ok(content) => {
                let path_str = file_path.to_string_lossy().to_string();

                check_snippet_secrets(&path_str, &content, hints);

                snippets.push(ConfigSnippet {
                    path: path_str,
                    content,
                });
            }
            Err(e) => {
                read_failures.push(format!("{}: {e}", file_path.to_string_lossy()));
            }
        }
    }
    if !read_failures.is_empty() {
        return Err(format!(
            "dracut config file read failures: {}",
            read_failures.join("; ")
        ));
    }
    Ok(snippets)
}

/// Check config snippet content for secret-like patterns.
fn check_snippet_secrets(path: &str, content: &str, hints: &mut Vec<RedactionHint>) {
    for line in content.lines() {
        let lower = line.to_lowercase();
        for pattern in SNIPPET_SECRET_PATTERNS {
            if lower.contains(&format!("{pattern}=")) {
                hints.push(RedactionHint {
                    path: path.to_string(),
                    reason: format!("config snippet may contain sensitive value ({pattern}=...)"),
                    confidence: Some(Confidence::Medium),
                });
                return; // One hint per file is enough
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_lsmod_basic() {
        let input = "Module                  Size  Used by\n\
                     bridge                307200  0\n\
                     stp                    16384  1 bridge\n";
        let modules = parse_lsmod(input);
        assert_eq!(modules.len(), 2);
        assert_eq!(modules[0].name, "bridge");
        assert_eq!(modules[0].size, "307200");
        assert_eq!(modules[0].used_by, "0");
        assert_eq!(modules[1].name, "stp");
        assert_eq!(modules[1].used_by, "1 bridge");
    }

    #[test]
    fn test_parse_lsmod_empty() {
        let input = "Module                  Size  Used by\n";
        let modules = parse_lsmod(input);
        assert!(modules.is_empty());
    }

    #[test]
    fn test_parse_sysctl_conf() {
        let input = "# comment\nkernel.sysrq = 1\nnet.ipv4.ip_forward = 1\n";
        let pairs = parse_sysctl_conf(input);
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0], ("kernel.sysrq".to_string(), "1".to_string()));
    }

    #[test]
    fn test_parse_locale() {
        assert_eq!(
            parse_locale("LANG=en_US.UTF-8\n"),
            Some("en_US.UTF-8".to_string())
        );
        assert_eq!(parse_locale("# comment\n"), None);
    }

    // --- Size-cap / read-failure → Degraded tests ---

    use crate::executor::mock::MockExecutor;
    use inspectah_core::traits::executor::ExecResult;
    use inspectah_core::traits::progress::NullProgress;
    use inspectah_core::types::os::OsRelease;
    use inspectah_core::types::system::SourceSystem;

    fn kb_test_os_release() -> OsRelease {
        OsRelease {
            name: "Red Hat Enterprise Linux".into(),
            version_id: "9.4".into(),
            id: "rhel".into(),
            ..Default::default()
        }
    }

    fn kb_base_mock() -> MockExecutor {
        MockExecutor::new()
            .with_file("/proc/cmdline", "root=/dev/sda1 ro quiet\n")
            .with_command(
                "lsmod",
                ExecResult {
                    stdout:
                        "Module                  Size  Used by\nbridge                307200  0\n"
                            .into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_command(
                "sysctl -a",
                ExecResult {
                    stdout: "kernel.sysrq = 0\n".into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_command(
                "timedatectl show --property=Timezone --value",
                ExecResult {
                    stdout: "America/New_York\n".into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_command(
                "tuned-adm active",
                ExecResult {
                    exit_code: 127,
                    ..Default::default()
                },
            )
            .with_dir("/etc/dracut.conf.d", vec![])
    }

    #[test]
    fn test_sysctl_config_read_failure_triggers_degraded() {
        // Directory is readable but individual file fails to read (simulates size cap)
        let exec = kb_base_mock()
            .with_dir("/etc/sysctl.d", vec!["99-custom.conf"])
            // NOTE: no .with_file for the .conf → read_file will return NotFound
            .with_dir("/usr/lib/sysctl.d", vec![])
            .with_dir("/etc/modprobe.d", vec![])
            .with_dir("/etc/modules-load.d", vec![]);

        let source = SourceSystem::PackageBased {
            os_release: kb_test_os_release(),
        };
        let inspector = KernelbootInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress);
        match result {
            Err(InspectorError::Degraded { reason, .. }) => {
                assert!(
                    reason.contains("sysctl") || reason.contains("99-custom.conf"),
                    "degraded reason must mention the failed sysctl config: {reason}"
                );
            }
            other => panic!("expected Degraded for unreadable sysctl config, got {other:?}"),
        }
    }

    #[test]
    fn test_modprobe_snippet_read_failure_triggers_degraded() {
        // modprobe.d directory readable but a file in it fails to read
        let exec = kb_base_mock()
            .with_dir("/etc/sysctl.d", vec![])
            .with_dir("/usr/lib/sysctl.d", vec![])
            .with_dir("/etc/modprobe.d", vec!["blacklist-custom.conf"])
            // no .with_file → read fails
            .with_dir("/etc/modules-load.d", vec![]);

        let source = SourceSystem::PackageBased {
            os_release: kb_test_os_release(),
        };
        let inspector = KernelbootInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress);
        match result {
            Err(InspectorError::Degraded { reason, .. }) => {
                assert!(
                    reason.contains("blacklist-custom.conf"),
                    "degraded reason must mention the failed snippet: {reason}"
                );
            }
            other => panic!("expected Degraded for unreadable modprobe snippet, got {other:?}"),
        }
    }

    #[test]
    fn test_dracut_file_read_failure_triggers_degraded() {
        // dracut.conf.d directory is readable, lists a file, but file read fails
        let exec = kb_base_mock()
            .with_dir("/etc/sysctl.d", vec![])
            .with_dir("/usr/lib/sysctl.d", vec![])
            .with_dir("/etc/modprobe.d", vec![])
            .with_dir("/etc/modules-load.d", vec![])
            .with_dir("/etc/dracut.conf.d", vec!["50-custom.conf"]);
        // NOTE: no .with_file for 50-custom.conf → read_file returns NotFound

        let source = SourceSystem::PackageBased {
            os_release: kb_test_os_release(),
        };
        let inspector = KernelbootInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress);
        match result {
            Err(InspectorError::Degraded { reason, .. }) => {
                assert!(
                    reason.contains("dracut") || reason.contains("50-custom.conf"),
                    "degraded reason must mention the failed dracut config: {reason}"
                );
            }
            other => panic!("expected Degraded for unreadable dracut config file, got {other:?}"),
        }
    }

    #[test]
    fn test_kernelboot_ok_when_all_readable() {
        // All files present and readable — should succeed
        let exec = kb_base_mock()
            .with_dir("/etc/sysctl.d", vec!["99-custom.conf"])
            .with_file("/etc/sysctl.d/99-custom.conf", "kernel.sysrq = 1\n")
            .with_dir("/usr/lib/sysctl.d", vec![])
            .with_dir("/etc/modprobe.d", vec!["blacklist.conf"])
            .with_file("/etc/modprobe.d/blacklist.conf", "blacklist nouveau\n")
            .with_dir("/etc/modules-load.d", vec![]);

        let source = SourceSystem::PackageBased {
            os_release: kb_test_os_release(),
        };
        let inspector = KernelbootInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress);
        assert!(
            result.is_ok(),
            "all files readable → must succeed, got: {result:?}"
        );
    }
}
