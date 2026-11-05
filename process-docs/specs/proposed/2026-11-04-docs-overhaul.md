# Documentation Overhaul: User Docs Refresh and Developer Guide

## Metadata

| Field | Value |
|---|---|
| Date | 2026-11-04 |
| Status | Proposed. Scope questions decided 2026-11-04 and folded into the body. |
| Scope | Track 1: the existing pages under `docs/` brought to the current code. Track 2: a rewritten `docs/contributing/developer-guide.md` and a new `docs/contributing/testing.md`. |
| Out of scope | New information architecture, a doc site generator change, a README restructure, a glossary, any page for a feature that exists only in a spec. |
| Verified | Drift audit run against `cafa0a5b` (HEAD, 2026-11-04). CLI output taken from `./inspectah-darwin-arm64`, `inspectah 0.9.0-beta.4 (commit 9f918710, built 2026-11-04)`; the six commits between `9f918710` and HEAD touch CI and skill files only. CI workflow and `smoke-integration.spec.ts` re-read at `6952f25d` when the decisions were folded in. Revised at `e645285c` (HEAD, 2026-11-04): the `/usr` walk's readers (§ 2.5 item 5), the diffs of the 33 excluded-scope fixes (§ 2.0), `RepoGroupHeader.tsx`, and the pre-commit hook's file triggers (§ 3.4) read at that commit. Refreshed at `ab0c6475` (HEAD, 2026-11-04) for the eleven commits after `0dc8b908`: § 2.5 item 5's source locations and planned fix, row 2's Node version, the `/srv` routing change (row 40), and the verification-gates skill file (§ 2.5 item 4, § 3.3, § 3.5) re-read at that commit. Revised at `87561ae5` (HEAD, 2026-11-04) for the `/usr` walk decision (§ 1): the `--include-unmanaged` help text (`crates/cli/src/commands/scan.rs:154`) and its gate (`scan.rs:698-745`) read at that commit; the files § 2.5 item 5 cites are unchanged since `ab0c6475`. |
| Audit window | `8ac0a19d^..HEAD`, 300 commits. `8ac0a19d` (2026-06-29, "docs: update user-facing docs for 0.8.7-beta.1 release") is the last substantive docs commit before July. The only later commits under `docs/` are a Jekyll en dash fix (2026-07-07), one sentence in the refine how-to (2026-10-31), and the terminal UI removal sweep (2026-11-04). |

**No page under `docs/` has been edited to produce this file.** The pages are written in a separate pass after this spec is approved.

## Purpose

New developers are joining the project. They and new users hit the same three pages first: getting started, the CLI reference, and the refine how-to. Those pages describe 0.8.7-beta.1. Four releases have shipped since (0.9.0-beta.1 through beta.4), the snapshot schema went from 20 to 25, the finding model gained dispositions, and the refine web UI's navigation was replaced. A reader following the current pages can fail at the first source build, run a documented `refine` flag the binary rejects, and look for UI sections that no longer exist.

This spec fixes that in two tracks, in order:

1. **User docs refresh.** Every existing page brought to the current code, driven by the drift audit in § 2.
2. **Developer guide.** A rewrite of the developer guide and a new testing contract page, with an acceptance test stated in what a new contributor can do, not in which pages exist.

## 1. Settled decisions

- **Refresh, not restructure.** The Diataxis layout under `docs/` stays. No page is added to the user-facing sections, none is removed, none is renamed. The only new page is `docs/contributing/testing.md`.
- **Priority order for Track 1:** getting started, the CLI reference, and the refine how-to first. The rest by severity (§ 2.3).
- **Track 2 merges after Track 1.** The developer guide points at user pages (the CLI reference, the refine how-to) and should point at their corrected versions. The tracks edit no file in common, so Track 2 may be drafted while Track 1 runs; a merge check ties them (§ 5).
- **The tutorial builds the refine export.** Step 6 names where the downloaded export lands and Step 7 builds that file, so the tutorial shows the triage decisions reaching the image. Its output blocks come from the § 4 real-host run.
- **The 0.9.0-beta.4 credential-rotation advisory stays out of the user docs.** The docs describe current behavior. The advisory lives in `CHANGELOG.md` and the beta.4 release notes, where an operator upgrading sees it; `how-to/review-and-refine.md` describes what redaction covers now.
- **Agent and harness content on public pages is one sentence.** The project is developed with AI assistance, and the core team keeps its harness setup in a private team repository. Nothing more about agents, harnesses, or VMs appears under `docs/`.
- **Every code block on an updated page is classified, and run or captured at implementation time.** See § 4.
- **The user docs do not mention the `/usr` walk** (decided 2026-11-04). `--include-unmanaged` is documented as its `--help` text describes it, "Catalog and bundle unmanaged files from /opt, /srv, /usr/local", which is what the user gets. The same switch also runs the `/usr` walk, whose output nothing renders or shows (§ 2.5 item 5); there is no way to request it separately and no surface on which a user could be misled, so a limitation statement would describe something users cannot see, act on, or turn off. The product is a pre-release beta, and the unified unpackaged content design is where the split gets fixed.

## 2. Track 1: the drift audit

### 2.0 Method

1. `git log -- docs/` to find the last docs commit before July: `8ac0a19d`.
2. `git log 8ac0a19d^..HEAD`, explicit range, no `--since`. 300 commits. Every `feat`, `feat!`, and `refactor` subject read (49), and every `fix` outside the `web-ui`, `test`, `ci`, `docs`, and `spec` scopes (about 130).
3. Every `fix(web-ui)`, `fix(web)`, `fix(test)`, and `fix(ci)` commit in the range walked by diff, not subject: `git log --oneline 8ac0a19d^..cafa0a5b | grep -E '^[0-9a-f]+ fix\((web-ui|web|test|ci)\)'`, 33 commits. `git show --stat` for each, then the diff of each of the 30 whose stat touches `crates/web/ui/src/` or `crates/web/src/` outside tests; the other three are two release-workflow fixes and one test-only commit, whose message was read. 20 of the 33 produced a drift row (rows 32 to 39). Of the 13 that did not, seven carry behavior an existing row already names (rows 6, 16, 21), three change no user-visible rule (an upload path comparison, an unlink confinement, a stale projection cache), one was superseded by a later `feat` the subject walk covered, and two are the release-workflow fixes.
4. `CHANGELOG.md` sections `[0.9.0-beta.1]` through `[0.9.0-beta.4]` read in full, plus `[0.8.7-beta.1]` because `8ac0a19d` was written the same day as that release and could have missed part of it (it did, see the refine how-to row). `[Unreleased]` was empty at `cafa0a5b`. At `ab0c6475` it holds three Fixed entries: the two `/usr` walk collector fixes (§ 2.5 item 5) and the `/srv` routing change (row 40).
5. Each page's headings, tables, flags, field names, and code blocks compared against the source and against `--help` output from the built binary.
6. Every `crates/...` path cited in `docs/` checked for existence. All resolve, apart from the two hypothetical files the "adding an inspector" walkthrough tells the reader to create.

**What this audit did not do.** It did not read the diff of every commit in the range: subjects and changelog entries drove the walk of the 49 feature and refactor commits and the in-scope fixes, the excluded-scope fixes were walked by diff (item 3), and source was read where a page made a concrete claim. It did not run a scan: the built binary is a macOS binary and `scan` needs a Linux host. Scan, refine, and build output blocks are therefore unverified here and are re-captured at implementation time (§ 4).

### 2.1 Drift table

Severity: **wrong today** means a reader following the page would fail or be misled. **Missing** means the page is accurate as far as it goes but leaves out current behavior in its scope. **Cosmetic** means wording, sample formatting, or a stale label that misleads nobody.

| # | Change (commit or changelog entry) | What changed for the user | Pages | Severity |
|---|---|---|---|---|
| 1 | Workspace `edition = "2024"` (`Cargo.toml:14`) | Building from source needs Rust 1.85 or newer. | `getting-started.md` ("Requires Rust 1.80+") | wrong today |
| 2 | `crates/web/build.rs` runs `npm ci` and `npm run build`; `crates/web/ui/dist/` is gitignored | A fresh clone without Node.js panics at build time: "No Node.js found and no pre-built ui/dist/ exists". CI uses Node 26 since `ab0c6475`, which moved off end-of-life Node 20; `src/api/__tests__/client.test.ts` fails on 20 and passes on 26. | `getting-started.md` (From source), `contributing/developer-guide.md` (Prerequisites) | wrong today |
| 3 | `refine --open` help text promises `--no-open` | `inspectah refine --no-open x.tar.gz` fails: `error: unexpected argument '--no-open' found`. The working form is `--open false` or `--open=false`. See § 2.5, item 1. | `reference/cli.md` (options table and example), `reference/configuration.md` (refine flags) | wrong today |
| 4 | `d4db2877` feat(cli,collect): `--scan-home`, `--scan-path`, `/var/www` default root (0.9.0-beta.1) | Two scan flags and a new default scan root. | `reference/cli.md`, `reference/configuration.md`, `how-to/non-rpm-software.md`, `reference/inspector-coverage.md` | missing (wrong today in `configuration.md`, see row 5) |
| 5 | 0.8.7-beta.1 `-y/--yes`, `--include-unmanaged`, `--exclude-path` | `configuration.md`'s scan flag table was never updated for 0.8.7 and lists none of these, so with row 4 it is five flags short of `scan --help`. | `reference/configuration.md` | wrong today |
| 6 | `90f446de` FindingKind enum replaces `include: bool` (schema 21); `66f0bb72`, `5331dd09`, `3f01bd73` advisory refusals | Every finding is Actionable, Advisory, or Inventory. Advisory and Inventory rows are not toggleable; a toggle on one is refused with HTTP 422 and a message. No item type carries `include: bool` any more (27 `disposition: FindingKind` fields, zero `include: bool`). | `reference/triage-classification.md` (§ Unified include-default model), `contributing/adding-an-inspector.md` (Step 1 type pattern), `how-to/review-and-refine.md`, `getting-started.md` (triage section) | wrong today |
| 7 | `d80838bb` feat(core)!: schema 23 with exact-match loading; 0.9.0-beta.4 Breaking: schema 25 | `SCHEMA_VERSION` and `MIN_SCHEMA` are both 25. A snapshot from any other schema does not load. | `reference/snapshot-schema.md` ("Current: **19**", and § Serialization's forward-compatibility claim), `how-to/aggregation.md` (inputs from mixed versions) | wrong today (schema page), missing (aggregation) |
| 8 | Snapshot struct field names (`crates/core/src/snapshot.rs`) | The baseline field serializes as `baseline`, not `baseline_data`. `unmanaged_files` and `redaction_skipped` are top-level fields no table lists. | `reference/snapshot-schema.md` | wrong today |
| 9 | `e2547ae4` sidebar overhaul to group-driven NavExpandable; `43cdd091` SectionGroup mapping; 0.8.7-beta.1 "sections now appear unconditionally" | The refine sidebar is eight groups (Packages, System Configuration, Services & Scheduling, Users & Identity, Network, Storage, Software & Files, Secrets & Subscription). The page's flat eleven-section table (Repositories, Quadlets, Flatpaks, Sysctl, Tuned as top-level sections) is gone, and "Sections only show up when the scan found relevant data" contradicts the explicit empty states shipped in 0.8.7-beta.1. | `how-to/review-and-refine.md` (§ Navigate the dashboard) | wrong today |
| 10 | 0.9.0-beta.1 refine additions: reset to defaults, per-section and per-group batch toggles with `Ctrl+Shift+A/X`, number keys 1 to 8 for groups, sidebar badges, auto-exclude reason labels; `7c1e9ddf` batch undo is one step | Controls the page does not mention. | `how-to/review-and-refine.md` | missing |
| 11 | 0.9.0-beta.4 "Repo and group controls act on their members" | A repo or group operation that would change nothing is refused with a message. The page says a repo toggle affects all its packages as a batch, which is still true, but not that a no-op is refused. | `how-to/review-and-refine.md` | missing |
| 12 | 0.9.0-beta.4 "A systemd drop-in has one include decision"; `f8839078` shadowed row names its owner | A service-owned Configuration Files row is inventory, badged "Controlled in Service Drop-ins" or "Merged into sysctl settings", with a link to the owning row. | `how-to/review-and-refine.md`, `reference/triage-classification.md` | missing |
| 13 | `fe62e611`, `3f4aabb5`, `a486d9e4`, `feadd277`: npm global packages and per-package pinning; `caf929b7` C-extension detection; system-site-packages badge (0.9.0-beta.1) | New detection method (`npm list -g`), pin/unpin in refine, two badges. | `how-to/non-rpm-software.md`, `reference/inspector-coverage.md`, `how-to/review-and-refine.md` | missing |
| 14 | `f3bb1671` quote paths in the language package renderer; `684d38f7` `--system-site-packages` | Rendered `RUN` lines single-quote paths (`RUN python3 -m venv '/opt/myapp/venv'`). The page's sample blocks show unquoted paths. | `how-to/non-rpm-software.md` (Containerfile output blocks) | cosmetic |
| 15 | `8c41c0d3`, `81b3acab`, `13f3bd88`, `1c51c2e6`, `26d1a8de`: `/var` directory discovery, backing, and provisioning | The storage inspector discovers `/var/lib`, `/var/log`, `/var/cache` directories to depth 2, classifies backing (exact tmpfiles creator, unit `StateDirectory=`/`CacheDirectory=`/`LogsDirectory=`, RPM ownership), and the export provisions unbacked ones through `config/usr/lib/tmpfiles.d/inspectah-var.conf` or `RUN mkdir -p`. | `reference/inspector-coverage.md` (StorageInspector reads fstab, findmnt, LVM only), `reference/output-artifacts.md` (the tmpfiles fragment is not listed), `how-to/build-bootc-image.md` | missing |
| 16 | `1cd00f43`, `37244ab0`, `cb26eb64`: full-shadow service detection and orphan synthesis; `64e1baf7` modernization advisories; `797c625e` cross-tree symlink advisories; `84fbb7b5` ifcfg banner | New detections and advisory kinds. | `reference/inspector-coverage.md` (Services, Config), `reference/triage-classification.md`, `how-to/review-and-refine.md` | missing |
| 17 | 0.9.0-beta.4 "`/etc` content is written from the Configuration Files decisions alone"; "Dead `/var` fields and phantom kernel decisions" | PAM files, audit rules, and custom tuned profiles are Configuration Files entries. Loaded kernel modules are inventory. | `reference/inspector-coverage.md` (lists PAM and audit rules under SelinuxInspector), `reference/snapshot-schema.md` (selinux row), `reference/triage-classification.md` ("Kernel module: `COPY <modules-load.d-conf>`" row) | wrong today |
| 18 | Network findings default to inventory (0.9.0-beta.1, 0.9.0-beta.2) | Firewall zones and NM connections are inventory. Nothing in the renderer emits `firewall-offline-cmd`. | `reference/triage-classification.md` ("Firewall zone: `RUN firewall-offline-cmd` directives" row) | wrong today |
| 19 | `cbf75fc3`, `adad8965`, `d28932e7`: file context rules replayed as `semanage fcontext` commands (0.9.0-beta.4) | The SELinux block installs `policycoreutils-python-utils`, replays each rule against the target's policy, adds a `/var/srv` equivalence when needed, and emits no `restorecon`. Rule order comes from `file_contexts.local` and `file_contexts.subs` when readable. | `reference/inspector-coverage.md` (SelinuxInspector), `how-to/build-bootc-image.md` (§ Review the Containerfile) | missing |
| 20 | `f7efdaec`, `8e671852`: repo-enabling packages by proven availability; CodeReady Builder carried as `--enablerepo` | A `-release` package installs only when its serving repo is proven carried; otherwise it is demoted to the upload path with an instruction naming the repo. | `how-to/repoless-rpms.md`, `how-to/build-bootc-image.md` | missing |
| 21 | `fdf0e7ad` one RPM filename allowlist; `3a06aa28` bounded-chunk staging; `4a5b0438` upload refusal surfaced | The upload endpoint refuses a filename outside the allowlist with HTTP 400, and the modal shows the server's reason. The page lists only extension, `name.arch` match, and size. | `how-to/repoless-rpms.md` (§ Single upload) | missing |
| 22 | `959f572a` EL8 target image mapping | RHEL 8 and CentOS Stream 8 resolve to the EL9 bootc base by default (`registry.redhat.io/rhel9/rhel-bootc:9.6`, `quay.io/centos-bootc/centos-bootc:stream9`). | `how-to/baseline-subtraction.md` (§ How baseline resolution works) | missing |
| 23 | 0.9.0-beta.4 Security: manifest, compose, quadlet, and timer scrubbing; unresolved scrubber findings downgrade to `PartiallyRedacted` | Redaction covers fields it did not, and a line the scrubber cannot parse now lowers the redaction state instead of passing as clean. | `reference/inspector-coverage.md` (§ Redaction engine), `how-to/review-and-refine.md` (§ Review sensitive data) | missing |
| 24 | `ade11cba` RPM-owned paths read one per line | The RPM inspector's ownership query is `--qf '[%{FILENAMES}\n]'`, not the sentinel `rpm -qa` format the table lists. | `reference/inspector-coverage.md` (RpmInspector data sources) | cosmetic |
| 25 | Long-standing: `.packages` is not a snapshot field | `jq '.packages | length'` on `--inspect-only` output returns `null`; packages live under `.rpm.packages_added`, and there is no `baseline_match` field on a package. Predates the window, found by the audit. | `how-to/ci-integration.md` (§ Extract specific data) | wrong today |
| 26 | Long-standing: the tutorial builds the original scan tarball | Step 7 runs `inspectah build webserver01-...tar.gz`, the scan output, so none of the Step 5 triage decisions reach the image. The refine export is a separate download. Predates the window, found by the audit. The fix: Step 7 builds the export Step 6 downloads (§ 1). | `tutorials/first-migration.md` (Step 6 and Step 7) | wrong today |
| 27 | `fd422ff6` fix(hooks): clippy gate able to fail | The hook runs `cargo clippy --workspace --all-targets -- -D clippy::all`; the guide states `cargo clippy --workspace -- -D warnings`. Resolved in § 3.3. | `contributing/developer-guide.md` | wrong today |
| 28 | Pre-commit hook and `core.hooksPath` (2026-06-28), commit-msg hook | Neither hook is mentioned; a contributor learns about them when a commit is refused. | `contributing/developer-guide.md` | missing |
| 29 | Fleet-to-aggregate rename (June) left doubled words | "aggregate aggregation", "Aggregate aggregate merges", "a aggregate scan", "a aggregate-wide" in 10 files. | `index.md`, `getting-started.md`, `how-to/review-and-refine.md`, `reference/output-artifacts.md` (3), `reference/triage-classification.md` (2), `explanation/triage-philosophy.md` (2), `explanation/architecture.md`, `contributing/adding-an-inspector.md`, `diagrams/aggregate-topology.html` (2), `diagrams/software-architecture.html` | cosmetic |
| 30 | Scan footer prints `Inspected in {secs:.0}s` (`progress/pretty.rs:335`) | The sample shows `Inspected in 42.3s`; the binary prints whole seconds. | `getting-started.md` (scan output block) | cosmetic |
| 31 | `90f446de` dispositions; triage diagram data | `diagrams/triage-decision-tree.html` shows the three buckets and the aggregate zones and nothing of the disposition axis. `diagrams/aggregate-topology.html` describes narrowing as "items below consensus threshold get include: false". | `diagrams/triage-decision-tree.html`, `diagrams/aggregate-topology.html` | missing (tree), cosmetic (topology) |
| 32 | `2494c2be` toggle for incomplete-provenance repos; `5d985b3d` no repo control the server will refuse; `6b303e55`, `c7a38239` state text; `67e93b15`, `37709a54` switch read from members | A repository gets a toggle only when it is not a distro repo, its provenance is `verified` or `incomplete`, and at least one member is effective (actionable and unlocked). Every other repository shows state text and no control: "always included" (distro), "disabled on host", "repo id not found in any .repo file" (stale), "installed outside any repo" (unknown provenance; the `@commandline` repo is titled "Local / Manual installs"), and "nothing to decide: all members are locked". A toggleable repo with incomplete provenance carries the note "repo definition incomplete". The switch is on only when every effective member is included, it posts the opposite of what it shows, and its row reads "N of M packages included" ("M packages excluded" at zero). The page's "all packages sourced from that repository are affected" is wrong for locked members. | `how-to/review-and-refine.md` (§ Understand triage indicators, the repository paragraph) | wrong today |
| 33 | `ed461d6c`, `b4adf043`, `5725a3ef` excluded-repo zone | Packages from an excluded repository are listed under "From excluded repos", each with its own checkbox, so one member can be re-included without the repository; the repo's count text then shows the partial inclusion. A package with no serving repository shows an Upload button in place of the checkbox, in this zone as in the main list. | `how-to/review-and-refine.md` | missing |
| 34 | `1c148e58`, `8f1d5eac`, `edca4ded` inert package groups | A package group whose present members are each either locked or from the base image offers no control and reads "nothing to decide: all members are locked or from base". A group operation on it is refused with HTTP 422. | `how-to/review-and-refine.md` | missing |
| 35 | `518f6a50` advisory stats bucket; `c2a1086a` config-borne advisories leave the toggleable config rows | The stats bar shows a third counter, "N advisory", beside included and excluded, omitted at zero. The export dialog reports advisories apart from exclusions: they are listed in the audit report and their files are not copied into the image. Modernization and cross-tree symlink advisories on a config entry render as advisories, not as deselected config files. | `how-to/review-and-refine.md` (the stats bar paragraph and the export step) | missing |
| 36 | `6c578207` sidebar keyboard and focus contract; `11a39389` no Users & Identity batch menu | Number keys 1 to 8 move focus to that sidebar group and keep it in the sidebar; a click moves focus to the content. `Ctrl+Shift+A` and `Ctrl+Shift+X` (`Cmd` works in place of `Ctrl`) act only from a group heading or a single-section group's item, not from a section link inside an expanded group. The batch menu appears only on groups with actionable sections, and never on Users & Identity. | `how-to/review-and-refine.md` | missing (extends row 10) |
| 37 | `d9a7f78f` live pin screen; `ecd5272b` pin from the keyboard | Each package in a language package environment has a pin checkbox, and every environment, including a single-package one, has a bulk pin button. `Enter` or `Space` on a focused package row toggles its pin. A search that matches a package expands its environment and focuses the first match. | `how-to/non-rpm-software.md`, `how-to/review-and-refine.md` | missing (extends row 13) |
| 38 | `d2cca24c` the `/var` summary says what the export does, not only what made the directory | The Storage view's collapsed `/var` summary counts "N unbacked, M unconfirmed" (or "all backed by tmpfiles.d, unit directives, or package ownership"). Each directory line gives the path, mode and owner (or "ownership not recorded"), the creator ("tmpfiles.d (conf)", "StateDirectory= (unit)", "CacheDirectory= (unit)", "LogsDirectory= (unit)", "package-owned (package)", or "nothing declares it"), and the outcome in the words § 2.2 item 2 names, each in its own column. | `how-to/review-and-refine.md`, `how-to/build-bootc-image.md` | missing (extends row 15) |
| 39 | `89b98414` version changes moved into the Packages view | Version changes are a "Version Changes" expandable panel inside the Packages view, not a section of their own. | `how-to/review-and-refine.md` (§ Handle version changes) | missing |
| 40 | `fe8addbf` route `/srv` findings to the `/var` strategy instead of `COPY` (`[Unreleased]`; after the audit window, found on the `ab0c6475` refresh) | An included unmanaged finding under `/srv` gets no `COPY`. The Containerfile carries a comment block and the audit report a note naming the paths and saying to carry them with the `/var` strategy: restore them into `/var/srv` after deployment, or give them their own mount. The files are still bundled under `unmanaged/srv/`. `/opt` and `/usr/local` findings are unaffected. | `how-to/non-rpm-software.md` (§ Containerfile output: "Regular files render as COPY directives"), `reference/output-artifacts.md` (`unmanaged/` row) | wrong today (`non-rpm-software.md`), missing (`output-artifacts.md`) |

### 2.2 Current behavior no page covers

Each of these is user-visible today and absent from every page under `docs/`. The implementation pass places each on the page named; none needs a new page.

1. **Finding dispositions.** Actionable, Advisory, and Inventory; what each means in the refine UI and the export; the 422 refusal. Home: `reference/triage-classification.md`, summarized in `how-to/review-and-refine.md`.
2. **`/var` directory backing and provisioning.** Discovery depth, the backing rules, the three outcome words of the `/var` summary ("backed: creator carried by this export", "unbacked: provisioned by this export", "backing unconfirmed"), and the tmpfiles fragment. Home: `reference/inspector-coverage.md`, `reference/output-artifacts.md`, one paragraph in `how-to/build-bootc-image.md`.
3. **Service shadows.** Full unit shadows, drop-ins, the one-decision rule between Configuration Files and Service Drop-ins, and the drop-in group for units at their preset state. Home: `reference/inspector-coverage.md`, `how-to/review-and-refine.md`.
4. **Advisory catalogs.** Modernization (SysVinit, xinetd, anacrontab), cross-tree symlinks, the ifcfg deprecation banner. Home: `reference/inspector-coverage.md`.
5. **Scan scope flags.** `--scan-home all|USER,...`, `--scan-path`, and `/var/www` as a default root. Home: `reference/cli.md`, `reference/configuration.md`, `how-to/non-rpm-software.md`.
6. **npm global packages and pinning**, the C-extension and system-site-packages badges. Home: `how-to/non-rpm-software.md`.
7. **SELinux file context replay** and why there is no `restorecon`. Home: `how-to/build-bootc-image.md`, `reference/inspector-coverage.md`.
8. **Repo-enabling packages** and the CodeReady Builder `--enablerepo`. Home: `how-to/repoless-rpms.md`.
9. **The refine control set**: eight sidebar groups, badges, number-key and `Ctrl+Shift+A/X` shortcuts and their focus rules, reset to defaults, refused no-op operations, auto-exclude reason labels, the advisory counter, and which repositories and package groups offer a control at all (rows 32 to 36). The repository paragraph states the eligibility rule of row 32 in full, with each state text, so a reader does not take every repository to be toggleable or a repo toggle to reach locked members. Home: `how-to/review-and-refine.md`.
10. **EL8 to EL9 default mapping.** Home: `how-to/baseline-subtraction.md`.
11. **Exact-match snapshot loading.** A tarball from a different inspectah release does not load in `refine` or `aggregate`; re-scan. Home: `reference/snapshot-schema.md`, one line each in `how-to/review-and-refine.md` and `how-to/aggregation.md`.
12. **Redaction's unresolved state.** A line the scrubber matched but could not parse is reported, not passed. Home: `how-to/review-and-refine.md` § Review sensitive data.

### 2.3 Per-page update order

The maintainer's priority pages first, then by severity, wrong today before missing before cosmetic. Within a severity, pages a reader meets earlier go first.

| Order | Page | Severity | Driving rows |
|---|---|---|---|
| 1 | `getting-started.md` | wrong today | 1, 2, 6, 29, 30 |
| 2 | `reference/cli.md` | wrong today | 3, 4 |
| 3 | `how-to/review-and-refine.md` | wrong today | 6, 9, 10, 11, 12, 13, 16, 23, 29, 32 to 39 |
| 4 | `reference/configuration.md` | wrong today | 3, 4, 5 |
| 5 | `tutorials/first-migration.md` | wrong today | 26, plus the triage wording from row 6 |
| 6 | `reference/triage-classification.md` | wrong today | 6, 12, 16, 17, 18, 29 |
| 7 | `reference/snapshot-schema.md` | wrong today | 7, 8, 17 |
| 8 | `reference/inspector-coverage.md` | wrong today | 4, 13, 15, 16, 17, 19, 23, 24 |
| 9 | `how-to/ci-integration.md` | wrong today | 25 |
| 10 | `how-to/non-rpm-software.md` | wrong today | 4, 13, 14, 37, 40 |
| 11 | `reference/output-artifacts.md` | missing | 15, 29, 40 |
| 12 | `how-to/build-bootc-image.md` | missing | 15, 19, 20, 38 |
| 13 | `how-to/repoless-rpms.md` | missing | 20, 21 |
| 14 | `how-to/aggregation.md` | missing | 7 |
| 15 | `how-to/baseline-subtraction.md` | missing | 22 |
| 16 | `explanation/triage-philosophy.md` | missing | one paragraph on dispositions as a second axis beside the buckets; 29 |
| 17 | `explanation/architecture.md` | missing | one paragraph on dispositions in core, one on the two export paths sharing the renderer's staged-directory writers; 29 |
| 18 | `diagrams/triage-decision-tree.html`, `diagrams/aggregate-topology.html` | missing, cosmetic | 31, 29; data edits per `process-docs/skills/docs-diagrams-are-d3-data-arrays.md` |
| 19 | `index.md` | cosmetic | 29 |

`contributing/developer-guide.md` (rows 2, 27, 28) and `contributing/adding-an-inspector.md` (rows 6, 29) are wrong today and move to Track 2, because a contributor reads them together and they share sources. The Step 1 type pattern in `adding-an-inspector.md` becomes a `disposition: FindingKind` field with a named serde default (`process-docs/skills/finding-disposition-serde-defaults.md`), and the walkthrough gains a schema bump step (`process-docs/skills/snapshot-schema-versioning.md`).

### 2.4 Pages that need no change

| Page | What was checked |
|---|---|
| `how-to/customize-output.md` | Every flag it uses (`--progress`, `-v`, `-q`, `-o`, `--inspect-only`, `--preserve`, `--no-redaction`, `--ack-sensitive`) against `scan --help`; the `INSPECTAH_PROGRESS` priority against `crates/cli/src/progress/mod.rs:41`. All match. Its sample output blocks are still re-run under § 4 if the page is touched for row 29. |
| `explanation/migration-model.md` | Conceptual. Searched for flags, counts, section names, field names, and include semantics; found none that changed. |
| `explanation/aggregate-consensus.md` | Conceptual. Same search; prevalence zones and the merge description still match the aggregate code paths named in `reference/triage-classification.md`. |
| `tutorials/index.md`, `how-to/index.md`, `reference/index.md`, `explanation/index.md`, `contributing/index.md` | Seven-line navigation stubs with front matter and no claims. |
| `diagrams/software-architecture.html`, `diagrams/user-flow.html` | Redrawn on 2026-11-04 for the terminal UI removal; the crate graph matches each `crates/*/Cargo.toml`. (`software-architecture.html` still carries one row 29 doubled word.) |
| `diagrams/conceptual-pipeline.html`, `diagrams/data-flow.html` | Searched for schema numbers, `include:` literals, inspector counts, and the removed terminal UI. Nothing found. Not read node by node. |

### 2.5 Code disagreements found by the audit

These are defects in code or CI rather than in the docs. They are recorded here so the docs pass does not write around them silently. Each is fixed outside this spec, apart from item 4, which Track 2 carries.

1. **`refine --open` help text names a flag that does not exist.** The clap help reads "Open browser automatically (use --no-open to suppress)", and `--no-open` is rejected as an unexpected argument. The fix is to the help text: it says `--open false`, which is what works and what the docs show. No `--no-open` flag is added. A one-line change to the doc comment on `open` in `crates/cli/src/commands/refine.rs`, tracked separately.
2. **CI's real-server e2e step cannot load its fixture.** `testdata/e2e-fixture.tar.gz` holds a schema 14 snapshot. Run against the current binary: `error: snapshot load error: json error: unknown variant `disabled`, expected `enable` or `disable``. The "Start refine server" step in `.github/workflows/rust-ci.yml` polls `/api/health` for 30 seconds and falls through without failing. Until `bafd5c70`, `e2e/smoke-integration.spec.ts` then skipped every case and the job was green with no real-server coverage; it now throws in CI when the backend is unreachable, so the failure surfaces in the smoke spec rather than at the step that caused it. The fix, tracked separately: regenerate `testdata/e2e-fixture.tar.gz` at schema 25 from a real scan, and make the start step fail when `/api/health` never answers. `testdata/single-host-e2e.tar.gz` and `testdata/fleet-e2e.tar.gz` are schema 17 and cannot load either.
3. **CI clippy was weaker than the hook. Fixed in `79e46050`.** `rust-ci.yml` ran `cargo clippy --workspace -- -W clippy::all`: no `--all-targets`, and `-W` cannot fail. It now runs `cargo clippy --workspace --all-targets -- -D clippy::all`, the hook's command since `fd422ff6`.
4. **Two skill files contradict the current tree.** `process-docs/skills/full-workspace-verification-gates.md` § 1 still says the hook and CI run `cargo clippy --workspace -- -W clippy::all`, and its front-matter `description` still says the `-D clippy::all --all-targets` form cannot pass and tells the reader how to count a baseline. § 2, which described the nine-warning baseline `fd422ff6` retired, was corrected in `aeac871b`: it now states the command as required and passing at zero warnings. `process-docs/skills/codebase-layout.md` gives the dependency direction as "pipeline → refine" (the manifests say refine depends on pipeline) and the inspector method as `collect()` (the trait method is `inspect`). The developer guide tells contributors to read both files first, so Track 2 refreshes them (§ 3.5).
5. **The `/usr` walk is collected and never read** (`33ba88e9`). With `--include-unmanaged`, the scan walks `/usr`, collapses paths no RPM owns to their shallowest unowned ancestor, and stores them in `usr_entries` (`crates/collect/src/inspectors/nonrpm.rs:2129`), a type that calls each entry actionable and in need of a `COPY` (`crates/core/src/types/nonrpm.rs:192-194`). Nothing outside the collector reads the list: the Containerfile renderer reads only `items` (`unmanaged_file_lines` and `included_var_srv_paths` in `crates/pipeline/src/render/unmanaged.rs:42-80`), the refine web adapter maps only `items` (`crates/web/src/adapter.rs:460-480`), and the aggregate merge builds its merged section with `usr_entries: Vec::new()` (`crates/core/src/aggregate/merge.rs:1931`), so an aggregate drops the list. Every other construction of the field, including `crates/pipeline/src/render/unmanaged.rs:169` and `crates/web/src/adapter.rs:2555`, is a test fixture. The collector itself is correct: `8cb3264d` applies the unmanaged files exclusions to the `/usr` walk (the scan roots under `/usr`, `--exclude-path`, and Tier 1 language environments), `96bc606b` counts the `/usr` form of a legacy usrmerge path as owned, and `56f9e8b8` pins both with tests. The output surface is the open part. The planned fix is the roadmap's "Unpackaged Content, Unified" entry (`ROADMAP.md`), tracked in the project backlog as `inspectah-unified-unpackaged-content`: one "Unpackaged content" section with location and provenance as attributes, in place of a separate `/usr` section, designed as the first design task for the engineers joining the project. The August presentation plan (`process-docs/plans/2026-08-15-usr-walk-presentation-plan.md`) is marked superseded (`6ffa3f60`). The user docs leave the `/usr` walk out on purpose: `--include-unmanaged` is one switch with no separate way to request the `/usr` walk and no UI section or Containerfile output for it, so there is nothing a user can see or act on, and the product is a pre-release beta whose unified design replaces the split (§ 1).

## 3. Track 2: developer guide and testing contract

### 3.1 Acceptance test

A new contributor, with a fresh clone and no one to ask, can answer each question below in under an hour of reading. "Answer" means they could act on it: run the command, put the file in the right place, predict the hook's verdict.

| # | Question | Answered by |
|---|---|---|
| A1 | How do I build and run the tests the way the pre-commit hook and CI run them, including the web UI toolchain and the insta snapshot flow? | `developer-guide.md` § Build, § Run the gates; `testing.md` § Gates |
| A2 | Where does each kind of change go: specs, plans, skills, changelog, docs, tests? | `developer-guide.md` § Where changes go |
| A3 | What rejects my commit, and why? | `developer-guide.md` § What rejects a commit |
| A4 | How do I validate against a real host, and when is it required? | `testing.md` § Real-host runs, pointing at `process-docs/skills/real-host-validation-runs.md` |
| A5 | What do I read first? | `developer-guide.md` § Read first |

**How the acceptance is checked.** Before the pages merge, a reviewer who has not worked on inspectah, or a fresh agent session given only a clone and these two pages, answers A1 to A5 in writing and runs the A1 commands on a clean checkout. The pages pass when every answer is correct and every command runs as written. A wrong answer is a page defect, not a reader defect.

### 3.2 `docs/contributing/developer-guide.md`, rewritten

Sections, in order. The existing workspace table and cross-compilation section carry over; the rest is new or replaced.

1. **Read first.** `CLAUDE.md` at the repo root, then `process-docs/skills/codebase-layout.md`, then `process-docs/skills/index.md`. One sentence each on what the file gives you. Then the one AI sentence: the project is developed with AI assistance, and the core team keeps its harness setup in a private team repository.
2. **Prerequisites.** A table in the style tool, why, check:
   - Rust 1.85 or newer via rustup, with `clippy` and `rustfmt` components (edition 2024). Check: `cargo --version`, `cargo clippy --version`.
   - Node.js 26 and npm, the line CI pins since `ab0c6475` (the web UI's vitest suite fails on 20). `crates/web/build.rs` runs `npm ci` and `npm run build` on every build that touches the web crate, and panics without Node unless `crates/web/ui/dist/` already exists. Check: `node --version`.
   - `cargo-insta`, for reviewing snapshot changes. Check: `cargo insta --version`.
   - Playwright's Chromium, for e2e only: `npx playwright install chromium` from `crates/web/ui`.
   - For real-host runs: a throwaway Linux VM you can revert (see `testing.md`).
   - `cargo-zigbuild`, for release cross-compiles only.
3. **Clone and build.** Clone, `git config core.hooksPath .githooks` (once per clone, and the hooks do nothing without it), `cargo build`. Release build and binary location carry over.
4. **Workspace structure.** The existing table and dependency sentence, which are correct.
5. **Run the gates.** The exact commands, in the order the hook runs them, then the frontend ones:
   - `cargo fmt --check`
   - `cargo clippy --workspace --all-targets -- -D clippy::all` (§ 3.3)
   - `cargo test --workspace`
   - from `crates/web/ui`: `npm ci`, `npm run test` (vitest), `npm run build` (runs `tsc`, so it is the type check), `npm run test:e2e` (Playwright)
   - The insta flow: a changed snapshot fails `cargo test`; run `cargo insta review`, accept only a change you intended, and commit the updated `.snap` file beside its test (`crates/cli/src/progress/snapshots/`, `crates/web/tests/snapshots/`, `crates/collect/tests/snapshots/`, `crates/pipeline/src/render/snapshots/`).
   - A pointer to `testing.md` for when each gate is required and what a green result does and does not prove.
6. **Where changes go.** A table: specs in `process-docs/specs/proposed/`, moved to `implemented/` once shipped; plans in `process-docs/plans/`; skill files in `process-docs/skills/<topic>.md`, indexed in `index.md`, with `codebase-layout.md` updated when a crate, command, or major directory changes; changelog entries under `## [Unreleased]` per `process-docs/changelog-policy.md`; user docs under `docs/` (GitHub Pages, never specs or plans); tests beside the code (`#[cfg(test)]` modules, `crates/*/tests/`, `crates/web/ui/src/**/*.test.tsx`, `crates/web/ui/e2e/`). Two rules that cost people time: a type change bumps the schema (`snapshot-schema-versioning.md`), and a flag change is hand-edited into all three files under `completions/` (`completions-are-checked-in-and-stale.md`).
7. **What rejects a commit.** The pre-commit hook's three gates, and that it skips entirely when no `.rs`, `Cargo.*`, or `build.rs` file is staged, so a frontend-only commit gets no check from it. The commit-msg hook, which refuses `Co-Authored-By`. Commit format `type(scope): description` in imperative mood, body explaining why. The attribution trailer for AI-assisted commits: one `Assisted-by: <tool> (<model>)` line. `--no-verify` as a last resort only. Pass a commit message with `-F <file>`, never through a pipe (`full-workspace-verification-gates.md` § 5).
8. **PR process.** The existing five steps, with step 3 replaced by "run the gates in § 5" and a sixth: add the changelog entry when the change is user-visible.
9. **Cross-compilation.** Carried over.

### 3.3 The clippy command

Four sources disagreed when this spec was briefed. Measured at HEAD (`6952f25d`):

| Source | Command it states |
|---|---|
| `.githooks/pre-commit` (since `fd422ff6`, 2026-11-04) | `cargo clippy --workspace --all-targets -- -D clippy::all` |
| `CLAUDE.md` § Key Conventions (same commit) | `cargo clippy --workspace --all-targets -- -D clippy::all` |
| `process-docs/skills/release-process.md` (same commit) | `cargo clippy --workspace --all-targets -- -D clippy::all` |
| `docs/contributing/developer-guide.md` | `cargo clippy --workspace -- -D warnings` |
| `.github/workflows/rust-ci.yml` (since `79e46050`, 2026-11-04) | `cargo clippy --workspace --all-targets -- -D clippy::all` |
| `process-docs/skills/full-workspace-verification-gates.md` (re-read at `ab0c6475`) | § 2 states the hook's command since `aeac871b`; § 1 still describes the pre-`fd422ff6` hook and the pre-`79e46050` CI line |

**Resolution: the guide states `cargo clippy --workspace --all-targets -- -D clippy::all`.** It is the command that actually rejects a commit, and `CLAUDE.md` and the release checklist already state it. It passes on HEAD (`cafa0a5b`, run for this spec: clean finish, no warnings). The skill file's warning that this form "exits 101 on a clean tree" described the nine-warning baseline, which `fd422ff6` fixed; `aeac871b` removed that warning from the skill file's § 2.

The guide's current `-D warnings` form also denies the default clippy lints, and rustc warnings besides, so the flag is not the gap that matters. The gap is `--all-targets`: without it clippy checks lib and bin targets only, and every warning this tree has carried lived in a test target. CI runs the hook's command since `79e46050`, so a PR that passes CI's clippy step passes the hook's.

### 3.4 `docs/contributing/testing.md`, new

The testing contract. What must pass, when a real host is required, when a browser pass is required, and what a green result does not prove. It carries measured facts from `process-docs/skills/full-workspace-verification-gates.md` and `process-docs/skills/e2e-fixtures-are-hand-maintained.md`, restated for a contributor rather than linked, because a contributor reads `docs/` first.

1. **Gates before a PR.** A table: gate, command, what it catches, required when.
   - fmt, clippy, `cargo test --workspace`: every PR that adds, copies, modifies, or renames a `.rs` file (build scripts included), a `Cargo.toml`, or `Cargo.lock`. These are the hook's exact triggers: `.githooks/pre-commit` matches `git diff --cached --name-only --diff-filter=ACMR` against `\.(rs)$|Cargo\.(toml|lock)$|build\.rs$` and skips every gate when nothing matches. A manifest-only or lockfile-only PR is in scope. A commit that only deletes such files matches nothing, so the hook skips it and the contributor runs the three gates by hand.
   - `npm run test`, `npm run build`, `npm run test:e2e`: every PR that touches `crates/web/ui/`, and every PR that changes a web DTO or handler under `crates/web/src/`, because the hook does not run them.
2. **What a green result does not prove.**
   - `npm run test:e2e` reports skipped tests as part of a green run. Locally, `e2e/smoke-integration.spec.ts` skips every case when no refine server answers on `127.0.0.1:8642`; under CI it fails instead (`bafd5c70`). Report the skipped count separately from the passed count, every time.
   - The e2e fixtures under `crates/web/ui/e2e/fixtures/` are hand-written JSON, not generated from the server's DTOs. A DTO change updates them by hand; a passing e2e run does not show the DTO matches what ships.
   - CI's real-server step loads nothing until its fixture is regenerated (§ 2.5, item 2). The page says so plainly rather than describing the step as coverage.
3. **How long the gates take.** From the skill file's measurement, with its machine named: a clean rebuild plus the full suite is one to two minutes, the hook on a realistic change is under a minute. A run that has not returned in a few minutes is hung, not slow; check `ps -eo pid,etime,command` for a `rustc` child before waiting longer. Periodic `cargo clean` is cheap maintenance.
4. **When a real-host run is required.** Required for a change to: an inspector or any non-test file under `crates/collect/`; the lines the Containerfile renderer emits; either export path (the CLI scan tarball or the refine web download); redaction; `/var` backing; SELinux replay; and any claim that a generated build succeeds. Not required for: presentation-only web UI changes with no DTO change, docs, and test-only changes. A test-only change touches nothing but test code (`tests/` directories, `#[cfg(test)]` modules, fixtures, and snapshot files), and it is exempt wherever it lives, `crates/collect/` included. Precedence, so each PR gets one answer: the requirement is decided by which non-test files the diff changes, and any non-test change in a required area requires the run, whatever the PR says about its effect. A refactor under `crates/collect/` that claims no output change therefore needs the run, because the run is what shows the output did not change. A manifest-only PR needs the local gates (item 1) and no real-host run, unless it changes a dependency of a crate in a required area (`collect`, the renderer in `pipeline`, redaction), which counts as a change to that area. The run is scan, refine, export, `podman build` against a host carrying synthetic drift from the companion fixture tool, never a clean install. The method and its traps are in `process-docs/skills/real-host-validation-runs.md`; this page links it and does not repeat it.
5. **When a browser pass is required.** Any change under `crates/web/ui/src/` that alters what renders or how a control behaves, any change to a web DTO or handler the UI consumes, and any keyboard or accessibility change. The pass runs a real refine server (`inspectah refine --open false --port 0 <tarball>`, which prints its origin), waits for the page to settle, exercises the changed control by mouse and by keyboard, and confirms the server's response, including a refusal. Unit tests and the e2e suite run against mocked APIs and do not substitute for it.
6. **One sentence on harnesses.** The same AI-assistance sentence as the developer guide. The browser and VM capabilities an agent needs are specified in the core team's private repository.

### 3.5 Skill file refresh (Track 2, process-docs)

Because the guide's first instruction is to read them:

- `full-workspace-verification-gates.md`: § 1 rewritten to the current hook command, with the CI line named as matching the hook (`79e46050`), and the front-matter `description` rewritten to drop the claims that the `-D` form cannot pass and that a baseline is counted. § 2 needs no change: `aeac871b` already states the command as required and passing at zero warnings. The timing table is kept, with its measurement date.
- `codebase-layout.md`: dependency direction corrected to the manifests (core, collect, pipeline, refine, web, cli, each depending only on those before it as the architecture page states), and the inspector method corrected to `inspect`.
- `e2e-fixtures-are-hand-maintained.md`: its two "Full write-up" pointers cite backlog items that live outside this repository. Replace each with a one-line summary; a public skill file should not send a reader somewhere they cannot follow.

## 4. Acceptance criteria

1. Every code block on an updated page is one of three kinds, and the implementation notes (the implementation commit body, or a notes file it names) list each block under its kind.
   - **Executable command**: something the reader runs. It is run at implementation time against the current binary and behaves as the page says, including a documented failure.
   - **Captured output**: pasted from a run made at implementation time, never kept from an earlier version of the page or an older run. The notes record, for each block, the command, the binary version or commit, the fixture or host profile, and the capture date. `--help` and argument-error blocks run against the built binary on any platform. Scan, refine, export, and build output runs on a real Linux host carrying synthetic drift, since a clean host produces near-empty output that would make every sample unrepresentative.
   - **Schematic snippet**: an explicitly hypothetical block, such as a file the reader is told to create or a template with placeholders. The prose around it says it is illustrative, and the notes give the reason a live capture is impossible. A run that was merely inconvenient is not a reason, and existing output is not reclassified as schematic to avoid re-capturing it.
   The reviewer of the implementation accounts for every schematic snippet and every captured block against the notes. A block in no list, or a captured block missing any of its four fields, fails this criterion.
2. Every row of § 2.1 is closed by an edit on each page it names, or by a note in the implementation commit saying why the row was not applicable on closer reading.
3. Every item of § 2.2 appears on the page named for it.
4. `grep -rn 'no-open' docs/` returns nothing. `grep -rniE 'aggregate aggregat|a aggregate' docs/` returns nothing.
5. Every `crates/...` path cited in `docs/` resolves, apart from the two hypothetical files in the adding-an-inspector walkthrough.
6. The two diagram edits are verified by serving `docs/` over HTTP, not `file://` (`docs-diagrams-are-d3-data-arrays.md`).
7. Track 2 passes the acceptance test in § 3.1.
8. The voice rules hold on every page touched: no em dashes, no "shape", no "immutable". Quoted command output keeps whatever it prints.
9. The Jekyll build renders every touched page; backtick-wrap any flag in prose so Jekyll does not render `--` as an en dash (`11939ac4`).

## 5. Sequence and sizing

| Track | Pages | Rough sessions | Depends on |
|---|---|---|---|
| 1. User docs refresh | 18 pages and 2 diagrams (§ 2.3): 10 wrong today, 7 missing, 1 cosmetic only, plus the two diagram data edits | 3 to 4. One for the three priority pages and `configuration.md`; one for the tutorial and the four reference pages; one or two for the how-tos, explanations, diagrams, and the doubled-word sweep, including the real-host capture for § 4 item 1 | This spec |
| 2. Developer guide | 2 pages (one rewritten, one new), `adding-an-inspector.md`, 3 skill files | 1 to 2 | Drafting: none. Merging: Track 1 merged, and the merge check below passed |

**The dependency between the tracks is on links and instructions, not files.** No file is edited by both: Track 1's pages are the § 2.3 list, and Track 2's are the developer guide, `testing.md`, `adding-an-inspector.md`, and three skill files. Track 2 may be drafted while Track 1 runs. Before Track 2 merges, a merge check confirms that every link from the guide and `testing.md` into a user page resolves on the merged Track 1 version, and that every instruction the guide repeats from a user page (the source build, the refine invocation, the CLI flags it cites) matches that page's final text.

**Track 2 also waits for the CI repair to settle.** § 2.5, § 3.3, and § 3.4 describe CI, which is under repair, and Track 2 re-audits them against HEAD before the guide is written. That is a scheduling choice by the maintainer, not a file dependency, and it lapses once CI is stable.

The real-host capture for Track 1's output blocks can be batched into one VM run that also exercises the Track 2 `testing.md` commands.

## 6. Counter-case

**The strongest case against this scope: wait for the aggregate UI convergence.** The aggregate UI convergence is also a Phase 3 item on the 1.0 road, and it will change the refine UI again. `how-to/review-and-refine.md` § Work with aggregate data and `how-to/aggregation.md` § Refine aggregate data describe a UI that convergence will replace, so writing them now risks writing them twice. The same argument applies, more weakly, to full capture and its consent gating, which the pre-1.0 docs item expects the docs to describe and which has not shipped.

**Why the recommendation still stands.** The pages that are wrong today are mostly single-host pages, the CLI reference, and the reference schema pages, and convergence does not touch them. The cost of waiting is paid by every new user and developer between now and convergence, on exactly the pages they read first. The aggregate sections are a small part of the refine how-to and can be corrected minimally now (exact-match loading, the doubled words) and rewritten with convergence.

**A second counter-case: swap the tracks.** New developers are arriving now, and a corrected developer guide is two pages against eighteen. Doing Track 2 first would unblock contributors in one session. Against that: the guide links the CLI reference and the refine how-to, and a contributor's first task is often a user-visible change that cites those pages.

**Evidence that would change the recommendation.** If the aggregate UI convergence is scheduled to merge within two weeks, the aggregate sections of both pages should wait for it and Track 1 shrinks by about one session. If new contributors are blocked on build or hook questions before Track 1 can finish, Track 2 should merge first, with the merge check run against the user pages as they stand and run again when Track 1 merges.

## Related

- `process-docs/specs/proposed/2026-11-01-tui-removal.md`: the most recent docs pass, and the source of the 2026-11-04 sweep this audit excludes from the drift window.
- `process-docs/skills/docs-diagrams-are-d3-data-arrays.md`: how the two diagram edits are made and verified.
- `process-docs/skills/full-workspace-verification-gates.md`, `process-docs/skills/e2e-fixtures-are-hand-maintained.md`, `process-docs/skills/real-host-validation-runs.md`: sources for `testing.md`.
- `process-docs/changelog-policy.md`: the changelog rule the developer guide restates.
