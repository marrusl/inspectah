use std::process::Command;

/// Value of `INSPECTAH_COMMIT` and `INSPECTAH_DATE` in non-release builds.
const DEV_METADATA: &str = "dev";

fn main() {
    // Only release builds embed the git commit and build date. Watching
    // `.git` in dev builds cost a cli rebuild far too often: in a linked
    // worktree `.git` is a file, so `../../.git/HEAD` never exists and cargo
    // reran this script (and rebuilt cli) on every invocation; in the main
    // checkout every commit moved `.git/refs` and rebuilt cli once per
    // commit (about 17 to 20 s, measured 2026-11-19). The date is fixed too,
    // since a real one would need a rerun on every build or go stale.
    // `PROFILE` is `release` for any profile that inherits from release.
    if std::env::var("PROFILE").as_deref() != Ok("release") {
        println!("cargo:rustc-env=INSPECTAH_COMMIT={DEV_METADATA}");
        println!("cargo:rustc-env=INSPECTAH_DATE={DEV_METADATA}");
        println!("cargo:rerun-if-changed=build.rs");
        return;
    }

    // Emit git commit hash.
    let commit = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_owned())
        .unwrap_or_else(|| "unknown".into());

    // Emit build date (UTC, YYYY-MM-DD).
    let date = Command::new("date")
        .args(["-u", "+%Y-%m-%d"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_owned())
        .unwrap_or_else(|| "unknown".into());

    println!("cargo:rustc-env=INSPECTAH_COMMIT={commit}");
    println!("cargo:rustc-env=INSPECTAH_DATE={date}");

    // Rerun when the git HEAD changes (covers commits, checkouts, rebases).
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/refs/");
}
