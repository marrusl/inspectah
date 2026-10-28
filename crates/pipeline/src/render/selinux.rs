//! SELinux file context rendering (spec § 3.1, § 5.1).
//!
//! The renderer has no access to the target system, so every decision that
//! depends on the target's policy is made by the emitted commands at build
//! time: each rule reads the target's local equivalence table and branches on
//! what it finds. This module holds the two emitted forms, the operand
//! validation that decides whether a rule can be emitted at all, and
//! [`selinux_equivalences`], the one equivalence list the Containerfile, the
//! audit report, and the HTML report all read, so that "synthesized by
//! inspectah" and a conflict are reported the same way everywhere.

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::FindingKind;
use inspectah_core::types::selinux::{
    FcontextFileType, FcontextRule, FcontextSource, SelinuxContext,
};

/// Top-level directories that are symlinks into `/var` on bootc derivation
/// bases and have no `subs_dist` alias in the policy, each paired with the
/// real path it resolves to. Verified for `/srv` on fedora-bootc:42 and
/// centos-bootc:stream10; the CoreOS-lineage extras (`/opt`, `/usr/local`,
/// `/home`, `/root`, `/mnt`) already have aliases.
pub const VAR_SYMLINKED_TOPLEVELS: &[(&str, &str)] = &[("/srv", "/var/srv")];

/// § 3.1's install precondition: `semanage` is not in the bootc base, and
/// every command this block emits needs it.
pub const INSTALL_PRECONDITION: &[&str] = &[
    "# The file context and port rules below need semanage, which this base does not",
    "# carry; policycoreutils-python-utils provides it.",
    "RUN dnf install -y policycoreutils-python-utils",
];

/// The SELinux user `-s` is suppressed for (§ 5.1).
const DEFAULT_USER: &str = "system_u";
/// The MLS level `-r` is suppressed for (§ 5.1).
const DEFAULT_LEVEL: &str = "s0";
/// The `-t` argument an `Unlabel` rule carries, quoted here because `<` and
/// `>` would otherwise reach the shell as redirections.
const UNLABEL_TYPE_ARG: &str = "'<<none>>'";

/// The bytes single quotes do not make inert, so no operand may carry them
/// (containerfile-value-quoting skill, § 3.1 rule 4).
const UNQUOTABLE: &[char] = &['\'', '\n', '\0'];

/// Characters an equivalence operand may not carry: `semanage fcontext -e`
/// takes a literal path, not a regex (§ 3.1 rule 4).
const EQUIVALENCE_FORBIDDEN: &[char] = &['*', '?', '[', ']', '(', ')', '|', '\\'];

/// First line of § 5.1's two-line unrenderable form. It stays generic: the
/// same two lines also serve a conflicting equivalence pair, which has no
/// type and no single spec. Line 2 carries the specifics.
const FIXME_INSTRUCTION: &str = "# FIXME: add this file context by hand with: RUN semanage fcontext -a -t <type> '<spec>' || semanage fcontext -m -t <type> '<spec>' (labels apply at deploy; no restorecon needed)";

/// Prefix of the second line of the unrenderable form, which carries the
/// cause.
const FIXME_CAUSE_PREFIX: &str = "#   the collected rule could not be rendered: ";

/// The caveat a sorted source puts before the rules (§ 3.1 rule 3).
const SEMANAGE_LIST_NOTE: &str = "# NOTE: rule order comes from a sorted listing, not insertion order; if two expressions overlap, reorder so the intended winner is last.";

/// Where an emitted equivalence came from.
#[derive(Debug, Clone, PartialEq)]
pub enum EquivalenceOrigin {
    /// Collected from the host's `file_contexts.subs`.
    Collected,
    /// Built by the renderer for a [`VAR_SYMLINKED_TOPLEVELS`] pair.
    Synthesized { reason: String },
    /// Two rows claim one subject, or a collected row maps the other
    /// direction. Neither is emitted; the operator keeps one. The entry's
    /// own rule is the first of `rows`.
    Conflict { rows: Vec<FcontextRule> },
}

/// What the Containerfile's SELinux block renders for the collected fcontext
/// rules.
pub struct FcontextRendering {
    /// Lines that introduce the rules: the order caveat for a sorted source
    /// and the count paragraph. They precede the install precondition.
    pub preamble: Vec<String>,
    /// The rules themselves: `RUN` instructions, the comments that explain
    /// them, and the FIXME blocks of the rules that could not be rendered.
    pub rules: Vec<String>,
    /// Whether `rules` holds at least one `semanage` command. The install
    /// precondition installs the tool those commands need, so a block that
    /// emits none does not carry it.
    pub emits_command: bool,
}

/// The deduplicated equivalence list: the collected `Equivalence` rows plus
/// the synthesized [`VAR_SYMLINKED_TOPLEVELS`] rows, keyed on `subject`
/// (§ 3.1 rule 2).
///
/// A collected row equal to a synthesized one is returned once, as the
/// collected row. Two rows claiming one subject with different targets, or a
/// collected row mapping the other direction, become one
/// [`EquivalenceOrigin::Conflict`] entry listing both.
pub fn selinux_equivalences(snap: &InspectionSnapshot) -> Vec<(FcontextRule, EquivalenceOrigin)> {
    let Some(sel) = snap.selinux.as_ref() else {
        return Vec::new();
    };

    let mut entries: Vec<(FcontextRule, EquivalenceOrigin)> = Vec::new();

    for rule in sel.fcontext_rules.iter().filter(|r| is_included(r)) {
        let Some((subject, _)) = equivalence_pair(rule) else {
            continue;
        };
        match entries
            .iter()
            .position(|(r, _)| subject_of(r) == Some(subject))
        {
            Some(idx) => absorb(&mut entries[idx], rule),
            None => entries.push((rule.clone(), EquivalenceOrigin::Collected)),
        }
    }

    for (link, target) in VAR_SYMLINKED_TOPLEVELS {
        if !sel
            .fcontext_rules
            .iter()
            .any(|r| triggers_synthesis(r, link))
        {
            continue;
        }
        let synthesized = FcontextRule::Equivalence {
            subject: (*target).to_string(),
            labeled_like: (*link).to_string(),
            disposition: FindingKind::included(),
            aggregate: None,
        };
        // A collected row mapping the other direction claims a different
        // subject, so the subject key below would never see it.
        if let Some(idx) = entries
            .iter()
            .position(|(r, _)| equivalence_pair(r) == Some((link, target)))
        {
            absorb(&mut entries[idx], &synthesized);
            continue;
        }
        match entries
            .iter()
            .position(|(r, _)| subject_of(r) == Some(target))
        {
            Some(idx) => absorb(&mut entries[idx], &synthesized),
            None => entries.push((
                synthesized,
                EquivalenceOrigin::Synthesized {
                    reason: format!(
                        "{link} is a symlink to {target} on bootc derivation bases \
                         and the policy carries no {target} alias"
                    ),
                },
            )),
        }
    }

    entries
}

/// Render the collected fcontext rules for the Containerfile's SELinux
/// block, in collected order.
pub fn render_fcontext_rules(snap: &InspectionSnapshot) -> FcontextRendering {
    let mut rendering = FcontextRendering {
        preamble: Vec::new(),
        rules: Vec::new(),
        emits_command: false,
    };
    let Some(sel) = snap.selinux.as_ref() else {
        return rendering;
    };

    let equivalences = selinux_equivalences(snap);
    let mut emitted = vec![false; equivalences.len()];

    let decisions = sel.fcontext_rules.iter().filter(|r| is_included(r)).count();
    if decisions > 0 {
        rendering.preamble.extend(count_paragraph(decisions));
    }

    let any_replay = sel
        .fcontext_rules
        .iter()
        .any(|r| is_label(r) && is_included(r) && offending_operand(r).is_none());
    if any_replay {
        if sel.fcontext_source == FcontextSource::SemanageList {
            rendering.preamble.insert(0, SEMANAGE_LIST_NOTE.to_string());
        }
        rendering.rules.extend(replay_paragraph());
    }

    for rule in &sel.fcontext_rules {
        if let FcontextRule::Unparsed { raw } = rule {
            rendering.rules.extend(fixme_lines(&format!("{raw:?}")));
            continue;
        }
        if !is_included(rule) {
            continue;
        }
        match rule {
            FcontextRule::Equivalence { .. } => {
                // A row absent from the list was deduplicated against an
                // identical earlier one, which has already been emitted.
                if let Some(idx) = entry_of(&equivalences, |row| row == rule) {
                    emit_equivalence(&equivalences[idx], &mut emitted[idx], &mut rendering);
                }
            }
            FcontextRule::Label { spec, .. } | FcontextRule::Unlabel { spec, .. } => {
                let lines = match offending_operand(rule) {
                    Some(offending) => fixme_lines(&format!("{offending:?}")),
                    None => {
                        // The equivalence that makes a `/srv` rule apply to
                        // the real path goes ahead of the first rule needing
                        // it (§ 3.1).
                        let synthesis = var_symlink_target(spec).and_then(|target| {
                            entry_of(&equivalences, |row| subject_of(row) == Some(target))
                        });
                        if let Some(idx) = synthesis {
                            emit_equivalence(&equivalences[idx], &mut emitted[idx], &mut rendering);
                        }
                        rendering.emits_command = true;
                        let args = rule1_args(rule);
                        rule1_lines(spec, &args.delete, &args.add_modify)
                    }
                };
                rendering.rules.extend(lines);
            }
            FcontextRule::Unparsed { .. } => unreachable!("handled above"),
        }
    }

    rendering
}

// ---------------------------------------------------------------------------
// The two emitted forms (§ 5.1)
// ---------------------------------------------------------------------------

/// The lines of § 3.1 rule 1 that carry no operand: by the time they run the
/// spec is in `$spec`, which the shell expands as data and never re-parses.
/// Literal text, so they read against the spec byte for byte.
const RULE1_GUARD_LINES: &[&str] = &[
    r#"    eqs="$(semanage fcontext -l -C)" || { printf 'FIXME: could not list the target file contexts before replaying the rule for %s; fix semanage in the base, or exclude this rule, then rebuild\n' "$spec" >&2; exit 1; }; \"#,
    r#"    if printf '%s\n' "$eqs" | awk '$2 == "=" { print $1 }' | grep -qxF -- "$spec"; then \"#,
    r#"      printf 'FIXME: skipped the file context rule for %s; the target maps that path as an equivalence, which semanage fcontext -d would delete instead of a label; delete it with semanage fcontext -d -e, or exclude this rule, then rebuild\n' "$spec" >&2; \"#,
];

/// § 3.1 rule 1: delete the exact key, then add, then modify, guarded by the
/// target's own equivalence table.
fn rule1_lines(spec: &str, delete_args: &str, add_modify_args: &str) -> Vec<String> {
    let mut lines = vec![format!("RUN spec='{spec}'; \\")];
    lines.extend(RULE1_GUARD_LINES.iter().map(|l| (*l).to_string()));
    lines.push(format!(
        r#"    else semanage fcontext -d{delete_args} "$spec" 2>/dev/null || true; \"#
    ));
    // § 5.1 requires the `-a` and the `-m` to carry identical arguments.
    // One binding interpolated twice in one expression is what holds that:
    // there is no second argument list to drift from this one.
    lines.extend(
        format!(
            "      semanage fcontext -a {add_modify_args} \"$spec\" \\\n      || semanage fcontext -m {add_modify_args} \"$spec\"; fi"
        )
        .lines()
        .map(|l| l.to_string()),
    );
    lines
}

/// § 3.1 rule 2: read the target's mapping for this subject, then add it,
/// leave it alone, or stop the build.
fn rule2_lines(subject: &str, labeled_like: &str) -> Vec<String> {
    // `{{` and `}}` are `format!`'s escapes; the emitted awk program carries
    // single braces.
    vec![
        format!(
            r#"RUN eq="$(semanage fcontext -l -C | awk -v s='{subject}' '$2 == "=" && $1 == s {{ print $3 }}')"; \"#
        ),
        format!(
            r#"    if [ -z "$eq" ]; then semanage fcontext -a -e '{labeled_like}' '{subject}'; \"#
        ),
        // The diagnostic is a static `printf` format with the operands as
        // arguments: a double-quoted `echo` would run the backticks a
        // target's own equivalence table can put in `$eq`.
        format!(
            r#"    elif [ "$eq" != '{labeled_like}' ]; then printf 'FIXME: %s is already equivalent to %s on the target, not %s; delete that equivalence with semanage fcontext -d -e, or exclude this rule, then rebuild\n' '{subject}' "$eq" '{labeled_like}' >&2; exit 1; fi"#
        ),
    ]
}

/// The argument lists one rule 1 renders with.
struct Rule1Args {
    /// What the `-d` carries beyond the spec, its leading space included so
    /// the command reads `-d{delete}`. Only `-f`: `-d` is not scoped to
    /// label keys and takes nothing else.
    delete: String,
    /// The arguments the `-a` and the `-m` share, in canonical flag order
    /// (`-f`, `-r`, `-s`, `-t`). `-f` leads so that `delete` is a prefix of
    /// this list; the rest take the man page's letter order.
    add_modify: String,
}

fn rule1_args(rule: &FcontextRule) -> Rule1Args {
    let (file_type, context) = match rule {
        FcontextRule::Label {
            file_type, context, ..
        } => (*file_type, Some(context)),
        FcontextRule::Unlabel { file_type, .. } => (*file_type, None),
        _ => (FcontextFileType::All, None),
    };

    // One `-f` fragment reaches all three commands.
    let file_flag = file_type.semanage_flag().map(|f| format!("-f {f}"));
    let delete = file_flag
        .as_ref()
        .map(|f| format!(" {f}"))
        .unwrap_or_default();

    let mut args: Vec<String> = file_flag.into_iter().collect();
    match context {
        Some(ctx) => {
            if let Some(level) = ctx.level.as_deref()
                && level != DEFAULT_LEVEL
            {
                args.push(format!("-r {level}"));
            }
            if ctx.user != DEFAULT_USER {
                args.push(format!("-s {}", ctx.user));
            }
            args.push(format!("-t {}", ctx.type_));
        }
        None => args.push(format!("-t {UNLABEL_TYPE_ARG}")),
    }

    Rule1Args {
        delete,
        add_modify: args.join(" "),
    }
}

// ---------------------------------------------------------------------------
// Comments
// ---------------------------------------------------------------------------

fn count_paragraph(count: usize) -> Vec<String> {
    let noun = if count == 1 { "rule" } else { "rules" };
    vec![
        format!(
            "# SELinux file contexts ({count} custom {noun}). Labels for image content are computed at"
        ),
        "# deploy time from the image's own policy under /etc/selinux, so a rule only needs".into(),
        "# to be in the policy store: no restorecon at build time. Data you restore into".into(),
        "# /var (/var/srv included) keeps the labels it arrived with; run restorecon -R on".into(),
        "# it before starting the services that use it.".into(),
    ]
}

fn replay_paragraph() -> Vec<String> {
    vec![
        "# Each rule removes its own key from the local store if present, then adds it, or".into(),
        "# modifies it when the target policy defines the key; the rules end the local".into(),
        "# list in the order the source host held them. A rule whose path the target".into(),
        "# already maps as an equivalence is skipped with a FIXME in the build output,".into(),
        "# because deleting its key would delete that equivalence.".into(),
    ]
}

fn symlink_paragraph(link: &str, target: &str) -> Vec<String> {
    vec![
        format!(
            "# {link} is a symlink to {target} on this base and the policy has no {target} alias;"
        ),
        format!("# this makes every {link} rule, stock and custom, apply to the real path. The"),
        "# equivalence is added when absent and left alone when identical; a different".into(),
        "# target mapping stops the build with an instruction.".into(),
    ]
}

/// § 5.1's two-line unrenderable form, imperative first.
fn fixme_lines(cause: &str) -> Vec<String> {
    vec![
        FIXME_INSTRUCTION.to_string(),
        format!("{FIXME_CAUSE_PREFIX}{cause}"),
    ]
}

/// The cause line of a conflicting equivalence pair: both rows, and the
/// instruction that resolves them.
fn conflict_cause(rows: &[FcontextRule]) -> String {
    let pairs: Vec<String> = rows
        .iter()
        .filter_map(equivalence_pair)
        .map(|(subject, labeled_like)| format!("{subject:?} = {labeled_like:?}"))
        .collect();
    format!(
        "these equivalences conflict: {}; keep one and exclude the other, then rebuild",
        pairs.join(" and ")
    )
}

// ---------------------------------------------------------------------------
// Operand validation (§ 3.1 rule 4)
// ---------------------------------------------------------------------------

/// A label `spec` is renderable when single quotes make it inert and it is a
/// `file_contexts` expression, which always begins with `/`. A spec that does
/// not (`--help`) would be read as a flag and skip its rule silently.
fn spec_ok(spec: &str) -> bool {
    spec.starts_with('/') && !spec.contains(UNQUOTABLE)
}

/// An equivalence operand is additionally a literal absolute path prefix:
/// `-e` takes a path, not a regex.
fn equivalence_operand_ok(operand: &str) -> bool {
    operand.starts_with('/')
        && !operand.contains(UNQUOTABLE)
        && !operand.contains(EQUIVALENCE_FORBIDDEN)
        && !operand.contains(char::is_whitespace)
}

/// `type_`, `user`, and `level` reach the command as bare tokens.
fn bare_token_ok(token: &str) -> bool {
    !token.is_empty()
        && token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ':' | '-'))
}

/// The first operand that stops a rule from being rendered, or `None` when
/// the whole rule can be.
fn offending_operand(rule: &FcontextRule) -> Option<&str> {
    match rule {
        FcontextRule::Label { spec, context, .. } => {
            if !spec_ok(spec) {
                return Some(spec);
            }
            offending_context_token(context)
        }
        FcontextRule::Unlabel { spec, .. } => (!spec_ok(spec)).then_some(spec.as_str()),
        FcontextRule::Equivalence {
            subject,
            labeled_like,
            ..
        } => {
            if !equivalence_operand_ok(subject) {
                Some(subject)
            } else if !equivalence_operand_ok(labeled_like) {
                Some(labeled_like)
            } else {
                None
            }
        }
        FcontextRule::Unparsed { raw } => Some(raw),
    }
}

/// Only the tokens a rule actually emits are checked: a non-default `user`
/// or `level` reaches the command, a default one is suppressed.
fn offending_context_token(context: &SelinuxContext) -> Option<&str> {
    if !bare_token_ok(&context.type_) {
        return Some(&context.type_);
    }
    if context.user != DEFAULT_USER && !bare_token_ok(&context.user) {
        return Some(&context.user);
    }
    match context.level.as_deref() {
        Some(level) if level != DEFAULT_LEVEL && !bare_token_ok(level) => Some(level),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Rule plumbing
// ---------------------------------------------------------------------------

fn is_included(rule: &FcontextRule) -> bool {
    match rule {
        FcontextRule::Label { disposition, .. }
        | FcontextRule::Unlabel { disposition, .. }
        | FcontextRule::Equivalence { disposition, .. } => disposition.is_included(),
        FcontextRule::Unparsed { .. } => false,
    }
}

fn is_label(rule: &FcontextRule) -> bool {
    matches!(
        rule,
        FcontextRule::Label { .. } | FcontextRule::Unlabel { .. }
    )
}

fn equivalence_pair(rule: &FcontextRule) -> Option<(&str, &str)> {
    match rule {
        FcontextRule::Equivalence {
            subject,
            labeled_like,
            ..
        } => Some((subject, labeled_like)),
        _ => None,
    }
}

fn subject_of(rule: &FcontextRule) -> Option<&str> {
    equivalence_pair(rule).map(|(subject, _)| subject)
}

/// Every row an entry speaks for: one, or both halves of a conflict.
fn rows_of<'a>(rule: &'a FcontextRule, origin: &'a EquivalenceOrigin) -> &'a [FcontextRule] {
    match origin {
        EquivalenceOrigin::Conflict { rows } => rows,
        _ => std::slice::from_ref(rule),
    }
}

/// The entry that speaks for a row matching `pred`, conflict halves
/// included: a rule reaches its entry through whichever row named it.
fn entry_of(
    entries: &[(FcontextRule, EquivalenceOrigin)],
    pred: impl Fn(&FcontextRule) -> bool,
) -> Option<usize> {
    entries
        .iter()
        .position(|(rule, origin)| rows_of(rule, origin).iter().any(&pred))
}

/// Fold another row claiming the same subject into an entry: a duplicate
/// changes nothing, a different target makes the entry a conflict.
fn absorb(entry: &mut (FcontextRule, EquivalenceOrigin), incoming: &FcontextRule) {
    if let EquivalenceOrigin::Conflict { rows } = &mut entry.1 {
        if !rows.contains(incoming) {
            rows.push(incoming.clone());
        }
        return;
    }
    if &entry.0 == incoming || equivalence_pair(&entry.0) == equivalence_pair(incoming) {
        return;
    }
    entry.1 = EquivalenceOrigin::Conflict {
        rows: vec![entry.0.clone(), incoming.clone()],
    };
}

/// Whether a collected rule makes the equivalence for `link` necessary: an
/// included `Label`/`Unlabel` the block will actually emit, whose spec begins
/// with the symlinked top level.
fn triggers_synthesis(rule: &FcontextRule, link: &str) -> bool {
    let spec = match rule {
        FcontextRule::Label { spec, .. } | FcontextRule::Unlabel { spec, .. } => spec,
        _ => return false,
    };
    is_included(rule) && offending_operand(rule).is_none() && begins_with_toplevel(spec, link)
}

/// The real path a spec's top level resolves to, when it has one.
fn var_symlink_target(spec: &str) -> Option<&'static str> {
    VAR_SYMLINKED_TOPLEVELS
        .iter()
        .find(|(link, _)| begins_with_toplevel(spec, link))
        .map(|(_, target)| *target)
}

/// A spec begins with a top level when it equals it, or continues with `/`
/// or `(` (§ 3.1).
fn begins_with_toplevel(spec: &str, toplevel: &str) -> bool {
    match spec.strip_prefix(toplevel) {
        Some("") => true,
        Some(rest) => rest.starts_with('/') || rest.starts_with('('),
        None => false,
    }
}

/// Emit one equivalence entry, once.
fn emit_equivalence(
    entry: &(FcontextRule, EquivalenceOrigin),
    emitted: &mut bool,
    rendering: &mut FcontextRendering,
) {
    if *emitted {
        return;
    }
    *emitted = true;

    let (rule, origin) = entry;
    if let EquivalenceOrigin::Conflict { rows } = origin {
        rendering.rules.extend(fixme_lines(&conflict_cause(rows)));
        return;
    }
    let Some((subject, labeled_like)) = equivalence_pair(rule) else {
        return;
    };
    if let Some(offending) = offending_operand(rule) {
        rendering
            .rules
            .extend(fixme_lines(&format!("{offending:?}")));
        return;
    }
    if matches!(origin, EquivalenceOrigin::Synthesized { .. }) {
        rendering
            .rules
            .extend(symlink_paragraph(labeled_like, subject));
    }
    rendering.rules.extend(rule2_lines(subject, labeled_like));
    rendering.emits_command = true;
}
