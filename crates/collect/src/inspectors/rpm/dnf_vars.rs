//! dnf substitution variables of the scanned host.
//!
//! Repo files name their GPG keys with dnf variables, such as EL10's
//! `gpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-$releasever_major`.
//! The collector has to expand them the way dnf on that host would, or it
//! reads a literal `$releasever_major` path, finds nothing, and the key never
//! reaches the image.
//!
//! What is derived, and from where (dnf `dnf/cli/cli.py` `_read_conf_file`,
//! `dnf/rpm/__init__.py` `detect_releasevers`, libdnf5 `conf/vars.cpp`):
//!
//! - **Variable files.** Each file in a vars directory defines a variable
//!   named after the file, whose value is the file's first line. dnf4 reads
//!   `/etc/yum/vars` then `/etc/dnf/vars`; dnf5 reads
//!   `/usr/share/dnf5/vars.d` then `/etc/dnf/vars`. A later directory wins.
//! - **`releasever`**, when no variable file sets it: the first installed
//!   package providing `system-release(releasever)`, `system-release`,
//!   `distribution-release(releasever)`, `distribution-release`,
//!   `redhat-release` or `suse-release`, in that order. Its value is the
//!   version of the matching `=` provide when the package's name differs
//!   from the capability, and the package version otherwise.
//! - **`releasever_major` / `releasever_minor`**: the package's
//!   `system-release(releasever_major)` / `(releasever_minor)` provide when
//!   present, else `releasever` split at its first `.`. A variable file
//!   naming either one wins over both.
//! - **`arch` / `basearch`**: the host machine type and dnf's base
//!   architecture for it.
//!
//! Environment variables (`DNF_VAR_*`, `DNF0`-`DNF9`) are not read: they
//! belong to a dnf invocation, not to the host.

use inspectah_core::traits::executor::Executor;
use std::cell::OnceCell;
use std::collections::HashMap;
use std::fmt;
use std::path::Path;

/// Release-package capabilities dnf probes for `releasever`, in its order.
const DISTROVERPKGS: &[&str] = &[
    "system-release(releasever)",
    "system-release",
    "distribution-release(releasever)",
    "distribution-release",
    "redhat-release",
    "suse-release",
];

/// Provide that overrides the major part of `releasever`.
const RELEASEVER_MAJOR_PROVIDE: &str = "system-release(releasever_major)";
/// Provide that overrides the minor part of `releasever`.
const RELEASEVER_MINOR_PROVIDE: &str = "system-release(releasever_minor)";

/// `RPMSENSE_EQUAL`: dnf takes a provide's version only for an exact `=`.
const RPMSENSE_EQUAL: u32 = 8;

/// Marks the start of one package in the provides query output.
const PACKAGE_MARKER: &str = "@@";

/// Query format: one `@@name<TAB>version` line per package, then one
/// `provide<TAB>flags<TAB>version` line per provide.
const PROVIDES_QUERY_FORMAT: &str =
    "@@%{NAME}\t%{VERSION}\n[%{PROVIDENAME}\t%{PROVIDEFLAGS}\t%{PROVIDEVERSION}\n]";

/// Present on a host whose dnf is dnf5 (EL10, Fedora 41 and later).
const DNF5_BINARY: &str = "/usr/bin/dnf5";
const DNF4_VARS_DIRS: &[&str] = &["/etc/yum/vars", "/etc/dnf/vars"];
const DNF5_VARS_DIRS: &[&str] = &["/usr/share/dnf5/vars.d", "/etc/dnf/vars"];

/// Nesting limit for `${var:-word}` words, as in libdnf5.
const MAX_EXPRESSION_DEPTH: usize = 32;

/// Why a value could not be expanded into a path the collector will read.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ExpandError {
    /// `$name` or `${name}` names a variable the host does not define.
    UnknownVariable(String),
    /// The variable's value could turn the path into a different one.
    UnsafeValue(String),
    /// Syntax this expander does not follow (a backslash, a `$` with no
    /// name, an unterminated or unknown `${...}` form).
    UnsupportedSyntax,
}

impl fmt::Display for ExpandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownVariable(name) => {
                write!(f, "dnf variable ${name} is not defined on this host")
            }
            Self::UnsafeValue(name) => write!(
                f,
                "dnf variable ${name} has a value that is not a plain path segment"
            ),
            Self::UnsupportedSyntax => {
                write!(
                    f,
                    "the value uses dnf variable syntax inspectah does not expand"
                )
            }
        }
    }
}

/// The scanned host's dnf variables, read on first use.
pub struct DnfVars<'a> {
    exec: Option<&'a dyn Executor>,
    values: OnceCell<HashMap<String, String>>,
}

impl<'a> DnfVars<'a> {
    /// Variables of the host behind `exec`. Nothing is read until a value
    /// with a `$` is expanded, so a host whose repo files use no variables
    /// runs no extra commands.
    pub fn from_host(exec: &'a dyn Executor) -> Self {
        Self {
            exec: Some(exec),
            values: OnceCell::new(),
        }
    }

    /// A fixed set of variables, for tests.
    #[cfg(test)]
    pub fn from_values(values: HashMap<String, String>) -> Self {
        let cell = OnceCell::new();
        let _ = cell.set(values);
        Self {
            exec: None,
            values: cell,
        }
    }

    fn values(&self) -> &HashMap<String, String> {
        self.values.get_or_init(|| match self.exec {
            Some(exec) => detect(exec),
            None => HashMap::new(),
        })
    }

    /// Expand every dnf variable in `text`: `$name`, `${name}`,
    /// `${name:-word}` and `${name:+word}`.
    pub fn expand(&self, text: &str) -> Result<String, ExpandError> {
        if !text.contains('$') {
            if text.contains('\\') {
                return Err(ExpandError::UnsupportedSyntax);
            }
            return Ok(text.to_string());
        }
        let (expanded, rest) = expand_until(text, self.values(), 0)?;
        if !rest.is_empty() {
            // A stray `}` at the top level.
            return Err(ExpandError::UnsupportedSyntax);
        }
        Ok(expanded)
    }
}

/// Expand `text` up to the end, or up to an unmatched `}` when `depth > 0`.
/// Returns the expansion and the unconsumed remainder (starting at that `}`).
fn expand_until<'t>(
    text: &'t str,
    vars: &HashMap<String, String>,
    depth: usize,
) -> Result<(String, &'t str), ExpandError> {
    if depth > MAX_EXPRESSION_DEPTH {
        return Err(ExpandError::UnsupportedSyntax);
    }
    let mut out = String::new();
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        match c {
            '\\' => return Err(ExpandError::UnsupportedSyntax),
            '}' if depth > 0 => return Ok((out, rest)),
            '$' => {
                let after = &rest[1..];
                if let Some(braced) = after.strip_prefix('{') {
                    let name_len = name_length(braced);
                    if name_len == 0 {
                        return Err(ExpandError::UnsupportedSyntax);
                    }
                    let name = &braced[..name_len];
                    let tail = &braced[name_len..];
                    if let Some(tail) = tail.strip_prefix('}') {
                        out.push_str(lookup(vars, name)?);
                        rest = tail;
                    } else if let Some(word) = tail.strip_prefix(":-") {
                        let (word, tail) = expand_until(word, vars, depth + 1)?;
                        let tail = tail
                            .strip_prefix('}')
                            .ok_or(ExpandError::UnsupportedSyntax)?;
                        match vars.get(name).filter(|v| !v.is_empty()) {
                            Some(_) => out.push_str(lookup(vars, name)?),
                            None => out.push_str(&word),
                        }
                        rest = tail;
                    } else if let Some(word) = tail.strip_prefix(":+") {
                        let (word, tail) = expand_until(word, vars, depth + 1)?;
                        let tail = tail
                            .strip_prefix('}')
                            .ok_or(ExpandError::UnsupportedSyntax)?;
                        // The value only decides; the word is what lands.
                        if vars.get(name).is_some_and(|v| !v.is_empty()) {
                            out.push_str(&word);
                        }
                        rest = tail;
                    } else {
                        return Err(ExpandError::UnsupportedSyntax);
                    }
                } else {
                    let name_len = name_length(after);
                    if name_len == 0 {
                        return Err(ExpandError::UnsupportedSyntax);
                    }
                    out.push_str(lookup(vars, &after[..name_len])?);
                    rest = &after[name_len..];
                }
            }
            _ => {
                out.push(c);
                rest = &rest[c.len_utf8()..];
            }
        }
    }
    if depth > 0 {
        // No closing `}` for a `${name:-word}` form.
        return Err(ExpandError::UnsupportedSyntax);
    }
    Ok((out, rest))
}

/// Length of the dnf variable name at the start of `s`: ASCII letters,
/// digits and `_`, as libdnf5 scans it.
fn name_length(s: &str) -> usize {
    s.bytes()
        .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_')
        .count()
}

/// The value of `name`, refused when it is unknown or could change which
/// file the path names.
fn lookup<'v>(vars: &'v HashMap<String, String>, name: &str) -> Result<&'v str, ExpandError> {
    let value = vars
        .get(name)
        .ok_or_else(|| ExpandError::UnknownVariable(name.to_string()))?;
    if is_safe_value(value) {
        Ok(value)
    } else {
        Err(ExpandError::UnsafeValue(name.to_string()))
    }
}

/// A value may be empty or a run of ASCII letters, digits and `._+-~`, and
/// may not be `.` or contain `..`. That refuses `/`, whitespace, line
/// breaks, control characters, shell characters and traversal.
fn is_safe_value(value: &str) -> bool {
    value != "."
        && !value.contains("..")
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'+' | b'-' | b'~'))
}

/// Read the host's variables as dnf would.
fn detect(exec: &dyn Executor) -> HashMap<String, String> {
    let mut vars = HashMap::new();

    let dirs = if exec.file_exists(Path::new(DNF5_BINARY)) {
        DNF5_VARS_DIRS
    } else {
        DNF4_VARS_DIRS
    };
    for dir in dirs {
        let Ok(mut entries) = exec.read_dir(Path::new(dir)) else {
            continue;
        };
        entries.sort();
        for name in entries {
            if name.starts_with('.') {
                continue;
            }
            if let Ok(content) = exec.read_file(&Path::new(dir).join(&name)) {
                let first = content.lines().next().unwrap_or("");
                vars.insert(name, first.to_string());
            }
        }
    }

    if !vars.contains_key("releasever") {
        if let Some(release) = detect_release(exec) {
            let (major, minor) = split_releasever(&release.releasever);
            vars.entry("releasever_major".into())
                .or_insert_with(|| release.major.unwrap_or(major));
            vars.entry("releasever_minor".into())
                .or_insert_with(|| release.minor.unwrap_or(minor));
            vars.insert("releasever".into(), release.releasever);
        }
    } else {
        let (major, minor) = split_releasever(&vars["releasever"]);
        vars.entry("releasever_major".into()).or_insert(major);
        vars.entry("releasever_minor".into()).or_insert(minor);
    }

    if !vars.contains_key("arch") || !vars.contains_key("basearch") {
        let result = exec.run("uname", &["-m"]);
        let machine = result.stdout.trim();
        if result.success()
            && let Some(basearch) = basearch_for(machine)
        {
            vars.entry("arch".into())
                .or_insert_with(|| machine.to_string());
            vars.entry("basearch".into())
                .or_insert_with(|| basearch.to_string());
        }
    }

    vars
}

/// `releasever` and its explicit major/minor overrides, from the rpmdb.
struct DetectedRelease {
    releasever: String,
    major: Option<String>,
    minor: Option<String>,
}

fn detect_release(exec: &dyn Executor) -> Option<DetectedRelease> {
    for capability in DISTROVERPKGS {
        let result = exec.run(
            "rpm",
            &[
                "-q",
                "--whatprovides",
                capability,
                "--queryformat",
                PROVIDES_QUERY_FORMAT,
            ],
        );
        if !result.success() {
            continue;
        }
        if let Some(release) = parse_release(&result.stdout, capability) {
            return Some(release);
        }
    }
    None
}

/// Read the first package block of the provides query, as dnf reads the
/// first matching header.
fn parse_release(stdout: &str, capability: &str) -> Option<DetectedRelease> {
    let mut lines = stdout
        .lines()
        .skip_while(|l| !l.starts_with(PACKAGE_MARKER));
    let header = lines.next()?.strip_prefix(PACKAGE_MARKER)?;
    let (name, version) = header.split_once('\t')?;
    let mut release = DetectedRelease {
        releasever: version.to_string(),
        major: None,
        minor: None,
    };
    for line in lines.take_while(|l| !l.starts_with(PACKAGE_MARKER)) {
        let mut fields = line.splitn(3, '\t');
        let (Some(provide), Some(flags), Some(ver)) = (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        if flags.parse::<u32>().ok() != Some(RPMSENSE_EQUAL) || ver.is_empty() {
            continue;
        }
        if provide == capability && name != capability {
            release.releasever = ver.to_string();
        } else if provide == RELEASEVER_MAJOR_PROVIDE {
            release.major = Some(ver.to_string());
        } else if provide == RELEASEVER_MINOR_PROVIDE {
            release.minor = Some(ver.to_string());
        }
    }
    (!release.releasever.is_empty()).then_some(release)
}

/// Split at the first `.`, as dnf's `splitReleasever` does.
fn split_releasever(releasever: &str) -> (String, String) {
    match releasever.split_once('.') {
        Some((major, minor)) => (major.to_string(), minor.to_string()),
        None => (releasever.to_string(), String::new()),
    }
}

/// dnf's base architecture for a machine type (`dnf/rpm/__init__.py`
/// `_BASEARCH_MAP`), for the architectures EL and Fedora build for.
fn basearch_for(machine: &str) -> Option<&'static str> {
    Some(match machine {
        "x86_64" | "amd64" | "ia32e" => "x86_64",
        "aarch64" => "aarch64",
        "ppc64le" => "ppc64le",
        "ppc64" | "ppc64iseries" | "ppc64p7" | "ppc64pseries" => "ppc64",
        "s390x" => "s390x",
        "i386" | "i486" | "i586" | "i686" | "athlon" | "geode" => "i386",
        "armv6hl" | "armv7hl" | "armv7hnl" | "armv8hl" => "armhfp",
        "riscv64" => "riscv64",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::mock::MockExecutor;
    use inspectah_core::traits::executor::ExecResult;

    fn vars(pairs: &[(&str, &str)]) -> DnfVars<'static> {
        DnfVars::from_values(
            pairs
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect(),
        )
    }

    #[test]
    fn expansion_table() {
        let v = vars(&[
            ("releasever", "10.2"),
            ("releasever_major", "10"),
            ("releasever_minor", "2"),
            ("basearch", "aarch64"),
            ("empty", ""),
            ("slash", "a/b"),
            ("dotdot", ".."),
            ("space", "a b"),
            ("newline", "a\nb"),
            ("ctrl", "a\u{1b}b"),
            ("dollar", "a$b"),
        ]);
        let exp = |s: &str| Ok::<String, ExpandError>(s.to_string());
        let cases: &[(&str, Result<String, ExpandError>)] = &[
            ("/k/plain", exp("/k/plain")),
            ("/k/EPEL-$releasever_major", exp("/k/EPEL-10")),
            ("/k/EPEL-$releasever", exp("/k/EPEL-10.2")),
            ("/k/EPEL-${releasever_major}x", exp("/k/EPEL-10x")),
            ("/k/$basearch/key", exp("/k/aarch64/key")),
            ("/k/a${releasever_minor:+.$releasever_minor}", exp("/k/a.2")),
            ("/k/a${empty:+.$releasever_minor}", exp("/k/a")),
            ("/k/a${nope:-$releasever_major}", exp("/k/a10")),
            ("/k/a${releasever_major:-x}", exp("/k/a10")),
            ("/k/$nope", Err(ExpandError::UnknownVariable("nope".into()))),
            (
                "/k/${nope}",
                Err(ExpandError::UnknownVariable("nope".into())),
            ),
            ("/k/$slash", Err(ExpandError::UnsafeValue("slash".into()))),
            ("/k/$dotdot", Err(ExpandError::UnsafeValue("dotdot".into()))),
            ("/k/$space", Err(ExpandError::UnsafeValue("space".into()))),
            (
                "/k/$newline",
                Err(ExpandError::UnsafeValue("newline".into())),
            ),
            ("/k/$ctrl", Err(ExpandError::UnsafeValue("ctrl".into()))),
            ("/k/$dollar", Err(ExpandError::UnsafeValue("dollar".into()))),
            ("/k/${slash:+x}", exp("/k/x")),
            ("/k/$", Err(ExpandError::UnsupportedSyntax)),
            ("/k/$-x", Err(ExpandError::UnsupportedSyntax)),
            ("/k/${releasever", Err(ExpandError::UnsupportedSyntax)),
            ("/k/${releasever:-x", Err(ExpandError::UnsupportedSyntax)),
            ("/k/${releasever:?x}", Err(ExpandError::UnsupportedSyntax)),
            ("/k/\\$releasever", Err(ExpandError::UnsupportedSyntax)),
            ("/k/a\\b", Err(ExpandError::UnsupportedSyntax)),
        ];
        for (input, expected) in cases {
            assert_eq!(&v.expand(input), expected, "input: {input:?}");
        }
    }

    fn ok(stdout: &str) -> ExecResult {
        ExecResult {
            stdout: stdout.into(),
            exit_code: 0,
            ..Default::default()
        }
    }

    fn provides_key(capability: &str) -> String {
        format!("rpm -q --whatprovides {capability} --queryformat {PROVIDES_QUERY_FORMAT}")
    }

    /// RHEL 10 style: the release package names its major explicitly.
    #[test]
    fn detects_release_and_explicit_major_from_the_rpmdb() {
        let mock = MockExecutor::new()
            .with_command(
                &provides_key("system-release(releasever)"),
                ok("@@redhat-release\t10.2\n\
                    redhat-release\t8\t10.2-0.13.el10\n\
                    system-release(releasever)\t8\t10.2\n\
                    system-release(releasever_major)\t8\t10\n\
                    system-release(releasever_minor)\t8\t2\n"),
            )
            .with_command("uname -m", ok("aarch64\n"));
        let values = detect(&mock);
        assert_eq!(values["releasever"], "10.2");
        assert_eq!(values["releasever_major"], "10");
        assert_eq!(values["releasever_minor"], "2");
        assert_eq!(values["basearch"], "aarch64");
        assert_eq!(values["arch"], "aarch64");
    }

    #[test]
    fn splits_releasever_when_no_major_provide_and_falls_through_capabilities() {
        let mock = MockExecutor::new()
            .with_command(
                &provides_key("system-release"),
                ok("@@centos-stream-release\t10.0\n\
                    system-release\t8\t10.1\n"),
            )
            .with_command("uname -m", ok("x86_64\n"));
        let values = detect(&mock);
        assert_eq!(values["releasever"], "10.1");
        assert_eq!(values["releasever_major"], "10");
        assert_eq!(values["releasever_minor"], "1");
        assert_eq!(values["basearch"], "x86_64");
    }

    #[test]
    fn package_version_is_kept_when_the_provide_names_the_package_itself() {
        let mock = MockExecutor::new().with_command(
            &provides_key("system-release(releasever)"),
            ok("@@system-release(releasever)\t9\nsystem-release(releasever)\t8\t9.9\n"),
        );
        assert_eq!(detect(&mock)["releasever"], "9");
    }

    /// A vars file wins over the rpmdb and over the split; a later
    /// directory wins over an earlier one.
    #[test]
    fn vars_files_override_detection_in_directory_order() {
        let mock = MockExecutor::new()
            .with_dir("/etc/yum/vars", vec!["releasever", "custom"])
            .with_file("/etc/yum/vars/releasever", "8\n")
            .with_file("/etc/yum/vars/custom", "old\n")
            .with_dir("/etc/dnf/vars", vec!["custom", "releasever_major"])
            .with_file("/etc/dnf/vars/custom", "new\nsecond line ignored\n")
            .with_file("/etc/dnf/vars/releasever_major", "9\n");
        let values = detect(&mock);
        assert_eq!(values["releasever"], "8");
        assert_eq!(values["releasever_major"], "9");
        assert_eq!(values["custom"], "new");
        assert!(
            mock.command_log().iter().all(|c| !c.starts_with("rpm ")),
            "a vars-file releasever must skip the rpmdb probe: {:?}",
            mock.command_log()
        );
    }

    #[test]
    fn dnf5_hosts_read_the_dnf5_vars_dirs() {
        let mock = MockExecutor::new()
            .with_file(DNF5_BINARY, "")
            .with_dir("/etc/yum/vars", vec!["custom"])
            .with_file("/etc/yum/vars/custom", "dnf4-only\n")
            .with_dir("/usr/share/dnf5/vars.d", vec!["custom"])
            .with_file("/usr/share/dnf5/vars.d/custom", "dnf5\n");
        assert_eq!(detect(&mock)["custom"], "dnf5");
    }

    #[test]
    fn unknown_machine_leaves_basearch_undefined() {
        let mock = MockExecutor::new().with_command("uname -m", ok("vax\n"));
        let values = detect(&mock);
        assert!(!values.contains_key("basearch"));
        assert!(!values.contains_key("arch"));
    }

    #[test]
    fn nothing_is_read_for_values_without_variables() {
        let mock = MockExecutor::new();
        let v = DnfVars::from_host(&mock);
        assert_eq!(
            v.expand("/etc/pki/rpm-gpg/KEY").unwrap(),
            "/etc/pki/rpm-gpg/KEY"
        );
        assert!(mock.command_log().is_empty());
    }
}
