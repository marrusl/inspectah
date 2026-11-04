//! Guards every snapshot tarball under `testdata/` against schema drift.
//!
//! This exists because `testdata/e2e-fixture.tar.gz` sat at schema 14 while
//! `MIN_SCHEMA` moved to 25, and nothing noticed for months. CI's "Start
//! refine server" step polls `/api/health` and proceeds regardless of the
//! outcome, and the only spec that talks to a real server skipped itself when
//! nothing answered, so the whole chain reported green against a server that
//! had never started. Two further tarballs reached schema 17 the same way and
//! were deleted unused.
//!
//! A unit test is the cheap place to catch that: no CI, no host, no browser,
//! and it fails in the pre-commit hook the moment the schema moves.
//!
//! **The directory is discovered, not listed.** A tarball added here at the
//! wrong schema fails on arrival rather than waiting for someone to remember
//! this file. The cost is that `testdata/` is for tracked fixtures only — a
//! scratch tarball left there will fail the suite, which is the intended
//! trade.

use std::path::{Path, PathBuf};

/// Repo root, from this crate's manifest directory.
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root resolves")
}

/// Every `*.tar.gz` directly under `testdata/`, sorted for stable output.
fn testdata_tarballs() -> Vec<PathBuf> {
    let dir = repo_root().join("testdata");
    let mut found: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.to_string_lossy().ends_with(".tar.gz"))
        .collect();
    found.sort();
    found
}

/// Every tarball must load through the same path the refine server uses.
#[test]
fn every_testdata_tarball_loads_at_current_schema() {
    let tarballs = testdata_tarballs();

    // A discovery-based test that finds nothing would pass silently, which is
    // the exact failure class this guard was written to end.
    assert!(
        !tarballs.is_empty(),
        "no tarballs found under testdata/ — this guard would be vacuous"
    );

    for path in &tarballs {
        if let Err(e) = inspectah_refine::tarball::from_tarball(path) {
            panic!(
                "{} no longer loads: {e}\n\
                 Tracked snapshot tarballs must be readable by the current \
                 binary. CI serves testdata/e2e-fixture.tar.gz to the \
                 Playwright real-server spec, so an unreadable snapshot means \
                 that spec tests nothing. Regenerate at the current \
                 SCHEMA_VERSION, or delete the tarball if nothing consumes it.",
                path.display()
            );
        }
    }
}

/// Loading is necessary but not sufficient: the real-server smoke spec needs
/// packages to toggle, so an empty-but-valid fixture must not pass.
#[test]
fn e2e_fixture_carries_packages_for_the_smoke_spec() {
    let path = repo_root().join("testdata/e2e-fixture.tar.gz");
    assert!(
        Path::new(&path).is_file(),
        "missing fixture: {}",
        path.display()
    );

    let session = inspectah_refine::tarball::from_tarball(&path)
        .expect("e2e fixture loads; see sibling test");

    assert!(
        !session.view().packages.is_empty(),
        "fixture must carry at least one package for the smoke spec to toggle"
    );
}
