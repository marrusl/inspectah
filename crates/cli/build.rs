use std::path::Path;
use std::process::Command;

/// Build metadata appended to the version in non-release builds.
const DEV_SUFFIX: &str = "+dev";

fn main() {
    let version = std::env::var("CARGO_PKG_VERSION").expect("cargo sets CARGO_PKG_VERSION");
    println!("cargo:rerun-if-changed=build.rs");

    // Only release builds read git. Watching `.git` in dev builds cost a cli
    // rebuild far too often: in a linked worktree `.git` is a file, so a
    // watched `../../.git/HEAD` never exists and cargo reran this script (and
    // rebuilt cli) on every invocation; in the main checkout every commit
    // moved `.git/refs` and rebuilt cli once per commit (about 17 to 20 s,
    // measured 2026-11-19). `PROFILE` is `release` for any profile that
    // inherits from release.
    let stamped = if std::env::var("PROFILE").as_deref() == Ok("release") {
        release_version(&version)
    } else {
        format!("{version}{DEV_SUFFIX}")
    };
    println!("cargo:rustc-env=INSPECTAH_VERSION={stamped}");
}

/// The bare version when HEAD carries the tag `v<version>`, otherwise
/// `<version>+g<short hash>`. A tree with no git, such as the source
/// tarball the RPMs build from, gets the bare version.
fn release_version(version: &str) -> String {
    // Resolve HEAD once, so the tag check and the hash name the same commit.
    let Some(head) = git(&["rev-parse", "--verify", "HEAD"]) else {
        return version.to_owned();
    };
    watch_git_refs();

    let tag = format!("v{version}");
    let tagged =
        git(&["tag", "--points-at", &head]).is_some_and(|tags| tags.lines().any(|t| t == tag));
    if tagged {
        return version.to_owned();
    }
    let short = git(&["rev-parse", "--short", &head]).unwrap_or(head);
    format!("{version}+g{short}")
}

/// Rerun when HEAD moves or a tag is created or deleted. `--git-path` finds
/// the files in a linked worktree too, where `.git` is a file: HEAD is in
/// the worktree's own git dir and refs live in the common one. A path that
/// does not exist would make cargo rerun on every build, so `packed-refs`
/// is watched only when present.
fn watch_git_refs() {
    let Some(paths) = git(&[
        "rev-parse",
        "--path-format=absolute",
        "--git-path",
        "HEAD",
        "--git-path",
        "refs",
        "--git-path",
        "packed-refs",
    ]) else {
        return;
    };
    for path in paths.lines().filter(|p| Path::new(p).exists()) {
        println!("cargo:rerun-if-changed={path}");
    }
}

/// Trimmed stdout of a successful git command, or `None`.
fn git(args: &[&str]) -> Option<String> {
    Command::new("git")
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
}
