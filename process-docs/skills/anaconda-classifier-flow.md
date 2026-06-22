# Anaconda Gap Classifier Flow

## Problem

Packages sourced from the `anaconda` pseudo-repo are installer artifacts,
not user intent. Without classification, every Anaconda-installed package
shows up in the migration scope, creating enormous noise.

## How It Works

The classifier runs during `RefineSession::new()` — before any user ops
are replayed. It examines each package's `source_repo` field:

1. **`source_repo == "anaconda"`** triggers classification
2. Packages with active services or modified configs are **promoted**
   (treated as user intent via `TriageReason::PackageInstallerPromotedService`
   or `PackageInstallerPromotedConfig`)
3. Remaining anaconda packages are classified as **platform plumbing**
   (`TriageReason::PackagePlatformPlumbing`) and locked-excluded

## Key Invariants

- **Locked items reject `set_include` ops.** A platform-plumbing package
  cannot be toggled back to included by the user. The session enforces
  this — see `locked_platform_plumbing_package_rejects_set_include` test.
- **User ops survive reclassification.** The session restores user
  refinement operations after anaconda reclassification runs. Commit
  `590424a4` fixed a bug where reclassification was silently discarding
  prior user toggles.
- **Containerfile rendering checks `source_repo`.** The renderer at
  `crates/pipeline/src/render/containerfile.rs:577` has special handling:
  anaconda packages with `include == true` (promoted ones) are rendered
  normally; platform plumbing is omitted.

## Where to Find It

- Classification logic: `crates/refine/src/session.rs` (search for
  `PackagePlatformPlumbing`)
- Triage reason enum: `crates/refine/src/types.rs`
- Web handler mapping: `crates/web/src/aggregate_handlers.rs` (search
  for `package_platform_plumbing`)
- Tests: `crates/web/tests/api_test.rs:1507`,
  `crates/refine/src/session.rs:4255`
