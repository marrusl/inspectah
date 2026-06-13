use assert_cmd::Command;
use predicates::prelude::*;

/// The --no-baseline flag must be rejected by clap after removal.
/// Runs without root — clap validates args before the root check.
#[test]
fn no_baseline_flag_rejected() {
    Command::cargo_bin("inspectah")
        .unwrap()
        .args(["scan", "--no-baseline"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unexpected argument"))
        .code(2);
}
