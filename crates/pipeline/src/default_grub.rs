//! `/etc/default/grub` against what the scan carries in kargs.d.
//!
//! The file is never copied into the image: bootc does not run
//! `grub2-mkconfig`, and the scan already carries the running kernel's
//! operator arguments into kargs.d (`render::configtree::kargs_toml`). So
//! the file tells the user something only when `GRUB_CMDLINE_LINUX` or
//! `GRUB_CMDLINE_LINUX_DEFAULT` sets an argument kargs.d will not hold:
//! one the running kernel was not booted with, such as an edit made for
//! the next boot, or one the kargs.d writer leaves out.
//!
//! The config inspector's modernization pass marks the file an advisory on
//! every host. It runs beside the kernel and boot inspector, so it cannot
//! see `/proc/cmdline`; [`settle_default_grub`] runs in
//! `finalize_cross_inspector`, where both sections are present and nothing
//! is redacted yet, and decides the row from the collected data.

use crate::redaction::engine::redacted_kernel_cmdline;
use crate::render::configtree::split_operator_kargs;
use crate::render::safety::{KernelArg, is_bootloader_karg, kernel_args, kernel_name_eq};
use inspectah_collect::inspectors::modernization::{DEFAULT_GRUB_PATH, DEFAULT_GRUB_RATIONALE};
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::config::ConfigFileEntry;
use inspectah_core::types::destination::Destination;
use inspectah_core::types::{AdvisoryType, FindingKind};
use std::iter::Peekable;
use std::str::Chars;

/// The variables `grub2-mkconfig` puts on a normal boot entry's command
/// line, in the order it writes them. `GRUB_CMDLINE_LINUX_DEFAULT` is
/// left off only the recovery entries, so the default entry the running
/// kernel booted from carries both.
const CMDLINE_VARS: [&str; 2] = ["GRUB_CMDLINE_LINUX", "GRUB_CMDLINE_LINUX_DEFAULT"];

/// The advisory when `/etc/default/grub` sets kernel arguments kargs.d
/// does not carry. Fixed text: nothing read from the file reaches it, so
/// a value the reader took for a name cannot either, and a value can be a
/// credential.
const NOT_CARRIED_RATIONALE: &str = "/etc/default/grub is not copied: an image-mode host does \
     not run grub2-mkconfig. inspectah carries the running kernel's arguments into \
     /usr/lib/bootc/kargs.d, but GRUB_CMDLINE_LINUX or GRUB_CMDLINE_LINUX_DEFAULT sets kernel \
     arguments that are not among them: ones the running kernel was not booted with, or ones \
     the scan cannot carry. Put any argument the image needs from this file in a \
     /usr/lib/bootc/kargs.d/*.toml file by hand.";

/// How the kernel arguments `/etc/default/grub` sets compare with the ones
/// the kargs.d writer carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Comparison {
    /// kargs.d carries every argument the file sets.
    Carried,
    /// The file sets an argument kargs.d does not carry.
    NotCarried,
    /// The comparison cannot be made: no running command line, no
    /// captured content, or a file this reader cannot be sure of.
    Unknown,
}

/// Decide each `/etc/default/grub` config row, whatever disposition the
/// collector gave it, so the file is never carried:
///
/// - kargs.d carries every argument it sets: the row is removed, since
///   there is nothing to tell the user;
/// - it sets arguments kargs.d does not carry: an advisory in fixed text
///   ([`NOT_CARRIED_RATIONALE`]);
/// - the comparison cannot be made: the modernization advisory as the
///   collector writes it.
///
/// A row is matched by its normalized destination, so the OSTree
/// collector's `etc/default/grub` is one too. A row whose content was not
/// captured keeps its `ContentNotCaptured` advisory, and a machine-state
/// row its class, which already keep it out of the export.
pub(crate) fn settle_default_grub(snapshot: &mut InspectionSnapshot) {
    let running = snapshot
        .kernel_boot
        .as_ref()
        .map(|kb| kb.cmdline.as_str())
        .filter(|cmdline| !cmdline.trim().is_empty());
    let Some(config) = snapshot.config.as_mut() else {
        return;
    };
    config.files.retain_mut(|entry| {
        if !is_default_grub(entry) || keeps_its_disposition(entry) {
            return true;
        }
        let comparison = match running {
            Some(running) if !entry.content.is_empty() => compare(&entry.content, running),
            _ => Comparison::Unknown,
        };
        let rationale = match comparison {
            Comparison::Carried => return false,
            Comparison::NotCarried => NOT_CARRIED_RATIONALE,
            Comparison::Unknown => DEFAULT_GRUB_RATIONALE,
        };
        entry.disposition = FindingKind::advisory(AdvisoryType::Modernization, rationale);
        true
    });
}

/// Whether `entry` is `/etc/default/grub`, however the collector spelled
/// the path.
fn is_default_grub(entry: &ConfigFileEntry) -> bool {
    Destination::new(&entry.path).is_ok_and(|dest| dest.as_str() == DEFAULT_GRUB_PATH)
}

/// Whether `entry`'s disposition already keeps it out of the export for a
/// reason of its own, which this pass leaves in place.
fn keeps_its_disposition(entry: &ConfigFileEntry) -> bool {
    entry.disposition.is_machine_state()
        || matches!(
            entry.disposition,
            FindingKind::Advisory {
                advisory_type: AdvisoryType::ContentNotCaptured,
                ..
            }
        )
}

/// Whether the kargs.d writer carries every argument `grub` configures,
/// for a host booted with `running`.
///
/// The carried arguments are the writer's own: `running` as redaction
/// leaves it ([`redacted_kernel_cmdline`]), through the writer's split
/// ([`split_operator_kargs`]), so an argument the writer drops (a control
/// character, a redacted credential, a boot loader name) is never one the
/// file can match. Arguments are compared by parsed name, `-` equal to
/// `_` as the kernel compares them, and value. A name the boot loader or
/// installer owns ([`is_bootloader_karg`]) is skipped on the file's side:
/// the writer never carries one, and the image must not set one.
fn compare(grub: &str, running: &str) -> Comparison {
    let Some(values) = cmdline_vars(grub) else {
        return Comparison::Unknown;
    };
    let configured = values.join(" ");
    let redacted = redacted_kernel_cmdline(running);
    let (carried, _) = split_operator_kargs(&redacted);
    let all_carried = kernel_args(&configured).iter().all(|arg| {
        arg.name().is_empty()
            || is_bootloader_karg(arg)
            || carried.iter().any(|run| same_arg(run, arg))
    });
    if all_carried {
        Comparison::Carried
    } else {
        Comparison::NotCarried
    }
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
/// The reader accepts an allow-list of line forms, the ones a real file
/// is written in, and nothing else:
///
/// - a blank line or a comment;
/// - one assignment, `NAME=value`;
/// - `export NAME=value` or `export NAME`, with that one argument.
///
/// A comment may end a line, and a backslash before a newline joins two.
/// Values may be double-quoted, single-quoted, unquoted or a mix, and a
/// later assignment replaces an earlier one. A tracked value may refer to
/// either tracked variable (`"$GRUB_CMDLINE_LINUX audit=1"`), and gets its
/// value so far. Any other line makes the file unreadable: a command
/// (`.`, `source`, `if`, `declare`), more than one word, `+=`, and any
/// line holding a construct the reader does not model ([`lines`]). A
/// tracked value holding any expansion but a tracked variable does too.
/// An untracked value may also hold another variable, a special parameter
/// or a plain command substitution inside double quotes, since
/// `GRUB_DISTRIBUTOR` holds one on every RHEL and Fedora host; a
/// substitution runs in a subshell and cannot assign a tracked variable.
fn cmdline_vars(text: &str) -> Option<[String; 2]> {
    let mut values = [String::new(), String::new()];
    for line in lines(text)? {
        let word = match line.as_slice() {
            [] => continue,
            [export, name] if export.raw == "export" && is_name(&name.raw) => continue,
            [export, word] if export.raw == "export" => word,
            [word] => word,
            _ => return None,
        };
        let (name, value) = assignment(word)?;
        if let Some(index) = CMDLINE_VARS.iter().position(|var| *var == name) {
            values[index] = expand(&value, &values)?;
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
    /// An expansion this reader does not evaluate: a special or positional
    /// parameter, or a command substitution.
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

/// Unquoted characters with a shell meaning this reader does not model:
/// the operators, which start another command, a pipeline, a redirection
/// or a subshell; a backtick, which starts a command substitution whose
/// end depends on backslash rules; `~`, which an assignment expands to a
/// home directory; and the glob and brace characters, which `export`'s
/// argument may expand.
const UNMODELED_UNQUOTED: &[char] = &[
    ';', '&', '|', '<', '>', '(', ')', '`', '~', '{', '}', '*', '?', '[', ']',
];

/// Split `text` into lines of words, the way `sh` reads a file of simple
/// commands, or `None` for any construct the reader does not model.
///
/// Unquoted blanks end a word, an unquoted newline ends a line, `#` at the
/// start of a word starts a comment, and a backslash before a newline
/// joins the lines. Single quotes, double quotes ([`double_quoted`]) and
/// `$` expansions ([`expansion`]) are read as the shell reads them. Any of
/// [`UNMODELED_UNQUOTED`], `$'...'` and `$"..."` quoting, any other
/// control character, an unterminated quote and a backtick anywhere make
/// the file unreadable.
fn lines(text: &str) -> Option<Vec<Vec<Word>>> {
    let mut chars = text.chars().peekable();
    let mut lines = Vec::new();
    let mut line: Vec<Word> = Vec::new();
    let mut word: Option<Word> = None;
    while let Some(c) = chars.next() {
        match c {
            ' ' | '\t' => line.extend(word.take()),
            '\n' => {
                line.extend(word.take());
                lines.push(std::mem::take(&mut line));
            }
            '#' if word.is_none() => {
                for skipped in chars.by_ref() {
                    if skipped == '\n' {
                        break;
                    }
                }
                lines.push(std::mem::take(&mut line));
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
                expansion(&mut chars, w, false)?;
            }
            other if UNMODELED_UNQUOTED.contains(&other) || other.is_control() => return None,
            other => {
                let w = word.get_or_insert_default();
                w.raw.push(other);
                w.push_text(other);
            }
        }
    }
    line.extend(word);
    lines.push(line);
    Some(lines)
}

/// The rest of a double-quoted string, its closing quote included. A
/// backslash escapes only `$`, `` ` ``, `"`, `\` and a newline, as in `sh`.
/// A backtick makes the file unreadable.
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
            '$' => expansion(chars, w, true)?,
            '`' => return None,
            other => w.push_text(other),
        }
    }
}

/// What follows a `$`, in double quotes when `quoted`: a variable
/// reference, a special parameter, a command substitution in double
/// quotes ([`command_substitution`]), or a literal `$` when nothing an
/// expansion starts with follows. `None` for `${` holding more than a
/// name, `$[`, an unquoted `$(`, and unquoted `$'` or `$"`.
fn expansion(chars: &mut Peekable<Chars<'_>>, w: &mut Word, quoted: bool) -> Option<()> {
    match chars.peek().copied() {
        Some('{') => {
            chars.next();
            w.raw.push('{');
            let name = name_chars(chars, w);
            if !is_name(&name) || chars.next()? != '}' {
                return None;
            }
            w.raw.push('}');
            w.pieces.push(Piece::Var(name));
        }
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {
            let name = name_chars(chars, w);
            w.pieces.push(Piece::Var(name));
        }
        Some(c) if c.is_ascii_digit() || "$?#@*!-".contains(c) => {
            chars.next();
            w.raw.push(c);
            w.pieces.push(Piece::Unsupported);
        }
        Some('(') if quoted => {
            chars.next();
            w.raw.push('(');
            command_substitution(chars, w)?;
            w.pieces.push(Piece::Unsupported);
        }
        Some('(' | '[') => return None,
        Some('\'' | '"') if !quoted => return None,
        _ => w.push_text('$'),
    }
    Some(())
}

/// The shell name characters that follow, consumed into `w.raw`.
fn name_chars(chars: &mut Peekable<Chars<'_>>, w: &mut Word) -> String {
    let mut name = String::new();
    while let Some(&c) = chars.peek() {
        if !(c.is_ascii_alphanumeric() || c == '_') {
            break;
        }
        chars.next();
        w.raw.push(c);
        name.push(c);
    }
    name
}

/// Characters a word of a command substitution may hold outside single
/// quotes, besides ASCII letters and digits.
const SUBSTITUTION_WORD_CHARS: &str = "_-.,/:=+@%^";

/// The shell's reserved words. One opens a compound command, and `case`
/// patterns end in a `)` that does not end a command substitution.
const RESERVED_WORDS: &[&str] = &[
    "case", "coproc", "do", "done", "elif", "else", "esac", "fi", "for", "function", "if", "in",
    "select", "then", "time", "until", "while",
];

/// A command substitution inside double quotes, after its `$(`, read to
/// and including its `)`.
///
/// The shell runs it in a subshell, so it cannot assign a tracked
/// variable; what matters is that this reader ends it where the shell
/// does. So it accepts only one plain command, which ends at the first
/// `)` outside single quotes: words of letters, digits and
/// [`SUBSTITUTION_WORD_CHARS`], and single-quoted strings, with no
/// unquoted word in [`RESERVED_WORDS`]. Anything else, an operator, a
/// newline, another quote or expansion, is `None`. The stock
/// `"$(sed 's, release .*$,,g' /etc/system-release)"` reads.
fn command_substitution(chars: &mut Peekable<Chars<'_>>, w: &mut Word) -> Option<()> {
    let mut word = String::new();
    let mut word_quoted = false;
    loop {
        let c = chars.next()?;
        w.raw.push(c);
        match c {
            ')' | ' ' | '\t' => {
                if !word_quoted && RESERVED_WORDS.contains(&word.as_str()) {
                    return None;
                }
                if c == ')' {
                    return Some(());
                }
                word.clear();
                word_quoted = false;
            }
            '\'' => {
                word_quoted = true;
                loop {
                    let quoted = chars.next()?;
                    w.raw.push(quoted);
                    match quoted {
                        '\'' => break,
                        '\n' => return None,
                        _ => {}
                    }
                }
            }
            c if c.is_ascii_alphanumeric() || SUBSTITUTION_WORD_CHARS.contains(c) => word.push(c),
            _ => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Comparison::{Carried, NotCarried, Unknown};
    use inspectah_core::types::config::{ConfigFileKind, ConfigSection};
    use inspectah_core::types::kernelboot::KernelBootSection;

    /// The command line a stock RHEL 9 host on LVM boots with.
    const RUNNING: &str = "BOOT_IMAGE=(hd0,gpt2)/vmlinuz-5.14.0 root=/dev/mapper/rhel-root ro \
         crashkernel=1G-4G:192M,4G-64G:256M,64G-:512M resume=/dev/mapper/rhel-swap \
         rd.lvm.lv=rhel/root rd.lvm.lv=rhel/swap rhgb quiet";

    /// A stock RHEL 9 `/etc/default/grub`.
    const STOCK: &str = "GRUB_TIMEOUT=5\n\
         GRUB_DISTRIBUTOR=\"$(sed 's, release .*$,,g' /etc/system-release)\"\n\
         GRUB_DEFAULT=saved\n\
         GRUB_DISABLE_SUBMENU=true\n\
         GRUB_TERMINAL_OUTPUT=\"console\"\n\
         GRUB_CMDLINE_LINUX=\"crashkernel=1G-4G:192M,4G-64G:256M,64G-:512M \
         resume=/dev/mapper/rhel-swap rd.lvm.lv=rhel/root rd.lvm.lv=rhel/swap rhgb quiet\"\n\
         GRUB_DISABLE_RECOVERY=\"true\"\n\
         GRUB_ENABLE_BLSCFG=true\n";

    /// Each file against `running`.
    #[test]
    fn configured_arguments_compare_with_the_carried_ones() {
        let cases: &[(&str, &str, &str, Comparison)] = &[
            ("stock file, as booted", STOCK, RUNNING, Carried),
            (
                "stock file plus an edit after it",
                &format!("{STOCK}GRUB_CMDLINE_LINUX=\"$GRUB_CMDLINE_LINUX audit=1\"\n"),
                RUNNING,
                NotCarried,
            ),
            (
                "double quoted",
                "GRUB_CMDLINE_LINUX=\"rhgb quiet audit=1 nosmt\"",
                "ro quiet nosmt",
                NotCarried,
            ),
            (
                "single quoted",
                "GRUB_CMDLINE_LINUX='audit=1 nosmt'",
                "audit=1",
                NotCarried,
            ),
            (
                "unquoted",
                "GRUB_CMDLINE_LINUX=audit=1",
                "quiet",
                NotCarried,
            ),
            (
                "mixed quoting in one word",
                "GRUB_CMDLINE_LINUX=\"audit=1 \"'nosmt 'mitigations=off",
                "audit=1 nosmt mitigations=off",
                Carried,
            ),
            (
                "a later assignment overrides",
                "GRUB_CMDLINE_LINUX=\"audit=1\"\nGRUB_CMDLINE_LINUX=\"nosmt\"",
                "nosmt",
                Carried,
            ),
            (
                "self-reference appends",
                "GRUB_CMDLINE_LINUX=\"quiet\"\nGRUB_CMDLINE_LINUX=\"${GRUB_CMDLINE_LINUX} audit=1\"",
                "quiet",
                NotCarried,
            ),
            (
                "_DEFAULT is on the booted entry",
                "GRUB_CMDLINE_LINUX=\"quiet\"\nGRUB_CMDLINE_LINUX_DEFAULT=\"mitigations=off\"",
                "quiet",
                NotCarried,
            ),
            (
                "a different value is not running",
                "GRUB_CMDLINE_LINUX=\"audit=1\"",
                "audit=0",
                NotCarried,
            ),
            (
                "- and _ name one parameter",
                "GRUB_CMDLINE_LINUX=\"transparent-hugepage=never\"",
                "transparent_hugepage=never",
                Carried,
            ),
            (
                "a quoted value compares as the kernel parses it",
                "GRUB_CMDLINE_LINUX=\"dyndbg=\\\"file x.c +p\\\"\"",
                "dyndbg=\"file x.c +p\"",
                Carried,
            ),
            (
                "boot loader names are skipped",
                "GRUB_CMDLINE_LINUX=\"root=/dev/sdb1 rd.lvm.lv=other/root rhgb\"",
                "ro",
                Carried,
            ),
            (
                "a repeated argument with both values running",
                "GRUB_CMDLINE_LINUX=\"isolcpus=1 isolcpus=2\"",
                "ro isolcpus=2 isolcpus=1",
                Carried,
            ),
            (
                "a repeated argument with one value running",
                "GRUB_CMDLINE_LINUX=\"isolcpus=1 isolcpus=2\"",
                "ro isolcpus=1",
                NotCarried,
            ),
            (
                "export, comments and a continued line",
                "# GRUB_CMDLINE_LINUX=\"ignored=1\"\nexport GRUB_CMDLINE_LINUX=\"quiet \\\n  audit=1\" # set by hand\nexport GRUB_CMDLINE_LINUX",
                "quiet audit=1",
                Carried,
            ),
            (
                "a credential not running",
                "GRUB_CMDLINE_LINUX=\"quiet rd.iscsi.password=Sup3rS3cret\"",
                "quiet",
                NotCarried,
            ),
            (
                "a credential running is redacted, so kargs.d leaves it out",
                "GRUB_CMDLINE_LINUX=\"quiet rd.iscsi.password=Sup3rS3cret\"",
                "quiet rd.iscsi.password=Sup3rS3cret",
                NotCarried,
            ),
            (
                "a running argument with a control character, which kargs.d leaves out",
                "GRUB_CMDLINE_LINUX=\"dyndbg=\\\"file\tx.c +p\\\"\"",
                "quiet dyndbg=\"file\tx.c +p\"",
                NotCarried,
            ),
            (
                "a credential name in a value the redaction pattern reads",
                "GRUB_CMDLINE_LINUX=\"quiet token:Sup3rS3cret\"",
                "quiet",
                NotCarried,
            ),
            ("no tracked variable", "GRUB_TIMEOUT=5", "quiet", Carried),
            (
                "ANSI-C quoting",
                "GRUB_CMDLINE_LINUX=$'rd.iscsi.password\\x3dSup3rS3cret'",
                "quiet",
                Unknown,
            ),
            (
                "locale quoting",
                "GRUB_CMDLINE_LINUX=$\"quiet\"",
                "quiet",
                Unknown,
            ),
            (
                "an operator after an untracked assignment",
                "GRUB_CMDLINE_LINUX=audit=1\nX=1|| GRUB_CMDLINE_LINUX=quiet",
                "quiet",
                Unknown,
            ),
            (
                "export with two arguments",
                "GRUB_CMDLINE_LINUX=audit=1\nexport GRUB_CMDLINE_LINUX=quiet GRUB_CMDLINE_LINUX_DEFAULT=\"$GRUB_CMDLINE_LINUX\"",
                "quiet",
                Unknown,
            ),
            (
                "declare",
                "declare GRUB_CMDLINE_LINUX=quiet",
                "quiet",
                Unknown,
            ),
            (
                "readonly",
                "readonly GRUB_CMDLINE_LINUX=quiet",
                "quiet",
                Unknown,
            ),
            ("local", "local GRUB_CMDLINE_LINUX=quiet", "quiet", Unknown),
            (
                "two assignments on a line",
                "X=1 GRUB_CMDLINE_LINUX=quiet",
                "quiet",
                Unknown,
            ),
            (
                "a semicolon",
                "X=1; GRUB_CMDLINE_LINUX=quiet",
                "quiet",
                Unknown,
            ),
            (
                "an ampersand",
                "X=1 & GRUB_CMDLINE_LINUX=quiet",
                "quiet",
                Unknown,
            ),
            ("a pipe", "X=1 | GRUB_CMDLINE_LINUX=quiet", "quiet", Unknown),
            ("&&", "X=1&&GRUB_CMDLINE_LINUX=quiet", "quiet", Unknown),
            (
                "a redirection",
                "GRUB_CMDLINE_LINUX=quiet >/dev/null",
                "quiet",
                Unknown,
            ),
            (
                "an input redirection",
                "GRUB_CMDLINE_LINUX=quiet</dev/null",
                "quiet",
                Unknown,
            ),
            ("a subshell", "(GRUB_CMDLINE_LINUX=quiet)", "quiet", Unknown),
            (
                "an array",
                "GRUB_CMDLINE_LINUX=(audit=1)",
                "audit=1",
                Unknown,
            ),
            (
                "a backtick in an untracked value",
                "X=\"`echo`\"\nGRUB_CMDLINE_LINUX=quiet",
                "quiet",
                Unknown,
            ),
            (
                "an unquoted command substitution",
                "X=$(true)\nGRUB_CMDLINE_LINUX=quiet",
                "quiet",
                Unknown,
            ),
            (
                "a case statement in a command substitution",
                "GRUB_CMDLINE_LINUX=audit=1\nX=\"$(case a in a)\"\nGRUB_CMDLINE_LINUX=quiet\nY=\";; esac)\"",
                "quiet",
                Unknown,
            ),
            (
                "a quote inside a command substitution",
                "X=\"$(echo \"a\")\"\nGRUB_CMDLINE_LINUX=quiet",
                "quiet",
                Unknown,
            ),
            ("a tilde", "GRUB_CMDLINE_LINUX=~/x", "quiet", Unknown),
            (
                "command substitution in a tracked value",
                "GRUB_CMDLINE_LINUX=\"$(cat /etc/kargs)\"",
                "quiet",
                Unknown,
            ),
            (
                "another variable in a tracked value",
                "GRUB_CMDLINE_LINUX=\"$EXTRA audit=1\"",
                "quiet",
                Unknown,
            ),
            (
                "a sourced file",
                ". /etc/default/grub.local\nGRUB_CMDLINE_LINUX=\"audit=1\"",
                "audit=1",
                Unknown,
            ),
            (
                "a conditional",
                "if true; then GRUB_CMDLINE_LINUX=\"audit=1\"; fi",
                "audit=1",
                Unknown,
            ),
            (
                "an append operator",
                "GRUB_CMDLINE_LINUX+=\" audit=1\"",
                "quiet",
                Unknown,
            ),
            (
                "a parameter operator",
                "X=${GRUB_CMDLINE_LINUX:=audit=1}",
                "quiet",
                Unknown,
            ),
            (
                "an unterminated quote",
                "GRUB_CMDLINE_LINUX=\"audit=1",
                "audit=1",
                Unknown,
            ),
        ];
        for (label, grub, running, want) in cases {
            assert_eq!(compare(grub, running), *want, "{label}");
        }
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

    fn grub_entry(path: &str, content: &str, disposition: FindingKind) -> ConfigFileEntry {
        ConfigFileEntry {
            path: path.into(),
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
    fn the_grub_row_is_settled_against_the_carried_arguments() {
        let modernization =
            FindingKind::advisory(AdvisoryType::Modernization, DEFAULT_GRUB_RATIONALE);
        let not_carried = FindingKind::advisory(AdvisoryType::Modernization, NOT_CARRIED_RATIONALE);
        let not_captured = FindingKind::advisory(AdvisoryType::ContentNotCaptured, "too large");
        let edited = "GRUB_CMDLINE_LINUX=\"quiet rd.iscsi.password=Sup3rS3cret audit=1\"\n";
        let generic = Some(modernization.clone());
        type Case<'a> = (
            &'a str,
            &'a str,
            &'a str,
            FindingKind,
            Option<&'a str>,
            Option<FindingKind>,
        );
        let cases: Vec<Case> = vec![
            (
                "matches: no row",
                DEFAULT_GRUB_PATH,
                "GRUB_CMDLINE_LINUX=\"quiet\"\n",
                modernization.clone(),
                Some("ro quiet"),
                None,
            ),
            (
                "matches, modernization did not run: no row",
                DEFAULT_GRUB_PATH,
                "GRUB_CMDLINE_LINUX=\"quiet\"\n",
                FindingKind::included(),
                Some("ro quiet"),
                None,
            ),
            (
                "matches, OSTree's relative path: no row",
                "etc/default/grub",
                "GRUB_CMDLINE_LINUX=\"quiet\"\n",
                FindingKind::included(),
                Some("ro quiet"),
                None,
            ),
            (
                "not carried: the fixed advisory",
                DEFAULT_GRUB_PATH,
                edited,
                FindingKind::included(),
                Some("ro quiet audit=1"),
                Some(not_carried.clone()),
            ),
            (
                "not carried, OSTree's relative path: the fixed advisory",
                "etc/default/grub",
                edited,
                FindingKind::included(),
                Some("ro quiet audit=1"),
                Some(not_carried.clone()),
            ),
            (
                "no running command line",
                DEFAULT_GRUB_PATH,
                edited,
                modernization.clone(),
                None,
                generic.clone(),
            ),
            (
                "an empty running command line",
                DEFAULT_GRUB_PATH,
                edited,
                modernization.clone(),
                Some(""),
                generic.clone(),
            ),
            (
                "unreadable, modernization did not run",
                DEFAULT_GRUB_PATH,
                "GRUB_CMDLINE_LINUX=\"$(cat x)\"\n",
                FindingKind::included(),
                Some("ro quiet"),
                generic.clone(),
            ),
            (
                "unreadable, OSTree's relative path",
                "etc/default/grub",
                "GRUB_CMDLINE_LINUX=\"$(cat x)\"\n",
                FindingKind::included(),
                Some("ro quiet"),
                generic.clone(),
            ),
            (
                "no captured content",
                DEFAULT_GRUB_PATH,
                "",
                modernization.clone(),
                Some("ro quiet"),
                generic.clone(),
            ),
            (
                "content not captured keeps its advisory",
                DEFAULT_GRUB_PATH,
                "",
                not_captured.clone(),
                Some("ro quiet"),
                Some(not_captured.clone()),
            ),
        ];
        for (label, path, content, disposition, cmdline, want) in cases {
            let mut snap = snapshot(grub_entry(path, content, disposition), cmdline);
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

    /// Nothing parsed from the file reaches the advisory: a value the
    /// reader could take for a name, and a credential the content
    /// redaction finds, appear nowhere in the row's finding.
    #[test]
    fn the_advisory_never_names_what_the_file_sets() {
        for grub in [
            "GRUB_CMDLINE_LINUX=$'rd.iscsi.password\\x3dSup3rS3cret'\n",
            "GRUB_CMDLINE_LINUX=\"quiet token:Sup3rS3cret\"\n",
            "GRUB_CMDLINE_LINUX=\"quiet rd.iscsi.password=Sup3rS3cret\"\n",
        ] {
            let mut snap = snapshot(
                grub_entry(DEFAULT_GRUB_PATH, grub, FindingKind::included()),
                Some("ro quiet"),
            );
            settle_default_grub(&mut snap);
            let files = snap.config.unwrap().files;
            let finding = serde_json::to_string(&files[0].disposition).expect("serializes");
            assert!(!finding.contains("Sup3rS3cret"), "{grub}: {finding}");
            assert!(!finding.contains("password"), "{grub}: {finding}");
            assert!(!finding.contains("token"), "{grub}: {finding}");
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
