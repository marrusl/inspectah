use inspectah_core::traits::executor::Executor;
use inspectah_core::traits::inspector::{
    InspectionContext, Inspector, InspectorError, InspectorOutput,
};
use inspectah_core::traits::progress::ProgressSink;
use inspectah_core::types::FindingKind;
use inspectah_core::types::completeness::{InspectorId, SectionData, SourceSystemKind};
use inspectah_core::types::kernelboot::{KernelBootSection, KernelModule};
use inspectah_core::types::redaction::{Confidence, RedactionHint};
use inspectah_core::types::sysctl::{
    SysctlDir, SysctlFile, SysctlFileBody, SysctlKey, SysctlLine, SysctlName, SysctlSection,
};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use crate::inspectors::config::MAX_CONFIG_FILE_SIZE;

/// The final target of a link that masks a sysctl file.
const DEV_NULL: &str = "/dev/null";

/// The locale file this inspector records the host's locale from. The
/// config inspector's `%ghost` branch leaves it to this section.
pub(crate) const LOCALE_CONF_PATH: &str = "/etc/locale.conf";

/// Secret-like patterns in the kernel cmdline.
const CMDLINE_SECRET_PATTERNS: &[&str] = &["password=", "key=", "secret="];

/// Inspects kernel boot configuration: cmdline, loaded modules, the sysctl
/// search path and runtime values, locale, timezone, and the active tuned
/// profile.
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
        // Every failed primary source, so one failure cannot hide another.
        let mut primary_failures: Vec<String> = Vec::new();

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
                primary_failures.push(format!("/proc/cmdline unreadable: {e}"));
                String::new()
            }
        };

        // 2. Run lsmod — PRIMARY source
        let loaded_modules = match collect_lsmod(exec) {
            Ok(modules) => modules,
            Err(reason) => {
                primary_failures.push(reason);
                Vec::new()
            }
        };

        // 3. Sysctl search path and runtime values — PRIMARY source. No
        // partial section is stored: one missing file or runtime value
        // would make the explanation claim drift that is not there.
        let sysctl = match collect_sysctl_section(exec) {
            Ok(section) => Some(section),
            Err(reason) => {
                primary_failures.push(reason);
                None
            }
        };

        // 4. Read /etc/locale.conf — OPTIONAL
        let locale = exec
            .read_file(Path::new(LOCALE_CONF_PATH))
            .ok()
            .and_then(|content| parse_locale(&content));

        // 5. Timezone — OPTIONAL
        let timezone = collect_timezone(exec);

        // 6. Tuned profile — OPTIONAL (tuned not installed is fine)
        let tuned_active = collect_tuned(exec);

        // The files under /etc/modprobe.d/, /etc/modules-load.d/,
        // /etc/dracut.conf.d/ and /etc/tuned/ are the config walker's:
        // it reads them, decides them, and reports its own read failures
        // as a degraded /etc walk.

        // Build section with what we have
        let section = KernelBootSection {
            cmdline,
            grub_defaults: String::new(),
            sysctl,
            loaded_modules,
            tuned_disposition: FindingKind::from_bool(!tuned_active.is_empty()),
            tuned_active,
            locale,
            timezone,
            alternatives: Vec::new(),
        };

        // Check for primary failure → Degraded
        if !primary_failures.is_empty() {
            let reason = primary_failures.join("; ");
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
                disposition: FindingKind::inventory(),
                locked: false,
                aggregate: None,
            });
        }
    }
    modules
}

/// Record the sysctl search path as it is on disk and the kernel's value of
/// every key it names (`sysctl.d(5)`). Resolves nothing: the explanation is
/// derived from the section by `inspectah-core`.
///
/// A directory that does not exist is normal. Any other failure to list a
/// directory, partway through included, to read an entry, or to run
/// `sysctl -a` is an error naming the cause.
fn collect_sysctl_section(exec: &dyn Executor) -> Result<SysctlSection, String> {
    let mut files = Vec::new();
    for dir in SysctlDir::ALL {
        // Strict: one entry missing from the listing could be the file
        // that sets, overrides or cancels a key.
        let entries = match exec.read_dir_strict(Path::new(dir.path())) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(format!("sysctl: cannot list {}: {e}", dir.path())),
        };
        for name in entries.into_iter().filter(|name| name.ends_with(".conf")) {
            let path = format!("{}/{name}", dir.path());
            let body = read_sysctl_entry(exec, &path)?;
            files.push(SysctlFile { dir, name, body });
        }
    }
    files.sort_by(|a, b| (&a.name, a.dir).cmp(&(&b.name, b.dir)));

    let named: BTreeSet<SysctlKey> = files
        .iter()
        .flat_map(SysctlFile::lines)
        .filter_map(|line| match line {
            SysctlLine::Assign {
                name: SysctlName::Key(key),
                ..
            }
            | SysctlLine::Exclude {
                name: SysctlName::Key(key),
                ..
            } => Some(key),
            SysctlLine::Assign { .. }
            | SysctlLine::Exclude { .. }
            | SysctlLine::Unparsed { .. } => None,
        })
        .collect();

    let mut runtime = BTreeMap::new();
    if !named.is_empty() {
        let result = exec.run("sysctl", &["-a"]);
        if !result.success() {
            return Err(format!(
                "sysctl: sysctl -a failed with exit code {}",
                result.exit_code
            ));
        }
        // stderr carries per-key read errors (`stable_secret`) on a
        // successful run; a key it names is simply absent from stdout.
        for (name, value) in parse_sysctl_output(&result.stdout) {
            let key = SysctlKey::normalize(name);
            if named.contains(&key) {
                runtime.insert(key, value.to_string());
            }
        }
    }

    Ok(SysctlSection {
        files,
        runtime,
        redacted_runtime: Vec::new(),
    })
}

/// What one `*.conf` entry is, decided by the path it resolves to through
/// every link, the entry's own and any ancestor directory's: `/dev/null`
/// is a mask and is not read; a path under `/etc/` or a sysctl directory
/// is read through `path`, recording that final path when it differs from
/// `path`; any other path is recorded and not read.
///
/// Only `InvalidInput` (`EINVAL`) from `read_link` means the entry itself
/// is not a link. Any other failure to read the link or to resolve the
/// path is an error, so an entry whose final path is not known is never
/// read.
fn read_sysctl_entry(exec: &dyn Executor, path: &str) -> Result<SysctlFileBody, String> {
    let entry = Path::new(path);
    let cannot_read = |e: std::io::Error| format!("sysctl: cannot read {path}: {e}");
    match exec.read_link(entry) {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::InvalidInput => {}
        Err(e) => return Err(cannot_read(e)),
    }
    let target = exec
        .resolve_final_target(entry)
        .map_err(cannot_read)?
        .to_string_lossy()
        .into_owned();
    if target == DEV_NULL {
        return Ok(SysctlFileBody::Mask);
    }
    if !is_sysctl_read_through_target(&target) {
        return Ok(SysctlFileBody::OutsideLink { target });
    }
    let content = match exec.read_file(Path::new(path)) {
        Ok(content) => content,
        // The read gate refused a machine-state file: not read, and it
        // still shadows by name, as an outside link does, so the section
        // stays complete and that file's assignments are unknown.
        Err(e) if let Some(member) = inspectah_core::machine_state::refusal(&e) => {
            return Ok(SysctlFileBody::Withheld { member });
        }
        Err(e) => return Err(cannot_read(e)),
    };
    Ok(SysctlFileBody::Read {
        content: checked_sysctl_content(path, content)?,
        link_target: Some(target).filter(|target| target != path),
        redacted: false,
    })
}

/// Whether an entry's final path is inside the configuration trees the
/// sysctl section reads: `/etc/` or one of the sysctl directories.
fn is_sysctl_read_through_target(target: &str) -> bool {
    target.starts_with("/etc/")
        || SysctlDir::ALL
            .iter()
            .any(|dir| target.starts_with(&format!("{}/", dir.path())))
}

/// An entry's content, held to the config inspector's size cap.
fn checked_sysctl_content(path: &str, content: String) -> Result<String, String> {
    if content.len() > MAX_CONFIG_FILE_SIZE {
        return Err(format!(
            "sysctl: {path} is over {MAX_CONFIG_FILE_SIZE} bytes"
        ));
    }
    Ok(content)
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
    }

    #[test]
    fn test_sysctl_config_read_failure_triggers_degraded() {
        // Directory is readable but individual file fails to read (simulates size cap)
        let exec = kb_base_mock()
            .with_dir("/etc/sysctl.d", vec!["99-custom.conf"])
            // NOTE: no .with_file for the .conf → read_file will return NotFound
            .with_dir("/usr/lib/sysctl.d", vec![]);

        let source = SourceSystem::PackageBased {
            os_release: kb_test_os_release(),
        };
        let inspector = KernelbootInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &inspectah_core::machine_state::ScreenedExecutor::new(&exec),
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
    fn test_kernelboot_ok_when_all_readable() {
        // All files present and readable — should succeed
        let exec = kb_base_mock()
            .with_dir("/etc/sysctl.d", vec!["99-custom.conf"])
            .with_file("/etc/sysctl.d/99-custom.conf", "kernel.sysrq = 1\n")
            .with_dir("/usr/lib/sysctl.d", vec![]);

        let source = SourceSystem::PackageBased {
            os_release: kb_test_os_release(),
        };
        let inspector = KernelbootInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &inspectah_core::machine_state::ScreenedExecutor::new(&exec),
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
