use inspectah_core::traits::executor::Executor;
use inspectah_core::traits::inspector::{
    InspectionContext, Inspector, InspectorError, InspectorOutput,
};
use inspectah_core::traits::progress::ProgressSink;
use inspectah_core::types::FindingKind;
use inspectah_core::types::completeness::{InspectorId, SectionData, SourceSystemKind};
use inspectah_core::types::selinux::{
    DisabledModule, FcontextFileType, FcontextRule, FcontextSource, ModuleOrigin, SelinuxBoolean,
    SelinuxContext, SelinuxPortLabel, SelinuxSection, is_boolean_name,
};
use inspectah_core::types::warnings::Warning;
use regex::Regex;
use std::path::Path;
use std::sync::LazyLock;

/// The SELinux configuration file this inspector reads the policy type
/// from. The config inspector's `%ghost` branch leaves it to this section.
pub(crate) const SELINUX_CONFIG_PATH: &str = "/etc/selinux/config";

/// Regex for parsing `semanage boolean -l -C` output lines:
///   name  (running , persistent)  description
static BOOL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\S+)\s+\((\w+)\s*,\s*(\w+)\)").unwrap());

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
        let store = PolicyStore::active(exec, &policy_type);
        collect_custom_modules(exec, &mut section, &mut warnings, &store);
        collect_boolean_overrides(
            exec,
            &mut section,
            &mut warnings,
            &mut degraded_reasons,
            &store,
        );
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

/// Reads SELINUXTYPE from `/etc/selinux/config`, defaulting to "targeted".
fn read_policy_type(exec: &dyn Executor) -> String {
    let content = match exec.read_file(Path::new(SELINUX_CONFIG_PATH)) {
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

/// The active policy store: `<store-root>/<policy_type>/active`.
///
/// Both parts of its path come from configuration, `store-root` from
/// `semanage.conf` and the policy type from `/etc/selinux/config`, and a
/// value there runs to the end of its line, so the path can hold any text
/// an operator wrote after it, a credential note included. Warnings are
/// not redacted. The path is therefore used to reach the filesystem and
/// never written into a diagnostic: the type has no `Display`, and a
/// diagnostic names the store by [`PolicyStore::LABEL`].
struct PolicyStore {
    path: String,
}

impl PolicyStore {
    /// How a diagnostic names the store, whatever its configured path.
    const LABEL: &'static str =
        "the active policy store that /etc/selinux/semanage.conf and /etc/selinux/config select";

    /// The store for `policy_type`, with `store-root` read from
    /// `semanage.conf` by [`store_root_from_conf`].
    fn active(exec: &dyn Executor, policy_type: &str) -> Self {
        let root = exec
            .read_file(Path::new(SEMANAGE_CONF))
            .ok()
            .and_then(|content| store_root_from_conf(&content))
            .unwrap_or_else(|| DEFAULT_STORE_ROOT.to_string());
        Self {
            path: format!("{}/{policy_type}/active", root.trim_end_matches('/')),
        }
    }

    /// The store itself, for filesystem access only.
    fn root(&self) -> &Path {
        Path::new(&self.path)
    }

    /// `relative` inside the store, for filesystem access only.
    fn join(&self, relative: &str) -> std::path::PathBuf {
        Path::new(&self.path).join(relative)
    }
}

/// The `store-root` that `semanage.conf` sets, read as libsemanage's
/// lexer and parser read it (`libsemanage/src/conf-scan.l` and
/// `conf-parse.y`, SELinux 3.6), or `None` when it sets none.
///
/// - `#` starts a comment only where a keyword could start; inside a value
///   it is literal.
/// - Spaces and tabs around `=` are dropped. The value runs to the end of
///   its line: `<arg>.*[^"\n]` takes it unquoted, with surrounding
///   whitespace trimmed (`my_strdup`); `<arg>\".+\"` takes it quoted, from
///   the first `"` to the last on the line, with the two quotes stripped
///   (`my_qstrdup`). Flex picks the longer match, so a quoted value with
///   anything after its closing quote, a trailing space included, is read
///   unquoted and keeps its quotes.
/// - `""`, nothing after `=`, or an unquoted value whose last character
///   is a `"` is no value (neither argument rule matches it whole).
///   libsemanage refuses such a file outright; here it sets nothing.
/// - Each assignment replaces the one before (`parse_store_root_path`
///   frees the previous path), so the last one wins.
fn store_root_from_conf(text: &str) -> Option<String> {
    let mut root = None;
    for line in text.lines() {
        let line = line.trim_start_matches([' ', '\t']);
        let Some(rest) = line.strip_prefix(STORE_ROOT_KEY) else {
            continue;
        };
        let Some(arg) = rest.trim_start_matches([' ', '\t']).strip_prefix('=') else {
            continue;
        };
        let arg = arg.trim_start_matches([' ', '\t']);
        let value = match arg.strip_prefix('"').and_then(|a| a.strip_suffix('"')) {
            Some(inner) if !inner.is_empty() => inner,
            Some(_) => continue,
            None if arg.ends_with('"') => continue,
            None => arg.trim(),
        };
        if !value.is_empty() {
            root = Some(value.to_string());
        }
    }
    root
}

// ---------------------------------------------------------------------------
// Custom modules
// ---------------------------------------------------------------------------

/// The priority the distribution's base policy modules are installed at.
const BASE_POLICY_PRIORITY: u16 = 100;

/// The priority an RPM's `%selinux_modules_install` installs its modules
/// at. The package that ships the module carries it into the image.
const PACKAGED_MODULE_PRIORITY: u16 = 200;

/// The priority `semodule -i` installs at when given none.
const DEFAULT_LOCAL_PRIORITY: u16 = 400;

/// Modules selinux-policy generates itself at [`DEFAULT_LOCAL_PRIORITY`]:
/// `/usr/libexec/selinux/varrun-convert.sh` and `binsbin-convert.sh`
/// build them from the installed packages' file contexts, and the policy's
/// RPM macros rebuild them in every transaction, so an image has its own
/// (`rhel-bootc:10.3` carries `extra_varrun` at 400). At any other
/// priority the name is the administrator's module.
const POLICY_GENERATED_MODULES: [&str; 2] = ["extra_varrun", "extra_binsbin"];

/// The `modules/` entry where libsemanage records the modules turned off
/// with `semodule -d`: one empty file per module name, whatever its
/// priority.
const DISABLED_DIR: &str = "disabled";

/// The local priority a `modules/<priority>` directory is, or `None` for
/// the base policy's and the one packages install at, and for `disabled/`.
/// `semodule -i` uses 400; `semodule -X <n> -i` can put a local module at
/// any other.
fn local_module_priority(dir: &str) -> Option<u16> {
    dir.parse::<u16>()
        .ok()
        .filter(|&p| p != BASE_POLICY_PRIORITY && p != PACKAGED_MODULE_PRIORITY)
}

/// Whether the module `name` at `priority` is one selinux-policy
/// generates, not the administrator's.
fn is_policy_generated(priority: u16, name: &str) -> bool {
    priority == DEFAULT_LOCAL_PRIORITY && POLICY_GENERATED_MODULES.contains(&name)
}

/// Discovers the locally installed SELinux policy modules in the active
/// policy store and the modules turned off with `semodule -d`.
///
/// A local module is one at a local priority (see
/// [`local_module_priority`]) and not generated by selinux-policy. A
/// module installed at two local priorities is listed once; `semodule -E`
/// extracts the highest. A module named in `modules/disabled/` goes to
/// `disabled_modules` with its origin, and a local one is then not listed
/// in `custom_modules`, since the host builds its policy without it.
///
/// Every directory is read in full or reported: a listing cut short by an
/// entry error would look like a complete one with modules missing. A
/// missing store is no finding on a host without SELinux; on a host with
/// an SELinux config it is reported, since the modules could not be read.
/// A store with no `disabled/` has nothing disabled.
///
/// A warning names a directory by its place in the store and an error by
/// its kind only: an I/O error's text can carry the path (see
/// [`PolicyStore`]).
fn collect_custom_modules(
    exec: &dyn Executor,
    section: &mut SelinuxSection,
    warnings: &mut Vec<Warning>,
    store: &PolicyStore,
) {
    let label = PolicyStore::LABEL;
    let modules_dir = store.join("modules");
    let entries = match exec.read_dir_strict(&modules_dir) {
        Ok(entries) => entries,
        Err(e) => {
            if e.kind() != std::io::ErrorKind::NotFound
                || exec.file_exists(Path::new(SELINUX_CONFIG_PATH))
            {
                warnings.push(selinux_warning(format!(
                    "SELinux custom policy modules not read: modules/ in {label}: {}",
                    e.kind()
                )));
            }
            return;
        }
    };

    let mut local = std::collections::BTreeSet::new();
    for (priority, dir) in entries
        .iter()
        .filter_map(|e| local_module_priority(e).map(|p| (p, e)))
    {
        match exec.read_dir_strict(&modules_dir.join(dir)) {
            Ok(names) => local.extend(
                names
                    .into_iter()
                    .filter(|name| !is_policy_generated(priority, name)),
            ),
            Err(e) => warnings.push(selinux_warning(format!(
                "SELinux custom policy modules not read: the priority {priority} modules in \
                 {label}: {}",
                e.kind()
            ))),
        }
    }

    let disabled = match exec.read_dir_strict(&modules_dir.join(DISABLED_DIR)) {
        Ok(names) => names,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => {
            warnings.push(selinux_warning(format!(
                "SELinux disabled policy modules not read: modules/{DISABLED_DIR}/ in {label}: \
                 {}; the modules listed as custom may include disabled ones",
                e.kind()
            )));
            Vec::new()
        }
    };
    let mut disabled_modules: Vec<DisabledModule> = disabled
        .into_iter()
        .map(|name| {
            let origin = if local.remove(&name) {
                ModuleOrigin::Local
            } else {
                ModuleOrigin::Packaged
            };
            DisabledModule { name, origin }
        })
        .collect();
    disabled_modules.sort();

    section.custom_modules = local.into_iter().collect();
    section.disabled_modules = disabled_modules;
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

/// The active store's record of the booleans set persistently: a comment
/// header, then one `name=0|1` line per boolean (libsemanage's format,
/// checked in a `rhel-bootc:10.3` container).
const BOOLEANS_LOCAL: &str = "booleans.local";

/// Why a `booleans.local` line was not read. A warning names the line by
/// its number and this reason, never by its text: warnings are not
/// redacted, and a line that is not what libsemanage writes can hold
/// anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BooleanLineFault {
    /// No `=`.
    NoAssignment,
    /// The name before `=` is not an SELinux boolean name.
    Name,
    /// The value after `=` is not one libsemanage reads.
    Value,
}

impl BooleanLineFault {
    fn describe(self) -> &'static str {
        match self {
            Self::NoAssignment => "no name=value assignment",
            Self::Name => "the name is not an SELinux boolean name",
            Self::Value => "the value is not 0, 1, true or false",
        }
    }
}

/// Parses one `booleans.local` assignment.
fn parse_boolean_line(line: &str) -> Result<SelinuxBoolean, BooleanLineFault> {
    let (name, value) = line.split_once('=').ok_or(BooleanLineFault::NoAssignment)?;
    let name = name.trim();
    if !is_boolean_name(name) {
        return Err(BooleanLineFault::Name);
    }
    let value = match value.trim() {
        "1" | "true" | "TRUE" => true,
        "0" | "false" | "FALSE" => false,
        _ => return Err(BooleanLineFault::Value),
    };
    Ok(SelinuxBoolean {
        name: name.to_string(),
        value,
    })
}

/// Parses `booleans.local` into the booleans it sets and, by 1-based line
/// number, the lines it could not read. libsemanage writes `name=0|1` and
/// also reads `true`/`false`; comment and blank lines are skipped.
fn parse_booleans_local(text: &str) -> (Vec<SelinuxBoolean>, Vec<(usize, BooleanLineFault)>) {
    let mut booleans = Vec::new();
    let mut faults = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        match parse_boolean_line(trimmed) {
            Ok(boolean) => booleans.push(boolean),
            Err(fault) => faults.push((index + 1, fault)),
        }
    }
    (booleans, faults)
}

/// Parses `semanage boolean -l -C`, which lists only the booleans set
/// locally. Each row prints the running value, then the persistent one;
/// the persistent one is what the host keeps across a reboot.
fn parse_semanage_local_booleans(text: &str) -> Vec<SelinuxBoolean> {
    text.lines()
        .filter_map(|line| BOOL_RE.captures(line.trim()))
        .map(|caps| SelinuxBoolean {
            name: caps[1].to_string(),
            value: &caps[3] == "on",
        })
        .collect()
}

/// Collects the booleans set persistently on the host.
///
/// The source is the active store's `booleans.local`, which `setsebool
/// -P` and `semanage boolean -m` write; it is a file read, so it does not
/// need `semanage` on the host. When the store cannot be read, `semanage
/// boolean -l -C` lists the same set. A runtime-only change is in neither
/// and is not collected: the host loses it at its next boot.
///
/// `semanage boolean -l` without `-C` is not used: a persistent change
/// rewrites the policy default it prints, so a "current differs from
/// default" filter over it keeps exactly the runtime-only changes and
/// drops the persistent ones.
fn collect_boolean_overrides(
    exec: &dyn Executor,
    section: &mut SelinuxSection,
    warnings: &mut Vec<Warning>,
    degraded_reasons: &mut Vec<String>,
    store: &PolicyStore,
) {
    let label = PolicyStore::LABEL;
    match exec.read_file(&store.join(BOOLEANS_LOCAL)) {
        Ok(content) => {
            let (booleans, faults) = parse_booleans_local(&content);
            section.boolean_overrides = booleans;
            if !faults.is_empty() {
                let lines: Vec<String> = faults
                    .iter()
                    .map(|(line, fault)| format!("line {line} ({})", fault.describe()))
                    .collect();
                warnings.push(selinux_warning(format!(
                    "SELinux {BOOLEANS_LOCAL} lines not read in {label}: {}",
                    lines.join(", ")
                )));
            }
            return;
        }
        // A readable store with no `booleans.local`: nothing set locally.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && exec.file_exists(store.root()) => {
            return;
        }
        Err(_) => {}
    }

    let host_root = exec.host_root().to_string_lossy().to_string();
    let res = exec.run("chroot", &[&host_root, "semanage", "boolean", "-l", "-C"]);
    if res.success() {
        section.boolean_overrides = parse_semanage_local_booleans(&res.stdout);
        return;
    }

    // No store and no semanage on a host without SELinux: nothing to read.
    if !exec.file_exists(Path::new(SELINUX_CONFIG_PATH)) {
        return;
    }
    warnings.push(selinux_warning(format!(
        "SELinux booleans set with setsebool -P not read: {BOOLEANS_LOCAL} in {label} is not \
         readable and semanage boolean -l -C failed"
    )));
    degraded_reasons.push("persistent SELinux booleans unavailable".into());
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
                "modules selinux-policy generates at 400 are not local",
                "targeted",
                None,
                vec![(var, "400", &["extra_varrun", "extra_binsbin", "myapp"])],
                &["myapp"],
            ),
            (
                "a generated module's name at another priority is the administrator's",
                "targeted",
                None,
                vec![
                    (var, "400", &["extra_varrun"]),
                    (var, "500", &["extra_varrun"]),
                    (var, "300", &["extra_binsbin"]),
                ],
                &["extra_binsbin", "extra_varrun"],
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
                "a quoted store-root is the path inside the quotes",
                "targeted",
                Some("store-root = \"/etc/selinux\"\n"),
                vec![(etc, "400", &["myapp"]), (var, "400", &["stale"])],
                &["myapp"],
            ),
            (
                "the last store-root assignment wins",
                "targeted",
                Some("store-root=/var/lib/selinux\nstore-root =\t/etc/selinux\n"),
                vec![(etc, "400", &["myapp"]), (var, "400", &["stale"])],
                &["myapp"],
            ),
            (
                "a commented-out store-root is no assignment",
                "targeted",
                Some("store-root=/etc/selinux\n# store-root=/var/lib/selinux\n"),
                vec![(etc, "400", &["myapp"]), (var, "400", &["stale"])],
                &["myapp"],
            ),
            (
                "an unquoted value runs to the end of the line, # included",
                "targeted",
                Some("store-root=/etc/selinux # bootc\n"),
                vec![(etc, "400", &["myapp"]), (var, "400", &["stale"])],
                &[],
            ),
            (
                "an empty value sets nothing",
                "targeted",
                Some("store-root=/etc/selinux\nstore-root = \"\"\n"),
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
            warnings.iter().any(|w| w.message
                == format!(
                    "SELinux custom policy modules not read: modules/ in {}: entity not found",
                    PolicyStore::LABEL
                )),
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

    // ---- Test 7b: modules turned off with semodule -d ----

    #[test]
    fn disabled_modules_are_kept_apart_from_active_ones() {
        let var = "/var/lib/selinux";
        let exec = module_host(
            "targeted",
            None,
            &[
                (var, "100", &["apache", "base"]),
                (var, "200", &["container"]),
                (var, "400", &["myapp", "other"]),
                (var, "500", &["override"]),
                (
                    var,
                    "disabled",
                    &["apache", "container", "myapp", "override"],
                ),
            ],
        );
        let (section, warnings) = inspect_with(&exec);
        // A disabled local module is not listed as active.
        assert_eq!(section.custom_modules, ["other"]);
        assert_eq!(
            section.disabled_modules,
            [
                DisabledModule {
                    name: "apache".into(),
                    origin: ModuleOrigin::Packaged,
                },
                DisabledModule {
                    name: "container".into(),
                    origin: ModuleOrigin::Packaged,
                },
                DisabledModule {
                    name: "myapp".into(),
                    origin: ModuleOrigin::Local,
                },
                DisabledModule {
                    name: "override".into(),
                    origin: ModuleOrigin::Local,
                },
            ]
        );
        assert!(
            !warnings.iter().any(|w| w.message.contains("modules")),
            "{warnings:?}"
        );
    }

    #[test]
    fn a_store_without_a_disabled_directory_has_no_disabled_modules() {
        let exec = module_host("targeted", None, &[("/var/lib/selinux", "400", &["myapp"])]);
        let (section, warnings) = inspect_with(&exec);
        assert_eq!(section.custom_modules, ["myapp"]);
        assert!(section.disabled_modules.is_empty());
        assert!(
            !warnings.iter().any(|w| w.message.contains("modules")),
            "{warnings:?}"
        );
    }

    // ---- Test 7c: module directories read in full or reported ----

    #[test]
    fn a_module_directory_read_part_way_is_reported() {
        let modules = "/var/lib/selinux/targeted/active/modules";
        let layout: &[(&str, &str, &[&str])] = &[
            ("/var/lib/selinux", "400", &["myapp"]),
            ("/var/lib/selinux", "disabled", &["apache"]),
        ];
        // (directory that fails part way, what is still listed, how the
        // warning names it)
        let cases: &[(String, &[&str], &str)] = &[
            (modules.to_string(), &[], "modules/ in"),
            (format!("{modules}/400"), &[], "the priority 400 modules in"),
            (
                format!("{modules}/disabled"),
                &["myapp"],
                "modules/disabled/ in",
            ),
        ];
        for (dir, custom, place) in cases {
            let exec = module_host("targeted", None, layout)
                .with_dir_iteration_error(dir, std::io::ErrorKind::PermissionDenied);
            let (section, warnings) = inspect_with(&exec);
            assert_eq!(section.custom_modules, *custom, "{dir}");
            assert!(
                warnings
                    .iter()
                    .any(|w| w.message.contains("not read") && w.message.contains(place)),
                "{dir}: {warnings:?}"
            );
        }

        // A disabled/ that cannot be opened at all is reported too.
        let exec = module_host("targeted", None, layout).with_dir_error(
            &format!("{modules}/disabled"),
            std::io::ErrorKind::PermissionDenied,
        );
        let (section, warnings) = inspect_with(&exec);
        assert_eq!(section.custom_modules, ["myapp"]);
        assert!(section.disabled_modules.is_empty());
        assert!(
            warnings
                .iter()
                .any(|w| w.message.contains("modules/disabled/ in")),
            "{warnings:?}"
        );
    }

    // ---- Test 7d: the configured store path reaches no warning ----

    /// The store's path comes from `semanage.conf` and `/etc/selinux/config`,
    /// and a value in either runs to the end of its line, so an operator's
    /// note after it is part of the path. Every warning about the store
    /// names it by [`PolicyStore::LABEL`] and an error by its kind, never by
    /// the path, which an I/O error's text also carries: warnings are not
    /// redacted.
    #[test]
    fn store_warnings_never_carry_the_configured_path() {
        const ROOT_SENTINEL: &str = "inspectah-sentinel-credential-1e5a";
        const TYPE_SENTINEL: &str = "inspectah-sentinel-credential-6d0c";
        let policy_type = format!("targeted # {TYPE_SENTINEL}");
        let root = format!("/var/lib/selinux # password={ROOT_SENTINEL}");
        let conf = format!("store-root = {root}\n");
        let store = format!("{root}/{policy_type}/active");
        let label = PolicyStore::LABEL;

        // A store that is not there: the modules and booleans.local.
        let missing = MockExecutor::new()
            .with_file(
                "/etc/selinux/config",
                &format!("SELINUX=enforcing\nSELINUXTYPE={policy_type}\n"),
            )
            .with_file("/etc/selinux/semanage.conf", &conf);
        // One that is there, with a priority directory, disabled/ and a
        // booleans.local line that do not read.
        let unreadable = module_host(&policy_type, Some(&conf), &[(&root, "400", &["myapp"])])
            .with_dir(&store, vec!["modules", "booleans.local"])
            .with_file(&format!("{store}/booleans.local"), "virt_use_nfs=maybe\n")
            .with_dir_iteration_error(
                &format!("{store}/modules/400"),
                std::io::ErrorKind::PermissionDenied,
            )
            .with_dir_error(
                &format!("{store}/modules/disabled"),
                std::io::ErrorKind::PermissionDenied,
            );
        let cases = [
            (
                "missing",
                missing,
                vec![
                    format!(
                        "SELinux custom policy modules not read: modules/ in {label}: \
                         entity not found"
                    ),
                    format!(
                        "SELinux booleans set with setsebool -P not read: booleans.local in \
                         {label} is not readable and semanage boolean -l -C failed"
                    ),
                ],
            ),
            (
                "unreadable",
                unreadable,
                vec![
                    format!(
                        "SELinux custom policy modules not read: the priority 400 modules in \
                         {label}: permission denied"
                    ),
                    format!(
                        "SELinux disabled policy modules not read: modules/disabled/ in \
                         {label}: permission denied; the modules listed as custom may \
                         include disabled ones"
                    ),
                    format!(
                        "SELinux booleans.local lines not read in {label}: \
                         line 1 (the value is not 0, 1, true or false)"
                    ),
                ],
            ),
        ];
        for (case, exec, expected) in cases {
            let (_, warnings) = inspect_with(&exec);
            let messages: Vec<&str> = warnings.iter().map(|w| w.message.as_str()).collect();
            assert_eq!(messages, expected, "{case}");
            let json = serde_json::to_string(&warnings).expect("the warnings serialize");
            for sentinel in [ROOT_SENTINEL, TYPE_SENTINEL] {
                assert!(!json.contains(sentinel), "{case}: {json}");
            }
        }
    }

    // ---- Test 8: persistent booleans come from the policy store ----

    /// The active store on a package-mode host.
    const VAR_STORE: &str = "/var/lib/selinux/targeted/active";

    /// `booleans.local` as libsemanage writes it.
    const BOOLEANS_LOCAL_FILE: &str = "# This file is auto-generated by libsemanage\n\
         # Do not edit directly.\n\
         \n\
         virt_use_nfs=1\n\
         httpd_can_network_connect=1\n\
         ftpd_full_access=0\n";

    /// `semanage boolean -l` without `-C` on the same host after a
    /// runtime-only `setsebool container_manage_cgroup on`: the two
    /// persistent changes print equal columns, the runtime one does not.
    const SEMANAGE_BOOLEAN_L: &str = "\
SELinux boolean                State  Default Description\n\
\n\
container_manage_cgroup        (on   ,  off)  Allow container to manage cgroup\n\
httpd_can_network_connect      (on   ,   on)  Allow httpd to can network connect\n\
virt_use_nfs                   (on   ,   on)  Allow virt to use nfs\n";

    fn boolean(name: &str, value: bool) -> SelinuxBoolean {
        SelinuxBoolean {
            name: name.into(),
            value,
        }
    }

    fn ok(stdout: &str) -> ExecResult {
        ExecResult {
            stdout: stdout.into(),
            exit_code: 0,
            ..Default::default()
        }
    }

    /// A configured targeted host, its store at `store` holding `local`
    /// as `booleans.local` when given, with a runtime-only change visible
    /// to `semanage boolean -l` and in sysfs.
    fn boolean_host(store: Option<&str>, local: Option<&str>) -> MockExecutor {
        let mut exec = MockExecutor::new()
            .with_file(
                "/etc/selinux/config",
                "SELINUX=enforcing\nSELINUXTYPE=targeted\n",
            )
            .with_command("chroot / semanage boolean -l", ok(SEMANAGE_BOOLEAN_L))
            .with_dir("/sys/fs/selinux/booleans", vec!["container_manage_cgroup"])
            .with_file("/sys/fs/selinux/booleans/container_manage_cgroup", "1 1");
        if let Some(store) = store {
            exec = exec.with_dir(store, vec!["modules"]);
            if let Some(local) = local {
                exec = exec.with_file(&format!("{store}/booleans.local"), local);
            }
        }
        exec
    }

    #[test]
    fn persistent_booleans_are_read_from_booleans_local() {
        let (section, warnings) =
            inspect_with(&boolean_host(Some(VAR_STORE), Some(BOOLEANS_LOCAL_FILE)));
        // In the file's order; a persistent `off` is carried too. The
        // runtime-only change is not.
        assert_eq!(
            section.boolean_overrides,
            [
                boolean("virt_use_nfs", true),
                boolean("httpd_can_network_connect", true),
                boolean("ftpd_full_access", false),
            ]
        );
        assert!(
            !warnings.iter().any(|w| w.message.contains("booleans")),
            "{warnings:?}"
        );
    }

    #[test]
    fn booleans_local_follows_a_relocated_store() {
        let exec = boolean_host(
            Some("/etc/selinux/targeted/active"),
            Some("httpd_can_network_relay=1\n"),
        )
        .with_file("/etc/selinux/semanage.conf", "store-root=/etc/selinux\n");
        let (section, _) = inspect_with(&exec);
        assert_eq!(
            section.boolean_overrides,
            [boolean("httpd_can_network_relay", true)]
        );
    }

    /// Two populated stores that set different booleans: the one
    /// `semanage.conf` names last is read, quoted or not.
    #[test]
    fn booleans_local_follows_the_last_store_root_assignment() {
        let confs = [
            "store-root=\"/etc/selinux\"\n",
            "store-root=/var/lib/selinux\nstore-root=/etc/selinux\n",
        ];
        for conf in confs {
            let exec = boolean_host(Some(VAR_STORE), Some("stale_boolean=1\n"))
                .with_dir("/etc/selinux/targeted/active", vec!["modules"])
                .with_file(
                    "/etc/selinux/targeted/active/booleans.local",
                    "httpd_can_network_relay=0\n",
                )
                .with_file("/etc/selinux/semanage.conf", conf);
            let (section, _) = inspect_with(&exec);
            assert_eq!(
                section.boolean_overrides,
                [boolean("httpd_can_network_relay", false)],
                "{conf:?}"
            );
        }
    }

    #[test]
    fn a_store_without_booleans_local_has_no_persistent_booleans() {
        let (section, warnings) = inspect_with(&boolean_host(Some(VAR_STORE), None));
        assert!(section.boolean_overrides.is_empty());
        assert!(
            !warnings.iter().any(|w| w.message.contains("booleans")),
            "{warnings:?}"
        );
    }

    #[test]
    fn parse_booleans_local_accepts_what_libsemanage_reads() {
        let (booleans, faults) = parse_booleans_local(
            "# header\n a = 1 \nb=true\nc=FALSE\nd=0\ne=2\n=1\nnot a boolean\n*=1\n",
        );
        assert_eq!(
            booleans,
            [
                boolean("a", true),
                boolean("b", true),
                boolean("c", false),
                boolean("d", false),
            ]
        );
        assert_eq!(
            faults,
            [
                (6, BooleanLineFault::Value),
                (7, BooleanLineFault::Name),
                (8, BooleanLineFault::NoAssignment),
                (9, BooleanLineFault::Name),
            ]
        );
    }

    /// A line the parser cannot read is reported by its number and why,
    /// never by its text: a warning is not redacted, and the line can
    /// hold anything.
    #[test]
    fn an_unreadable_booleans_local_line_is_reported_without_its_text() {
        const SENTINEL: &str = "inspectah-sentinel-credential-7c2d";
        let (section, warnings) = inspect_with(&boolean_host(
            Some(VAR_STORE),
            Some(&format!(
                "virt_use_nfs=1\nvirt_use_samba=maybe\napi_token={SENTINEL}\n{SENTINEL}\n"
            )),
        ));
        assert_eq!(section.boolean_overrides, [boolean("virt_use_nfs", true)]);
        let messages: Vec<&str> = warnings
            .iter()
            .map(|w| w.message.as_str())
            .filter(|m| m.contains("booleans"))
            .collect();
        assert_eq!(
            messages,
            [format!(
                "SELinux booleans.local lines not read in {}: \
                 line 2 (the value is not 0, 1, true or false), \
                 line 3 (the value is not 0, 1, true or false), \
                 line 4 (no name=value assignment)",
                PolicyStore::LABEL
            )
            .as_str()]
        );
        let json = serde_json::to_string(&warnings).expect("the warnings serialize");
        assert!(!json.contains(SENTINEL), "{json}");
        assert!(!json.contains("maybe"), "{json}");
    }

    // ---- Test 9: without a readable store ----

    #[test]
    fn without_a_store_semanage_local_list_supplies_the_persistent_value() {
        // `-l -C` prints (running, persistent); the persistent value wins.
        let exec = boolean_host(None, None).with_command(
            "chroot / semanage boolean -l -C",
            ok("\
SELinux boolean                State  Default Description\n\
\n\
httpd_can_network_connect      (on   ,   on)  Allow httpd to can network connect\n\
virt_use_nfs                   (on   ,  off)  Allow virt to use nfs\n"),
        );
        let (section, _) = inspect_with(&exec);
        assert_eq!(
            section.boolean_overrides,
            [
                boolean("httpd_can_network_connect", true),
                boolean("virt_use_nfs", false),
            ]
        );
    }

    #[test]
    fn without_a_store_or_semanage_the_section_degrades() {
        let exec = boolean_host(None, None);
        let rpm_state = empty_rpm_state();
        let source = test_source_system();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: Some(&rpm_state),
            baseline_data: None,
        };
        match SelinuxInspector::new().inspect(&ctx, &NullProgress) {
            Err(InspectorError::Degraded { partial, reason }) => {
                assert!(reason.contains("persistent SELinux booleans"), "{reason}");
                assert!(
                    partial
                        .warnings
                        .iter()
                        .any(|w| w.message.contains("setsebool -P not read")),
                    "{:?}",
                    partial.warnings
                );
                // The runtime-only change in sysfs is not a substitute.
                let SectionData::Selinux(section) = partial.section else {
                    panic!("expected the SELinux section");
                };
                assert!(section.boolean_overrides.is_empty());
            }
            other => panic!("expected a degraded section, got {other:?}"),
        }
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
