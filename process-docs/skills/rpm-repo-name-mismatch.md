# RPM Repo Name Mismatch

## Problem

RHEL systems use two different naming schemes for the same repository:

- **Install-time short names** from `dnf repoquery --installed --queryformat "%{from_repo}"`:
  `AppStream`, `baseos`, `BaseOS`, `anaconda`
- **Full repo IDs** from `dnf repolist --enabled` and `.repo` file `[section]` headers:
  `rhel-9-for-aarch64-appstream-rpms`, `rhel-9-for-x86_64-baseos-rpms`

Exact string comparison between these two schemes always fails on real
RHEL systems. This caused ~50% false-positive repo-less flagging.

## Two separate callsites, two different rules

These used to share one plain case-insensitive substring rule. Phase 0
of the refine UI fixup replaced the rule at one of the two callsites
with an ordered, multi-outcome classification (§ 3.2); the other
callsite still runs the original, simpler substring check. They now do
different things, on purpose, for different questions:

1. **`repoless.rs::repo_matches_enabled()`** — unchanged. Answers a
   yes/no question for repo-less detection: does a package's recorded
   `source_repo` appear, case-insensitively, as a substring of any repo
   ID in the host's already-enabled-only list (`dnf repolist --enabled`)?
   There is no exact-match preference and no distinction among
   ambiguous, stale, or pseudo values — just a bool.
2. **`source_repos.rs::classify_recorded_repo()`**, called from
   `normalize_source_repos()` — replaced. Produces a `RepoAttribution`
   (`crates/core/src/types/repo.rs`) per package and rewrites
   `source_repo` to the canonical id where the rule resolves one:
   - **Exact:** the recorded value equals some `.repo` section id,
     case-insensitively, whether or not that section is enabled. Wins
     first, over every other rule, because dnf recorded that exact id.
   - Otherwise, case-insensitive **substring match against `enabled =
     true` sections only**: exactly one hit is `Resolved`; more than one
     is `Ambiguous` (`source_repo` is left as recorded).
   - Otherwise: `@commandline`, `anaconda`, `@System` (case-insensitive)
     are `Pseudo`; an empty recorded value is `Unrecorded`; anything
     else is `Stale`.

   This resolution order is applied independently per package, so the
   result never depends on the order `.repo` files were read in —
   `classify_recorded_repo`'s doc comment states this and
   `normalization_is_order_independent_and_never_relabels` proves it by
   running the same input through both file orders.

   `RepoId` folds ASCII case for `Eq`, `Hash`, and `Ord` (so
   `RepoId::new("EPEL") == RepoId::new("epel")` and both hash and sort
   together), while `Display` prints the id exactly as the host spelled
   it — `crates/core/src/types/repo.rs`'s own doc comment calls this out
   because it is the reason `Ord`-based tie-breaking there uses the raw
   string, not `RepoId`'s own folded `Ord`.

If you touch repo-name matching, decide which of the two questions you
are answering (repo-less yes/no, or full attribution) before reusing
either function — they are no longer the same logic wearing two names.

## Edge case: anaconda

`anaconda` is NOT a real repo. It is the install-time source recorded by
the Anaconda installer. No RHEL repo ID contains "anaconda", so
`repo_matches_enabled()`'s substring check correctly leaves it unmatched
and `classify_recorded_repo()` gives it `Pseudo { which: Anaconda, .. }`
rather than `Stale`. Packages from `anaconda` are properly flagged as
repo-less.

## Method constant registry

Non-RPM detection methods are centralized in `crates/core/src/util.rs`.
Every module that routes on `NonRpmItem.method` must use these constants:

| Constant | Value | Meaning |
|----------|-------|---------|
| `METHOD_PYTHON_VENV` | `"python venv"` | pyvenv.cfg-based venv |
| `METHOD_PIP_DIST_INFO` | `"pip dist-info"` | System pip via dist-info |
| `METHOD_NPM_LOCKFILE` | `"npm lockfile"` | package-lock.json |
| `METHOD_NPM_MANIFEST` | `"npm manifest"` | package.json only (no lockfile) |
| `METHOD_GEM_LOCKFILE` | `"gem lockfile"` | Gemfile.lock |
| `METHOD_GEM_SYSTEM` | `"gem system"` | `gem list --local` |

The `dedup_ecosystem()` function in `nonrpm.rs` maps these to ecosystem
buckets. New method constants must be added to both `util.rs` and
`dedup_ecosystem()`, plus the renderer's `is_language_env()` gate in
`language_packages.rs`.
