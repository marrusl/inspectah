//! `/etc/default/grub` against the running kernel command line.
//!
//! The file is never copied into the image: bootc does not run
//! `grub2-mkconfig`, and the scan already carries the running kernel's
//! operator arguments into kargs.d (`render::configtree::kargs_toml`). So
//! the file tells the user something only when `GRUB_CMDLINE_LINUX` or
//! `GRUB_CMDLINE_LINUX_DEFAULT` sets an argument the running kernel was not
//! booted with: an edit made for the next boot, which kargs.d does not
//! hold.
//!
//! The config inspector's modernization pass marks the file an advisory on
//! every host. It runs beside the kernel and boot inspector, so it cannot
//! see `/proc/cmdline`; [`settle_default_grub`] runs in
//! `finalize_cross_inspector`, where both sections are present and nothing
//! is redacted yet, and decides the row from the collected data.

use crate::render::safety::{KernelArg, is_bootloader_karg, kernel_args, kernel_name_eq};
use inspectah_collect::inspectors::modernization::{DEFAULT_GRUB_PATH, DEFAULT_GRUB_RATIONALE};
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::{AdvisoryType, FindingKind};
use std::iter::Peekable;
use std::str::Chars;

/// The variables `grub2-mkconfig` puts on a normal boot entry's command
/// line, in the order it writes them. `GRUB_CMDLINE_LINUX_DEFAULT` is
/// left off only the recovery entries, so the default entry the running
/// kernel booted from carries both.
const CMDLINE_VARS: [&str; 2] = ["GRUB_CMDLINE_LINUX", "GRUB_CMDLINE_LINUX_DEFAULT"];

/// Decide the `/etc/default/grub` config row, whatever disposition the
/// collector gave it, so the file is never carried:
///
/// - its arguments all run now: the row is removed, since there is nothing
///   to tell the user;
/// - it sets arguments the running kernel lacks: an advisory naming them,
///   never their values, which can be credentials;
/// - the comparison cannot be made (no running command line, no captured
///   content, or a file this pass cannot parse with confidence): the
///   modernization advisory as the collector writes it.
///
/// A row whose content was not captured keeps its `ContentNotCaptured`
/// advisory, which already keeps it out of the export.
pub(crate) fn settle_default_grub(snapshot: &mut InspectionSnapshot) {
    let running = snapshot
        .kernel_boot
        .as_ref()
        .map(|kb| kb.cmdline.clone())
        .filter(|cmdline| !cmdline.trim().is_empty());
    let Some(config) = snapshot.config.as_mut() else {
        return;
    };
    let Some(index) = config
        .files
        .iter()
        .position(|entry| entry.path == DEFAULT_GRUB_PATH)
    else {
        return;
    };
    let entry = &mut config.files[index];
    if entry.disposition.is_machine_state()
        || matches!(
            entry.disposition,
            FindingKind::Advisory {
                advisory_type: AdvisoryType::ContentNotCaptured,
                ..
            }
        )
    {
        return;
    }
    let not_running = running
        .as_deref()
        .filter(|_| !entry.content.is_empty())
        .and_then(|running| configured_not_running(&entry.content, running));
    match not_running {
        Some(names) if names.is_empty() => {
            config.files.remove(index);
        }
        Some(names) => {
            entry.disposition =
                FindingKind::advisory(AdvisoryType::Modernization, not_running_rationale(&names));
        }
        None => {
            entry.disposition =
                FindingKind::advisory(AdvisoryType::Modernization, DEFAULT_GRUB_RATIONALE);
        }
    }
}

/// The advisory for arguments `/etc/default/grub` sets that the running
/// kernel lacks.
fn not_running_rationale(names: &[String]) -> String {
    format!(
        "/etc/default/grub is not copied: an image-mode host does not run grub2-mkconfig, and \
         bootc reads kernel arguments from /usr/lib/bootc/kargs.d. GRUB_CMDLINE_LINUX or \
         GRUB_CMDLINE_LINUX_DEFAULT sets kernel arguments the running kernel was not booted \
         with, so the image does not carry them: {}. Put the ones the image needs in a \
         /usr/lib/bootc/kargs.d/*.toml file.",
        names.join(", ")
    )
}

/// The names of the arguments `grub` configures that `running` does not
/// have with the same value, in order and once each, or `None` when the
/// file cannot be parsed with confidence.
///
/// Both sides are read with [`kernel_args`] and compared by parsed name,
/// `-` equal to `_` as the kernel compares them, and value. A name the
/// boot loader or installer owns ([`is_bootloader_karg`]) is skipped: the
/// kargs.d writer never carries one, so naming it would ask the user to
/// do what the image must not. A name is shown escaped, so a control
/// character in it cannot start a new line where the advisory is printed
/// as a comment.
fn configured_not_running(grub: &str, running: &str) -> Option<Vec<String>> {
    let configured = cmdline_vars(grub)?.join(" ");
    let running = kernel_args(running);
    let mut names: Vec<String> = Vec::new();
    for arg in kernel_args(&configured) {
        if arg.name().is_empty()
            || is_bootloader_karg(&arg)
            || running.iter().any(|run| same_arg(run, &arg))
        {
            continue;
        }
        let name = arg.name().escape_debug().to_string();
        if !names.contains(&name) {
            names.push(name);
        }
    }
    Some(names)
}

/// Whether two arguments set the same parameter to the same value.
fn same_arg(a: &KernelArg<'_>, b: &KernelArg<'_>) -> bool {
    kernel_name_eq(a.name(), b.name()) && a.value() == b.value()
}

// ---------------------------------------------------------------------------
// The shell subset /etc/default/grub is written in
// ---------------------------------------------------------------------------

/// The values `/etc/default/grub` gives [`CMDLINE_VARS`] when the shell
/// sources it, or `None` when this reader cannot be sure.
///
/// A real file is a list of plain assignments, so that is all this
/// accepts: each command must be assignments only, optionally after
/// `export`. Values may be double-quoted, single-quoted, unquoted or a mix,
/// and a later assignment replaces an earlier one. A tracked value may
/// refer to either tracked variable (`"$GRUB_CMDLINE_LINUX audit=1"`), and
/// gets its value so far. Anything else that could change a tracked value
/// makes the file unreadable: a command (`.`, `source`, `if`), `+=`, a
/// tracked value with command substitution or another variable, an
/// unterminated quote, or an untracked assignment whose text names a
/// tracked variable (`${GRUB_CMDLINE_LINUX:=...}` assigns it). An
/// untracked value may hold anything else, since `GRUB_DISTRIBUTOR` holds
/// a command substitution on every RHEL and Fedora host.
fn cmdline_vars(text: &str) -> Option<[String; 2]> {
    let mut values = [String::new(), String::new()];
    for command in commands(text)? {
        let exported = command.first().is_some_and(|word| word.raw == "export");
        for word in command.iter().skip(usize::from(exported)) {
            match assignment(word) {
                Some((name, value)) => {
                    if let Some(index) = CMDLINE_VARS.iter().position(|var| *var == name) {
                        values[index] = expand(&value, &values)?;
                    } else if word.raw.contains(CMDLINE_VARS[0]) {
                        return None;
                    }
                }
                None if exported && is_name(&word.raw) => {}
                None => return None,
            }
        }
    }
    Some(values)
}

/// One part of a shell word, after quote removal.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Piece {
    Text(String),
    /// `$NAME` or `${NAME}`.
    Var(String),
    /// An expansion this reader does not evaluate: command substitution,
    /// a parameter operator, a positional or special parameter, or a
    /// shell operator character.
    Unsupported,
}

/// A shell word: its source text and its pieces.
#[derive(Debug, Default)]
struct Word {
    raw: String,
    pieces: Vec<Piece>,
}

impl Word {
    fn push_text(&mut self, c: char) {
        if let Some(Piece::Text(text)) = self.pieces.last_mut() {
            text.push(c);
        } else {
            self.pieces.push(Piece::Text(c.to_string()));
        }
    }
}

/// The split of `name=value` when `word` is an assignment: an unquoted
/// shell name, then `=`. The name and `=` are the start of the first text
/// piece, since they are unquoted.
fn assignment(word: &Word) -> Option<(String, Vec<Piece>)> {
    let (name, _) = word.raw.split_once('=')?;
    if !is_name(name) {
        return None;
    }
    let mut pieces = word.pieces.clone();
    match pieces.first_mut() {
        Some(Piece::Text(text)) => {
            text.drain(..=name.len());
        }
        _ => return None,
    }
    Some((name.to_string(), pieces))
}

/// A shell variable name.
fn is_name(s: &str) -> bool {
    let mut chars = s.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// A tracked value, with references to tracked variables replaced by
/// their values so far; `None` for anything else.
fn expand(pieces: &[Piece], values: &[String; 2]) -> Option<String> {
    let mut out = String::new();
    for piece in pieces {
        match piece {
            Piece::Text(text) => out.push_str(text),
            Piece::Var(name) => {
                let index = CMDLINE_VARS.iter().position(|var| var == name)?;
                out.push_str(&values[index]);
            }
            Piece::Unsupported => return None,
        }
    }
    Some(out)
}

/// Split `text` into commands of words, the way `sh` reads a file of
/// simple commands: unquoted newline and `;` end a command, unquoted
/// blanks end a word, `#` at the start of a word starts a comment, and a
/// backslash before a newline joins the lines. `None` for an
/// unterminated quote or `${`.
fn commands(text: &str) -> Option<Vec<Vec<Word>>> {
    let mut chars = text.chars().peekable();
    let mut commands = Vec::new();
    let mut command: Vec<Word> = Vec::new();
    let mut word: Option<Word> = None;
    while let Some(c) = chars.next() {
        match c {
            ' ' | '\t' => command.extend(word.take()),
            '\n' | ';' => {
                command.extend(word.take());
                commands.push(std::mem::take(&mut command));
            }
            '#' if word.is_none() => {
                for skipped in chars.by_ref() {
                    if skipped == '\n' {
                        break;
                    }
                }
                commands.push(std::mem::take(&mut command));
            }
            '\\' => match chars.next() {
                Some('\n') => {}
                Some(escaped) => {
                    let w = word.get_or_insert_default();
                    w.raw.push(c);
                    w.raw.push(escaped);
                    w.push_text(escaped);
                }
                None => {
                    let w = word.get_or_insert_default();
                    w.raw.push(c);
                    w.push_text(c);
                }
            },
            '\'' => {
                let w = word.get_or_insert_default();
                w.raw.push(c);
                w.pieces.push(Piece::Text(String::new()));
                loop {
                    let quoted = chars.next()?;
                    w.raw.push(quoted);
                    if quoted == '\'' {
                        break;
                    }
                    w.push_text(quoted);
                }
            }
            '"' => {
                let w = word.get_or_insert_default();
                w.raw.push(c);
                w.pieces.push(Piece::Text(String::new()));
                double_quoted(&mut chars, w)?;
            }
            '$' => {
                let w = word.get_or_insert_default();
                w.raw.push(c);
                expansion(&mut chars, w)?;
            }
            '`' | '|' | '&' | '<' | '>' | '(' | ')' => {
                let w = word.get_or_insert_default();
                w.raw.push(c);
                w.pieces.push(Piece::Unsupported);
            }
            other => {
                let w = word.get_or_insert_default();
                w.raw.push(other);
                w.push_text(other);
            }
        }
    }
    command.extend(word);
    commands.push(command);
    Some(commands.into_iter().filter(|c| !c.is_empty()).collect())
}

/// The rest of a double-quoted string, its closing quote included. A
/// backslash escapes only `$`, `` ` ``, `"`, `\` and a newline, as in `sh`.
fn double_quoted(chars: &mut Peekable<Chars<'_>>, w: &mut Word) -> Option<()> {
    loop {
        let c = chars.next()?;
        w.raw.push(c);
        match c {
            '"' => return Some(()),
            '\\' => match chars.peek() {
                Some('\n') => {
                    chars.next();
                }
                Some(&escaped @ ('$' | '`' | '"' | '\\')) => {
                    chars.next();
                    w.raw.push(escaped);
                    w.push_text(escaped);
                }
                _ => w.push_text(c),
            },
            '$' => expansion(chars, w)?,
            '`' => w.pieces.push(Piece::Unsupported),
            other => w.push_text(other),
        }
    }
}

/// What follows a `$`: a variable reference, an expansion this reader
/// does not evaluate, or a literal `$` when nothing an expansion starts
/// with follows.
fn expansion(chars: &mut Peekable<Chars<'_>>, w: &mut Word) -> Option<()> {
    match chars.peek().copied() {
        Some('{') => {
            chars.next();
            w.raw.push('{');
            let mut inner = String::new();
            loop {
                let c = chars.next()?;
                w.raw.push(c);
                if c == '}' {
                    break;
                }
                inner.push(c);
            }
            w.pieces.push(if is_name(&inner) {
                Piece::Var(inner)
            } else {
                Piece::Unsupported
            });
        }
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {
            let mut name = String::new();
            while let Some(&c) = chars.peek() {
                if !(c.is_ascii_alphanumeric() || c == '_') {
                    break;
                }
                chars.next();
                w.raw.push(c);
                name.push(c);
            }
            w.pieces.push(Piece::Var(name));
        }
        Some(c) if c.is_ascii_digit() || "($?#@*!-".contains(c) => {
            chars.next();
            w.raw.push(c);
            w.pieces.push(Piece::Unsupported);
        }
        _ => w.push_text('$'),
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::types::config::{ConfigFileEntry, ConfigFileKind, ConfigSection};
    use inspectah_core::types::kernelboot::KernelBootSection;

    /// The command line a stock RHEL 9 host on LVM boots with.
    const RUNNING: &str = "BOOT_IMAGE=(hd0,gpt2)/vmlinuz-5.14.0 root=/dev/mapper/rhel-root ro \
         crashkernel=1G-4G:192M,4G-64G:256M,64G-:512M resume=/dev/mapper/rhel-swap \
         rd.lvm.lv=rhel/root rd.lvm.lv=rhel/swap rhgb quiet";

    /// A stock RHEL 9 `/etc/default/grub`, with `GRUB_CMDLINE_LINUX` as
    /// `{}` for each case to fill.
    const STOCK: &str = "GRUB_TIMEOUT=5\n\
         GRUB_DISTRIBUTOR=\"$(sed 's, release .*$,,g' /etc/system-release)\"\n\
         GRUB_DEFAULT=saved\n\
         GRUB_DISABLE_SUBMENU=true\n\
         GRUB_TERMINAL_OUTPUT=\"console\"\n\
         GRUB_CMDLINE_LINUX=\"crashkernel=1G-4G:192M,4G-64G:256M,64G-:512M \
         resume=/dev/mapper/rhel-swap rd.lvm.lv=rhel/root rd.lvm.lv=rhel/swap rhgb quiet\"\n\
         GRUB_DISABLE_RECOVERY=\"true\"\n\
         GRUB_ENABLE_BLSCFG=true\n";

    /// Each file's names configured but not running, against `running`,
    /// or `None` for a file read as unparseable.
    #[test]
    fn configured_arguments_compare_with_the_running_ones() {
        type Case<'a> = (&'a str, &'a str, &'a str, Option<&'a [&'a str]>);
        let cases: &[Case] = &[
            ("stock file, as booted", STOCK, RUNNING, Some(&[])),
            (
                "stock file plus an edit after it",
                &format!("{STOCK}GRUB_CMDLINE_LINUX=\"$GRUB_CMDLINE_LINUX audit=1\"\n"),
                RUNNING,
                Some(&["audit"]),
            ),
            (
                "double quoted",
                "GRUB_CMDLINE_LINUX=\"rhgb quiet audit=1 nosmt\"",
                "ro quiet nosmt",
                Some(&["audit"]),
            ),
            (
                "single quoted",
                "GRUB_CMDLINE_LINUX='audit=1 nosmt'",
                "audit=1",
                Some(&["nosmt"]),
            ),
            (
                "unquoted",
                "GRUB_CMDLINE_LINUX=audit=1",
                "quiet",
                Some(&["audit"]),
            ),
            (
                "mixed quoting in one word",
                "GRUB_CMDLINE_LINUX=\"audit=1 \"'nosmt 'mitigations=off",
                "audit=1 nosmt",
                Some(&["mitigations"]),
            ),
            (
                "a later assignment overrides",
                "GRUB_CMDLINE_LINUX=\"audit=1\"\nGRUB_CMDLINE_LINUX=\"nosmt\"",
                "nosmt",
                Some(&[]),
            ),
            (
                "self-reference appends",
                "GRUB_CMDLINE_LINUX=\"quiet\"\nGRUB_CMDLINE_LINUX=\"${GRUB_CMDLINE_LINUX} audit=1\"",
                "quiet",
                Some(&["audit"]),
            ),
            (
                "_DEFAULT is on the booted entry",
                "GRUB_CMDLINE_LINUX=\"quiet\"\nGRUB_CMDLINE_LINUX_DEFAULT=\"mitigations=off\"",
                "quiet",
                Some(&["mitigations"]),
            ),
            (
                "a different value is not running",
                "GRUB_CMDLINE_LINUX=\"audit=1\"",
                "audit=0",
                Some(&["audit"]),
            ),
            (
                "- and _ name one parameter",
                "GRUB_CMDLINE_LINUX=\"transparent-hugepage=never\"",
                "transparent_hugepage=never",
                Some(&[]),
            ),
            (
                "a quoted value compares as the kernel parses it",
                "GRUB_CMDLINE_LINUX=\"dyndbg=\\\"file x.c +p\\\"\"",
                "dyndbg=\"file x.c +p\"",
                Some(&[]),
            ),
            (
                "boot loader names are never named",
                "GRUB_CMDLINE_LINUX=\"root=/dev/sdb1 rd.lvm.lv=other/root rhgb\"",
                "ro",
                Some(&[]),
            ),
            (
                "each name once",
                "GRUB_CMDLINE_LINUX=\"console=ttyS0 isolcpus=1 isolcpus=2\"",
                "ro",
                Some(&["isolcpus"]),
            ),
            (
                "export, comments and a continued line",
                "# GRUB_CMDLINE_LINUX=\"ignored=1\"\nexport GRUB_CMDLINE_LINUX=\"quiet \\\n  audit=1\" # set by hand\nexport GRUB_CMDLINE_LINUX",
                "quiet",
                Some(&["audit"]),
            ),
            (
                "a credential is named, never its value",
                "GRUB_CMDLINE_LINUX=\"quiet rd.iscsi.password=Sup3rS3cret\"",
                "quiet",
                Some(&["rd.iscsi.password"]),
            ),
            ("no tracked variable", "GRUB_TIMEOUT=5", "quiet", Some(&[])),
            (
                "command substitution in a tracked value",
                "GRUB_CMDLINE_LINUX=\"$(cat /etc/kargs)\"",
                "quiet",
                None,
            ),
            (
                "another variable in a tracked value",
                "GRUB_CMDLINE_LINUX=\"$EXTRA audit=1\"",
                "quiet",
                None,
            ),
            (
                "a sourced file",
                ". /etc/default/grub.local\nGRUB_CMDLINE_LINUX=\"audit=1\"",
                "audit=1",
                None,
            ),
            (
                "a conditional",
                "if true; then GRUB_CMDLINE_LINUX=\"audit=1\"; fi",
                "audit=1",
                None,
            ),
            (
                "an append operator",
                "GRUB_CMDLINE_LINUX+=\" audit=1\"",
                "quiet",
                None,
            ),
            (
                "an untracked assignment that assigns a tracked one",
                "X=${GRUB_CMDLINE_LINUX:=audit=1}",
                "quiet",
                None,
            ),
            (
                "an unterminated quote",
                "GRUB_CMDLINE_LINUX=\"audit=1",
                "audit=1",
                None,
            ),
            ("an array", "GRUB_CMDLINE_LINUX=(audit=1)", "audit=1", None),
        ];
        for (label, grub, running, want) in cases {
            let got = configured_not_running(grub, running);
            let want: Option<Vec<String>> =
                want.map(|names| names.iter().map(|n| n.to_string()).collect());
            assert_eq!(got, want, "{label}");
        }
    }

    /// A control character in a name is escaped in the advisory.
    #[test]
    fn a_name_is_shown_escaped() {
        assert_eq!(
            configured_not_running("GRUB_CMDLINE_LINUX='\"a\nb\"=1'", "quiet"),
            Some(vec!["a\\nb\\\"".to_string()])
        );
    }

    fn snapshot(grub: ConfigFileEntry, cmdline: Option<&str>) -> InspectionSnapshot {
        InspectionSnapshot {
            config: Some(ConfigSection {
                files: vec![grub],
                ..Default::default()
            }),
            kernel_boot: cmdline.map(|cmdline| KernelBootSection {
                cmdline: cmdline.into(),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn grub_entry(content: &str, disposition: FindingKind) -> ConfigFileEntry {
        ConfigFileEntry {
            path: DEFAULT_GRUB_PATH.into(),
            kind: ConfigFileKind::RpmOwnedModified,
            content: content.into(),
            disposition,
            ..Default::default()
        }
    }

    /// The row the pass leaves, by case. Every row it leaves is an
    /// advisory, whatever the collector gave it, so the file is never
    /// carried, even when the modernization pass did not run.
    #[test]
    fn the_grub_row_is_settled_against_the_running_command_line() {
        let modernization =
            FindingKind::advisory(AdvisoryType::Modernization, DEFAULT_GRUB_RATIONALE);
        let not_captured = FindingKind::advisory(AdvisoryType::ContentNotCaptured, "too large");
        let edited = "GRUB_CMDLINE_LINUX=\"quiet rd.iscsi.password=Sup3rS3cret audit=1\"\n";
        let generic = Some(modernization.clone());
        type Case<'a> = (
            &'a str,
            &'a str,
            FindingKind,
            Option<&'a str>,
            Option<FindingKind>,
        );
        let cases: Vec<Case> = vec![
            (
                "matches: no row",
                "GRUB_CMDLINE_LINUX=\"quiet\"\n",
                modernization.clone(),
                Some("ro quiet"),
                None,
            ),
            (
                "matches, modernization did not run: no row",
                "GRUB_CMDLINE_LINUX=\"quiet\"\n",
                FindingKind::included(),
                Some("ro quiet"),
                None,
            ),
            (
                "no running command line",
                edited,
                modernization.clone(),
                None,
                generic.clone(),
            ),
            (
                "an empty running command line",
                edited,
                modernization.clone(),
                Some(""),
                generic.clone(),
            ),
            (
                "unparseable, modernization did not run",
                "GRUB_CMDLINE_LINUX=\"$(cat x)\"\n",
                FindingKind::included(),
                Some("ro quiet"),
                generic.clone(),
            ),
            (
                "no captured content",
                "",
                modernization.clone(),
                Some("ro quiet"),
                generic.clone(),
            ),
            (
                "content not captured keeps its advisory",
                "",
                not_captured.clone(),
                Some("ro quiet"),
                Some(not_captured.clone()),
            ),
        ];
        for (label, content, disposition, cmdline, want) in cases {
            let mut snap = snapshot(grub_entry(content, disposition), cmdline);
            settle_default_grub(&mut snap);
            let got = snap
                .config
                .unwrap()
                .files
                .first()
                .map(|e| e.disposition.clone());
            assert_eq!(got, want, "{label}");
        }
    }

    /// An argument set in the file and not running is named; its value,
    /// which here is a credential, appears nowhere in the row's finding.
    #[test]
    fn the_advisory_names_arguments_and_never_values() {
        let mut snap = snapshot(
            grub_entry(
                "GRUB_CMDLINE_LINUX=\"quiet rd.iscsi.password=Sup3rS3cret audit=1\"\n",
                FindingKind::included(),
            ),
            Some("ro quiet audit=1"),
        );
        settle_default_grub(&mut snap);
        let files = snap.config.unwrap().files;
        match &files[0].disposition {
            FindingKind::Advisory {
                advisory_type: AdvisoryType::Modernization,
                rationale,
            } => {
                assert!(
                    rationale.contains("so the image does not carry them: rd.iscsi.password."),
                    "{rationale}"
                );
                assert!(!rationale.contains("Sup3rS3cret"), "{rationale}");
                assert!(!rationale.contains("audit"), "{rationale}");
                assert!(rationale.contains("kargs.d"), "{rationale}");
            }
            other => panic!("expected a modernization advisory, got {other:?}"),
        }
    }

    /// No row, no change: a host without the file.
    #[test]
    fn a_host_without_the_file_is_left_alone() {
        let other = ConfigFileEntry {
            path: "/etc/chrony.conf".into(),
            ..Default::default()
        };
        let mut snap = snapshot(other.clone(), Some("ro quiet"));
        settle_default_grub(&mut snap);
        assert_eq!(snap.config.unwrap().files, vec![other]);
    }
}
