use inspectah_core::traits::executor::Executor;
use inspectah_core::traits::inspector::{
    InspectionContext, Inspector, InspectorError, InspectorOutput,
};
use inspectah_core::traits::progress::ProgressSink;
use inspectah_core::types::FindingKind;
use inspectah_core::types::completeness::{InspectorId, SectionData, SourceSystemKind};
use inspectah_core::types::selinux::{
    FcontextFileType, FcontextRule, FcontextSource, SelinuxContext, SelinuxPortLabel,
    SelinuxSection,
};
use inspectah_core::types::warnings::Warning;
use regex::Regex;
use std::path::Path;
use std::sync::LazyLock;

/// Regex for parsing `semanage boolean -l` output lines:
///   name  (current , default)  description
static BOOL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\S+)\s+\((\w+)\s*,\s*(\w+)\)\s+(.*)").unwrap());

/// Regex for parsing `semanage port -l -C` output lines:
///   type  protocol  port[,port...]
static PORT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(\S+)\s+(tcp|udp)\s+([\d,\-\s]+)").unwrap());

/// Splits a `semanage fcontext -l -C` table row into columns. Column gaps
/// are never a single space; path regexes can contain one.
static FCONTEXT_TABLE_SPLIT_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r" {2,}").unwrap());

/// Matches a `semanage fcontext -l -C` equivalence row: `subject = labeled_like`.
static FCONTEXT_EQUIV_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\S+) = (\S+)$").unwrap());

/// Inspects SELinux/security state: mode, custom modules, boolean overrides,
/// fcontext rules, port labels, FIPS mode.
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

    fn inspect(
        &self,
        ctx: &InspectionContext<'_>,
        _progress: &dyn ProgressSink,
    ) -> Result<InspectorOutput, InspectorError> {
        if ctx.rpm_state.is_none() {
            return Err(InspectorError::Failed {
                reason: "RPM prerequisite unavailable".into(),
            });
        }

        let exec = ctx.executor;
        let mut warnings: Vec<Warning> = Vec::new();
        let mut degraded_reasons: Vec<String> = Vec::new();

        let mut section = SelinuxSection::default();

        collect_selinux_mode(exec, &mut section, &mut degraded_reasons);
        let policy_type = read_policy_type(exec);
        let store = active_store(exec, &policy_type);
        collect_custom_modules(exec, &mut section, &mut warnings, &store);
        collect_boolean_overrides(exec, &mut section, &mut warnings, &mut degraded_reasons);
        collect_fcontext_rules(exec, &mut section, &policy_type);
        collect_port_labels(exec, &mut section);
        collect_fips_mode(exec, &mut section);

        let output = InspectorOutput {
            section: SectionData::Selinux(section),
            warnings,
            // The two collectors that emitted hints (audit rules, PAM
            // configs) moved to the config walker; nothing this inspector
            // still reads carries free text.
            redaction_hints: Vec::new(),
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

/// The SELinux configuration file.
const SELINUX_CONFIG: &str = "/etc/selinux/config";

/// Reads SELINUXTYPE from `/etc/selinux/config`, defaulting to "targeted".
fn read_policy_type(exec: &dyn Executor) -> String {
    let content = match exec.read_file(Path::new(SELINUX_CONFIG)) {
        Ok(c) => c,
        Err(_) => return "targeted".to_string(),
    };

    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(val) = trimmed.strip_prefix("SELINUXTYPE=")
            && !val.is_empty()
        {
            return val.to_string();
        }
    }

    "targeted".to_string()
}

// ---------------------------------------------------------------------------
// Policy store
// ---------------------------------------------------------------------------

/// libsemanage's configuration, which names the policy store's root.
const SEMANAGE_CONF: &str = "/etc/selinux/semanage.conf";

/// The policy store root libsemanage uses when `semanage.conf` sets no
/// `store-root`. Package-mode RHEL 9 and 10 hosts keep their modules and
/// local settings here, not under `/etc/selinux` (measured on RHEL 9.8 and
/// 10.2); a bootc image sets `store-root=/etc/selinux` instead.
const DEFAULT_STORE_ROOT: &str = "/var/lib/selinux";

/// The `semanage.conf` key that relocates the policy store.
const STORE_ROOT_KEY: &str = "store-root";

/// The active policy store for `policy_type`:
/// `<store-root>/<policy_type>/active`, with `store-root` read from
/// `semanage.conf` as libsemanage reads it.
fn active_store(exec: &dyn Executor, policy_type: &str) -> String {
    let root = exec
        .read_file(Path::new(SEMANAGE_CONF))
        .ok()
        .and_then(|content| {
            content.lines().find_map(|line| {
                let (key, value) = line.trim().split_once('=')?;
                (key.trim() == STORE_ROOT_KEY && !value.trim().is_empty())
                    .then(|| value.trim().trim_end_matches('/').to_string())
            })
        })
        .unwrap_or_else(|| DEFAULT_STORE_ROOT.to_string());
    format!("{root}/{policy_type}/active")
}

// ---------------------------------------------------------------------------
// Custom modules
// ---------------------------------------------------------------------------

/// The priority the distribution's base policy modules are installed at.
const BASE_POLICY_PRIORITY: u16 = 100;

/// The priority an RPM's `%selinux_modules_install` installs its modules
/// at. The package that ships the module carries it into the image.
const PACKAGED_MODULE_PRIORITY: u16 = 200;

/// Modules selinux-policy generates itself at the default local priority,
/// 400: `/usr/libexec/selinux/varrun-convert.sh` and `binsbin-convert.sh`
/// build them from the installed packages' file contexts, and the policy's
/// RPM macros rebuild them in every transaction, so an image has its own
/// (`rhel-bootc:10.3` carries `extra_varrun` at 400).
const POLICY_GENERATED_MODULES: [&str; 2] = ["extra_varrun", "extra_binsbin"];

/// Whether a `modules/<priority>` directory holds locally installed
/// modules: any numeric priority except the base policy's and the one
/// packages install at. `semodule -i` uses 400; `semodule -X <n> -i` can
/// put a local module at any other. `disabled/` is not a priority.
fn is_local_module_priority(dir: &str) -> bool {
    dir.parse::<u16>()
        .is_ok_and(|p| p != BASE_POLICY_PRIORITY && p != PACKAGED_MODULE_PRIORITY)
}

/// Discovers locally installed SELinux policy modules in the active policy
/// store: every module at a local priority (see
/// [`is_local_module_priority`]) except the ones selinux-policy generates.
/// A module installed at two local priorities is listed once; `semodule
/// -E` extracts the highest.
///
/// A missing store is no finding on a host without SELinux; on a host with
/// an SELinux config it is reported, since the modules could not be read.
fn collect_custom_modules(
    exec: &dyn Executor,
    section: &mut SelinuxSection,
    warnings: &mut Vec<Warning>,
    store: &str,
) {
    let modules_dir = format!("{store}/modules");
    let priorities = match exec.read_dir(Path::new(&modules_dir)) {
        Ok(entries) => entries,
        Err(e) => {
            if e.kind() != std::io::ErrorKind::NotFound
                || exec.file_exists(Path::new(SELINUX_CONFIG))
            {
                warnings.push(selinux_warning(format!(
                    "SELinux custom policy modules not read: {modules_dir}: {e}"
                )));
            }
            return;
        }
    };

    let mut names = std::collections::BTreeSet::new();
    for priority in priorities.iter().filter(|p| is_local_module_priority(p)) {
        let dir = format!("{modules_dir}/{priority}");
        match exec.read_dir(Path::new(&dir)) {
            Ok(entries) => names.extend(
                entries
                    .into_iter()
                    .filter(|name| !POLICY_GENERATED_MODULES.contains(&name.as_str())),
            ),
            Err(e) => warnings.push(selinux_warning(format!(
                "SELinux custom policy modules not read: {dir}: {e}"
            ))),
        }
    }
    section.custom_modules = names.into_iter().collect();
}

/// A warning from this inspector.
fn selinux_warning(message: String) -> Warning {
    Warning {
        inspector: "selinux".into(),
        message,
        severity: None,
        extra: Default::default(),
    }
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

/// Reads custom local fcontext rules, preferring the policy store files and
/// falling back to `semanage fcontext -l -C` via chroot.
///
/// Order is the property this collector has to preserve, not just the data:
/// `file_contexts.local` and `file_contexts.subs` hold rules in insertion
/// order, which is also the order the policy resolves overlapping local
/// expressions by (recency). `semanage fcontext -l -C` prints the same
/// local table sorted by expression instead, which can silently invert
/// which rule wins — so it is read only when neither store file is.
/// `file_contexts.subs_dist` is the distro's own substitutions and is never
/// collected.
fn collect_fcontext_rules(exec: &dyn Executor, section: &mut SelinuxSection, policy_type: &str) {
    let local_path = format!("/etc/selinux/{policy_type}/contexts/files/file_contexts.local");
    let subs_path = format!("/etc/selinux/{policy_type}/contexts/files/file_contexts.subs");

    let local = exec.read_file(Path::new(&local_path)).ok();
    let subs = exec.read_file(Path::new(&subs_path)).ok();

    if local.is_none() && subs.is_none() {
        let host_root = exec.host_root().to_string_lossy().to_string();
        let res = exec.run("chroot", &[&host_root, "semanage", "fcontext", "-l", "-C"]);
        if res.success() {
            section.fcontext_rules = parse_semanage_fcontext_list(&res.stdout);
        }
        section.fcontext_source = FcontextSource::SemanageList;
        return;
    }

    let mut rules = Vec::new();
    if let Some(content) = local {
        rules.extend(content.lines().filter_map(parse_file_contexts_local_line));
    }
    if let Some(content) = subs {
        rules.extend(content.lines().filter_map(parse_file_contexts_subs_line));
    }
    section.fcontext_rules = rules;
    section.fcontext_source = FcontextSource::PolicyStore;
}

/// Parses `user:role:type[:level]`. `level`, when present, may itself
/// contain `:` (an MLS range like `s0-s0:c0`), so it captures everything
/// after the third colon.
fn parse_selinux_context(text: &str) -> Option<SelinuxContext> {
    let mut parts = text.splitn(4, ':');
    let user = parts.next()?.to_string();
    let role = parts.next()?.to_string();
    let type_ = parts.next()?.to_string();
    if user.is_empty() || role.is_empty() || type_.is_empty() {
        return None;
    }
    let level = parts.next().map(|s| s.to_string());
    Some(SelinuxContext {
        user,
        role,
        type_,
        level,
    })
}

/// Maps a `file_contexts.local` flag token to its file type. These are the
/// literal tokens the policy store file uses, distinct from both the CLI
/// `-f` letter ([`FcontextFileType::semanage_flag`]) and the `-l -C` type
/// column text ([`FcontextFileType::parse_column`]).
fn parse_local_flag_token(token: &str) -> Option<FcontextFileType> {
    match token {
        "--" => Some(FcontextFileType::RegularFile),
        "-d" => Some(FcontextFileType::Directory),
        "-c" => Some(FcontextFileType::CharDevice),
        "-b" => Some(FcontextFileType::BlockDevice),
        "-s" => Some(FcontextFileType::Socket),
        "-l" => Some(FcontextFileType::Symlink),
        "-p" => Some(FcontextFileType::NamedPipe),
        _ => None,
    }
}

/// Parses one `file_contexts.local` line: `regex [flag] user:role:type[:level]`,
/// whitespace-separated. `<<none>>` as the context means `Unlabel`; an
/// absent flag token means [`FcontextFileType::All`]. A `#`-prefixed line is
/// a comment and is skipped (`None`); any other line this shape cannot
/// place becomes `Unparsed` rather than being dropped, so the preferred
/// source doesn't lose data the fallback `-l -C` source would have kept.
pub fn parse_file_contexts_local_line(line: &str) -> Option<FcontextRule> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.starts_with('#') {
        return None;
    }
    let tokens: Vec<&str> = trimmed.split_whitespace().collect();
    let (spec, file_type, context_token) = match tokens.as_slice() {
        [spec, context] => (*spec, FcontextFileType::All, *context),
        [spec, flag, context] => match parse_local_flag_token(flag) {
            Some(file_type) => (*spec, file_type, *context),
            None => {
                return Some(FcontextRule::Unparsed {
                    raw: trimmed.to_string(),
                });
            }
        },
        _ => {
            return Some(FcontextRule::Unparsed {
                raw: trimmed.to_string(),
            });
        }
    };

    if context_token.eq_ignore_ascii_case("<<none>>") {
        return Some(FcontextRule::Unlabel {
            spec: spec.to_string(),
            file_type,
            disposition: FindingKind::included(),
            aggregate: None,
        });
    }

    match parse_selinux_context(context_token) {
        Some(context) => Some(FcontextRule::Label {
            spec: spec.to_string(),
            file_type,
            context,
            disposition: FindingKind::included(),
            aggregate: None,
        }),
        None => Some(FcontextRule::Unparsed {
            raw: trimmed.to_string(),
        }),
    }
}

/// Parses one `file_contexts.subs` line: `subject labeled_like`,
/// whitespace-separated, exactly two tokens. A `#`-prefixed line is a
/// comment and is skipped (`None`); any other line this shape cannot place
/// becomes `Unparsed` rather than being dropped (see
/// `parse_file_contexts_local_line`).
pub fn parse_file_contexts_subs_line(line: &str) -> Option<FcontextRule> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.starts_with('#') {
        return None;
    }
    let tokens: Vec<&str> = trimmed.split_whitespace().collect();
    let [subject, labeled_like] = tokens.as_slice() else {
        return Some(FcontextRule::Unparsed {
            raw: trimmed.to_string(),
        });
    };
    let (subject, labeled_like) = (subject.to_string(), labeled_like.to_string());
    Some(FcontextRule::Equivalence {
        subject,
        labeled_like,
        disposition: FindingKind::included(),
        aggregate: None,
    })
}

/// Parses the full `semanage fcontext -l -C` output, skipping header lines
/// (both the table header and the `SELinux Local fcontext Equivalence`
/// section header start with `SELinux`). Every other non-blank line is
/// handed to [`parse_semanage_line`], whose own shape-based matching
/// decides table row, equivalence row, or `Unparsed` regardless of which
/// section it came from.
fn parse_semanage_fcontext_list(text: &str) -> Vec<FcontextRule> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("SELinux"))
        .filter_map(parse_semanage_line)
        .collect()
}

/// Parses one non-header, non-blank line of `semanage fcontext -l -C`
/// output. A row split by a run of two or more spaces into exactly three
/// columns with a recognized type column is `Label` (or `Unlabel` when the
/// context is `<<None>>`, case-insensitively); an `x = y` row is
/// `Equivalence`; anything else becomes `Unparsed` so the renderer can
/// print it for the operator to transcribe.
pub fn parse_semanage_line(line: &str) -> Option<FcontextRule> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    let columns: Vec<&str> = FCONTEXT_TABLE_SPLIT_RE.split(trimmed).collect();
    if let [spec, type_col, context_col] = columns.as_slice()
        && let Some(file_type) = FcontextFileType::parse_column(type_col)
    {
        if context_col.eq_ignore_ascii_case("<<none>>") {
            return Some(FcontextRule::Unlabel {
                spec: spec.to_string(),
                file_type,
                disposition: FindingKind::included(),
                aggregate: None,
            });
        }
        if let Some(context) = parse_selinux_context(context_col) {
            return Some(FcontextRule::Label {
                spec: spec.to_string(),
                file_type,
                context,
                disposition: FindingKind::included(),
                aggregate: None,
            });
        }
    }

    if let Some(caps) = FCONTEXT_EQUIV_RE.captures(trimmed) {
        return Some(FcontextRule::Equivalence {
            subject: caps[1].to_string(),
            labeled_like: caps[2].to_string(),
            disposition: FindingKind::included(),
            aggregate: None,
        });
    }

    Some(FcontextRule::Unparsed {
        raw: trimmed.to_string(),
    })
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
                    disposition: FindingKind::included(),
                    locked: false,
                    aggregate: None,
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
// FIPS mode
// ---------------------------------------------------------------------------

/// Reads `/proc/sys/crypto/fips_enabled`.
fn collect_fips_mode(exec: &dyn Executor, section: &mut SelinuxSection) {
    if let Ok(content) = exec.read_file(Path::new("/proc/sys/crypto/fips_enabled")) {
        section.fips_mode = content.trim() == "1";
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
    use inspectah_core::traits::inspector::RpmState;
    use inspectah_core::traits::progress::NullProgress;
    use inspectah_core::types::os::OsRelease;
    use inspectah_core::types::system::SourceSystem;

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
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress);
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
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress);
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
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress);
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
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress);
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

    // ---- Test 6: custom modules come from the active policy store ----

    /// The inspector's output over `exec`, with the warnings.
    fn inspect_with(exec: &MockExecutor) -> (SelinuxSection, Vec<Warning>) {
        let rpm_state = empty_rpm_state();
        let source = test_source_system();
        let ctx = InspectionContext {
            source_system: &source,
            executor: exec,
            rpm_state: Some(&rpm_state),
            baseline_data: None,
        };
        let output = match SelinuxInspector::new().inspect(&ctx, &NullProgress) {
            Ok(output) => output,
            Err(InspectorError::Degraded { partial, .. }) => *partial,
            Err(e) => panic!("unexpected error: {e}"),
        };
        match output.section {
            SectionData::Selinux(section) => (section, output.warnings),
            other => panic!("expected Selinux section, got {other:?}"),
        }
    }

    /// A host with an SELinux config of `SELINUXTYPE=<policy_type>`, an
    /// optional `semanage.conf`, and the module store `modules` laid out
    /// as `(store_root, priority, module names)`.
    fn module_host(
        policy_type: &str,
        semanage_conf: Option<&str>,
        modules: &[(&str, &str, &[&str])],
    ) -> MockExecutor {
        let mut exec = MockExecutor::new().with_file(
            "/etc/selinux/config",
            &format!("SELINUX=enforcing\nSELINUXTYPE={policy_type}\n"),
        );
        if let Some(conf) = semanage_conf {
            exec = exec.with_file("/etc/selinux/semanage.conf", conf);
        }
        let mut priorities: std::collections::BTreeMap<String, Vec<&str>> = Default::default();
        for (root, priority, names) in modules {
            let modules_dir = format!("{root}/{policy_type}/active/modules");
            priorities
                .entry(modules_dir.clone())
                .or_default()
                .push(priority);
            exec = exec.with_dir(&format!("{modules_dir}/{priority}"), names.to_vec());
        }
        for (dir, entries) in priorities {
            exec = exec.with_dir(&dir, entries);
        }
        exec
    }

    #[test]
    fn custom_modules_are_read_from_the_policy_store() {
        let var = "/var/lib/selinux";
        let etc = "/etc/selinux";
        // (case, policy type, semanage.conf, store layout, expected modules)
        type Case<'a> = (
            &'a str,
            &'a str,
            Option<&'a str>,
            Vec<(&'a str, &'a str, &'a [&'a str])>,
            &'a [&'a str],
        );
        let cases: Vec<Case> = vec![
            (
                "priority 400 in the default store, sorted",
                "targeted",
                None,
                vec![(var, "400", &["webapp", "myapp", "permissive_rhcd_t"])],
                &["myapp", "permissive_rhcd_t", "webapp"],
            ),
            (
                "the base policy (100) and packaged modules (200) are not local",
                "targeted",
                None,
                vec![
                    (var, "100", &["apache", "base"]),
                    (var, "200", &["container"]),
                    (var, "400", &["myapp"]),
                    (var, "disabled", &["zebra"]),
                ],
                &["myapp"],
            ),
            (
                "any other numeric priority is local; a name at two is listed once",
                "targeted",
                None,
                vec![
                    (var, "300", &["myapp", "lowprio"]),
                    (var, "400", &["myapp"]),
                    (var, "500", &["override"]),
                ],
                &["lowprio", "myapp", "override"],
            ),
            (
                "modules selinux-policy generates are not local",
                "targeted",
                None,
                vec![(var, "400", &["extra_varrun", "extra_binsbin", "myapp"])],
                &["myapp"],
            ),
            (
                "the policy type comes from the SELinux config",
                "mls",
                None,
                vec![(var, "400", &["mlsapp"])],
                &["mlsapp"],
            ),
            (
                "semanage.conf relocates the store",
                "targeted",
                Some("module-store = direct\nstore-root=/etc/selinux\n"),
                vec![(etc, "400", &["myapp"]), (var, "400", &["stale"])],
                &["myapp"],
            ),
            (
                "the /etc/selinux policy directory is not the default store",
                "targeted",
                None,
                vec![(etc, "400", &["not_a_store"])],
                &[],
            ),
        ];
        for (case, policy_type, conf, layout, expected) in cases {
            let (section, _) = inspect_with(&module_host(policy_type, conf, &layout));
            assert_eq!(section.custom_modules, expected, "{case}");
        }
    }

    // ---- Test 7: a missing store ----

    #[test]
    fn missing_module_store_warns_only_when_selinux_is_configured() {
        // SELinux configured, no store: the modules could not be read.
        let (section, warnings) = inspect_with(&module_host("targeted", None, &[]));
        assert!(section.custom_modules.is_empty());
        assert!(
            warnings.iter().any(|w| w
                .message
                .contains("modules not read: /var/lib/selinux/targeted/active/modules")),
            "{warnings:?}"
        );

        // No SELinux at all: nothing to report.
        let (section, warnings) = inspect_with(&MockExecutor::new());
        assert!(section.custom_modules.is_empty());
        assert!(
            !warnings
                .iter()
                .any(|w| w.message.contains("modules not read")),
            "{warnings:?}"
        );
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
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress);
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
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress);
        let section = extract_section(&result);
        assert_eq!(section.fcontext_rules.len(), 3);
        fn spec(rule: &FcontextRule) -> &str {
            match rule {
                FcontextRule::Label { spec, .. } => spec.as_str(),
                other => panic!("expected Label, got {other:?}"),
            }
        }
        assert_eq!(spec(&section.fcontext_rules[0]), "/opt/myapp(/.*)?");
        assert_eq!(spec(&section.fcontext_rules[1]), "/srv/data(/.*)?");
        assert_eq!(spec(&section.fcontext_rules[2]), "/var/lib/mydb(/.*)?");
        assert_eq!(section.fcontext_source, FcontextSource::SemanageList);
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
        assert!(results[0].disposition.is_included());

        assert_eq!(results[1].label_type, "http_port_t");
        assert_eq!(results[1].port, "8080");

        assert_eq!(results[2].label_type, "http_port_t");
        assert_eq!(results[2].port, "8443");

        assert_eq!(results[3].label_type, "redis_port_t");
        assert_eq!(results[3].port, "6380");
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
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress);
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
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress);
        let section = extract_section(&result);
        assert!(!section.fips_mode);
    }

    // ---- Test 18: test_selinux_empty_system ----

    #[test]
    fn test_selinux_empty_system() {
        // No SELinux, no FIPS -> minimal section
        let exec = MockExecutor::new();

        let rpm_state = empty_rpm_state();
        let source = test_source_system();
        let inspector = SelinuxInspector::new();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: Some(&rpm_state),
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress);
        // Should be degraded since getenforce fails and sysfs unavailable,
        // plus semanage unavailable
        let section = extract_section(&result);
        assert!(section.mode.is_empty());
        assert!(section.custom_modules.is_empty());
        assert!(section.boolean_overrides.is_empty());
        assert!(section.fcontext_rules.is_empty());
        assert!(!section.fips_mode);
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
            baseline_data: None,
        };

        let result = inspector.inspect(&ctx, &NullProgress);
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
