//! Guards the tracked e2e fixture against schema drift.
//!
//! This exists because `testdata/e2e-fixture.tar.gz` sat at schema 14 while
//! `MIN_SCHEMA` moved to 25, and nothing noticed for months. CI's "Start
//! refine server" step polls `/api/health` and proceeds regardless of the
//! outcome, and the only spec that talks to a real server skipped itself when
//! nothing answered, so the whole chain reported green against a server that
//! had never started.
//!
//! A unit test is the cheap place to catch that: it needs no CI, no host and
//! no browser, and it fails in the pre-commit hook the moment the schema moves.

use std::path::PathBuf;

/// Repo root, from this crate's manifest directory.
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root resolves")
}

/// The fixture CI serves to the Playwright real-server spec. It must load
/// through exactly the path the server uses, not a hand-rolled parse.
#[test]
fn e2e_fixture_loads_at_current_schema() {
    let path = repo_root().join("testdata/e2e-fixture.tar.gz");
    assert!(path.is_file(), "missing fixture: {}", path.display());

    let session = inspectah_refine::tarball::from_tarball(&path).unwrap_or_else(|e| {
        panic!(
            "testdata/e2e-fixture.tar.gz no longer loads: {e}\n\
             CI serves this tarball to the Playwright real-server spec, so a \
             snapshot the current binary cannot read means that spec tests \
             nothing. Regenerate it at the current SCHEMA_VERSION."
        )
    });

    // The real-server smoke spec needs packages to toggle and a preview
    // string; a fixture that loads but is empty would pass the load check and
    // still leave the spec with nothing to exercise.
    let view = session.view();
    assert!(
        !view.packages.is_empty(),
        "fixture must carry at least one package for the smoke spec to toggle"
    );
}

// NOT covered here, deliberately, and not because it would be inconvenient:
// `testdata/single-host-e2e.tar.gz` and `testdata/fleet-e2e.tar.gz` are both
// schema 17 and would fail this guard today. Bringing them current means
// regenerating host-derived data for the 663 KB one, which is an exposure
// question rather than a test fix, so it is tracked separately rather than
// silently folded in here. Widen this test to cover all three once they are
// regenerated. Backlog: inspectah-two-stale-e2e-tarballs-predate-the-schema.
