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
                        }),
                        warnings: Vec::new(),
                        redaction_hints: Vec::new(),
                    }),
                    reason,
                });
            }
        };

        // 4. Compare state vs preset — build state_changes
        let mut state_changes = Vec::new();
        let mut enabled_units = Vec::new();
        let mut disabled_units = Vec::new();

        for unit in &units {
            // Skip template units and static units
            if unit.unit.contains('@') || unit.state == "static" {
                continue;
            }

            match unit.state.as_str() {
                "enabled" => enabled_units.push(unit.unit.clone()),
                "disabled" => disabled_units.push(unit.unit.clone()),
                _ => {}
            }

            // Look up preset default
            let default_state = resolve_preset(&unit.unit, &preset_rules);

            // Only record divergences
            if let Some(ref default) = default_state {
                if *default != unit.state {
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
                    });
                }
            }
            // No matching preset rule → no state_change entry (we cannot determine divergence)
        }

        // 5. Scan drop-in directories
        let (drop_ins, redaction_hints, dropin_read_failures) = collect_drop_ins(exec);

        // 6. Build result
        let section = ServiceSection {
            state_changes,
            enabled_units,
            disabled_units,
            drop_ins,
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

    let usr_ok = if let Ok(entries) = exec.read_dir(usr_dir) {
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
    } else {
        false
    };

    let etc_ok = if let Ok(entries) = exec.read_dir(etc_dir) {
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
    } else {
        false
    };

    if !usr_ok && !etc_ok {
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
        Err(_) => return (drop_ins, hints, read_failures),
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
        };

        let result = inspector.inspect(&ctx);
        assert!(
            result.is_ok(),
            "all drop-ins readable → must succeed, got: {result:?}"
        );
    }
}
