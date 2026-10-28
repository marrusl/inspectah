//! Shared fixtures and parsing helpers for the SELinux block tests.
//!
//! Two test targets read this module: `smoke_render.rs`, which compares the
//! rendered block against the § 5.1 text, and `selinux_block_shell_test.rs`,
//! which executes the block's `RUN` bodies under a stub `semanage`. They have
//! to agree on how a `RUN` is split out of the block and on how a build folds
//! its continuations, so that lives here once.

#![allow(dead_code)]

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::FindingKind;
use inspectah_core::types::selinux::{
    FcontextFileType, FcontextRule, FcontextSource, SelinuxContext, SelinuxPortLabel,
    SelinuxSection,
};
use inspectah_pipeline::render::containerfile;

/// The banner `section()` emits for the Security & Access Control section.
pub const SELINUX_HEADER: &str = "# === Security & Access Control ===";

/// Banner prefix of every Containerfile section, used to confirm the blank
/// line that ends the SELinux block really ended it.
const SECTION_BANNER_PREFIX: &str = "# === ";

// ---------------------------------------------------------------------------
// Snapshot fixtures
// ---------------------------------------------------------------------------

/// `system_u:object_r:<type_>:s0`, the context every rule here carries unless
/// it says otherwise. `system_u` and `s0` are the two values § 5.1 suppresses
/// the `-s` and `-r` flags for, so a rule built from this context renders
/// `-t` alone.
pub fn ctx(type_: &str) -> SelinuxContext {
    SelinuxContext {
        user: "system_u".into(),
        role: "object_r".into(),
        type_: type_.into(),
        level: Some("s0".into()),
    }
}

pub fn label(spec: &str, file_type: FcontextFileType, type_: &str) -> FcontextRule {
    FcontextRule::Label {
        spec: spec.into(),
        file_type,
        context: ctx(type_),
        disposition: FindingKind::included(),
        aggregate: None,
    }
}

pub fn unlabel(spec: &str, file_type: FcontextFileType) -> FcontextRule {
    FcontextRule::Unlabel {
        spec: spec.into(),
        file_type,
        disposition: FindingKind::included(),
        aggregate: None,
    }
}

pub fn equivalence(subject: &str, labeled_like: &str) -> FcontextRule {
    FcontextRule::Equivalence {
        subject: subject.into(),
        labeled_like: labeled_like.into(),
        disposition: FindingKind::included(),
        aggregate: None,
    }
}

pub fn port_label(label_type: &str, protocol: &str, port: &str) -> SelinuxPortLabel {
    SelinuxPortLabel {
        protocol: protocol.into(),
        port: port.into(),
        label_type: label_type.into(),
        disposition: FindingKind::included(),
        locked: false,
        aggregate: None,
    }
}

/// A snapshot whose only populated section is SELinux.
pub fn snapshot(
    rules: Vec<FcontextRule>,
    source: FcontextSource,
    ports: Vec<SelinuxPortLabel>,
) -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.selinux = Some(SelinuxSection {
        mode: "enforcing".into(),
        fcontext_rules: rules,
        fcontext_source: source,
        port_labels: ports,
        ..Default::default()
    });
    snap
}

/// The common case: policy-store order, no port labels.
pub fn snapshot_rules(rules: Vec<FcontextRule>) -> InspectionSnapshot {
    snapshot(rules, FcontextSource::PolicyStore, Vec::new())
}

// ---------------------------------------------------------------------------
// Block extraction
// ---------------------------------------------------------------------------

/// The body lines of the Security & Access Control section, banner excluded.
///
/// Panics when the section is absent, so a fixture that silently renders no
/// block fails its test instead of passing every absence assertion it makes.
pub fn selinux_block(output: &str) -> Vec<String> {
    let lines: Vec<&str> = output.lines().collect();
    let start = lines
        .iter()
        .position(|l| *l == SELINUX_HEADER)
        .unwrap_or_else(|| panic!("no SELinux section in the rendered Containerfile:\n{output}"));

    let mut body = Vec::new();
    let mut idx = start + 1;
    while idx < lines.len() && !lines[idx].is_empty() {
        body.push(lines[idx].to_string());
        idx += 1;
    }
    // `section()` ends a section with one blank line and the next section
    // opens with its own banner. If anything else follows the blank line the
    // block is longer than this function thinks and every "no line contains"
    // assertion below it would be reading a truncated block.
    if idx + 1 < lines.len() {
        let next = lines[idx + 1];
        assert!(
            next.starts_with(SECTION_BANNER_PREFIX),
            "the blank line after the SELinux block is not a section terminator; \
             the next line is {next:?}, so the block was truncated"
        );
    }
    assert!(
        !body.is_empty(),
        "the SELinux section rendered an empty body"
    );
    body
}

/// One `RUN` instruction of the block.
pub struct RunInstruction {
    /// The instruction as rendered, its lines rejoined with `\n`. This is what
    /// § 8.9's whole-body comparison compares.
    pub rendered: String,
    /// The command a build hands to `/bin/sh -c`: the `RUN` keyword removed
    /// and every `\`-continuation folded away, the way the Containerfile
    /// parser folds them (the backslash and the newline go, the continuation
    /// line's own leading whitespace stays).
    pub command: String,
}

/// Every `RUN` of the block, in order.
///
/// Total over the block: a line that is neither blank, nor a comment, nor part
/// of a `RUN` is a panic rather than a skip. A dropped line is an injected
/// line the executed-shell test would never run, which is exactly the failure
/// this test suite exists to catch.
pub fn run_instructions(block: &[String]) -> Vec<RunInstruction> {
    let mut runs: Vec<Vec<String>> = Vec::new();
    let mut open = false;

    for line in block {
        if open {
            assert!(
                !line.trim_start().starts_with('#'),
                "comment line inside a RUN continuation: {line:?}. A build's parser \
                 drops it, so the executed command would differ from the rendered one."
            );
            let last = runs.last_mut().expect("a RUN is open");
            open = line.ends_with('\\');
            last.push(line.clone());
            continue;
        }
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        assert!(
            line.starts_with("RUN "),
            "unclassifiable line in the SELinux block: {line:?}. Only blank lines, \
             comments, and RUN instructions are understood here."
        );
        open = line.ends_with('\\');
        runs.push(vec![line.clone()]);
    }
    assert!(
        !open,
        "the SELinux block ends inside a `\\` continuation; the last RUN is unterminated"
    );

    runs.into_iter()
        .map(|lines| RunInstruction {
            rendered: lines.join("\n"),
            command: fold_continuations(&lines),
        })
        .collect()
}

/// Render the block for a snapshot and split out its `RUN` instructions.
pub fn block_runs(snap: &InspectionSnapshot) -> Vec<RunInstruction> {
    let output = containerfile::render_containerfile(snap, None, None);
    run_instructions(&selinux_block(&output))
}

/// Fold one rendered `RUN` into the command a build hands to `/bin/sh -c`.
/// Lets the executed-shell test run a body it built itself, which is how
/// § 8.9's negative case gets a round 2 `echo` diagnostic in front of a shell.
pub fn command_of(rendered: &str) -> String {
    let lines: Vec<String> = rendered.lines().map(|l| l.to_string()).collect();
    fold_continuations(&lines)
}

fn fold_continuations(lines: &[String]) -> String {
    let mut cmd = String::new();
    for (i, line) in lines.iter().enumerate() {
        let text = if i == 0 {
            line.strip_prefix("RUN ").expect("a RUN instruction")
        } else {
            line.as_str()
        };
        match text.strip_suffix('\\') {
            Some(head) => cmd.push_str(head),
            None => cmd.push_str(text),
        }
    }
    cmd
}

// ---------------------------------------------------------------------------
// Expected bodies — the § 3.1 templates, transcribed, with holes
// ---------------------------------------------------------------------------

/// § 3.1 rule 1, transcribed from the spec. `@D_ARGS@` is what the `-d`
/// carries beyond the spec (`" -f d"` or nothing), `@AM_ARGS@` the arguments
/// the `-a` and the `-m` share.
const RULE1_TEMPLATE: &str = r#"RUN spec='@SPEC@'; \
    eqs="$(semanage fcontext -l -C)" || { printf 'FIXME: could not list the target file contexts before replaying the rule for %s; fix semanage in the base, or exclude this rule, then rebuild\n' "$spec" >&2; exit 1; }; \
    if printf '%s\n' "$eqs" | awk '$2 == "=" { print $1 }' | grep -qxF -- "$spec"; then \
      printf 'FIXME: skipped the file context rule for %s; the target maps that path as an equivalence, which semanage fcontext -d would delete instead of a label; delete it with semanage fcontext -d -e, or exclude this rule, then rebuild\n' "$spec" >&2; \
    else semanage fcontext -d@D_ARGS@ "$spec" 2>/dev/null || true; \
      semanage fcontext -a @AM_ARGS@ "$spec" \
      || semanage fcontext -m @AM_ARGS@ "$spec"; fi"#;

/// § 3.1 rule 2, transcribed from the spec.
const RULE2_TEMPLATE: &str = r#"RUN eq="$(semanage fcontext -l -C | awk -v s='@SUBJECT@' '$2 == "=" && $1 == s { print $3 }')"; \
    if [ -z "$eq" ]; then semanage fcontext -a -e '@LABELED_LIKE@' '@SUBJECT@'; \
    elif [ "$eq" != '@LABELED_LIKE@' ]; then printf 'FIXME: %s is already equivalent to %s on the target, not %s; delete that equivalence with semanage fcontext -d -e, or exclude this rule, then rebuild\n' '@SUBJECT@' "$eq" '@LABELED_LIKE@' >&2; exit 1; fi"#;

/// The rule 1 body for a rule whose `-d` carries `d_args` and whose `-a`/`-m`
/// carry `am_args`.
pub fn rule1_rendered(spec: &str, d_args: &str, am_args: &str) -> String {
    RULE1_TEMPLATE
        .replace("@D_ARGS@", d_args)
        .replace("@AM_ARGS@", am_args)
        .replace("@SPEC@", spec)
}

/// The rule 2 body for an equivalence.
pub fn rule2_rendered(subject: &str, labeled_like: &str) -> String {
    RULE2_TEMPLATE
        .replace("@LABELED_LIKE@", labeled_like)
        .replace("@SUBJECT@", subject)
}

/// The `printf` line of a rule 2 body, the half the § 8.9 negative case
/// substitutes away.
pub fn rule2_printf_line(subject: &str, labeled_like: &str) -> String {
    format!(
        "    elif [ \"$eq\" != '{labeled_like}' ]; then printf 'FIXME: %s is already equivalent to %s on the target, not %s; delete that equivalence with semanage fcontext -d -e, or exclude this rule, then rebuild\\n' '{subject}' \"$eq\" '{labeled_like}' >&2; exit 1; fi"
    )
}

/// The round 2 diagnostic the spec replaced: one `echo` over a double-quoted
/// string with the operands interpolated into it. § 8.9 substitutes this for
/// the `printf` line and requires the whole-body comparison to reject it.
pub fn rule2_echo_line(subject: &str, labeled_like: &str) -> String {
    format!(
        "    elif [ \"$eq\" != '{labeled_like}' ]; then echo \"FIXME: {subject} is already equivalent to $eq on the target, not {labeled_like}; delete that equivalence with semanage fcontext -d -e, or exclude this rule, then rebuild\" >&2; exit 1; fi"
    )
}

/// A rule 2 body with the round 2 `echo` diagnostic in place of the `printf`.
pub fn rule2_rendered_round2_echo(subject: &str, labeled_like: &str) -> String {
    let good = rule2_rendered(subject, labeled_like);
    let printf_line = rule2_printf_line(subject, labeled_like);
    assert!(
        good.contains(&printf_line),
        "the rule 2 template no longer holds the printf line this substitution replaces"
    );
    good.replace(&printf_line, &rule2_echo_line(subject, labeled_like))
}

// ---------------------------------------------------------------------------
// The install precondition and the unrenderable form
// ---------------------------------------------------------------------------

/// § 3.1's install precondition: two comment lines and one `RUN`.
pub const INSTALL_PRECONDITION: &[&str] = &[
    "# The file context and port rules below need semanage, which this base does not",
    "# carry; policycoreutils-python-utils provides it.",
    "RUN dnf install -y policycoreutils-python-utils",
];

/// The package `semanage` comes from. Named because three separate rules are
/// about where this string may and may not appear.
pub const SEMANAGE_PACKAGE: &str = "policycoreutils-python-utils";

/// First line of § 5.1's two-line unrenderable form: the imperative, which the
/// § 5.6 wording rule puts ahead of the cause.
pub const UNRENDERABLE_FIXME_INSTRUCTION: &str = "# FIXME: add this file context by hand with: RUN semanage fcontext -a -t <type> '<spec>' || semanage fcontext -m -t <type> '<spec>' (labels apply at deploy; no restorecon needed)";

/// Prefix of the second line, which carries the cause.
pub const UNRENDERABLE_FIXME_CAUSE_PREFIX: &str = "#   the collected rule could not be rendered: ";

/// The two-line unrenderable form for one offending operand. The operand is
/// quoted with Rust's `Debug` escaping, the repo's existing convention for a
/// FIXME that has to show a byte single quotes do not make inert
/// (`containerfile.rs:1438`, `:1463`, `:1501`).
pub fn unrenderable_fixme(offending: &str) -> Vec<String> {
    vec![
        UNRENDERABLE_FIXME_INSTRUCTION.to_string(),
        format!("{UNRENDERABLE_FIXME_CAUSE_PREFIX}{offending:?}"),
    ]
}

// ---------------------------------------------------------------------------
// § 5.1 prose
// ---------------------------------------------------------------------------

/// The `# NOTE:` caveat a `SemanageList` source puts before the first rule.
pub const SEMANAGE_LIST_NOTE: &str = "# NOTE: rule order comes from a sorted listing, not insertion order; if two expressions overlap, reorder so the intended winner is last.";
