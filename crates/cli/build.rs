use std::process::Command;

fn main() {
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
