# RPM Repo Name Mismatch

## Problem

RHEL systems use two different naming schemes for the same repository:

- **Install-time short names** from `dnf repoquery --installed --queryformat "%{name} %{arch} %{from_repo}"`:
  `AppStream`, `baseos`, `BaseOS`, `anaconda`
- **Full repo IDs** from `dnf repolist --enabled` and `.repo` file `[section]` headers:
  `rhel-9-for-aarch64-appstream-rpms`, `rhel-9-for-x86_64-baseos-rpms`

Exact string comparison between these two schemes always fails on real
RHEL systems. This caused ~50% false-positive repo-less flagging.

## The recorded repo is per architecture

`populate_source_repos` keys what dnf recorded by `name.arch`, never by
bare name. A multilib host can have `libfoo.i686` from AppStream and
`libfoo.x86_64` from a local RPM path; when the query was keyed by name,
the first line dnf printed went to both, so the 64-bit library read as
installable by name and a unit owner needing it was carried into a build
that could not resolve it. What cannot be told apart is unknown, and
`source_repo` stays empty (`MissingSourceRepo`, so nothing installs it
by name and an owner that may pull it in is held): two records for one
`name.arch` that name different repos, and a record with no
architecture (a two-field dnf line, or an `rpm -qi` record whose
`Architecture` line does not come before its repo line), which voids
every entry of that name. A mock of the query answers
`<name> <arch> <repo>` lines; the old two-field answer now reads as
arch-less and leaves the name unknown.

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
   - **Pseudo:** `@commandline`, `anaconda`, `@System`
     (case-insensitive) win first, before any section is looked at. They
     record how a package was installed, not a repo, so a `.repo` file
     with an `[anaconda]` section does not turn installer provenance into
     a repo the export could enable.
   - Otherwise **Exact:** the recorded value names some `.repo` section,
     whether or not that section is enabled, because dnf recorded that
     exact id. The section spelled exactly as recorded wins; a case variant
     stands in only when it is the only one (`section_spelling`), and two
     or more case variants with no exact spelling are `Ambiguous` among
     them. dnf ids are case-sensitive, so a recorded `ab` is never
     rewritten to `AB` because an `[AB]` exists beside `[ab]`.
   - Otherwise, case-insensitive **substring match against `enabled =
     true` sections only**: exactly one hit is `Resolved`; more than one
     is `Ambiguous` (`source_repo` is left as recorded).
   - Otherwise: an empty recorded value is `Unrecorded`; anything else
     is `Stale`.

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

   The same folding makes `RepoId` the wrong key for any set of ids that
   is emitted, such as the ids that become `--enablerepo` flags. A
   `BTreeSet<RepoId>` keeps one of `site` and `SITE` and drops the other,
   though dnf treats them as two repos. Collect emitted ids as exact
   strings (`render/containerfile.rs::crb_ids` does), and prove it with a
   case-variant pair in both file orders (`crb_flags_every_dnf_transaction`).

A third, render-side question sits on top of the first:
**`render/repoless.rs::disabled_repo_install()`** asks whether a package
the scan left repo-less can install from its own repo with
`--enablerepo`. It matches `source_repo` against the section ids of the
`.repo` files the build has when it installs (`InstallRepos`, built
once per export from `staged_writes`), by the same exact-first spelling
rule (`section_spelling`, which is what `normalize_source_repos` already
rewrote `source_repo` by), returns the id as that section spells it,
and accepts only a section that is disabled, defines a source, and whose
keys the build's dnf needs (`RepoSectionDef::required_keys(dnf)`, with
`InstallRepos::dnf`: every local key it names while that dnf checks
signatures, and none when the host turned checking off). Read the three
options apart: dnf 4 (RHEL 9 and 10) checks packages by `gpgcheck` and
has no `pkg_gpgcheck`; dnf 5 (Fedora) binds both names to one option, the
later assignment winning; both read `repo_gpgcheck`; an unset package
check counts as on. One slot for `gpgcheck` and `pkg_gpgcheck` let a
dnf 5 setting switch off a check RHEL's dnf 4 still makes. The dnf is
`BuildDnf::for_host`, from the host's `os_release.id`; unknown means a
key counts when either would check)
are in the image when it installs (`keys_included`, shared with the
repo-enabling proof and the route-loss decision): a key the base
image ships, or one the GPG Keys section copies, which is an included
key the stager actually writes (`InstallRepos::copies_key`). An included
key the stager withholds or refuses is not copied, and dnf cannot verify
the package without it; checking the key's disposition is not enough,
for the same reason checking a repo file's is not.

That set is what the package phase's early `COPY config/etc/yum.repos.d/`
puts in the image, so it follows the `COPY`, not the repo inventory:

- The `COPY` renders only when an included repo file that is not
  `is_default_repo` is staged (`InstallRepos::custom_count`, which is
  also the Custom Repositories count). With none, the set is empty: the
  rest of `config/etc/` is copied after the packages install.
- When it renders it copies the whole staged directory. **Default repo
  files are not left out**: an included `redhat.repo` lands beside the
  custom files and competes for an id like any other. The stager does not
  exclude default files, and the route does not either; an excluded
  default file is simply not staged.
- A repo file the stager withholds (a conflict) or refuses (a path that
  is not a `Destination`) is not in the set, however it is disposed.
- The content is the staged content: where an included config entry at
  the same path replaces a repo file's content, dnf reads the config
  entry's. Checking that a path is staged is not enough.
- Files the base image itself ships in `/etc/yum.repos.d` are unknown to
  the export and not part of the set.

When more than one file defines the id, the section it judges is the one
dnf 4 uses (`effective_definition` in `inspectah_core::types::repo`):
files sorted by path with `.repo` cut off, compared by component, first
definition wins (dnf's `RepoDict.add` drops later ones with "listed more
than once"). A header repeated inside one file is one section to dnf, and
`parse_repo_sections` merges it the same way: each option keeps the last
value any block assigned it, `baseurl`, `metalink` and `mirrorlist` are
separate options (clearing one keeps the others, an empty final value is
no source), and the key list is the final `gpgkey=` alone. Never decide by
the order of `repo_files`: it is the host's `read_dir` order. A package the scan
attributed to a pseudo repo (`RepoAttribution::Pseudo`: `anaconda`,
`@System`) never takes this route, whatever a `.repo` section is called;
test the attribution, not the spelling of `source_repo`.
It is decided at render rather than at scan because whether the `.repo`
file ships is a refine decision. It also requires the package to be
installable by name (`is_package_installable`), and it does **not** read
the package's disposition: it says how a package installs, not whether.
The Containerfile renderer feeds it the plain list's selection (included,
baseline, leaf and lock rules, and the carried unit owners), plus every
package `leaf_tree_pulled_in` reports: a dependency of a leaf the export
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
enables every one of the disabled repos. Nothing an enabled repo serves
joins it: moving a package there lets dnf pick a version only the
disabled repo serves, and routing on what a package may pull in failed
in new ways each round, so the routing is beta.6's (Mark Russell,
2026-11-21). An upload takes its package off the disabled-repo route, so a local RPM can
be the dependency of a
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
another. The transaction groups its packages by the id exactly as the
route spells it, never by `RepoId`: `[ab]` and `[AB]` are two repos and
each gets its own `--enablerepo`. A folded key once put both packages
under one flag, and the package only the other repo held could not be
found.

If you touch repo-name matching, decide which of these questions you
are answering (repo-less yes/no, full attribution, or install from a
disabled repo) before reusing any of the functions — they are no longer
the same logic wearing different names.

## One resolver per question

Which definition of a repo id applies is decided in one place for each
of the two questions that ask it. Never resolve an id by walking
`repo_files` and taking the first or last section that matches.

- **Host questions** (what the source host's dnf uses: refine's repo
  provenance and its repo-toggle gate) ask
  `inspectah_core::types::repo::effective_definition` over the captured
  repo files.
- **Export questions** (what the image build's dnf uses: the
  disabled-repo route, the repo-enabling proof, its demotion FIXME, the
  route-loss decision and the audit's "Carried by Export") ask
  `InstallRepos::effective`, or `effective_section` for a section whose
  exact spelling the caller already holds, which is
  `effective_definition` over the staged files and nothing when the
  Custom Repositories `COPY` does not render. The repo-enabling proof
  accepts that definition only when it is enabled, keyed and has a
  source (`has_source`): an enabled copy with no `baseurl`, `metalink`
  or `mirrorlist`, or one whose source a later assignment cleared,
  cannot supply the package. Recovery advice (`serving_loss`, asked
  only once the verdict is `Lost`) names **only a control refine offers**:
  the repo toggle, and the RPM upload. Refine has no control for a repo
  file or a key on its own. An owned, unmodified `.repo` file has no
  Configuration Files row (`SetInclude Config` on it is `unknown target`),
  and a row that does exist changes `config.files`, not the
  `rpm.repo_files` decision the proof and the Custom Repositories `COPY`
  read. So the advice never says "include <path> under Configuration
  Files"; it names a repo. A repo is named only when it has a switch that
  can bring back what it owns (`control`: a full switch for a toggleable
  repo, not distro and enabled where the host's dnf reads it, which the
  FIXME says also re-includes the repo's packages; or a carriage-only
  switch, by both clauses of the ownership spec's § 5.2: a distro or
  host-disabled repo that owns a carriable entry and is enabled in a file
  that defines it or has a package attributed to it by exact spelling
  first, or one that owns a carriable entry no such repo owns; the FIXME
  says it brings back its repo files and keys) and
  `toggle_restores` says re-including it serves the package:
  it re-stages the whole export as the toggle leaves it (every unlocked
  repo file defining that exact section, every key its sections name that
  the base image does not ship) and asks the verdict again. Candidates, in
  order: the serving ids as a captured section spells them, then the
  sections the package's own repo files define, sorted, since a dropped
  owned `vendor.repo` comes back only with a repo it defines. When none
  serves, the FIXME offers only the upload. The demotion FIXME is the
  package's only advice: `repoless_plan` gets the demoted set and prints
  no MANUAL block for those packages, whose "add a repo, or uncomment"
  advice contradicted the FIXME (release check on RHEL 9.8, beta.6).
  Adding a file to the staged set by hand is not enough to judge a
  candidate: a config entry at `a.repo/stray` makes the stager withhold
  both it and an included `a.repo`.
- **A package installed by name loses its route by one decision.**
  `repoless::route_lost` decides it for a section by exact spelling
  (`SectionId`), and package routing (`InstallRepos::lost_route`), the
  restoring-switch advice (`lost_route_loss`) and unit-owner holds
  (`not_carried::loses_its_repo`) all read it through
  `InstallRepos::is_lost`. Never write a second rule beside it: three
  rules disagreed on the base image's repos, case variants and keys. Lost
  exactly when (1) the host's dnf installed by name from the section (its
  stored definition enabled with a source; or no stored file defines it,
  or redaction changed its stored definition (`redaction_changed`), and
  the collector attributed the package to it before redaction), (2)
  the base image does not supply it (`base_supplies`: a default repo
  file, or a file the collector marked `verified_base_owner`, as
  `fedora-repos` owns `fedora.repo`; a `company.repo` defining `[baseos]`
  is not supplied by its name). The mark is set at collection only when a
  baseline package owns the file and an `rpm -Va` that ran to the end did
  not report it. Never read an empty `rpm_va` as "unmodified": a failed
  `rpm -Va` leaves it empty too (`rpm_va_complete` is not serialized), and
  a replaced `fedora.repo` passed for the base image's. A fleet keeps the
  mark only where every host has it. And (3) the build's definition of that
  exact spelling does not serve, a key counting only when dnf checks
  signatures for it. A package the collector attributed (`Exact` or
  `Resolved`) is routed by that captured section, whatever the stored
  files define now: a surviving `[SITE]` never stands in for a `[site]`
  redaction removed or changed, and the lost definition counts as lost.
  Only an unattributed package resolves exact spelling first, so `[site]`
  losing its key never demotes `[SITE]`'s package. It is not installed
  by name: it gets the demotion FIXME, worked out once per exact section,
  and takes an upload. Its disposition never changes; the route is read
  from the repo files, the attribution and the staging, never from
  `repoless_annotation`, which refine's `demote_lost_routes` writes for
  the upload control.

Both are exact-first and case-aware. dnf keys repos by exact spelling,
so `[ab]` and `[AB]` are two repos, in one file or in two;
`section_spelling` picks the section spelled as asked, falls back to a
case variant only when it is the only one, and names none when two
variants compete. `RepoId`'s folded equality is for loose attribution
and grouping by recorded name (`RepoIndex::packages_by_repo`), never for
choosing which section dnf uses or which repo a toggle reaches. Other
files defining the id stay in the inventory
(`RepoIndex::repo_file_by_section`, `gpg_keys_by_section`), which
exclusion needs whichever definition wins.

Refine's repo toggle works in one identity, `RepoIndex::repo_identity`:
the section spelled exactly as given, else the lone case variant, else
the given spelling itself. The toggle target, each package's
`source_repo`, the excluded set, the file and key inventories, the
effective-member counts and the repo groups' enabled state all go
through it, so excluding `[site]` leaves `[SITE]`'s packages, files and
keys carried, while a package recorded `Vendor` still follows the one
`[vendor]` section.

## Edge case: anaconda

`anaconda` is NOT a real repo. It is the install-time source recorded by
the Anaconda installer. No RHEL repo ID contains "anaconda", so
`repo_matches_enabled()`'s substring check correctly leaves it unmatched
and `classify_recorded_repo()` gives it `Pseudo { which: Anaconda, .. }`
rather than `Stale`, even on a host whose `.repo` files define an
`[anaconda]` section. Packages from `anaconda` are properly flagged as
repo-less. A test of this belongs on the scan's path (normalization, then
`scan_dnf_cache_for_repoless`, then the route), not on a hand-built
`Pseudo`: that is how the classifier's order once went unnoticed.

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
