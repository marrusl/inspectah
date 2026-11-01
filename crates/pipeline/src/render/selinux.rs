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
use inspectah_core::types::aggregate::AggregatePrevalence;
use inspectah_core::types::selinux::{
    FcontextFileType, FcontextRule, FcontextSource, SelinuxContext,
};

/// Top-level directories that are symlinks into `/var` and that the policy
/// does not alias, each paired with the real path it resolves to.
///
/// Measured on `registry.redhat.io/rhel9/rhel-bootc:9.8` (2026-10-28):
/// `/srv` is the only `/var` symlink with no `file_contexts.subs_dist` entry.
/// `/home`, `/root` and `/mnt` are symlinks and are aliased; `/opt` and
/// `/usr/local` are real directories on that base, so the alias question does
/// not arise for them. A fedora-bootc or centos-bootc base creates a
/// different set, so this wants re-verifying when inspectah grows base
/// support.
pub const VAR_SYMLINKED_TOPLEVELS: &[(&str, &str)] = &[("/srv", "/var/srv")];

/// § 3.1's install precondition: `semanage` is not in the bootc base, and
/// every command this block emits needs it.
///
/// The cleanup is the form every other `dnf install` in `containerfile.rs`
/// uses, and it is not optional here: measured on `rhel9/rhel-bootc:9.8`, the
/// uncleaned layer is 617 MB for a 10.9 MB payload, 533 MB of it
/// `/var/cache/dnf`, which `bootc install` then writes to the machine's
/// persistent `/var`. This block renders after the package install block, so
/// without this it also re-fills the cache that block just cleared. The
/// package name stays on the `RUN` line rather than on a continuation, unlike
/// the package block's, so the two are never mistaken for each other.
pub const INSTALL_PRECONDITION: &[&str] = &[
    "# The file context and port rules below need semanage, which this base does not",
    "# carry; policycoreutils-python-utils provides it.",
    "RUN dnf install -y policycoreutils-python-utils \\",
    "    && dnf clean all \\",
    "    && rm -rf \\",
    "        /var/cache/dnf \\",
    "        /var/lib/dnf/history* \\",
    "        /var/log/dnf* \\",
    "        /var/log/hawkey.log \\",
    "        /var/log/rhsm",
];

/// The SELinux user `-s` is suppressed for (§ 5.1).
const DEFAULT_USER: &str = "system_u";
/// The MLS level `-r` is suppressed for (§ 5.1).
const DEFAULT_LEVEL: &str = "s0";
/// The `-t` argument an `Unlabel` rule carries, quoted here because `<` and
/// `>` would otherwise reach the shell as redirections.
const UNLABEL_TYPE_ARG: &str = "'<<none>>'";

/// Bytes no operand may carry: the apostrophe, which single quotes cannot
/// make inert (containerfile-value-quoting skill, § 3.1 rule 4), and the
/// control characters.
///
/// `\n` and `\0` are the two § 3.1 rule 4 names and both are control
/// characters. The rest of C0 is inert to the shell but misrepresents the
/// artifact: a spec carrying CR or an ANSI escape renders a Containerfile
/// that displays, to a reviewer who was not the scanner, as something other
/// than its bytes. A real `file_contexts` expression never carries one.
fn is_unquotable(c: char) -> bool {
    c == '\'' || c.is_control()
}

/// Characters an equivalence operand may not carry: `semanage fcontext -e`
/// takes a literal path, not a regex (§ 3.1 rule 4). The backslash is here
/// for a second reason: the subject reaches `awk -v s=`, which
/// escape-processes its assignment value under POSIX semantics, so a
/// surviving backslash would compare a different string at build time.
const EQUIVALENCE_FORBIDDEN: &[char] = &['*', '?', '[', ']', '(', ')', '|', '\\'];

/// First line of § 5.1's two-line unrenderable form. It stays generic: the
/// same two lines also serve a conflicting equivalence pair, which has no
/// type and no single spec. Line 2 carries the specifics.
///
/// § 5.1 writes the parenthetical as "labels apply at deploy", which is the
/// wrong moment: ostree-ext relabels against the image's own policy when
/// bootc pulls the image, and deployment checkout is later (verified in
/// `container/store.rs` against rhel9/rhel-bootc:9.8, 2026-10-28). The
/// count paragraph says the same thing, and two sentences in one block
/// disagreeing is worse than one being imprecise (Mark, 2026-10-28).
const FIXME_INSTRUCTION: &str = "# FIXME: add this file context by hand with: RUN semanage fcontext -a -t <type> '<spec>' || semanage fcontext -m -t <type> '<spec>' (labels apply when bootc pulls the image; no restorecon needed)";

/// Prefix of the second line of the unrenderable form, which carries the
/// cause.
const FIXME_CAUSE_PREFIX: &str = "#   the collected rule could not be rendered: ";

/// The caveat a sorted source puts before the rules (§ 3.1 rule 3).
const SEMANAGE_LIST_NOTE: &str = "# NOTE: rule order comes from a sorted listing, not insertion order; if two expressions overlap, reorder so the intended winner is last.";

/// One row of the equivalence list, with where it came from and what the
/// Containerfile does with it.
///
/// Emission is part of the contract rather than something each consumer
/// re-derives: it depends on operand validation, on conflicts, and (since the
/// collision rule) on whether any rule needing a synthesized row survives,
/// none of which a consumer can see. § 3.1 says this helper exists so the
/// three consumers report the same thing, and a row reported as synthesized
/// that the build never adds breaks exactly that (Mark, 2026-10-28).
///
/// The cause travels with the fact because a consumer cannot reconstruct it:
/// [`rule_faults`] is private, and a synthesized row nothing needs leaves no
/// trace anywhere else: the Containerfile emits no command *and no FIXME*
/// for it, so the reports are the only surface that can say it existed.
#[derive(Debug, Clone, PartialEq)]
pub struct Equivalence {
    pub rule: FcontextRule,
    pub origin: EquivalenceOrigin,
    /// Why the Containerfile renders no `semanage` command for this row, or
    /// `None` when it renders one. Set for a conflict, for operands that fail
    /// validation, and for a synthesized row whose every triggering rule was
    /// refused.
    pub not_emitted: Option<String>,
}

/// The cause a synthesized row carries when nothing is left for it to serve.
///
/// Unlike the other two causes this text appears nowhere in the Containerfile:
/// a comment about an equivalence that was never going to be added is noise in
/// a build file. It is still a fact about the export, so the reports carry it.
fn unneeded_cause(link: &str, target: &str) -> String {
    format!(
        "nothing is left needing it, because every included {link} rule is itself refused, \
         so the block replays none of them and adds no {target} equivalence"
    )
}

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
pub fn selinux_equivalences(snap: &InspectionSnapshot) -> Vec<Equivalence> {
    let Some(sel) = snap.selinux.as_ref() else {
        return Vec::new();
    };

    let mut entries: Vec<Equivalence> = Vec::new();

    for rule in sel.fcontext_rules.iter().filter(|r| is_included(r)) {
        let Some((subject, _)) = equivalence_pair(rule) else {
            continue;
        };
        match entries
            .iter()
            .position(|e| subject_of(&e.rule) == Some(subject))
        {
            Some(idx) => absorb(&mut entries[idx], rule),
            None => entries.push(Equivalence {
                rule: rule.clone(),
                origin: EquivalenceOrigin::Collected,
                not_emitted: None,
            }),
        }
    }
    // The synthesis loop asks `collision` which rules survive, and that reads
    // `not_emitted`, so the collected rows settle first. They can: a collected
    // row's emission depends on nothing but itself.
    settle_emission(&mut entries);

    for (link, target) in VAR_SYMLINKED_TOPLEVELS {
        let synthesized = FcontextRule::Equivalence {
            subject: (*target).to_string(),
            labeled_like: (*link).to_string(),
            disposition: FindingKind::included(),
            aggregate: None,
        };

        // A collected row mapping the other direction is a conflict whether
        // or not anything triggers the synthesis, and whatever the subject
        // key below would do with it (Mark, 2026-10-28). `/srv` **is** a
        // symlink to `/var/srv` on the image, so a row declaring the reverse
        // inverts the base's own layout; emitting it is wrong on its own
        // terms. It claims a different subject, so the subject key would
        // never see it.
        if let Some(idx) = entries
            .iter()
            .position(|e| equivalence_pair(&e.rule) == Some((link, target)))
        {
            absorb(&mut entries[idx], &synthesized);
            continue;
        }

        let triggers: Vec<&FcontextRule> = sel
            .fcontext_rules
            .iter()
            .filter(|r| triggers_synthesis(r, link))
            .collect();
        if triggers.is_empty() {
            continue;
        }
        // A triggering rule the collision rule refuses emits no command, so
        // it needs no equivalence. When every trigger is refused the row is
        // still reported, because the operator should know what the build
        // would have added, but it is reported as not emitted. The row can
        // only collide with specs under `target`, which never trigger it, so
        // this does not depend on itself.
        let needed = triggers.iter().any(|r| collision(&entries, r).is_none());

        match entries
            .iter()
            .position(|e| subject_of(&e.rule) == Some(target))
        {
            Some(idx) => absorb(&mut entries[idx], &synthesized),
            None => entries.push(Equivalence {
                rule: synthesized,
                origin: EquivalenceOrigin::Synthesized {
                    reason: symlink_fact(link, target),
                },
                not_emitted: (!needed).then(|| unneeded_cause(link, target)),
            }),
        }
    }

    settle_emission(&mut entries);
    entries
}

/// Record, on every entry the block will not render as a command, why it
/// will not. It never clears a cause, so the synthesis loop's "no surviving
/// trigger" answer survives the second call.
fn settle_emission(entries: &mut [Equivalence]) {
    for entry in entries {
        // [`emit_equivalence`]'s precedence, so the cause reported is the one
        // the build prints: a conflicting pair is a conflict even when one of
        // its rows also carries an operand that fails validation.
        entry.not_emitted = match &entry.origin {
            EquivalenceOrigin::Conflict { rows } => Some(conflict_cause(rows)),
            _ if !is_renderable(&entry.rule) => Some(unrenderable_cause(&entry.rule)),
            // A row nothing needs keeps the cause the synthesis loop gave it.
            _ => entry.not_emitted.take(),
        };
    }
}

// ---------------------------------------------------------------------------
// The report surfaces (§ 4)
// ---------------------------------------------------------------------------

/// What the SELinux block does with one row, as the audit and HTML reports
/// show it.
///
/// Two variants rather than a command and a reason side by side: "no command
/// and no reason" and "a command and a reason" are both lies about the build,
/// and neither is expressible here.
#[derive(Debug, Clone, PartialEq)]
pub enum BuildAction {
    /// The `RUN` the block emits for this row, its lines joined with `\n`.
    Emits(String),
    /// The block emits no command for this row, and why. For a rule the
    /// renderer refused this is the cause printed in the Containerfile's own
    /// FIXME; for an excluded rule and for a synthesized row nothing needs,
    /// the Containerfile is silent and this is the only place it is said.
    Skips(String),
}

/// One row of the fcontext story the reports tell: a collected rule, or a row
/// the renderer synthesized, with what the build does about it.
///
/// Both report surfaces read this rather than walking `fcontext_rules`
/// themselves, because the gap that matters is between what the Containerfile
/// does and what a report says it does, and a second walk is a second place
/// for that gap to open.
#[derive(Debug, Clone, PartialEq)]
pub struct FcontextSummary {
    /// The rule this row speaks for. Owned because a synthesized row has no
    /// counterpart in the snapshot.
    pub rule: FcontextRule,
    /// Where an equivalence came from. `None` for a label, an unlabel, and an
    /// unparsed line.
    pub origin: Option<EquivalenceOrigin>,
    pub action: BuildAction,
}

/// Whether a rule carries a disposition, and so is one the § 4 count line
/// counts. An `Unparsed` line is display-only: nothing decided it and nothing
/// can include it.
pub fn is_decision_bearing(rule: &FcontextRule) -> bool {
    !matches!(rule, FcontextRule::Unparsed { .. })
}

/// How a row is named on the report surfaces: the identity an operator
/// recognises from their own policy, not the command it renders as.
pub fn summary_label(rule: &FcontextRule) -> String {
    match rule {
        FcontextRule::Label {
            spec,
            file_type,
            context,
            ..
        } => format!(
            "{spec}{} -> {}",
            file_type_suffix(*file_type),
            context.type_
        ),
        FcontextRule::Unlabel {
            spec, file_type, ..
        } => format!("{spec}{} -> <<none>>", file_type_suffix(*file_type)),
        FcontextRule::Equivalence {
            subject,
            labeled_like,
            ..
        } => format!("{subject} = {labeled_like}"),
        // The line is its own identity: nothing was parsed out of it to name
        // it by.
        FcontextRule::Unparsed { raw } => raw.clone(),
    }
}

/// The `-f` scope a rule carries, or nothing for the default.
fn file_type_suffix(file_type: FcontextFileType) -> String {
    file_type
        .semanage_flag()
        .map(|flag| format!(" (-f {flag})"))
        .unwrap_or_default()
}

/// The parenthetical both report surfaces put after a row's name: where it
/// came from, when that is not already obvious, and what the build does
/// about it.
///
/// One function rather than one per surface. § 3.1 asks the consumers to
/// report a synthesis and a conflict the same way everywhere, and two
/// orderings of the same two clauses are two stories, which is what the
/// first version of these surfaces shipped.
///
/// A collected row came from the host and needs no note. A conflict's cause
/// already says it conflicts and names both rows.
pub fn summary_disposition(row: &FcontextSummary) -> String {
    let mut clauses = Vec::new();
    if let Some(EquivalenceOrigin::Synthesized { reason }) = &row.origin {
        clauses.push(format!("synthesized by inspectah: {reason}"));
    }
    clauses.push(match &row.action {
        BuildAction::Emits(_) => "applied".to_string(),
        BuildAction::Skips(cause) => format!("not applied: {cause}"),
    });
    clauses.join("; ")
}

/// The command the block emits for a row, when it emits one.
pub fn summary_command(row: &FcontextSummary) -> Option<&str> {
    match &row.action {
        BuildAction::Emits(command) => Some(command),
        BuildAction::Skips(_) => None,
    }
}

/// Why a rule nobody included is not in the export.
///
/// Two sentences, not one. `aggregate_count` is the only thing that separates
/// a key only some hosts carry from a key every host carries and disagrees
/// about, and an audit that prints "excluded" for both erases the distinction
/// the aggregate merge exists to surface. `aggregate_hosts` names *which*
/// hosts dissent and never *what* they carry, so the line stops there.
fn exclusion_cause(rule: &FcontextRule) -> String {
    let Some(prevalence) = aggregate_of(rule) else {
        return "excluded from the export".to_string();
    };
    let total = prevalence.total;
    let shown = prevalence.hosts.join(", ");

    let Some(key_count) = prevalence.aggregate_count else {
        return format!(
            "this rule is on {} of {total} hosts ({shown}); excluded from the export, \
             include it explicitly to ship it",
            prevalence.count
        );
    };

    let dissenting: Vec<&str> = prevalence
        .aggregate_hosts
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(String::as_str)
        .filter(|host| !prevalence.hosts.iter().any(|kept| kept == host))
        .collect();
    format!(
        "{key_count} of {total} hosts define this rule and disagree on its content; the \
         content shown is the one carried by {shown}; it differs on {}; excluded from the \
         export, include it explicitly to ship the content shown",
        dissenting.join(", ")
    )
}

fn aggregate_of(rule: &FcontextRule) -> Option<&AggregatePrevalence> {
    match rule {
        FcontextRule::Label { aggregate, .. }
        | FcontextRule::Unlabel { aggregate, .. }
        | FcontextRule::Equivalence { aggregate, .. } => aggregate.as_ref(),
        FcontextRule::Unparsed { .. } => None,
    }
}

/// The cause a second collected row carries when an identical earlier one
/// already stands for it. The block adds the equivalence once.
const DUPLICATE_EQUIVALENCE: &str =
    "an identical equivalence is listed above; the block adds it once";

/// Every fcontext row the reports show, in the order the Containerfile
/// renders them: the collected rules in collected order, each synthesized row
/// ahead of the first rule that needs it, and any synthesized row nothing
/// needs at the end.
///
/// Excluded rules are rows here, not omissions (§ 4 lists "dispositions",
/// plural). Dropping them would make a contested key vanish from the report
/// entirely, which is the one thing the aggregate merge's exclusion exists to
/// prevent.
pub fn fcontext_summaries(snap: &InspectionSnapshot) -> Vec<FcontextSummary> {
    let Some(sel) = snap.selinux.as_ref() else {
        return Vec::new();
    };
    let equivalences = selinux_equivalences(snap);
    let mut shown = vec![false; equivalences.len()];
    let mut rows = Vec::new();

    for rule in &sel.fcontext_rules {
        if let FcontextRule::Unparsed { .. } = rule {
            rows.push(skipped(rule.clone(), None, unrenderable_cause(rule)));
            continue;
        }
        if !is_included(rule) {
            rows.push(skipped(rule.clone(), None, exclusion_cause(rule)));
            continue;
        }
        let Some(replay) = ReplayRule::of(rule) else {
            // An included rule that is not a replay is an equivalence. It
            // reaches its entry through whichever row named it, so both
            // halves of a conflict find the one entry that speaks for them.
            let Some(idx) = entry_of(&equivalences, |row| row == rule) else {
                continue;
            };
            rows.push(equivalence_row(
                rule.clone(),
                &equivalences[idx],
                shown[idx],
            ));
            shown[idx] = true;
            continue;
        };

        match collision(&equivalences, rule) {
            Some(cause) => rows.push(skipped(rule.clone(), None, cause)),
            None => {
                let synthesis = var_symlink_target(replay.spec).and_then(|target| {
                    entry_of(&equivalences, |row| subject_of(row) == Some(target))
                });
                if let Some(idx) = synthesis
                    && !shown[idx]
                {
                    let entry = &equivalences[idx];
                    rows.push(equivalence_row(entry.rule.clone(), entry, false));
                    shown[idx] = true;
                }
                let args = rule1_args(replay.file_type, replay.context);
                rows.push(FcontextSummary {
                    rule: rule.clone(),
                    origin: None,
                    action: BuildAction::Emits(
                        rule1_lines(replay.spec, &args.delete, &args.add_modify).join("\n"),
                    ),
                });
            }
        }
    }

    // A synthesized row no rule reached: the one case the Containerfile says
    // nothing at all about, so the reports are the only surface that carries
    // it.
    for (idx, entry) in equivalences.iter().enumerate() {
        if !shown[idx] {
            rows.push(equivalence_row(entry.rule.clone(), entry, false));
        }
    }
    rows
}

fn skipped(
    rule: FcontextRule,
    origin: Option<EquivalenceOrigin>,
    cause: String,
) -> FcontextSummary {
    FcontextSummary {
        rule,
        origin,
        action: BuildAction::Skips(cause),
    }
}

/// One equivalence row, reading the entry's own cause so that a report and
/// the build never name different reasons for the same refusal.
fn equivalence_row(
    rule: FcontextRule,
    entry: &Equivalence,
    already_shown: bool,
) -> FcontextSummary {
    let origin = Some(entry.origin.clone());
    if let Some(cause) = &entry.not_emitted {
        return skipped(rule, origin, cause.clone());
    }
    if already_shown {
        return skipped(rule, origin, DUPLICATE_EQUIVALENCE.to_string());
    }
    let (subject, labeled_like) =
        equivalence_pair(&entry.rule).expect("an equivalence entry holds an Equivalence rule");
    FcontextSummary {
        rule,
        origin,
        action: BuildAction::Emits(rule2_lines(subject, labeled_like).join("\n")),
    }
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
        .any(|r| is_included(r) && renders_as_replay(r, &equivalences));
    if any_replay {
        if sel.fcontext_source == FcontextSource::SemanageList {
            rendering.preamble.insert(0, SEMANAGE_LIST_NOTE.to_string());
        }
        rendering.rules.extend(replay_paragraph());
    }

    for rule in &sel.fcontext_rules {
        if let FcontextRule::Unparsed { .. } = rule {
            rendering
                .rules
                .extend(fixme_lines(&unrenderable_cause(rule)));
            continue;
        }
        if !is_included(rule) {
            continue;
        }
        let Some(replay) = ReplayRule::of(rule) else {
            // An included rule that is not a replay is an equivalence. A row
            // absent from the list was deduplicated against an identical
            // earlier one, which has already been emitted.
            if let Some(idx) = entry_of(&equivalences, |row| row == rule) {
                emit_equivalence(&equivalences[idx], &mut emitted[idx], &mut rendering);
            }
            continue;
        };

        let lines = match collision(&equivalences, rule) {
            Some(cause) => fixme_lines(&cause),
            None => {
                // The equivalence that makes a `/srv` rule apply to the real
                // path goes ahead of the first rule needing it (§ 3.1).
                let synthesis = var_symlink_target(replay.spec).and_then(|target| {
                    entry_of(&equivalences, |row| subject_of(row) == Some(target))
                });
                if let Some(idx) = synthesis {
                    emit_equivalence(&equivalences[idx], &mut emitted[idx], &mut rendering);
                }
                rendering.emits_command = true;
                let args = rule1_args(replay.file_type, replay.context);
                rule1_lines(replay.spec, &args.delete, &args.add_modify)
            }
        };
        rendering.rules.extend(lines);
    }

    rendering
}

/// The two variants rule 1 replays, carrying only the fields the replay
/// needs. Converting once here is what keeps `FcontextRule::Equivalence` and
/// `FcontextRule::Unparsed` out of [`rule1_args`], whose `None` context means
/// "render `<<none>>`".
struct ReplayRule<'a> {
    spec: &'a str,
    file_type: FcontextFileType,
    /// `None` for an `Unlabel`, which carries no context field.
    context: Option<&'a SelinuxContext>,
}

impl<'a> ReplayRule<'a> {
    fn of(rule: &'a FcontextRule) -> Option<Self> {
        match rule {
            FcontextRule::Label {
                spec,
                file_type,
                context,
                ..
            } => Some(ReplayRule {
                spec,
                file_type: *file_type,
                context: Some(context),
            }),
            FcontextRule::Unlabel {
                spec, file_type, ..
            } => Some(ReplayRule {
                spec,
                file_type: *file_type,
                context: None,
            }),
            _ => None,
        }
    }
}

/// Why a `Label`/`Unlabel` cannot be rendered as a command, if it cannot:
/// its own operands, or a collision with an equivalence this block adds.
fn collision(equivalences: &[Equivalence], rule: &FcontextRule) -> Option<String> {
    if !is_renderable(rule) {
        return Some(unrenderable_cause(rule));
    }
    let spec = ReplayRule::of(rule)?.spec;
    // Measured on rhel9/rhel-bootc:9.8 (2026-10-28): with `/var/srv = /srv`
    // in the store, `semanage fcontext -a -t t "/var/srv/data(/.*)?"` exits 1
    // with "File spec ... conflicts with equivalency rule", and so does the
    // `-m`. Rule 1 has no `|| true` after the `-m`, so the build would die,
    // and it would die because of an equivalence this block added two lines
    // earlier. The refusal is deliberate rather than a rewrite of the
    // operator's spec: the rewritten form is only equivalent while the
    // equivalence is in place, which is a target-side fact the renderer
    // cannot know, so the FIXME hands the operator the rewrite as advice.
    let (subject, labeled_like) = equivalences
        .iter()
        .filter(|entry| entry.not_emitted.is_none())
        .find_map(|entry| {
            let (subject, labeled_like) = equivalence_pair(&entry.rule)?;
            begins_with_toplevel(spec, subject).then_some((subject, labeled_like))
        })?;
    let rewritten = format!("{labeled_like}{}", &spec[subject.len()..]);
    Some(collision_cause(spec, subject, &rewritten))
}

/// Whether a rule renders as the rule 1 replay triple.
fn renders_as_replay(rule: &FcontextRule, equivalences: &[Equivalence]) -> bool {
    is_label(rule) && collision(equivalences, rule).is_none()
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
    r#"      printf 'FIXME: skipped the file context rule for %s; the target maps that path as an equivalence, which semanage fcontext -d would delete instead of a label; delete it with semanage fcontext -d -e <its target> %s, or exclude this rule, then rebuild\n' "$spec" "$spec" >&2; \"#,
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
///
/// **The listing is captured and its failure stops the build, which § 3.1
/// says is unnecessary.** The spec's reason is that `add_equal` refuses a
/// duplicate subject, so a failed listing would leave `$eq` empty, run the
/// `-a -e`, and stop on semanage's own message. Measured false on
/// `registry.redhat.io/rhel9/rhel-bootc:9.8` with `libsemanage-3.6-5.el9_6`
/// (2026-10-28): with `/var/srv = /opt/srv` already in the store, the `-a -e`
/// prints `Equivalence class for /var/srv already exists, modifying instead`
/// and **exits 0**, replacing the operator's equivalence with no diagnostic.
/// That is the same auto-modify `-a` performs on a duplicate label key, which
/// the spec already recorded as a correction but did not carry across to this
/// rule. The capture is rule 1's, applied symmetrically, and it is the only
/// thing standing between a failed listing and silent destruction of target
/// state. A spec correction is filed; do not remove this.
fn rule2_lines(subject: &str, labeled_like: &str) -> Vec<String> {
    // `{{` and `}}` are `format!`'s escapes; the emitted awk program carries
    // single braces.
    vec![
        format!(
            r#"RUN eqs="$(semanage fcontext -l -C)" || {{ printf 'FIXME: could not list the target file contexts before adding the equivalence for %s; fix semanage in the base, or exclude this rule, then rebuild\n' '{subject}' >&2; exit 1; }}; \"#
        ),
        format!(
            r#"    eq="$(printf '%s\n' "$eqs" | awk -v s='{subject}' '$2 == "=" && $1 == s {{ print $3 }}')"; \"#
        ),
        format!(
            r#"    if [ -z "$eq" ]; then semanage fcontext -a -e '{labeled_like}' '{subject}'; \"#
        ),
        // The diagnostic is a static `printf` format with the operands as
        // arguments: a double-quoted `echo` would run the backticks a
        // target's own equivalence table can put in `$eq`. It spells the
        // whole delete command because `semanage fcontext -d -e` without
        // both operands answers with a KeyError.
        format!(
            r#"    elif [ "$eq" != '{labeled_like}' ]; then printf 'FIXME: %s is already equivalent to %s on the target, not %s; delete that equivalence with semanage fcontext -d -e %s %s, or exclude this rule, then rebuild\n' '{subject}' "$eq" '{labeled_like}' "$eq" '{subject}' >&2; exit 1; fi"#
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

/// `context` is `None` for an `Unlabel`, which carries no context field and
/// renders the `<<none>>` literal instead. The two are parameters rather than
/// a `&FcontextRule` so that no other variant can reach this function and
/// pick up an unlabel by default.
fn rule1_args(file_type: FcontextFileType, context: Option<&SelinuxContext>) -> Rule1Args {
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

/// The paragraph that opens the fcontext rules.
///
/// The count is the rules collected from the source host, including any the
/// block could not render: it is what this block is responsible for, not what
/// it applies, and the sentence about FIXMEs says so. "when bootc pulls the
/// image" is the moment ostree-ext relabels against the image's own policy
/// (`container/store.rs`, verified on rhel9/rhel-bootc:9.8); deployment
/// checkout is later and is the wrong place to look when debugging.
fn count_paragraph(count: usize) -> Vec<String> {
    let noun = if count == 1 { "rule" } else { "rules" };
    vec![
        format!(
            "# SELinux file contexts ({count} custom {noun}). Labels for image content are computed"
        ),
        "# when bootc pulls the image, from the image's own policy under /etc/selinux, so".into(),
        "# a rule only needs to be in the policy store: no restorecon at build time. Each".into(),
        "# rule below is either a command or a FIXME saying why it could not be one. Data".into(),
        "# you restore into /var (/var/srv included) keeps the labels it arrived with;".into(),
        "# run restorecon -R on it before starting the services that use it.".into(),
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

/// Why a synthesized equivalence exists. One sentence, one place: the
/// Containerfile's comment and [`EquivalenceOrigin::Synthesized`]'s `reason`
/// both read it, so the audit report and the build cannot come to say
/// different things about the same row.
fn symlink_fact(link: &str, target: &str) -> String {
    format!("{link} is a symlink to {target} on this base and the policy has no {target} alias")
}

fn symlink_paragraph(link: &str, target: &str) -> Vec<String> {
    vec![
        format!("# {};", symlink_fact(link, target)),
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

/// The cause line of a rule no operand check passed: every failing operand
/// with its reason, then enough of the rule to locate it in the source host's
/// policy. Line 1 is generic, so without the second half a type-rejected rule
/// would name no path and a spec-rejected rule would name no type.
fn unrenderable_cause(rule: &FcontextRule) -> String {
    if let FcontextRule::Unparsed { raw } = rule {
        // The line is its own identity; § 5.1's "raw line" case.
        return format!("{raw:?} ({UNPARSED_LINE})");
    }
    let faults: Vec<String> = rule_faults(rule)
        .iter()
        .map(|f| format!("{:?} ({}: {})", f.value, f.field, f.reason))
        .collect();
    format!("{}; {}", faults.join(", "), rule_identity(rule))
}

/// How a rule is named once its own operands can no longer be trusted to
/// name it.
fn rule_identity(rule: &FcontextRule) -> String {
    match rule {
        FcontextRule::Label { spec, context, .. } => {
            format!("the rule labels {spec:?} as type {:?}", context.type_)
        }
        FcontextRule::Unlabel { spec, .. } => format!("the rule unlabels {spec:?}"),
        FcontextRule::Equivalence {
            subject,
            labeled_like,
            ..
        } => format!("the rule maps {subject:?} to {labeled_like:?}"),
        FcontextRule::Unparsed { raw } => format!("the collected line was {raw:?}"),
    }
}

/// Marks the row of a conflict that the renderer contributes, so that the
/// instruction below names an action the operator actually has.
const SYNTHESIS_ATTRIBUTION: &str = "inspectah adds this one itself";

/// Whether a row is the equivalence the renderer synthesizes for a
/// [`VAR_SYMLINKED_TOPLEVELS`] pair.
///
/// A *collected* row carrying the same two operands answers yes, and that is
/// right rather than a false positive: excluding it does not remove it,
/// because the synthesis puts it back. The attribution is about what the
/// operator can act on, not about which struct the row came from.
fn is_synthesized_pair(rule: &FcontextRule) -> bool {
    equivalence_pair(rule).is_some_and(|pair| {
        VAR_SYMLINKED_TOPLEVELS
            .iter()
            .any(|(link, target)| pair == (*target, *link))
    })
}

/// The cause line of a conflicting equivalence pair: both rows, and the
/// instruction that resolves them.
///
/// When one row is the renderer's own, it is named as such. Without that,
/// "keep one and exclude the other" offers a choice that is only half
/// available and does not say which half: a synthesized row has no
/// disposition to turn off. Two collected rows are both the operator's and
/// keep the original wording, because the note would be noise there.
fn conflict_cause(rows: &[FcontextRule]) -> String {
    let pairs: Vec<String> = rows
        .iter()
        .filter_map(|rule| {
            let (subject, labeled_like) = equivalence_pair(rule)?;
            Some(match is_synthesized_pair(rule) {
                true => format!("{subject:?} = {labeled_like:?} ({SYNTHESIS_ATTRIBUTION})"),
                false => format!("{subject:?} = {labeled_like:?}"),
            })
        })
        .collect();
    let instruction = match rows.iter().any(is_synthesized_pair) {
        true => "exclude the other and rebuild",
        false => "keep one and exclude the other, then rebuild",
    };
    format!(
        "these equivalences conflict: {}; {instruction}",
        pairs.join(" and ")
    )
}

/// The cause line of a rule whose path an equivalence this block adds makes
/// unaddressable. `semanage` refuses such a spec on both the `-a` and the
/// `-m`, so emitting it would end the build.
fn collision_cause(spec: &str, subject: &str, rewritten: &str) -> String {
    format!(
        "{spec:?} sits under {subject:?}, which this block adds as an equivalence, and \
         semanage refuses a file context under an equivalence subject; label {rewritten:?} \
         instead, which reaches the same files through that equivalence, or exclude the \
         equivalence, then rebuild"
    )
}

// ---------------------------------------------------------------------------
// Operand validation (§ 3.1 rule 4)
// ---------------------------------------------------------------------------

const NOT_ABSOLUTE: &str = "it is not an absolute path";
const UNQUOTABLE_BYTE: &str = "it carries an apostrophe or a control character";
const HAS_WHITESPACE: &str = "it carries whitespace";
const REGEX_CHARACTER: &str = "it carries a regex character, and -e takes a literal path";
const NOT_BARE_TOKEN: &str = "it is not a bare token";
const UNPARSED_LINE: &str = "the collector could not place this line";

/// One operand that stops its rule from being rendered.
struct OperandFault<'a> {
    /// The field the operand came from, named as § 3.1 names it.
    field: &'static str,
    value: &'a str,
    reason: &'static str,
}

/// Why a label `spec` cannot be rendered, if it cannot.
///
/// Whitespace is refused even though single quotes make it inert, because the
/// build-time guard reads the target's listing through `awk`, which splits on
/// a single space where the collector deliberately splits on two or more. A
/// space-bearing spec would slip past the guard and reach a
/// `semanage fcontext -d` that deletes the target's equivalence instead of a
/// label key.
fn spec_fault(spec: &str) -> Option<&'static str> {
    if !spec.starts_with('/') {
        // `semanage fcontext -a --help` answers with usage text and exits 0,
        // so a spec that is not a path would skip its rule silently.
        Some(NOT_ABSOLUTE)
    } else if spec.contains(is_unquotable) {
        Some(UNQUOTABLE_BYTE)
    } else if spec.contains(char::is_whitespace) {
        Some(HAS_WHITESPACE)
    } else {
        None
    }
}

/// Why an equivalence operand cannot be rendered, if it cannot. It is
/// additionally a literal absolute path prefix: `-e` takes a path, not a
/// regex.
fn equivalence_operand_fault(operand: &str) -> Option<&'static str> {
    if !operand.starts_with('/') {
        Some(NOT_ABSOLUTE)
    } else if operand.contains(is_unquotable) {
        Some(UNQUOTABLE_BYTE)
    } else if operand.contains(char::is_whitespace) {
        Some(HAS_WHITESPACE)
    } else if operand.contains(EQUIVALENCE_FORBIDDEN) {
        Some(REGEX_CHARACTER)
    } else {
        None
    }
}

/// `type_`, `user` and `level` are the only operands the emitted command
/// carries unquoted, so this check, and not any quoting, is what keeps them
/// inert. It is coupled to the emission conditions in [`rule1_args`]: a field
/// is checked exactly when it is emitted.
fn bare_token_fault(token: &str) -> Option<&'static str> {
    let ok = !token.is_empty()
        && token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ':' | '-'));
    (!ok).then_some(NOT_BARE_TOKEN)
}

/// Every operand of a rule that cannot be rendered, in the order the command
/// would have carried them. Empty means the rule renders.
fn rule_faults(rule: &FcontextRule) -> Vec<OperandFault<'_>> {
    fn fault<'a>(
        field: &'static str,
        value: &'a str,
        reason: Option<&'static str>,
    ) -> Option<OperandFault<'a>> {
        reason.map(|reason| OperandFault {
            field,
            value,
            reason,
        })
    }

    let mut faults = Vec::new();
    match rule {
        FcontextRule::Label { spec, context, .. } => {
            faults.extend(fault("spec", spec, spec_fault(spec)));
            faults.extend(fault(
                "type",
                &context.type_,
                bare_token_fault(&context.type_),
            ));
            if context.user != DEFAULT_USER {
                faults.extend(fault(
                    "user",
                    &context.user,
                    bare_token_fault(&context.user),
                ));
            }
            if let Some(level) = context.level.as_deref()
                && level != DEFAULT_LEVEL
            {
                faults.extend(fault("level", level, bare_token_fault(level)));
            }
        }
        FcontextRule::Unlabel { spec, .. } => {
            faults.extend(fault("spec", spec, spec_fault(spec)));
        }
        FcontextRule::Equivalence {
            subject,
            labeled_like,
            ..
        } => {
            faults.extend(fault(
                "subject",
                subject,
                equivalence_operand_fault(subject),
            ));
            faults.extend(fault(
                "labeled_like",
                labeled_like,
                equivalence_operand_fault(labeled_like),
            ));
        }
        FcontextRule::Unparsed { raw } => {
            faults.extend(fault("raw", raw, Some(UNPARSED_LINE)));
        }
    }
    faults
}

/// Whether every operand of a rule passed its check.
fn is_renderable(rule: &FcontextRule) -> bool {
    rule_faults(rule).is_empty()
}

// ---------------------------------------------------------------------------
// Rule plumbing
// ---------------------------------------------------------------------------

/// Whether the export carries this rule at all. An `Unparsed` line never is:
/// nothing decided it, so nothing can include it.
pub fn is_included(rule: &FcontextRule) -> bool {
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
fn rows_of(entry: &Equivalence) -> &[FcontextRule] {
    match &entry.origin {
        EquivalenceOrigin::Conflict { rows } => rows,
        _ => std::slice::from_ref(&entry.rule),
    }
}

/// The entry that speaks for a row matching `pred`, conflict halves
/// included: a rule reaches its entry through whichever row named it.
fn entry_of(entries: &[Equivalence], pred: impl Fn(&FcontextRule) -> bool) -> Option<usize> {
    entries
        .iter()
        .position(|entry| rows_of(entry).iter().any(&pred))
}

/// Fold another row claiming the same subject into an entry: a duplicate
/// changes nothing, a different target makes the entry a conflict.
fn absorb(entry: &mut Equivalence, incoming: &FcontextRule) {
    if let EquivalenceOrigin::Conflict { rows } = &mut entry.origin {
        if !rows.contains(incoming) {
            rows.push(incoming.clone());
        }
        return;
    }
    if &entry.rule == incoming || equivalence_pair(&entry.rule) == equivalence_pair(incoming) {
        return;
    }
    entry.origin = EquivalenceOrigin::Conflict {
        rows: vec![entry.rule.clone(), incoming.clone()],
    };
}

/// Whether a collected rule makes the equivalence for `link` necessary: an
/// included `Label`/`Unlabel` the block will actually emit, whose spec begins
/// with the symlinked top level.
fn triggers_synthesis(rule: &FcontextRule, link: &str) -> bool {
    let Some(replay) = ReplayRule::of(rule) else {
        return false;
    };
    is_included(rule) && is_renderable(rule) && begins_with_toplevel(replay.spec, link)
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
fn emit_equivalence(entry: &Equivalence, emitted: &mut bool, rendering: &mut FcontextRendering) {
    if *emitted {
        return;
    }
    *emitted = true;

    let Equivalence {
        rule,
        origin,
        not_emitted,
    } = entry;
    // The cause the entry carries is the one printed, so the reports and the
    // build cannot name different reasons for the same refusal. Only a
    // synthesized row nothing needs has a cause with no FIXME, and nothing
    // reaches this function for one: it is reached from a rule that asks for
    // it, and a rule asking for it is a surviving trigger.
    if let Some(cause) = not_emitted {
        rendering.rules.extend(fixme_lines(cause));
        return;
    }
    let Some((subject, labeled_like)) = equivalence_pair(rule) else {
        return;
    };
    if matches!(origin, EquivalenceOrigin::Synthesized { .. }) {
        rendering
            .rules
            .extend(symlink_paragraph(labeled_like, subject));
    }
    rendering.rules.extend(rule2_lines(subject, labeled_like));
    rendering.emits_command = true;
}
