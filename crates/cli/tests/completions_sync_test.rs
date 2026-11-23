//! The checked-in shell completions under `completions/` match what the
//! CLI generates today.
//!
//! The files are `clap_complete` output committed to the tree. Without this
//! test a changed flag never reaches them: they once drifted for months and
//! kept offering `--tui` after it was removed.

use std::path::Path;
use std::process::{Command, Stdio};

/// The command that rewrites all three files from the current CLI, run from
/// the repository root.
const REGENERATE: &str = "for s in bash zsh fish; do \
cargo run -q -p inspectah-cli -- completions $s > completions/inspectah.$s; done";

/// Each shell `inspectah completions` takes, with its checked-in file.
const SHELLS: [(&str, &str); 3] = [
    ("bash", "inspectah.bash"),
    ("zsh", "inspectah.zsh"),
    ("fish", "inspectah.fish"),
];

fn generate(shell: &str) -> Vec<u8> {
    let output = Command::new(env!("CARGO_BIN_EXE_inspectah"))
        .args(["completions", shell])
        .stdin(Stdio::null())
        .output()
        .expect("inspectah completions runs");
    assert!(
        output.status.success(),
        "inspectah completions {shell} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

#[test]
fn checked_in_completions_match_the_cli() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../completions");
    let mut stale = Vec::new();
    for (shell, file) in SHELLS {
        let path = dir.join(file);
        let tracked =
            std::fs::read(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
        if generate(shell) != tracked {
            stale.push(format!("completions/{file}"));
        }
    }
    assert!(
        stale.is_empty(),
        "stale shell completions, not what `inspectah completions <shell>` prints: {}. \
         Regenerate all three from the repository root with:\n\n    {REGENERATE}\n",
        stale.join(", ")
    );
}
