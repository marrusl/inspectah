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
   `normalize_source_repos`'s doc comment states this and
   `normalization_is_order_independent_and_never_relabels` proves it by
   running the same input through both file orders.

   `RepoId` folds ASCII case for `Eq`, `Hash`, and `Ord` (so
   `RepoId::new("EPEL") == RepoId::new("epel")` and both hash and sort
   together), while `Display` prints the id exactly as the host spelled
   it — `crates/core/src/types/repo.rs`'s own doc comment calls this out
   because it is the reason `Ord`-based tie-breaking there uses the raw
   string, not `RepoId`'s own folded `Ord`.

A third, render-side question sits on top of the first:
**`render/repoless.rs::disabled_repo_install()`** asks whether a package
the scan left repo-less can install from its own repo with
`--enablerepo`. It matches `source_repo` against the section ids of the
*included* repo files by case-insensitive equality (the `Exact` rule,
which is what `normalize_source_repos` already rewrote `source_repo`
to), and accepts only a section that is disabled, defines a source, and
names no `gpgkey=file://` key the export leaves out (`keys_included`,
shared with the repo-enabling proof).
It is decided at render rather than at scan because whether the `.repo`
file ships is a refine decision. It also requires the package to be
installable by name (`is_package_installable`), and it does **not** read
the package's disposition: it says how a package installs, not whether.
The Containerfile renderer feeds it the plain list's selection (included,
baseline, leaf and lock rules), plus every package
`pulled_in_by_included` reports: a dependency of a leaf the export
installs is in the image whatever its disposition, and only its own repo's
transaction can install it, so it is named there even when refine's
non-leaf default or the operator excluded it. Selection, not a
disposition flip, is what keeps the operator's include state as it is.
An excluded package nothing included depends on renders nowhere on
either export path; reading the disposition here would send a
refine-excluded package to the repo-less block while the scan tarball,
where the same package is included, rendered it differently.
The Containerfile renderer, the repo-less block and refine's trust gate
all ask this one function, and any package carrying a
`repoless_annotation` stays out of the plain `dnf install` list whatever
the answer: no enabled repo on the host serves it, so a plain install
fails with `Unable to find a match`.

**Install order in the package phase is a correctness rule, not
layout.** Every step that installs an RPM runs before anything that can
consume it: repo-enabling packages, then **one combined install** (the
`COPY` of every active local RPM, then a single `dnf install` naming the
disabled-repo packages and the local RPM paths, with an `--enablerepo`
for each disabled repo plus CRB), then the repo-less advice (MANUAL,
FIXMEs, a pre-excluded cached RPM's commented block), then `dnf group
install`, then the main `dnf install` list. Services, config
and non-RPM content all come after `packages_section_lines`. The
repo-less block used to sit with the non-RPM software, which put
`systemctl enable` of a unit an uploaded RPM ships, and the plain install
of a package that needs it, ahead of the install itself.

All selected disabled-repo packages and every active local RPM (verified
upload or included cached RPM) install in **one** transaction that
enables every one of the disabled repos. An upload takes its package off
the disabled-repo route, so a local RPM can be the dependency of a
disabled-repo package or depend on one; installing the two sets in
separate steps fails in one direction or the other, which is why they
share the step. A transaction per repo, in `RepoId`
order, broke whenever a package in one disabled repo depended on one in
another that sorted later. Ordering by the dependency graph was the
alternative; it cannot see dependencies `leaf_dep_tree` does not record
and needs a merge for cycles, so it was not taken. The isolation that
matters still holds: no group or plain-list transaction carries the
flags. What one transaction gives up is isolation between the disabled
repos themselves, so a package in one can resolve a dependency from
another.

If you touch repo-name matching, decide which of these questions you
are answering (repo-less yes/no, full attribution, or install from a
disabled repo) before reusing any of the functions — they are no longer
the same logic wearing different names.

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
