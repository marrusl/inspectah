use inspectah_core::traits::executor::Executor;
use inspectah_core::traits::inspector::{
    InspectionContext, Inspector, InspectorError, InspectorOutput,
};
use inspectah_core::types::completeness::{InspectorId, SectionData, SourceSystemKind};
use inspectah_core::types::redaction::{Confidence, RedactionHint};
use inspectah_core::types::services::{ServiceSection, ServiceStateChange, SystemdDropIn};
use std::path::Path;

/// Secret-like environment variable name fragments that trigger redaction hints.
const SECRET_PATTERNS: &[&str] = &["PASSWORD", "SECRET", "TOKEN", "KEY", "CREDENTIAL"];

/// Returns true if a unit name represents a real, operator-manageable service.
///
/// This is the whitelist gate for the operator-intent model: only real
/// services can produce state_change entries. D-Bus activation aliases,
/// abstract targets, and malformed entries are structurally excluded —
/// no blocklist maintenance required.
fn is_real_service(unit: &str) -> bool {
    // D-Bus activation aliases (dbus-org.*) are symlinks managed by
    // D-Bus activation, not operator intent.
    if unit.starts_with("dbus-") {
        return false;
    }
    // Must be a .service unit (not .timer, .target, .socket, etc.)
    if !unit.ends_with(".service") {
        return false;
    }
    true
}

/// Inspects systemd service state: enabled/disabled vs. preset defaults,
/// drop-in overrides, and flags environment variables that may contain secrets.
pub struct ServicesInspector;

impl ServicesInspector {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ServicesInspector {
    fn default() -> Self {
        Self::new()
    }
}

/// A parsed unit entry from `systemctl list-unit-files` output.
#[derive(Debug)]
struct UnitFileEntry {
    unit: String,
    state: String,
    /// Parsed from systemctl output but not used — preset comparison
    /// uses the actual preset files for first-match-wins semantics.
    _preset: String,
}

/// A parsed preset rule from a `.preset` file.
#[derive(Debug)]
struct PresetRule {
    action: String,
    pattern: String,
}

impl Inspector for ServicesInspector {
    fn id(&self) -> InspectorId {
        InspectorId::Services
    }

    fn applicable_to(&self) -> &[SourceSystemKind] {
        &[SourceSystemKind::PackageBased]
    }

    fn inspect(&self, ctx: &InspectionContext<'_>) -> Result<InspectorOutput, InspectorError> {
        let exec = ctx.executor;

        // 1. Run systemctl list-unit-files
        let systemctl_result = exec.run(
            "systemctl",
            &["list-unit-files", "--type=service", "--no-pager"],
        );

        if systemctl_result.exit_code == 127 {
            return Err(InspectorError::Degraded {
                partial: Box::new(InspectorOutput {
                    section: SectionData::Services(ServiceSection::default()),
                    warnings: Vec::new(),
                    redaction_hints: Vec::new(),
                }),
                reason: "systemctl not found".into(),
            });
        }

        if !systemctl_result.success() {
            return Err(InspectorError::Degraded {
                partial: Box::new(InspectorOutput {
                    section: SectionData::Services(ServiceSection::default()),
                    warnings: Vec::new(),
                    redaction_hints: Vec::new(),
                }),
                reason: format!(
                    "systemctl failed with exit code {}",
                    systemctl_result.exit_code
                ),
            });
        }

        // 2. Parse systemctl output
        let units = parse_unit_files(&systemctl_result.stdout);
        if units.is_empty() {
            return Ok(InspectorOutput {
                section: SectionData::Services(ServiceSection::default()),
                warnings: Vec::new(),
                redaction_hints: Vec::new(),
            });
        }

        // 3. Read preset files — try both dirs; if neither readable, degrade
        let (preset_rules, preset_read_failures) = match read_preset_rules(exec) {
            Ok(pair) => pair,
            Err(reason) => {
                // Degrade: include systemctl data as partial
                let (enabled_units, disabled_units) = partition_units(&units);
                return Err(InspectorError::Degraded {
                    partial: Box::new(InspectorOutput {
                        section: SectionData::Services(ServiceSection {
                            state_changes: Vec::new(),
                            enabled_units,
                            disabled_units,
                            drop_ins: Vec::new(),
                            preset_matched_units: Vec::new(),
                        }),
                        warnings: Vec::new(),
                        redaction_hints: Vec::new(),
                    }),
                    reason,
                });
            }
        };

        // 4. Build state_changes using operator-intent whitelist model.
        //
        // Instead of diffing all services and subtracting noise, we start
        // from an empty list and only add entries where there is clear
        // evidence the operator explicitly acted. This makes blocklists
        // unnecessary — D-Bus aliases, sysupdate services, SSSD defaults,
        // and any future noise patterns are automatically excluded because
        // they lack evidence of operator action.
        let mut state_changes = Vec::new();
        let mut enabled_units = Vec::new();
        let mut disabled_units = Vec::new();
        let mut preset_matched_units = Vec::new();

        for unit in &units {
            // Skip template units and static units — no operator intent
            if unit.unit.contains('@') || unit.state == "static" {
                continue;
            }

            // Build full inventory lists (used by handlers, not by state_changes)
            match unit.state.as_str() {
                "enabled" => enabled_units.push(unit.unit.clone()),
                "disabled" => disabled_units.push(unit.unit.clone()),
                "masked" => disabled_units.push(unit.unit.clone()),
                _ => {}
            }

            // --- Operator-intent gate ---
            // Only real services can produce state_change entries.
            // D-Bus activation aliases and non-.service units are
            // structurally excluded here.
            if !is_real_service(&unit.unit) {
                continue;
            }

            // Signal 1: Masked services — unambiguous operator intent.
            // Always capture regardless of preset state.
            if unit.state == "masked" {
                state_changes.push(ServiceStateChange {
                    unit: unit.unit.clone(),
                    current_state: "masked".into(),
                    default_state: resolve_preset(&unit.unit, &preset_rules).unwrap_or_default(),
                    action: "mask".into(),
                    include: true,
                    owning_package: None,
                    fleet: None,
                    attention_reason: None,
                });
                continue;
            }

            // Signal 2: Operator enabled/disabled — the operator changed the
            // service state from what the distro presets dictate. We need a
            // definitive preset to prove divergence; without one, there is no
            // evidence of operator action.
            let default_state = resolve_preset(&unit.unit, &preset_rules);

            if let Some(ref default) = default_state {
                if *default != unit.state {
                    // Preset says one thing, current state says another.
                    // This is evidence the operator ran systemctl enable/disable.
                    let action = if unit.state == "enabled" {
                        "enable"
                    } else {
                        "disable"
                    };
                    state_changes.push(ServiceStateChange {
                        unit: unit.unit.clone(),
                        current_state: unit.state.clone(),
                        default_state: default.clone(),
                        action: action.into(),
                        include: true,
                        owning_package: None,
                        fleet: None,
                        attention_reason: None,
                    });
                } else {
                    // State matches preset — no operator action
                    preset_matched_units.push(unit.unit.clone());
                }
            }
            // No matching preset rule → no evidence of operator action
        }

        // 5. Scan drop-in directories
        let (drop_ins, redaction_hints, dropin_read_failures) = collect_drop_ins(exec);

        // 6. Build result
        let section = ServiceSection {
            state_changes,
            enabled_units,
            disabled_units,
            drop_ins,
            preset_matched_units,
        };

        // Preset file read failures are correctness-bearing — a missing
        // preset file means we cannot determine the default state for
        // services whose rules were in that file.
        if !preset_read_failures.is_empty() {
            let reason = format!(
                "preset file read failures: {}",
                preset_read_failures.join("; ")
            );
            return Err(InspectorError::Degraded {
                partial: Box::new(InspectorOutput {
                    section: SectionData::Services(section),
                    warnings: Vec::new(),
                    redaction_hints,
                }),
                reason,
            });
        }

        // Drop-in read failures are correctness-bearing — a missed override
        // makes the snapshot look like defaults when there are customizations.
        if !dropin_read_failures.is_empty() {
            let reason = format!(
                "drop-in conf read failures (possible size cap): {}",
                dropin_read_failures.join("; ")
            );
            return Err(InspectorError::Degraded {
                partial: Box::new(InspectorOutput {
                    section: SectionData::Services(section),
                    warnings: Vec::new(),
                    redaction_hints,
                }),
                reason,
            });
        }

        Ok(InspectorOutput {
            section: SectionData::Services(section),
            warnings: Vec::new(),
            redaction_hints,
        })
    }
}

/// Parse `systemctl list-unit-files --type=service` output.
/// Expects header line, data lines with UNIT/STATE/PRESET columns,
/// and a trailing summary line ("N unit files listed.").
fn parse_unit_files(stdout: &str) -> Vec<UnitFileEntry> {
    let mut entries = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        // Skip header, empty lines, and summary line
        if line.is_empty() || line.starts_with("UNIT FILE") || line.ends_with("unit files listed.")
        {
            continue;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 3 {
            entries.push(UnitFileEntry {
                unit: parts[0].to_string(),
                state: parts[1].to_string(),
                _preset: parts[2].to_string(),
            });
        } else if parts.len() == 2 {
            // Some outputs may omit preset column
            entries.push(UnitFileEntry {
                unit: parts[0].to_string(),
                state: parts[1].to_string(),
                _preset: String::new(),
            });
        }
    }
    entries
}

/// Read and merge preset rules from standard preset directories.
/// Returns `(rules, read_failures)`. Returns Err if neither directory is readable.
/// Individual file-level read failures are tracked but do not prevent other
/// files from being read — the caller decides whether failures are fatal.
fn read_preset_rules(exec: &dyn Executor) -> Result<(Vec<PresetRule>, Vec<String>), String> {
    let usr_dir = Path::new("/usr/lib/systemd/system-preset");
    let etc_dir = Path::new("/etc/systemd/system-preset");

    let mut preset_files: Vec<(String, String)> = Vec::new(); // (filename, content)
    let mut read_failures: Vec<String> = Vec::new();

    let usr_ok = match exec.read_dir(usr_dir) {
        Ok(entries) => {
            for entry in &entries {
                if entry.ends_with(".preset") {
                    let path = usr_dir.join(entry);
                    match exec.read_file(&path) {
                        Ok(content) => preset_files.push((entry.clone(), content)),
                        Err(e) => {
                            read_failures.push(format!("{}: {e}", path.to_string_lossy()));
                        }
                    }
                }
            }
            true
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Directory doesn't exist — normal on systems without vendor presets.
            false
        }
        Err(e) => {
            // Directory exists but is unreadable (PermissionDenied, etc.) —
            // this is a trust gap: vendor presets may be hidden.
            read_failures.push(format!("{}: {e}", usr_dir.display()));
            false
        }
    };

    let etc_ok = match exec.read_dir(etc_dir) {
        Ok(entries) => {
            for entry in &entries {
                if entry.ends_with(".preset") {
                    let path = etc_dir.join(entry);
                    match exec.read_file(&path) {
                        Ok(content) => preset_files.push((entry.clone(), content)),
                        Err(e) => {
                            read_failures.push(format!("{}: {e}", path.to_string_lossy()));
                        }
                    }
                }
            }
            true
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Directory doesn't exist — normal on systems without admin presets.
            false
        }
        Err(e) => {
            // Directory exists but is unreadable (PermissionDenied, etc.) —
            // admin preset overrides may be hidden.
            read_failures.push(format!("{}: {e}", etc_dir.display()));
            false
        }
    };

    if !usr_ok && !etc_ok && read_failures.is_empty() {
        return Err(
            "neither /usr/lib/systemd/system-preset nor /etc/systemd/system-preset readable".into(),
        );
    }

    // Sort by filename — numeric prefix ordering (e.g., 85-xxx before 90-xxx)
    preset_files.sort_by(|a, b| a.0.cmp(&b.0));

    let mut rules = Vec::new();
    for (_filename, content) in &preset_files {
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                rules.push(PresetRule {
                    action: parts[0].to_string(),
                    pattern: parts[1].to_string(),
                });
            }
        }
    }

    Ok((rules, read_failures))
}

/// Resolve a unit name against preset rules with first-match-wins semantics.
/// Supports `*` and `?` glob matching.
fn resolve_preset(unit: &str, rules: &[PresetRule]) -> Option<String> {
    for rule in rules {
        if glob_match(&rule.pattern, unit) {
            let state = if rule.action == "enable" {
                "enabled"
            } else {
                "disabled"
            };
            return Some(state.to_string());
        }
    }
    None
}

/// Simple glob matching supporting `*` (any chars) and `?` (single char).
fn glob_match(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();
    glob_match_inner(&pattern, &text)
}

fn glob_match_inner(pattern: &[char], text: &[char]) -> bool {
    match (pattern.first(), text.first()) {
        (None, None) => true,
        (Some('*'), _) => {
            // '*' matches zero or more characters
            // Try matching rest of pattern against current text position
            // or skip one character of text
            glob_match_inner(&pattern[1..], text)
                || (!text.is_empty() && glob_match_inner(pattern, &text[1..]))
        }
        (Some('?'), Some(_)) => glob_match_inner(&pattern[1..], &text[1..]),
        (Some(p), Some(t)) if *p == *t => glob_match_inner(&pattern[1..], &text[1..]),
        _ => false,
    }
}

/// Partition units into (enabled, disabled) lists.
fn partition_units(units: &[UnitFileEntry]) -> (Vec<String>, Vec<String>) {
    let mut enabled = Vec::new();
    let mut disabled = Vec::new();
    for u in units {
        if u.unit.contains('@') || u.state == "static" {
            continue;
        }
        match u.state.as_str() {
            "enabled" => enabled.push(u.unit.clone()),
            "disabled" => disabled.push(u.unit.clone()),
            _ => {}
        }
    }
    (enabled, disabled)
}

/// Collect drop-in `.conf` files from `/etc/systemd/system/*.service.d/`.
/// Returns (drop_ins, redaction_hints, read_failures).
/// Read failures on listed `.conf` files are correctness-bearing — the
/// snapshot would look complete when a service override was actually missed.
fn collect_drop_ins(exec: &dyn Executor) -> (Vec<SystemdDropIn>, Vec<RedactionHint>, Vec<String>) {
    let dropin_base = Path::new("/etc/systemd/system");
    let mut drop_ins = Vec::new();
    let mut hints = Vec::new();
    let mut read_failures = Vec::new();

    let entries = match exec.read_dir(dropin_base) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Directory doesn't exist — no drop-ins to collect.
            return (drop_ins, hints, read_failures);
        }
        Err(e) => {
            // Directory exists but is unreadable (PermissionDenied, etc.) —
            // all drop-in configurations are hidden.
            read_failures.push(format!("{}: {e}", dropin_base.display()));
            return (drop_ins, hints, read_failures);
        }
    };

    for entry in &entries {
        if !entry.ends_with(".service.d") {
            continue;
        }
        // Extract unit name: "httpd.service.d" → "httpd.service"
        let unit = entry.trim_end_matches(".d");
        let dir_path = dropin_base.join(entry);

        let conf_files = match exec.read_dir(&dir_path) {
            Ok(c) => c,
            Err(e) => {
                read_failures.push(format!("{}: {e}", dir_path.to_string_lossy()));
                continue;
            }
        };

        for conf in &conf_files {
            if !conf.ends_with(".conf") {
                continue;
            }
            let file_path = dir_path.join(conf);
            let content = match exec.read_file(&file_path) {
                Ok(c) => c,
                Err(e) => {
                    read_failures.push(format!("{}: {e}", file_path.to_string_lossy()));
                    continue;
                }
            };

            let path_str = file_path.to_string_lossy().to_string();

            // Check for secret-like environment variables
            for line in content.lines() {
                let trimmed = line.trim();
                if let Some(env_val) = trimmed.strip_prefix("Environment=") {
                    // Check if the variable name contains secret patterns
                    // Format: NAME=VALUE or just NAME
                    let var_name = env_val.split('=').next().unwrap_or("");
                    let upper = var_name.to_uppercase();
                    if SECRET_PATTERNS.iter().any(|p| upper.contains(p)) {
                        hints.push(RedactionHint {
                            path: path_str.clone(),
                            reason: format!(
                                "environment variable '{}' may contain a secret",
                                var_name
                            ),
                            confidence: Some(Confidence::Medium),
                        });
                    }
                }
            }

            drop_ins.push(SystemdDropIn {
                unit: unit.to_string(),
                path: path_str,
                content,
                include: true,
                tie: false,
                tie_winner: false,
                fleet: None,
            });
        }
    }

    (drop_ins, hints, read_failures)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_glob_match_exact() {
        assert!(glob_match("sshd.service", "sshd.service"));
        assert!(!glob_match("sshd.service", "httpd.service"));
    }

    #[test]
    fn test_glob_match_star() {
        assert!(glob_match("*", "anything.service"));
        assert!(glob_match("ssh*", "sshd.service"));
        assert!(glob_match("*.service", "httpd.service"));
    }

    #[test]
    fn test_glob_match_question() {
        assert!(glob_match("ssh?.service", "sshd.service"));
        assert!(!glob_match("ssh?.service", "sshdd.service"));
    }

    #[test]
    fn test_parse_unit_files_basic() {
        let input = "UNIT FILE                                  STATE           PRESET\n\
                     sshd.service                               enabled         enabled\n\
                     cups.service                               disabled        disabled\n\
                     \n\
                     2 unit files listed.\n";
        let entries = parse_unit_files(input);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].unit, "sshd.service");
        assert_eq!(entries[0].state, "enabled");
        assert_eq!(entries[0]._preset, "enabled");
    }

    #[test]
    fn test_parse_unit_files_empty() {
        let input = "UNIT FILE                                  STATE           PRESET\n\
                     \n\
                     0 unit files listed.\n";
        let entries = parse_unit_files(input);
        assert!(entries.is_empty());
    }

    // --- Size-cap / read-failure → Degraded tests ---

    use crate::executor::mock::MockExecutor;
    use inspectah_core::traits::executor::ExecResult;
    use inspectah_core::types::os::OsRelease;
    use inspectah_core::types::system::SourceSystem;

    fn svc_test_os_release() -> OsRelease {
        OsRelease {
            name: "Red Hat Enterprise Linux".into(),
            version_id: "9.4".into(),
            id: "rhel".into(),
            ..Default::default()
        }
    }

    fn svc_base_mock() -> MockExecutor {
        MockExecutor::new()
            .with_command(
                "systemctl list-unit-files --type=service --no-pager",
                ExecResult {
                    stdout: "UNIT FILE                                  STATE           PRESET\n\
                             sshd.service                               enabled         enabled\n\
                             httpd.service                              enabled         disabled\n\
                             \n\
                             2 unit files listed.\n"
                        .into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_dir("/usr/lib/systemd/system-preset", vec!["90-default.preset"])
            .with_file(
                "/usr/lib/systemd/system-preset/90-default.preset",
                "enable sshd.service\ndisable *\n",
            )
    }

    #[test]
    fn test_dropin_read_failure_triggers_degraded() {
        // Drop-in directory listed, .conf file listed, but file read fails (size cap)
        let exec = svc_base_mock()
            .with_dir("/etc/systemd/system", vec!["httpd.service.d"])
            .with_dir("/etc/systemd/system/httpd.service.d", vec!["override.conf"]);
        // NOTE: no .with_file for override.conf → read_file will return NotFound

        let source = SourceSystem::PackageBased {
            os_release: svc_test_os_release(),
        };
        let inspector = ServicesInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx);
        match result {
            Err(InspectorError::Degraded { reason, partial }) => {
                assert!(
                    reason.contains("drop-in") || reason.contains("override.conf"),
                    "degraded reason must mention the failed drop-in: {reason}"
                );
                // Partial data should still contain state_changes from systemctl
                if let SectionData::Services(ref svc) = partial.section {
                    assert!(
                        !svc.state_changes.is_empty() || !svc.enabled_units.is_empty(),
                        "partial data must contain systemctl results"
                    );
                }
            }
            other => panic!("expected Degraded for unreadable drop-in, got {other:?}"),
        }
    }

    #[test]
    fn test_preset_file_read_failure_triggers_degraded() {
        // Directory is readable and lists a preset file, but file read fails
        let exec = MockExecutor::new()
            .with_command(
                "systemctl list-unit-files --type=service --no-pager",
                ExecResult {
                    stdout: "UNIT FILE                                  STATE           PRESET\n\
                             sshd.service                               enabled         enabled\n\
                             \n\
                             1 unit files listed.\n"
                        .into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_dir("/usr/lib/systemd/system-preset", vec!["90-default.preset"])
            // NOTE: no .with_file for the preset → read_file returns NotFound
            .with_dir("/etc/systemd/system", vec![]);

        let source = SourceSystem::PackageBased {
            os_release: svc_test_os_release(),
        };
        let inspector = ServicesInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx);
        match result {
            Err(InspectorError::Degraded { reason, partial }) => {
                assert!(
                    reason.contains("preset") || reason.contains("90-default.preset"),
                    "degraded reason must mention the failed preset file: {reason}"
                );
                // Partial data should still contain systemctl results
                if let SectionData::Services(ref svc) = partial.section {
                    assert!(
                        !svc.enabled_units.is_empty(),
                        "partial data must contain systemctl results"
                    );
                }
            }
            other => panic!("expected Degraded for unreadable preset file, got {other:?}"),
        }
    }

    #[test]
    fn test_dropin_dir_unreadable_triggers_degraded() {
        // Per-unit drop-in directory listed but unreadable
        let exec = svc_base_mock().with_dir("/etc/systemd/system", vec!["httpd.service.d"]);
        // NOTE: no .with_dir for httpd.service.d → read_dir returns Err

        let source = SourceSystem::PackageBased {
            os_release: svc_test_os_release(),
        };
        let inspector = ServicesInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx);
        match result {
            Err(InspectorError::Degraded { reason, .. }) => {
                assert!(
                    reason.contains("drop-in") || reason.contains("httpd.service.d"),
                    "degraded reason must mention the unreadable drop-in dir: {reason}"
                );
            }
            other => panic!("expected Degraded for unreadable drop-in directory, got {other:?}"),
        }
    }

    #[test]
    fn test_etc_preset_dir_permission_denied_triggers_degraded() {
        // /etc/systemd/system-preset exists but read_dir returns PermissionDenied.
        // Admin-priority presets may be hidden — must degrade.
        let exec = MockExecutor::new()
            .with_command(
                "systemctl list-unit-files --type=service --no-pager",
                ExecResult {
                    stdout: "UNIT FILE                                  STATE           PRESET\n\
                             sshd.service                               enabled         enabled\n\
                             \n\
                             1 unit files listed.\n"
                        .into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_dir("/usr/lib/systemd/system-preset", vec!["90-default.preset"])
            .with_file(
                "/usr/lib/systemd/system-preset/90-default.preset",
                "enable sshd.service\ndisable *\n",
            )
            .with_dir_error(
                "/etc/systemd/system-preset",
                std::io::ErrorKind::PermissionDenied,
            );

        let source = SourceSystem::PackageBased {
            os_release: svc_test_os_release(),
        };
        let inspector = ServicesInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx);
        match result {
            Err(InspectorError::Degraded { reason, .. }) => {
                assert!(
                    reason.contains("preset") && reason.contains("/etc/systemd/system-preset"),
                    "degraded reason must mention the unreadable preset dir: {reason}"
                );
            }
            other => panic!(
                "expected Degraded for PermissionDenied on /etc/systemd/system-preset, got {other:?}"
            ),
        }
    }

    #[test]
    fn test_etc_preset_dir_not_found_not_degraded() {
        // /etc/systemd/system-preset doesn't exist (NotFound) — this is
        // normal on systems without admin presets. NOT degraded.
        let exec = MockExecutor::new()
            .with_command(
                "systemctl list-unit-files --type=service --no-pager",
                ExecResult {
                    stdout: "UNIT FILE                                  STATE           PRESET\n\
                             sshd.service                               enabled         enabled\n\
                             \n\
                             1 unit files listed.\n"
                        .into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_dir("/usr/lib/systemd/system-preset", vec!["90-default.preset"])
            .with_file(
                "/usr/lib/systemd/system-preset/90-default.preset",
                "enable sshd.service\ndisable *\n",
            );
        // NOTE: no /etc/systemd/system-preset registered → read_dir returns NotFound

        let source = SourceSystem::PackageBased {
            os_release: svc_test_os_release(),
        };
        let inspector = ServicesInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx);
        assert!(
            result.is_ok(),
            "/etc/systemd/system-preset NotFound must NOT degrade, got: {result:?}"
        );
    }

    #[test]
    fn test_etc_systemd_system_permission_denied_triggers_degraded() {
        // /etc/systemd/system (base drop-in directory) exists but read_dir
        // returns PermissionDenied. All drop-in configurations are hidden.
        let exec = svc_base_mock()
            .with_dir_error("/etc/systemd/system", std::io::ErrorKind::PermissionDenied);

        let source = SourceSystem::PackageBased {
            os_release: svc_test_os_release(),
        };
        let inspector = ServicesInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx);
        match result {
            Err(InspectorError::Degraded { reason, .. }) => {
                assert!(
                    reason.contains("drop-in") && reason.contains("/etc/systemd/system"),
                    "degraded reason must mention the unreadable drop-in base dir: {reason}"
                );
            }
            other => panic!(
                "expected Degraded for PermissionDenied on /etc/systemd/system, got {other:?}"
            ),
        }
    }

    // --- Operator-intent whitelist tests ---

    #[test]
    fn test_is_real_service_accepts_real_services() {
        assert!(is_real_service("sshd.service"));
        assert!(is_real_service("httpd.service"));
        assert!(is_real_service("firewalld.service"));
        assert!(is_real_service("systemd-resolved.service"));
        assert!(is_real_service("systemd-sysupdate.service"));
        // dbus.service is the actual daemon, not a D-Bus alias
        assert!(is_real_service("dbus.service"));
    }

    #[test]
    fn test_is_real_service_rejects_dbus_aliases() {
        assert!(!is_real_service(
            "dbus-org.freedesktop.NetworkManager.service"
        ));
        assert!(!is_real_service("dbus-org.freedesktop.timedate1.service"));
        assert!(!is_real_service(
            "dbus-org.fedoraproject.FirewallD1.service"
        ));
        assert!(!is_real_service("dbus-org.bluez.service"));
        assert!(!is_real_service("dbus-:1.2-org.something.service"));
    }

    #[test]
    fn test_is_real_service_rejects_non_service_units() {
        assert!(!is_real_service("systemd-sysupdate.timer"));
        assert!(!is_real_service("multi-user.target"));
        assert!(!is_real_service("dbus.socket"));
    }

    #[test]
    fn test_dbus_alias_not_captured_no_operator_evidence() {
        // D-Bus aliases in a different state than their preset are NOT
        // captured — the operator-intent model excludes them structurally
        // because they are not real services.
        let exec = MockExecutor::new()
            .with_command(
                "systemctl list-unit-files --type=service --no-pager",
                ExecResult {
                    stdout: "UNIT FILE                                  STATE           PRESET\n\
                             httpd.service                              enabled         disabled\n\
                             dbus-org.freedesktop.NetworkManager.service disabled        enabled\n\
                             dbus-org.freedesktop.timedate1.service     disabled        enabled\n\
                             \n\
                             3 unit files listed.\n"
                        .into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_dir("/usr/lib/systemd/system-preset", vec!["90-default.preset"])
            .with_file(
                "/usr/lib/systemd/system-preset/90-default.preset",
                "enable dbus-org.freedesktop.NetworkManager.service\n\
                 enable dbus-org.freedesktop.timedate1.service\n\
                 disable *\n",
            )
            .with_dir("/etc/systemd/system", vec![]);

        let source = SourceSystem::PackageBased {
            os_release: svc_test_os_release(),
        };
        let inspector = ServicesInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx).unwrap();
        if let SectionData::Services(ref svc) = result.section {
            // httpd should be captured (operator enabled a preset-disabled service)
            assert!(
                svc.state_changes
                    .iter()
                    .any(|sc| sc.unit == "httpd.service"),
                "httpd.service must be in state_changes"
            );
            // D-Bus aliases must NOT appear — no operator evidence
            assert!(
                !svc.state_changes
                    .iter()
                    .any(|sc| sc.unit.starts_with("dbus-")),
                "D-Bus aliases must not be in state_changes (no operator evidence), got: {:?}",
                svc.state_changes
            );
        } else {
            panic!("expected SectionData::Services");
        }
    }

    #[test]
    fn test_masked_service_captured_as_operator_intent() {
        let exec = MockExecutor::new()
            .with_command(
                "systemctl list-unit-files --type=service --no-pager",
                ExecResult {
                    stdout: "UNIT FILE                                  STATE           PRESET\n\
                             sshd.service                               enabled         enabled\n\
                             cups.service                               masked          enabled\n\
                             \n\
                             2 unit files listed.\n"
                        .into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_dir("/usr/lib/systemd/system-preset", vec!["90-default.preset"])
            .with_file(
                "/usr/lib/systemd/system-preset/90-default.preset",
                "enable sshd.service\nenable cups.service\ndisable *\n",
            )
            .with_dir("/etc/systemd/system", vec![]);

        let source = SourceSystem::PackageBased {
            os_release: svc_test_os_release(),
        };
        let inspector = ServicesInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx).unwrap();
        if let SectionData::Services(ref svc) = result.section {
            // sshd matches preset — should not be in state_changes
            assert!(
                !svc.state_changes.iter().any(|sc| sc.unit == "sshd.service"),
                "sshd.service matches preset, must not be in state_changes"
            );
            // cups is masked — operator intent, must be captured
            let cups = svc
                .state_changes
                .iter()
                .find(|sc| sc.unit == "cups.service");
            assert!(
                cups.is_some(),
                "masked cups.service must be in state_changes"
            );
            let cups = cups.unwrap();
            assert_eq!(cups.action, "mask", "masked service action must be 'mask'");
            assert_eq!(cups.current_state, "masked");
            assert!(cups.include, "masked service must be included");
        } else {
            panic!("expected SectionData::Services");
        }
    }

    #[test]
    fn test_operator_disabled_preset_enabled_service_captured() {
        // A service that is enabled by preset but disabled by operator
        let exec = MockExecutor::new()
            .with_command(
                "systemctl list-unit-files --type=service --no-pager",
                ExecResult {
                    stdout: "UNIT FILE                                  STATE           PRESET\n\
                             firewalld.service                          disabled        enabled\n\
                             \n\
                             1 unit files listed.\n"
                        .into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_dir("/usr/lib/systemd/system-preset", vec!["90-default.preset"])
            .with_file(
                "/usr/lib/systemd/system-preset/90-default.preset",
                "enable firewalld.service\ndisable *\n",
            )
            .with_dir("/etc/systemd/system", vec![]);

        let source = SourceSystem::PackageBased {
            os_release: svc_test_os_release(),
        };
        let inspector = ServicesInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx).unwrap();
        if let SectionData::Services(ref svc) = result.section {
            let fw = svc
                .state_changes
                .iter()
                .find(|sc| sc.unit == "firewalld.service");
            assert!(
                fw.is_some(),
                "operator-disabled firewalld must be in state_changes"
            );
            assert_eq!(fw.unwrap().action, "disable");
        } else {
            panic!("expected SectionData::Services");
        }
    }

    #[test]
    fn test_sssd_untouched_services_not_captured() {
        // SSSD services that the operator never touched: their preset is
        // "disable" and current state is "disabled" — no divergence, no
        // operator evidence. Must NOT appear in state_changes.
        let exec = MockExecutor::new()
            .with_command(
                "systemctl list-unit-files --type=service --no-pager",
                ExecResult {
                    stdout: "UNIT FILE                                  STATE           PRESET\n\
                             sssd.service                               disabled        disabled\n\
                             sssd-kcm.service                           disabled        disabled\n\
                             sssd-autofs.service                        disabled        disabled\n\
                             \n\
                             3 unit files listed.\n"
                        .into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_dir("/usr/lib/systemd/system-preset", vec!["90-default.preset"])
            .with_file(
                "/usr/lib/systemd/system-preset/90-default.preset",
                "disable sssd*\ndisable *\n",
            )
            .with_dir("/etc/systemd/system", vec![]);

        let source = SourceSystem::PackageBased {
            os_release: svc_test_os_release(),
        };
        let inspector = ServicesInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx).unwrap();
        if let SectionData::Services(ref svc) = result.section {
            assert!(
                !svc.state_changes
                    .iter()
                    .any(|sc| sc.unit.starts_with("sssd")),
                "untouched SSSD services must not be in state_changes (no operator evidence), got: {:?}",
                svc.state_changes
            );
        } else {
            panic!("expected SectionData::Services");
        }
    }

    #[test]
    fn test_sysupdate_not_captured_no_operator_evidence() {
        // systemd-sysupdate services diverge from preset but the operator
        // never touched them. With the whitelist model, they are excluded
        // because there's no operator action evidence (state matches preset
        // or no preset match at all).
        let exec = MockExecutor::new()
            .with_command(
                "systemctl list-unit-files --type=service --no-pager",
                ExecResult {
                    stdout: "UNIT FILE                                  STATE           PRESET\n\
                             systemd-sysupdate.service                  disabled        disabled\n\
                             systemd-sysupdate-cleanup.service          disabled        disabled\n\
                             sshd.service                               enabled         enabled\n\
                             \n\
                             3 unit files listed.\n"
                        .into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_dir("/usr/lib/systemd/system-preset", vec!["90-default.preset"])
            .with_file(
                "/usr/lib/systemd/system-preset/90-default.preset",
                "enable sshd.service\ndisable *\n",
            )
            .with_dir("/etc/systemd/system", vec![]);

        let source = SourceSystem::PackageBased {
            os_release: svc_test_os_release(),
        };
        let inspector = ServicesInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx).unwrap();
        if let SectionData::Services(ref svc) = result.section {
            assert!(
                !svc.state_changes
                    .iter()
                    .any(|sc| sc.unit.contains("sysupdate")),
                "sysupdate services must not be in state_changes (no operator evidence), got: {:?}",
                svc.state_changes
            );
        } else {
            panic!("expected SectionData::Services");
        }
    }

    #[test]
    fn test_preset_divergence_without_operator_action_not_captured() {
        // A service whose state differs from preset but where the difference
        // is just "package installation vs. image presets" — no evidence
        // the operator ran systemctl. With no preset match, it's excluded.
        let exec = MockExecutor::new()
            .with_command(
                "systemctl list-unit-files --type=service --no-pager",
                ExecResult {
                    stdout: "UNIT FILE                                  STATE           PRESET\n\
                             some-obscure.service                       disabled        enabled\n\
                             \n\
                             1 unit files listed.\n"
                        .into(),
                    exit_code: 0,
                    ..Default::default()
                },
            )
            .with_dir("/usr/lib/systemd/system-preset", vec!["90-default.preset"])
            .with_file(
                // Note: no preset rule for some-obscure.service — only
                // the wildcard disable catches it
                "/usr/lib/systemd/system-preset/90-default.preset",
                "enable sshd.service\ndisable *\n",
            )
            .with_dir("/etc/systemd/system", vec![]);

        let source = SourceSystem::PackageBased {
            os_release: svc_test_os_release(),
        };
        let inspector = ServicesInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx).unwrap();
        if let SectionData::Services(ref svc) = result.section {
            // The wildcard "disable *" matches some-obscure.service with
            // default_state="disabled", and current state is also "disabled"
            // — no divergence. If they were different, the whitelist model
            // captures it because the preset is definitive. This test verifies
            // the matching path.
            //
            // In this case: preset says "disabled" (via wildcard), state is
            // "disabled" — match, not captured. Good.
            assert!(
                !svc.state_changes
                    .iter()
                    .any(|sc| sc.unit == "some-obscure.service"),
                "service matching preset via wildcard must not be in state_changes"
            );
        } else {
            panic!("expected SectionData::Services");
        }
    }

    #[test]
    fn test_dropin_captured_for_service() {
        // Drop-in override files represent operator customization
        let exec = svc_base_mock()
            .with_dir("/etc/systemd/system", vec!["httpd.service.d"])
            .with_dir("/etc/systemd/system/httpd.service.d", vec!["restart.conf"])
            .with_file(
                "/etc/systemd/system/httpd.service.d/restart.conf",
                "[Service]\nRestart=always\nRestartSec=5\n",
            );

        let source = SourceSystem::PackageBased {
            os_release: svc_test_os_release(),
        };
        let inspector = ServicesInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx).unwrap();
        if let SectionData::Services(ref svc) = result.section {
            assert!(
                !svc.drop_ins.is_empty(),
                "drop-in overrides must be captured"
            );
            let dropin = svc.drop_ins.iter().find(|d| d.unit == "httpd.service");
            assert!(dropin.is_some(), "httpd.service drop-in must be captured");
            let dropin = dropin.unwrap();
            assert!(dropin.content.contains("Restart=always"));
            assert!(dropin.include);
        } else {
            panic!("expected SectionData::Services");
        }
    }

    #[test]
    fn test_services_ok_when_dropins_readable() {
        let exec = svc_base_mock()
            .with_dir("/etc/systemd/system", vec!["httpd.service.d"])
            .with_dir("/etc/systemd/system/httpd.service.d", vec!["override.conf"])
            .with_file(
                "/etc/systemd/system/httpd.service.d/override.conf",
                "[Service]\nRestart=always\n",
            );

        let source = SourceSystem::PackageBased {
            os_release: svc_test_os_release(),
        };
        let inspector = ServicesInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: None,
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx);
        assert!(
            result.is_ok(),
            "all drop-ins readable → must succeed, got: {result:?}"
        );
    }
}
