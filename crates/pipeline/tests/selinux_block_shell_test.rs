//! The SELinux block's `RUN` bodies, executed (spec § 8.9).
//!
//! Every other test in this suite reads the renderer's output. This one runs
//! it. Each case takes a `RUN` of the block, folds its continuations the way a
//! build does, and hands the result to `/bin/sh -c` under a `PATH` whose first
//! entry holds a stub `semanage`. `awk`, `grep`, and `printf` are the host's.
//!
//! **What makes an absence assertion mean something here.** Every case asserts
//! that no operand executed, and an absence is only evidence if the harness
//! can see a presence. Three things make that true:
//!
//! 1. Execution is measured by a side effect, never by text: the stub
//!    directory holds a `c6_exec` script that creates a file whose absolute
//!    path is baked into the script when the case writes it. No environment
//!    variable can misdirect it, and `C6_EXECUTED` appearing in stderr as data
//!    cannot be mistaken for a run.
//! 2. Operands that would execute something name `c6_exec`, not `id`. An
//!    operand that runs `id` leaves no file behind, so a case built on one
//!    asserts the absence of something that could never have appeared.
//! 3. Each case asserts the block's command inventory before it runs anything.
//!    A renderer that refuses the rule and emits a comment leaves nothing to
//!    execute, and without the inventory check every absence assertion below
//!    would hold against it.
//!
//! [`selinux_round_2_echo_diagnostic_executes_the_operands`] is the paired positive:
//! the same harness, the same operands, the round 2 `echo` diagnostic the spec
//! rejected, and the marker file present.

mod selinux_support;

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::selinux::FcontextFileType;
use selinux_support as sel;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

/// `HOME` in every case's environment, so an operand carrying `$HOME` has one
/// expected expansion when a buggy renderer lets it expand.
const FIXED_HOME: &str = "/c6-home";

/// The file `c6_exec` creates. Its absence is the claim every case makes.
const EXEC_MARKER: &str = "C6_EXECUTED";

/// The `semanage fcontext -l -C` call, logged like any other.
const LISTING_CALL: &[&str] = &["fcontext", "-l", "-C"];

const HTTPD_TYPE: &str = "httpd_sys_content_t";

/// The two operands § 8.9 runs the equivalence form with: every byte an
/// equivalence operand is allowed to carry that a shell would act on.
const SHELL_SUBJECT: &str = "/literal/`printf${IFS}C6_EXECUTED`";
const SHELL_LABELED_LIKE: &str = "/l{a,b}$HOME`c6_exec`";

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

struct Harness {
    dir: TempDir,
}

struct Outcome {
    status: i32,
    stdout: String,
    stderr: String,
    /// The stub's argv log, one argument per line, across every call.
    argv: Vec<String>,
    /// Whether `c6_exec` ran.
    executed: bool,
}

impl Harness {
    /// `listing` is what the stub prints for `fcontext -l -C`; `listing_exit`
    /// is the status it returns for that call and `add_exit` the status it
    /// returns for any `fcontext -a`.
    fn new(listing: &str, listing_exit: i32, add_exit: i32) -> Self {
        let dir = TempDir::new().expect("temp dir");
        let bin = dir.path().join("bin");
        fs::create_dir(&bin).expect("stub dir");

        let listing_path = dir.path().join("listing");
        fs::write(&listing_path, listing).expect("listing");

        let argv_log = dir.path().join("argv.log");
        let marker = dir.path().join(EXEC_MARKER);

        write_script(
            &bin.join("semanage"),
            &format!(
                "#!/bin/sh\n\
                 for a in \"$@\"; do\n\
                 \x20 printf '%s\\n' \"$a\" >> '{log}'\n\
                 done\n\
                 if [ \"$1\" = fcontext ] && [ \"$2\" = -l ] && [ \"$3\" = -C ]; then\n\
                 \x20 cat '{listing}'\n\
                 \x20 exit {listing_exit}\n\
                 fi\n\
                 if [ \"$2\" = -a ]; then\n\
                 \x20 exit {add_exit}\n\
                 fi\n\
                 exit 0\n",
                log = argv_log.display(),
                listing = listing_path.display(),
            ),
        );
        // The marker path is a literal in the script, not a lookup. A case
        // whose environment lost a variable would otherwise write the marker
        // somewhere the assertion never looks and report a false absence.
        write_script(
            &bin.join("c6_exec"),
            &format!("#!/bin/sh\n: > '{}'\n", marker.display()),
        );

        Harness { dir }
    }

    fn run(&self, command: &str) -> Outcome {
        let bin = self.dir.path().join("bin");
        let host_path = std::env::var("PATH").unwrap_or_default();
        // The command goes in as one argument. Re-quoting it through another
        // shell would be a second parse the real build never performs.
        let out = Command::new("/bin/sh")
            .arg("-c")
            .arg(command)
            .env("PATH", format!("{}:{host_path}", bin.display()))
            .env("HOME", FIXED_HOME)
            .current_dir(self.dir.path())
            .output()
            .expect("run the rendered command");

        let argv = fs::read_to_string(self.dir.path().join("argv.log"))
            .unwrap_or_default()
            .lines()
            .map(|l| l.to_string())
            .collect();

        Outcome {
            status: out.status.code().expect("the shell exited normally"),
            stdout: String::from_utf8(out.stdout).expect("utf-8 stdout"),
            stderr: String::from_utf8(out.stderr).expect("utf-8 stderr"),
            argv,
            executed: self.dir.path().join(EXEC_MARKER).exists(),
        }
    }
}

fn write_script(path: &Path, body: &str) {
    fs::write(path, body).expect("write stub");
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).expect("chmod stub");
}

fn argv(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|p| p.to_string()).collect()
}

/// The assertion every case makes: exit status, whole argv log, exact stdout
/// and stderr, and no execution.
#[track_caller]
fn assert_outcome(
    case: &str,
    got: &Outcome,
    status: i32,
    stdout: &str,
    stderr: &str,
    log: &[&str],
) {
    assert_eq!(got.stdout, stdout, "{case}: stdout");
    assert_eq!(got.stderr, stderr, "{case}: stderr");
    assert_eq!(got.argv, argv(log), "{case}: argv log");
    assert_eq!(got.status, status, "{case}: exit status");
    assert!(
        !got.executed,
        "{case}: an operand executed — the {EXEC_MARKER} marker exists"
    );
}

// ---------------------------------------------------------------------------
// Command selection, with the inventory asserted
// ---------------------------------------------------------------------------

/// The block's `RUN` commands with the install precondition's own dropped.
///
/// Panics when none is left: a case whose rule the renderer refused has
/// nothing to execute, and every absence assertion it makes would hold for the
/// wrong reason.
fn rule_commands(snap: &InspectionSnapshot) -> Vec<String> {
    let commands: Vec<String> = sel::block_runs(snap)
        .into_iter()
        .map(|r| r.command)
        .filter(|c| c != "dnf install -y policycoreutils-python-utils")
        .collect();
    assert!(
        !commands.is_empty(),
        "the block rendered no rule command to execute"
    );
    commands
}

/// The single rule 1 (`Label`/`Unlabel`) command of a fixture.
fn rule1_command(snap: &InspectionSnapshot) -> String {
    let mut found: Vec<String> = rule_commands(snap)
        .into_iter()
        .filter(|c| c.starts_with("spec="))
        .collect();
    assert_eq!(
        found.len(),
        1,
        "expected exactly one rule 1 command, got {found:#?}"
    );
    found.pop().expect("one command")
}

/// The single rule 2 (equivalence) command of a fixture.
fn rule2_command(snap: &InspectionSnapshot) -> String {
    let mut found: Vec<String> = rule_commands(snap)
        .into_iter()
        .filter(|c| c.starts_with("eq="))
        .collect();
    assert_eq!(
        found.len(),
        1,
        "expected exactly one rule 2 command, got {found:#?}"
    );
    found.pop().expect("one command")
}

fn label_command(spec: &str, file_type: FcontextFileType) -> String {
    rule1_command(&sel::snapshot_rules(vec![sel::label(
        spec, file_type, HTTPD_TYPE,
    )]))
}

fn equivalence_command(subject: &str, labeled_like: &str) -> String {
    rule2_command(&sel::snapshot_rules(vec![sel::equivalence(
        subject,
        labeled_like,
    )]))
}

// ---------------------------------------------------------------------------
// Expected diagnostics
// ---------------------------------------------------------------------------

fn conflict_diagnostic(subject: &str, listed: &str, labeled_like: &str) -> String {
    format!(
        "FIXME: {subject} is already equivalent to {listed} on the target, not {labeled_like}; \
         delete that equivalence with semanage fcontext -d -e, or exclude this rule, then rebuild\n"
    )
}

fn skip_diagnostic(spec: &str) -> String {
    format!(
        "FIXME: skipped the file context rule for {spec}; the target maps that path as an \
         equivalence, which semanage fcontext -d would delete instead of a label; delete it \
         with semanage fcontext -d -e, or exclude this rule, then rebuild\n"
    )
}

fn listing_failure_diagnostic(spec: &str) -> String {
    format!(
        "FIXME: could not list the target file contexts before replaying the rule for {spec}; \
         fix semanage in the base, or exclude this rule, then rebuild\n"
    )
}

// ---------------------------------------------------------------------------
// Rule 2: the equivalence form
// ---------------------------------------------------------------------------

#[test]
fn selinux_equivalence_absent_adds_it() {
    for (case, subject, labeled_like) in equivalence_operands() {
        let cmd = equivalence_command(subject, labeled_like);
        let got = Harness::new("", 0, 0).run(&cmd);
        assert_outcome(
            &format!("{case}/absent"),
            &got,
            0,
            "",
            "",
            &[
                LISTING_CALL,
                &["fcontext", "-a", "-e", labeled_like, subject],
            ]
            .concat(),
        );
    }
}

#[test]
fn selinux_equivalence_identical_mutates_nothing() {
    for (case, subject, labeled_like) in equivalence_operands() {
        let cmd = equivalence_command(subject, labeled_like);
        let listing = format!("{subject} = {labeled_like}\n");
        let got = Harness::new(&listing, 0, 0).run(&cmd);
        assert_outcome(&format!("{case}/identical"), &got, 0, "", "", LISTING_CALL);
    }
}

#[test]
fn selinux_equivalence_conflict_refuses_with_one_diagnostic() {
    const LISTED: &str = "/opt/srv";
    for (case, subject, labeled_like) in equivalence_operands() {
        let cmd = equivalence_command(subject, labeled_like);
        let listing = format!("{subject} = {LISTED}\n");
        let got = Harness::new(&listing, 0, 0).run(&cmd);
        assert_outcome(
            &format!("{case}/conflict"),
            &got,
            1,
            "",
            &conflict_diagnostic(subject, LISTED, labeled_like),
            LISTING_CALL,
        );
    }
}

/// The plain pair and the pair carrying every shell-looking byte an
/// equivalence operand may hold.
fn equivalence_operands() -> Vec<(&'static str, &'static str, &'static str)> {
    vec![
        ("plain", "/var/srv", "/srv"),
        ("shell-looking", SHELL_SUBJECT, SHELL_LABELED_LIKE),
    ]
}

// ---------------------------------------------------------------------------
// § 8.9's negative case, which is also this harness's positive control
// ---------------------------------------------------------------------------

#[test]
fn selinux_round_2_echo_diagnostic_executes_the_operands() {
    const LISTED: &str = "/opt/srv";
    let body = sel::rule2_rendered_round2_echo(SHELL_SUBJECT, SHELL_LABELED_LIKE);
    let cmd = sel::command_of(&body);
    let listing = format!("{SHELL_SUBJECT} = {LISTED}\n");
    let got = Harness::new(&listing, 0, 0).run(&cmd);

    // The subject's backticks ran: `printf${IFS}C6_EXECUTED` collapsed to the
    // marker word inside the double-quoted string.
    assert!(
        got.stderr.contains("/literal/C6_EXECUTED"),
        "the round 2 diagnostic did not interpolate the subject: {:?}",
        got.stderr
    );
    assert!(
        got.stderr.contains(FIXED_HOME),
        "the round 2 diagnostic did not expand $HOME: {:?}",
        got.stderr
    );
    // And this is the harness proving it can see a run at all. Every absence
    // assertion in this file is read against this.
    assert!(
        got.executed,
        "the harness cannot detect execution: {EXEC_MARKER} absent after \
         an operand that calls c6_exec ran unquoted"
    );
}

// ---------------------------------------------------------------------------
// Rule 1: the label guard
// ---------------------------------------------------------------------------

/// Every spec § 8.9 replays the label guard with. `id` proves an operand's
/// bytes survive into argv; `c6_exec` proves they did not run.
fn guard_specs() -> Vec<&'static str> {
    vec![
        "/srv/www(/.*)?",
        r"/etc/foo\.conf",
        "/x/`id`",
        "/x/$(id)",
        "/x/`c6_exec`",
        "/x/$(c6_exec)",
    ]
}

#[test]
fn selinux_label_without_a_same_subject_equivalence_runs_the_triple() {
    for spec in guard_specs() {
        let cmd = label_command(spec, FcontextFileType::All);
        let got = Harness::new("", 0, 0).run(&cmd);
        assert_outcome(
            spec,
            &got,
            0,
            "",
            "",
            &[
                LISTING_CALL,
                &["fcontext", "-d", spec],
                &["fcontext", "-a", "-t", HTTPD_TYPE, spec],
            ]
            .concat(),
        );
    }
}

#[test]
fn selinux_label_falls_back_to_modify_when_add_fails() {
    for spec in guard_specs() {
        let cmd = label_command(spec, FcontextFileType::All);
        let got = Harness::new("", 0, 1).run(&cmd);
        assert_outcome(
            spec,
            &got,
            0,
            "",
            "",
            &[
                LISTING_CALL,
                &["fcontext", "-d", spec],
                &["fcontext", "-a", "-t", HTTPD_TYPE, spec],
                &["fcontext", "-m", "-t", HTTPD_TYPE, spec],
            ]
            .concat(),
        );
    }
}

#[test]
fn selinux_directory_rule_carries_the_file_type_flag_on_all_three_commands() {
    const SPEC: &str = "/x/`c6_exec`";
    let cmd = label_command(SPEC, FcontextFileType::Directory);
    let got = Harness::new("", 0, 1).run(&cmd);
    assert_outcome(
        SPEC,
        &got,
        0,
        "",
        "",
        &[
            LISTING_CALL,
            &["fcontext", "-d", "-f", "d", SPEC],
            &["fcontext", "-a", "-f", "d", "-t", HTTPD_TYPE, SPEC],
            &["fcontext", "-m", "-f", "d", "-t", HTTPD_TYPE, SPEC],
        ]
        .concat(),
    );
}

#[test]
fn selinux_label_on_a_same_subject_equivalence_is_skipped_without_mutation() {
    const SPEC: &str = "/var/srv";
    let listing = "/var/srv = /srv\n";
    let cases: Vec<(&str, InspectionSnapshot)> = vec![
        (
            "label",
            sel::snapshot_rules(vec![sel::label(SPEC, FcontextFileType::All, HTTPD_TYPE)]),
        ),
        (
            "unlabel",
            sel::snapshot_rules(vec![sel::unlabel(SPEC, FcontextFileType::All)]),
        ),
        (
            "label -f d",
            sel::snapshot_rules(vec![sel::label(
                SPEC,
                FcontextFileType::Directory,
                HTTPD_TYPE,
            )]),
        ),
    ];
    for (case, snap) in cases {
        let got = Harness::new(listing, 0, 0).run(&rule1_command(&snap));
        assert_outcome(case, &got, 0, "", &skip_diagnostic(SPEC), LISTING_CALL);
    }
}

#[test]
fn selinux_descendant_of_an_equivalence_subject_is_not_skipped() {
    // The guard is an exact-subject match, not a prefix match: `semanage
    // fcontext -d` only deletes an equivalence whose subject is the spec.
    const SPEC: &str = "/var/srv/www(/.*)?";
    let cmd = label_command(SPEC, FcontextFileType::All);
    let got = Harness::new("/var/srv = /srv\n", 0, 0).run(&cmd);
    assert_outcome(
        SPEC,
        &got,
        0,
        "",
        "",
        &[
            LISTING_CALL,
            &["fcontext", "-d", SPEC],
            &["fcontext", "-a", "-t", HTTPD_TYPE, SPEC],
        ]
        .concat(),
    );
}

#[test]
fn selinux_label_stops_the_build_when_the_listing_fails() {
    for spec in ["/x/$(id)", r"/etc/foo\.conf"] {
        let cmd = label_command(spec, FcontextFileType::All);
        let got = Harness::new("", 1, 1).run(&cmd);
        assert_outcome(
            spec,
            &got,
            1,
            "",
            &listing_failure_diagnostic(spec),
            LISTING_CALL,
        );
    }
}

#[test]
fn selinux_equivalence_adds_once_and_fails_when_the_listing_fails() {
    // Rule 2 has no guard on the listing: an empty read looks like an absent
    // equivalence, so it adds, and the add's own failure is the exit status.
    let cmd = equivalence_command("/var/srv", "/srv");
    let got = Harness::new("", 1, 1).run(&cmd);
    assert_eq!(got.stdout, "");
    assert_eq!(
        got.argv,
        argv(&[LISTING_CALL, &["fcontext", "-a", "-e", "/srv", "/var/srv"]].concat())
    );
    assert_ne!(got.status, 0, "a failed add must not exit 0");
    assert!(!got.executed, "an operand executed");
}
