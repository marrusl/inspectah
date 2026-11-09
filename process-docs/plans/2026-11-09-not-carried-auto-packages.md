# Not-carried auto packages: implementation plan

> **Status: Proposed (2026-11-09).** Not executed.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task by task. Steps use checkbox (`- [ ]`) syntax for tracking. Every task is test-first: write the tests it names, watch them fail, then implement.

**Goal:** an auto package (not user-installed) that no included package pulls in is excluded on the scan path as it already is in refine, every consumer treats it as absent, and both export paths name it in a visible "Not carried" list in the Containerfile and in `audit-report.md`. `/var` evidence counts a package that something included pulls in as installed.

**Tech stack:** Rust, serde, insta, the existing render modules in `crates/pipeline/src/render/`.

**Verification convention:** every `cargo` command runs without a pipe, so the exit code is cargo's. The gate for each commit is `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D clippy::all` and `cargo test --workspace`, as the pre-commit hook runs them.

---

## Decision record

- **Option B, with a visible list (decided 2026-11-09).** Of the three options for "included, not a leaf, needed by nothing", the scan path excludes these packages, matching refine. The image follows the base image's choices, not the installer's, and the operator sees what was left out and can include it.
- **The class.** A package in `auto_packages` that no included package pulls in, under the corrected `leaf_dep_tree` (file, weak and rich dependencies). The measured set at `e55a45f7`: 38 on RHEL 9.8 (18 recorded by dnf as group installs, such as `cronie`, `firewalld`, `rsyslog`, `insights-client`, `rhc`, `grubby` and `dracut-config-rescue`, plus 20 that only those need) and 24 on RHEL 10.2. None of them is a dependency the tree should have caught.
- **Refine's defaults are the target for parity.** Refine's leaf default already excludes this class. The scan path moves to it; refine's normalizer does not change.
- **Excluded is not absent.** A package an included one depends on is in the image whatever its disposition (`pulled_in_by_included`). The new class is defined against that same predicate, so a pulled-in package is never listed as not carried.
- **Surfaces, on both paths:**
  - **Containerfile:** a comment block in the packages section, following the `# Omitted:` pattern: a header line with the count, then one line per package naming any unit it had enabled, for example `#   firewalld (firewalld.service was enabled)`.
  - **`audit-report.md`:** a "Not carried" table in the Packages group with the columns Package, Source repo, Enabled units, Reason.
  - **Ordering, on every surface:** packages that own a unit in `services.enabled_units` first, then the rest, each part sorted by name.
- **Later and separate (not in this plan's execution):** a refine UI group with one-click include, owned by the web frontend owner, and possibly an `x-export-warnings` entry for packages whose units were enabled. Tasks 11 and 12 record them.

## Out of scope

- The deferred target-evaluation item: the leaf tree judges rich dependencies on the source host, not the target (`inspectah-leaf-dep-tree-judges-rich-dependencies-on-the-source-host`).
- The socket-units gap.
- The refine disabled-repo row.
- The other differences in the scan-versus-refine defaults item: RPM-modified configs, `sshd.service` from orphan-shadow synthesis, and `epel-release`. None of them falls out of this work.
- Packages with `source_repo == "anaconda"`. Refine's anaconda classifier decides them and the scan path bypasses the leaf filter for them. That divergence predates this plan and stays as it is.
- Repo-less packages (`PackageRoute::Upload` or `Repoless`). The repo-less exception is refine-only by design.
- Aggregate snapshots. Their `packages_added` is leaf-only, and refine clears `leaf_packages` for them.

---

## The design choice: where the class is computed and applied

### Recommendation: a pure classifier in the pipeline crate, applied once by the scan command

New module `crates/pipeline/src/render/not_carried.rs`, beside `service_intent.rs`, owns three things:

```rust
/// How an added package reaches the target image, for the auto class.
pub enum AutoCarriage {
    /// A leaf, or not in `auto_packages`: not this class.
    NotAuto,
    /// An auto package an included leaf pulls in (`pulled_in_by_included`).
    PulledIn,
    /// An auto package nothing included pulls in.
    Unneeded,
}

/// `None` when the snapshot cannot support the judgement (see Fallback).
pub fn classify_auto_carriage(rpm: &RpmSection) -> Option<HashMap<String, AutoCarriage>>;

/// Scan path only: set `Unneeded` packages excluded.
pub fn apply_scan_package_defaults(snap: &mut InspectionSnapshot);

/// Every surface: the excluded `Unneeded` packages, in display order.
pub fn not_carried_packages(snap: &InspectionSnapshot) -> Vec<NotCarriedPackage>;
```

- **Classification is a function of the current snapshot, dispositions included.** `pulled_in_by_included` reads which leaves are included, so the class moves when an operator toggles a leaf in refine. A pure function that every renderer calls answers correctly on both paths and after any refine decision.
- **Application is a disposition change on the scan path, nothing more.** `apply_scan_package_defaults` runs in `crates/cli/src/commands/scan.rs` after redaction and before the `--inspect-only` branch, so every snapshot the scan writes says the same thing, and in `run_pipeline` (`crates/pipeline/src/orchestrate.rs`) after `redact`. Once the disposition is excluded, the consumers that already follow disposition become correct with no change of their own: the services tiers omit the unit (Tier 2), `/var` evidence stops counting the package, and the install list was already leaving it out.
- **Refine is untouched.** Its leaf default produces the same exclusions. The parity test (Task 8) holds the two paths together, so a later change to either side that breaks agreement fails the suite.

### Alternatives considered

- **In the collector, writing the result into dispositions at scan time.** Rejected. The collector records host facts; this is a default. It would also bake a judgement into the per-host snapshot that the aggregate merge reads, and the judgement depends on dispositions that refine changes later. The pure classifier is needed for the visible list in any case, so the collector option would leave two deciders.
- **Reusing refine's normalizer from the scan path.** Rejected. `inspectah-refine` depends on `inspectah-pipeline`, not the reverse, and `normalize_package_defaults` brings refine's whole package policy with it: the repo-less trust gate (the one intended difference between the paths), the Investigate bucket, and the anaconda classifier. That is the L parity item, not this one.
- **No disposition change: every consumer asks an "effectively carried" predicate.** Rejected. It leaves the scan snapshot saying "included" for a package the image does not install, which is the inconsistency option B exists to remove, and every new consumer would have to remember the predicate.

### The snapshot format does not change

No field is added, renamed or retyped, so `SCHEMA_VERSION` stays at 25. What changes is a value: the scan path writes `disposition` excluded for these packages where the collector left it included. A refine session overwrites package dispositions in `normalize_package_defaults` anyway, so it reads a new scan snapshot and an old one the same way. **If any task finds it needs a new field (for example dnf's install reason, see "Needs a decision"), stop and ask before adding it.**

### Fallback: no leaf or dependency data never excludes everything

`classify_auto_carriage` returns `None`, and the scan path changes nothing and every surface lists nothing, when any of these hold:

- `leaf_packages` or `auto_packages` is `None` (classification unavailable);
- `leaf_dep_tree` is not a JSON object;
- `leaf_dep_tree` is an empty object while `auto_packages` is non-empty. That is the degraded marker the collector writes, and reading it as "no leaf needs anything" would exclude every auto package;
- the snapshot is an aggregate (`aggregate_meta` is set).

Within a classified snapshot, a package is never `Unneeded` when it is locked, has `source_repo == "anaconda"`, is in `baseline_package_names` by name, or routes as `Upload` or `Repoless` (`render::repoless::route_for`). These keep their existing handling.

The Containerfile and the audit report do not mention the list when the classifier returns `None`; the audit report's existing leaf-data notes already say when leaf data is missing.

---

## Tasks

### Task 1: The shared classification

**Files:**
- Create: `crates/pipeline/src/render/not_carried.rs`
- Modify: `crates/pipeline/src/render/mod.rs` (module declaration)

**Tests first** (table-driven, in the module's `#[cfg(test)]`): one test iterating cases over a small `RpmSection` built in the test.

- [ ] A leaf is `NotAuto`.
- [ ] An auto package listed under an included leaf is `PulledIn`.
- [ ] An auto package listed only under an excluded leaf is `Unneeded`.
- [ ] An auto package listed under no leaf is `Unneeded`, and so is an auto package that only another `Unneeded` package needs (the tree records leaf closures only).
- [ ] `leaf_packages: None` returns `None`.
- [ ] `leaf_dep_tree` null returns `None`.
- [ ] `leaf_dep_tree` `{}` with non-empty `auto_packages` returns `None`.
- [ ] An aggregate snapshot returns `None` (test through a wrapper that takes the snapshot, or assert at the call site in Task 2).
- [ ] Locked, `anaconda`-sourced, baseline-named, and `Upload`/`Repoless`-routed auto packages are never `Unneeded`.
- [ ] Identity is `name.arch`: two arches of one name classify independently.

**Acceptance criteria:**
- `classify_auto_carriage` calls `service_intent::pulled_in_by_included` for the pulled-in set and does not reimplement it.
- No function outside `not_carried.rs` decides membership of the class.
- The module doc comment states the class, the fallback, and the limits it inherits from the tree.

### Task 2: Apply it on the scan path

**Files:**
- Modify: `crates/pipeline/src/render/not_carried.rs` (`apply_scan_package_defaults`)
- Modify: `crates/cli/src/commands/scan.rs` (call after redaction, before `--inspect-only`)
- Modify: `crates/pipeline/src/orchestrate.rs` (call after `redact` in `run_pipeline`)
- Test: `crates/core/tests/aggregate_merge_test.rs` (one new case)

**Tests first:**
- [ ] After `apply_scan_package_defaults`, every `Unneeded` package is excluded and every other package's disposition is unchanged.
- [ ] The function is idempotent: a second call changes nothing. Excluding an auto package cannot change `pulled_in_by_included`, since only leaves contribute, so the class is stable.
- [ ] With each fallback condition from the design section, it changes nothing.
- [ ] An aggregate merge of two host snapshots, one with the defaults applied, produces the same `packages_added` and dispositions as the merge of the same snapshots without them.

**Acceptance criteria:**
- The scan tarball's `inspection-snapshot.json` and the `--inspect-only` JSON carry the same dispositions.
- `crates/cli/tests/copy_source_exists_test.rs` still passes on both paths.
- No change in `crates/refine/`.

### Task 3: The ordered list and the unit lookup

**Files:**
- Modify: `crates/pipeline/src/render/not_carried.rs` (`NotCarriedPackage`, `not_carried_packages`)

`NotCarriedPackage` carries `name`, `arch`, `source_repo` and `enabled_units: Vec<String>`. A package's enabled units are the units in `services.enabled_units` that it owns: by `owning_package` on a `state_changes` entry, or by owning the vendor unit file (`/usr/lib/systemd/system/<unit>`) in `rpm.file_ownership`.

**Tests first:**
- [ ] The list holds exactly the excluded `Unneeded` packages.
- [ ] Packages with at least one enabled unit come first; each part is sorted by name, then arch.
- [ ] A preset-matched unit (in `enabled_units` and `preset_matched_units`, absent from `state_changes`) is found through file ownership. This is how `firewalld`, `auditd` and `crond` appear on the measured hosts.
- [ ] A unit owned by a different package is not attributed.
- [ ] On a refine-normalized snapshot of the same host, the list equals the scan path's list.

**Acceptance criteria:** the list is the only input the Containerfile block (Task 4) and the audit table (Task 5) read for membership and order.

### Task 4: The Containerfile comment block

**Files:**
- Modify: `crates/pipeline/src/render/containerfile.rs` (packages section)
- Snapshots: `crates/pipeline/src/render/snapshots/` (insta)

Wording, held in `not_carried.rs` beside the list so the Containerfile and the report share it:

```
# Not carried: not user-installed on the source host, and nothing this image installs needs it (38)
#   firewalld (firewalld.service was enabled)
#   cronie (crond.service was enabled)
#   dracut-config-rescue
```

**Tests first:**
- [ ] A snapshot with a non-empty list renders the header with the count and one line per package, in the Task 3 order.
- [ ] A package with several enabled units names them comma-separated in one line.
- [ ] An empty list, or a `None` classification, renders no block and no header.
- [ ] The block is comments only: no package in it appears in any `dnf install` line.
- [ ] Rendered through `render_all` and through `render_refine_export` on equivalent snapshots, the block is byte-identical.

**Acceptance criteria:** the block sits in the packages section after the install lines, beside the existing provenance comments, and the insta snapshot is reviewed and committed.

### Task 5: The audit-report table

**Files:**
- Modify: `crates/pipeline/src/render/audit.rs` (`render_packages_section` takes the snapshot, or the list, as well as the RPM section)

**Tests first:**
- [ ] The Packages group carries `### Not carried (N)` with the header `| Package | Source Repo | Enabled Units | Reason |`, rows in the Task 3 order.
- [ ] Enabled Units shows `-` for a package with none.
- [ ] The Reason column uses the shared wording from Task 4.
- [ ] No list, no section.
- [ ] `### Added Packages (N)` no longer counts these packages on the scan path (falls out of Task 2; assert it).

**Acceptance criteria:** both paths write the table (both call `audit::render_audit`), with identical rows for equivalent snapshots.

### Task 6: Services consistency

**Files:**
- Modify: `crates/pipeline/src/render/service_intent.rs` (`OmissionReason::NotCarried` and its `describe` text; Tier 2 picks it when the owner is in the not-carried list)
- Modify: the web adapter that maps `OmissionReason` to a subtitle, if its match is exhaustive (it must compile; the wording comes from `describe`)

Task 2 already makes Tier 2 omit these units. This task makes the omission say why, so a scan-path user does not read "package 'firewalld' was excluded" for a decision they never made.

**Tests first:**
- [ ] A user-enabled unit (in `state_changes`, included) owned by an `Unneeded` package, for example `rhcd.service` from `rhc`, is in no `systemctl` line on the scan path and is rendered `# Omitted: rhcd.service (package 'rhc' is not carried: ...)`.
- [ ] The same for a disabled or masked unit owned by one (`systemctl disable firewalld.service` must not appear).
- [ ] An operator-excluded leaf still gets `PackageExcluded`.
- [ ] An auto package that is `PulledIn` keeps its unit (unchanged behavior, regression guard).
- [ ] Same Omitted lines on both paths for equivalent snapshots.

**Acceptance criteria:** no surface on either path emits a `systemctl` line for a unit whose owner is in the not-carried list. `OmissionReason::describe` remains the only wording source.

### Task 7: The `/var` fix

**Files:**
- Modify: `crates/pipeline/src/render/var_dirs.rs` (`Evidence::from_snapshot`: `install_set` also holds the names in `service_intent::pulled_in_by_included`, subject to the same repo-enabling demotion filter)

Mark asked for this earlier: `/var` evidence counts a package that something included pulls in as installed, through the shared predicate. Today `install_set` reads disposition alone, so on the refine path a pulled-in dependency that refine starts excluded reads as not carried and its directory is provisioned, while on the scan path an unneeded auto package reads as carried. Task 2 fixes the second; this task fixes the first.

**Tests first:**
- [ ] A `/var` directory owned by an excluded auto package that an included leaf pulls in reads as `Carried`, on both paths.
- [ ] A `/var` directory owned by an `Unneeded` package reads as not carried on the scan path (the `rsyslog`, `audit` and `insights-client` directories from the 2026-11-08 build check).
- [ ] The tmpfiles fragment's comment no longer says a creator's package is "not carried" when the export installs it as a dependency (the `nginx` case in `inspectah-tmpfiles-fragment-comments-contradict-the-install-list`). If the fragment decides this through `Evidence`, the test passes with no further change; if it has its own decider, route it through `Evidence` and note it in the report.

**Acceptance criteria:** `var_dirs.rs` calls `pulled_in_by_included` and holds no copy of its logic. The `install_set` doc comment is updated to say what it now counts.

### Task 8: Parity test, scan tarball against an unchanged refine export

**Files:**
- Create: `crates/refine/tests/not_carried_parity_test.rs`

The fixture is collector-built, not hand-set: a snapshot with leaf data where
- an included leaf pulls in two auto packages, one with a `/var` directory and one owning an enabled unit;
- one `Unneeded` auto package owns a preset-enabled unit (the `firewalld` case), with a `/var` directory;
- one `Unneeded` auto package owns a user-enabled unit in `state_changes` (the `rhcd` case);
- one `Unneeded` auto package is needed only by another `Unneeded` one;
- one `anaconda`-sourced auto package and one repo-less auto package, both outside the class.

The scan half runs `apply_scan_package_defaults` and `render_all`. The refine half builds a `RefineSession` from the same collected snapshot and exports with no operations.

**Tests first (this whole task is the test):**
- [ ] The `dnf install` package names are equal.
- [ ] The Not carried Containerfile blocks are byte-identical.
- [ ] The `systemctl` lines and the `# Omitted:` lines are equal.
- [ ] `inspectah-var.conf` is byte-identical.
- [ ] The audit report's Not carried tables are equal.
- [ ] A companion assertion fails if the fixture's not-carried list is empty, so the test cannot pass vacuously.

**Acceptance criteria:** the test sits beside `disabled_repo_gate_test.rs` and names the other known scan-versus-refine differences as outside its scope in its module doc, so nobody widens it into the L item by accident.

### Task 9: CHANGELOG, skills, docs

**Files:**
- Modify: `CHANGELOG.md` (`## [Unreleased]`, per `process-docs/changelog-policy.md`)
- Modify: `process-docs/skills/service-omission-model.md` (the `NotCarried` reason; the not-carried class under "Excluded is not absent")
- Create: `process-docs/skills/not-carried-auto-packages.md`, and add it to `process-docs/skills/index.md`
- Modify: `process-docs/skills/codebase-layout.md` (the new render module)
- Check, and update if it describes the packages section: `docs/reference/output-artifacts.md`

**Acceptance criteria:**
- The CHANGELOG entry says, in user terms: the scan tarball no longer counts installer-default packages nothing needs as part of the image; it lists them as Not carried in the Containerfile and the audit report; their units are omitted with a reason; `/var` directories of packages a kept package pulls in are no longer provisioned. It names the change in the Added Packages count.
- The new skill states the class, the one decider, the fallback, and the rule: a new consumer that needs "is this package in the image" asks disposition plus `pulled_in_by_included`, never the leaf filter.
- No team member names anywhere.

### Task 10: Real-host validation

**Files:**
- Modify: `process-docs/skills/real-host-validation-runs.md` (a dated "Measured" section)

Follow `process-docs/skills/real-host-validation-runs.md` and the hypervisor skill supplied with the dispatch.

- [ ] Revert `rhel9` and `rhel10` to their clean snapshot, apply the same drift and plants as the 2026-11-09 rescan (driftify `--profile standard`, `cockpit`, `pcp-zeroconf`, and `openh264` from a disabled repo on RHEL 9), then scan with a build of this branch.
- [ ] The not-carried list equals the 38 (RHEL 9.8) and 24 (RHEL 10.2) sets from the 2026-11-09 rescan by `name.arch`. A difference is explained before the run is called good.
- [ ] Packages owning an enabled unit (at least `firewalld`, `cronie`, `audit`) lead the list.
- [ ] Export from refine with no changes and compare against the scan tarball: the same install names, Not carried block, `systemctl` lines and `inspectah-var.conf`. The other known L-item differences may appear and are recorded, not fixed.
- [ ] Run the COPY-source check from `export-paths-stage-different-trees.md` on both extracted exports, then `podman build` both on each host. All four builds succeed.
- [ ] In each built image, `rpm -q` on three listed packages reports not installed, and no `systemctl` step failed.

**Acceptance criteria:** the measured section records commit, hosts, counts, build results, and any difference from the expected sets with its cause.

### Task 11 (later, separate): refine UI group with one-click include

**Owner: the web frontend owner, not this plan's implementer.** Not executed with Tasks 1 to 10.

- A "Not carried" group in the refine packages view, fed by `not_carried_packages` through the projection, in the same order, with a one-click include per package.
- Including one must name it in the install list (refine's `original_includes` path already bypasses the leaf filter for a changed decision) and must restore its units and `/var` carriage.
- Known limit to surface in the UI: the tree records leaf closures only, so the dependencies of an included auto package are not credited as pulled in and may stay listed as not carried until the tree records them.

### Task 12 (later, separate, needs a decision): `x-export-warnings` for enabled units

Possibly add an `x-export-warnings` entry when a not-carried package owned a unit enabled on the host. Not executed with this plan. Bring to Mark with the measured list from Task 10, since on the measured hosts this would warn for `firewalld`, `auditd` and `crond` on every export.

---

## Needs a decision before execution

1. **The reason wording.** The agreed example said "installed by the OS installer on the source host". The snapshot does not record dnf's install reason, so the renderer cannot know that per package. On the measured hosts it is true for the 18 group installs and indirect for the 20 that only they need, and on another host an orphan left by `dnf remove` would fall in the class too. This plan uses "not user-installed on the source host, and nothing this image installs needs it". Saying "installed by the OS installer" truthfully needs dnf's reason in the snapshot, which is a format change.
2. **"Enabled", not "running".** Ordering reads `services.enabled_units`, which records enablement, not whether a unit was running at scan time. The plan says "was enabled" on every surface.

## Risks

- **A running firewall, audit daemon and cron daemon leave the image by default.** That is the decision, and the list is the mitigation, but a reader who never opens the audit report or reads Containerfile comments will not notice. The list's ordering puts these first; Task 12 is the stronger mitigation and is deferred.
- **The class is only as right as the tree.** The tree judges rich dependencies on the source host (the deferred item), and records leaf closures only. A package dnf would install on the target through a condition the host did not satisfy is listed as not carried and loses its unit and `/var` carriage. The rescan found no such case on RHEL 9.8 or 10.2, but those hosts did not exercise the joint-Supplements or self-conditional rules.
- **Weak dependencies assume `install_weak_deps` on.** The tree counts Recommends as pulled in. If a base image turns weak dependencies off, a Recommends-only package is classed `PulledIn` but not installed. That is the old failure in the other direction (a unit kept for a package dnf skips) and predates this plan.
- **The scan snapshot's dispositions change value.** Consumers outside this repo that read `inspection-snapshot.json` and counted included packages will see fewer. The schema version does not change, because no type changed.

### The strongest case against this plan

Applying the default on the scan path makes the scan tarball depend on the leaf tree in a way it did not before. Today a wrong tree on the scan path costs a stray `systemctl` line or a `/var` directory not provisioned; after this plan a wrong tree silently removes a package's unit and provisions around it, and the only signal is a comment line. The tree has been rewritten twice in a week (`c397f274`, then the file, weak and rich rework) and the one item that would make it judge the target, not the host, is deferred. A more cautious sequence would ship the visible list first with no disposition change (Tasks 3 to 5 against an "included but unneeded" reading), let one release of real scans confirm the list matches what operators expect, and only then flip the default. What would settle it: a third real host with a different install profile (a minimal install, or one with a desktop group) whose not-carried list contains no package the built image turns out to need. Task 10 covers two hosts, both from the same profile.
