# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed
- **File context rules are replayed as real commands** — custom `semanage fcontext` rules were collected as raw table rows, and every one of them failed the renderer's shell safety check, so the generated build carried a FIXME in place of each of the first ten rules, nothing at all for any beyond that, and a commented-out `restorecon -Rv /` suggestion at the end. They are now collected as typed records and emitted as `semanage fcontext` commands that are valid against the target's policy. Order follows the source host's policy store when `file_contexts.local` and `file_contexts.subs` can be read, and the sorted output of `semanage fcontext -l -C` otherwise, with a note in the build saying which. Each label and unlabel rule removes its own key from the target's local store, then adds it, or modifies it when the target policy already defines the key; a rule whose path the target already maps as an equivalence is skipped at build time with a FIXME, because deleting its key would delete that equivalence. An equivalence rule instead reads the target's own mapping for its subject: added when absent, left alone when identical, and the build stops with an instruction when the target maps it elsewhere. There is no build-time `restorecon`, because a bootc target labels the deployed tree from the image's own policy. `/srv` is a symlink to `/var/srv` on the RHEL bootc base and the policy carries no alias for it, so a `/var/srv` equivalence is added once, ahead of the first `/srv` rule that needs it, to make the rules reach the real path.
- **A repo-enabling package installs from the repo that served it** — a `-release` package installed outside any repo rendered twice, once as an active install and once as a manual block, and the build could resolve neither reliably. Such a package is now recognised only when the host's enabled repos actually served it, the serving repo ids travel in the snapshot, and the export keeps the active install only while one of those repo definitions is still carried, enabled, and keyed, and every repo file and GPG key the package itself provides is still included. When no proof survives, the package is demoted to the upload path with an instruction naming the repo. A CodeReady Builder section enabled on the source host is carried as `--enablerepo` on the four `dnf` transactions that resolve against it: the repo-enabling bootstrap, the package list, the group install, and the module-stream enable.
- **Repo and group controls act on their members** — a repo toggle was decided by repo-exclusion history and a group toggle by a check that read every declared member present in the package list and answered "already satisfied" for a group with no member present at all, so include-all on a repo whose history said enabled returned success and recorded nothing even with a member individually excluded, and a group exclude did the same whenever the check came back empty. Both now read the effective member set, the packages the operation would actually change, and an operation that would change nothing is refused with a message saying so. The repo switch reads the same counts and posts the direction it derives from them, in the same release, so no control it offers can post an op the server refuses. Its accessible name carries the repo id ahead of those counts, so two repos whose counts agree are still told apart by a screen reader navigating control to control.
- **`/etc` content is written from the Configuration Files decisions alone** — the export rewrote the kernel snippet directories after honouring those decisions, with no include check, so excluding a file under Configuration Files did not keep it out of the image, and RPM-owned unmodified `modprobe.d` files shipped in every build. There is now one source for `/etc` content. PAM files, audit rules, and custom tuned profiles become ordinary Configuration Files entries with their own include decision and their own rows in the reports.
- **A `/var` directory is backed only by a tmpfiles entry that creates it** — backing was decided by an unanchored recursive grep that walked up to `/var`, so a stock `d /var/lib` line marked every directory under it as backed and the unbacked-directory advisory never fired. Only a `d`, `D`, `v`, `q`, or `Q` entry at exactly that path, a unit `StateDirectory=`/`CacheDirectory=`/`LogsDirectory=`, or RPM ownership counts. The export then checks whether that creator reaches the image and provisions the directory when it does not, or prints an instruction when it cannot tell.
- **Uploaded RPMs keep their real filename and are proven before export** — an RPM whose filename did not match the host's exact version was matched, marked, and then silently left out of the build, so the build failed on a missing `COPY` source. Uploads are now staged under their own name through one allowlist, streamed to disk in bounded chunks rather than buffered whole, placed and recorded under one lock, and verified as a regular file inside the staging directory before the export copies them. A staged file that cannot be verified demotes its package with an instruction instead of failing the export.
- **The `/var` summary's outcome column uses the words the count above it uses** — a directory the export has to make itself was listed as "provisioned by this export" under a summary reading "1 unbacked", so the two halves of the same row carried opposite valence and shared no word the reader could match them on. "carried" was also the only one of the three still written in the code's own vocabulary, and it is the value that means "nothing to do here". Each outcome now leads with the word it is counted under: "backed: creator carried by this export", "unbacked: provisioned by this export", and "backing unconfirmed", which already matched.
- **Scan output drops the em dash** — a non-success inspector line read `FAIL — podman not found` and the subscription certificate warnings ended `— rebuild soon` and `— will not work on unregistered systems`. The reason now follows a colon (`FAIL: podman not found`, `skip: disabled`) and each subscription warning ends in its own sentence, matching the wording the generated README already used.

### Removed
- **Dead `/var` fields and phantom kernel decisions** — `size_estimate` and `recommendation` were written by nothing and rendered as placeholder text on every `/var` row. Every loaded kernel module carried an include decision no surface read and no renderer honoured; they are inventory now. `non_default_modules` and `repo_providing_packages` were never populated.

### Security
- **RPM filenames are allowlisted before they reach a `COPY`, a `RUN`, or the filesystem** — an uploaded or cached RPM filename reaches four places in the generated build: a `COPY` source, a shell-form `RUN dnf localinstall`, the `rm` operand that follows it, and the commented copy of all three that a pre-excluded package carries. None of them was checked. The cached-RPM path built the filename from four snapshot fields and interpolated it straight into the `RUN`, so a scanned host reporting a package named `evil$(id)` produced a build that ran `id` as root, and the manual-resolution block put an unescaped package name in a comment, where a newline ended the comment and made the rest of the name an instruction. The upload handler had no allowlist either: it stripped the directory components off the name it was given and accepted whatever was left, so long as it ended in `.rpm`. There is now one ASCII allowlist, and one type that can only be built by passing it. The upload handler runs it and answers 400, the export runs it and demotes the package to a FIXME with a warning, the cached-RPM renderer and the source-tarball extraction run it and emit a FIXME instead of a `COPY`, and the upload-directory rescan on session resume runs it and leaves an unrecognised file unattached. A name the allowlist refuses is never sanitised into one it accepts. The export writes each uploaded package's file from the handle it verified rather than from a recorded path, and it is the only writer of that path.
- **`/var` directory paths are allowlisted before they reach a `RUN` or a tmpfiles.d entry** — the second instance of the same class, and the entry above does not cover it. A `/var` directory the export has to provision is named in a shell-form `RUN mkdir -p` when the scan captured no mode for it and in a tmpfiles.d `d` entry when it did. The path comes from the scanned host, tarball loading checks traversal, entry type, size, schema and provenance but not paths, and neither emission checked or quoted it. A snapshot carrying `/var/lib/app;id` produced a build that ran `id` as root, and a path with a newline could add a tmpfiles.d directive of its own for systemd to apply at boot. Both emissions now take a path that has passed an ASCII allowlist, through a type that has no other constructor, and a path the allowlist refuses is refused rather than rewritten: the artifact carries an instruction to create the directory by hand in place of the build step.

### Breaking
- **Snapshot schema 23** — removed fields and changed types make schema 22 snapshots unloadable. Re-scan; aggregates re-aggregate from re-scanned hosts.

### Fixed
- **A failed RPM upload no longer reports success** — the refine web UI branched on the response status alone and never read the server's `{"error": "…"}` body, and the upload modal caught the resulting error and discarded it. Every refusal left the green "`foo.rpm` matches nginx" checkmark on screen with the Upload button live again, so an operator whose upload was rejected for its filename, its size, or a staging conflict saw nothing at all go wrong. Both upload routes now surface the server's reason: the modal replaces the checkmark with the refusal text and offers no retry, and the status code rides along in the message because two of the reasons ("internal error", and axum's "failed to read stream" for a body over the 500 MiB route limit) say nothing on their own. The DELETE route had the same defect and the same fix, so removing a package whose RPM came from the host's dnf cache now reports why instead of logging "Conflict".
- **Reference sections no longer lag behind the session's own decisions** — two surfaces answered from a projection of the session's original snapshot, built once and cached for the server's lifetime. Once a decision was made, the cached surfaces kept reporting the original include state: a Configuration Files exclusion dropped the file from the export while its Kernel and Boot mirror kept reporting it included, and the Storage view reported a /var directory as carried while the export wrote a provisioning line for it. A resumed session's first read reported an exclusion it had just replayed as still included, since the cache read the pristine snapshot rather than the replayed state. Both caches are gone: reference sections now project from the live session on each request.
- **The refine web download's Containerfile no longer references a tmpfiles fragment the archive doesn't contain** — the tarball downloaded from the refine UI emitted `COPY config/usr/lib/tmpfiles.d/inspectah-var.conf` in its Containerfile without writing that file into the archive, so `podman build` failed on a missing COPY source with nothing said beforehand. The CLI's own render path wrote the member correctly; only the web download's export path skipped it. The mismatch predates this phase, but it previously needed a scan verdict of an unbacked /var directory, which no decision in the UI could produce. This phase made it reachable by decision: excluding a tmpfiles.d fragment under Configuration Files now provisions the directory and emits the COPY line, so an ordinary UI action could produce a download that fails to build.

## [0.9.0-beta.3] - 2026-08-16

### Fixed
- **RPM-owned paths containing a space were treated as unmanaged** — the owned-path set was built from the first whitespace-delimited field of each `rpm --query --all --dump` line. `rpm` quotes nothing and puts the path first, so an owned path containing a space was cut at that space and never entered the set. Stock packages ship such paths routinely, firmware blobs named after the hardware they drive being the common case. Two consumers read that set and both were wrong: `/var` directory backing analysis fell through to "unbacked" for directories RPM already provides, so the generated build provisioned them through `tmpfiles.d` when it did not need to, and the `/usr` walk recorded stock files as unmanaged entries in the snapshot. Ownership is now read one path per line via `--qf '[%{FILENAMES}\n]'`. This is a pre-existing defect, not a beta.2 regression.

## [0.9.0-beta.2] - 2026-08-16

### Fixed
- **Config advisories no longer count as files you excluded** — section stats bucketed every finding by its include flag, so an advisory landed in the excluded count. The refine stats bar read "0 included / 1 excluded" beside the advisory row it was describing, and the TUI's System Configuration badge, which reports advisories as whatever the decisions leave over, read "0 adv" for a group holding two of them. Section stats now carry a third `advisory` bucket, and the three partition the section. The refine web stats bar and the TUI status bar each render that bucket as a third counter on the sections that have one, and stay two-counter bars on the hosts that have none — before, a three-item config section holding two advisories read "1 incl / 0 excl" on the TUI bar, leaving two of its three items unaccounted for. The export dialog no longer counts advisories among the files you excluded; it reports them separately, noting that they are listed in the audit report while their files are not copied into the image.
- **A space in a venv directory name broke the generated build** — the venv directory name is interpolated into `/tmp/<name>-requirements.txt`, a separate token from the venv path and outside the quotes that path gets. A legal path like `/opt/app/my venv` rendered a three-argument `COPY` and handed `pip install -r` a stray token, so the build failed. All three interpolation sites now quote the name. This is a pre-existing issue predating 0.9.0-beta.1.
- **Comment-only values could emit live Containerfile instructions** — the npm global prefix and the pip package list are interpolated into comment lines, and neither was filtered. A newline in a prefix or in a package name closed the comment, and the rest of the value became an active `RUN` instruction in the generated Containerfile. pip package names come from `.dist-info` directory names, which a service account owning a venv under `/opt` can write. Both values are now escaped for their comment context. This is a pre-existing issue predating 0.9.0-beta.1, not a beta.2 regression.
- **Config advisories now appear in the reports** — modernization findings (sysvinit scripts and the rest of the pattern catalog) and cross-tree symlink findings are carried on the config entry's disposition, which every include-filtered table treats as excluded. They reached no report at all. Both the HTML and markdown audit reports now list them as advisories with their rationale, and the System Configuration group badge counts them.
- **Config advisories now appear in the TUI** — the same modernization and cross-tree symlink findings rendered in the TUI's Configuration Files section as excluded, toggleable files. They now render as advisory rows with the `ℹ` marker, their advisory type, and their rationale in the detail view, off the same collector the reports use.
- **Doubled leading slashes in language environment paths** — the npm global and system gem collectors stripped only the first leading slash, so a prefix reported as `//usr/lib/node_modules` was stored one way and rendered another. Every collector now strips all leading slashes, and the Containerfile renderer restores exactly one.
- **Shell metacharacter rejection widened** — the renderer safety guard for language-package paths and package names rejected only single quotes and newlines. It now also rejects `;`, `$`, backtick, `|`, and `&`, which matters most for npm global package names, since those interpolate into `RUN npm install -g` unquoted.
- **Orphan full-shadow services labelled as advisories in the HTML report** — a preset-matched service whose unit file is fully shadowed in `/etc/systemd/system/` has no state change to act on, but rendered as an ordinary included row in the service table and was left out of the group advisory badge. It now renders in the advisory list with its rationale and counts toward the badge, matching the audit report.
- **Advisory toggles are now refused, not silently ignored** — `SetInclude` on an advisory config finding (cross-tree symlink, modernization) returned success while discarding the change. It now fails with a clear "advisory item is not toggleable" error, matching the existing inventory behaviour.
- **Advisory and inventory findings no longer render as toggleable in the refine web UI** — the frontend read a finding's include flag as `disposition?.include ?? true`, and neither an advisory nor an inventory finding carries that key. Every one of them rendered as an item the user had chosen to bake into the image, with a live checkbox whose toggle the server discarded. The web contract now models all three dispositions, advisories render with their type and rationale and no toggle, and inventory findings render as inventory.
- **Language package pins are reachable from the keyboard** — a package row inside an expanded npm global environment was focusable and arrow-navigable, but Enter and Space did nothing there: the pin was reachable only by tabbing into the checkbox inside the row. Enter and Space now toggle the pin from the row, and the change is announced the same way a mouse click is.
- **Config advisories now appear in the refine web UI** — the same modernization and cross-tree symlink findings the reports and the TUI gained rendered here as excluded, toggleable config files, and a host whose only config finding was an advisory got an "all configuration files excluded" empty state that hid it entirely. They now render as advisory rows with their type and rationale, off the same collector the reports use, and a replayed pre-beta.2 toggle can no longer downgrade one to an included file.
- **A resumed stale toggle can no longer bake an advisory file into the image** — `resume_from` replays an autosaved timeline without re-validating each op, and the projection applied a `SetInclude` to a config entry unconditionally. A session saved before advisory toggles were refused could therefore replay one, rewriting a modernization or cross-tree symlink finding as actionable-included in the projection the export renders from — so the flagged file was copied into the image. The projection now preserves advisory and inventory findings, matching the merge layer.
- **Refused toggles return 422 instead of 500** — the refine web API answered a `SetInclude` on an advisory or inventory finding with a bare "internal error" at HTTP 500, which reads as a tool crash and hides the reason. Both refusals now return 422 with a message naming the item and why it cannot be toggled.
- **Advisory findings survive refine normalization** — opening a snapshot in refine overwrote advisory config dispositions with tier-based include defaults, erasing the rationale and letting the export copy a flagged file into the image. Advisory and inventory findings now pass through normalization unchanged.
- **Disposition defaults on snapshot re-render** — network connections, firewall zones, firewall direct rules, and the tuned profile selection no longer deserialize as "actionable, included" when their `disposition` key is absent from a snapshot. Network findings default to inventory and the tuned selection defaults to excluded, so `--from-snapshot` re-renders no longer bake inventory-only items into the Containerfile.

## [0.9.0-beta.1] - 2026-08-14

### Added
- **Reset to defaults** — new "Reset to defaults" button next to undo/redo reverts all include/exclude changes to the initial analysis state. Gated behind a confirmation dialog, disabled when no ops have been applied.
- **Auto-exclude reason labels** — pre-excluded unlocked packages now show a grey badge explaining why they were excluded: "Installer default", "Unclear provenance", "No repo source", or "Dependency". Locked items are unaffected.
- **Per-section batch toggle** — "Include all" / "Exclude all" kebab menus in the sidebar are now anchored to individual triage sections (e.g. "Configuration Files") rather than group headings (e.g. "System Configuration"). Ctrl+Shift+A/X shortcuts work from per-section nav items.
- **Config all-excluded empty state** — when every config file is excluded, the config section shows an "All configuration files excluded" empty state instead of the header and empty decision list.
- **npm global package detection** — globally-installed npm packages are detected via `npm list -g --json` (high confidence) and directory walk (medium confidence), with per-prefix binding and RPM filtering. Scoped packages (`@angular/cli`) supported.
- **C-extension detection** — pip environments with native `.so` files in site-packages are flagged with `has_c_extensions`, surfaced as an orange "C extensions" badge in the refine UI.
- **Scan expansion** — `--scan-home all|user,...` and `--scan-path /path` flags expand non-RPM scanning beyond the default roots. `/var/www` added as a default scan root. Scan scope persisted in snapshot metadata.
- **Per-package version pinning** — npm global packages support per-package pin/unpin in refine. Pinned packages render as `name@version` in `npm install -g` commands. New `SetPackagePin` and `SetBulkPackagePin` refine ops with full session persistence.
- **Expandable package sublists** — npm global environments in the refine UI expand to show individual packages with pin toggles, keyboard navigation (ArrowUp/Down/Escape), bulk pin/unpin, aria-live announcements, and search-driven auto-expansion.
- **system_site_packages badge** — pip venvs using system site-packages show a blue badge in the refine UI.
- **Extended finding kinds** — — findings now carry Advisory, Inventory, or Actionable semantics. Advisory findings (cross-tree symlinks, modernization, unbacked /var) display rationale but are non-toggleable. Inventory findings (network connections) are informational-only across all surfaces.
- **Network section in HTML report** — network connections now render as an inventory section in the HTML report, with ifcfg deprecation advisory when legacy network-scripts are detected.
- **`/var` directory discovery** — storage inspector scans /var/lib, /var/log, /var/cache for non-trivial directories and classifies their backing mechanism (tmpfiles.d, StateDirectory, CacheDirectory, LogsDirectory, RPM-owned, unbacked).
- **Full-shadow detection for all services** — full unit-file shadows at /etc/systemd/system/ are detected even when no .service.d/ drop-in directory exists, with rationale about base-image update implications.
- **8-group sidebar** — refine web sidebar replaced with data-driven NavExpandable groups (Packages, System Config, Services & Scheduling, Users & Identity, Network, Storage, Software, Secrets) sourced from a `/api/groups` endpoint. Collapsed state persists across sections.
- **Sidebar badges** — triage sections show blue badges with decision counts; reference sections show grey badges. Cleared-state "0" badge with screen-reader announcement when all items in a section are excluded.
- **Sidebar keyboard navigation** — number keys 1-8 jump to groups, aria-current stays on active section when parent is collapsed, focus restores to group heading on collapse.
- **Group-level batch toggle** — kebab menu on triage group headings with "Include all" / "Exclude all". Packages group has a confirmation dialog for "Exclude all". Ctrl+Shift+A/X shortcuts.
- **Full-shadow service rendering** — services with full unit-file shadows show a warning amber border, "Shadow override" gold badge, and shadow rationale linked via aria-describedby. Section header shows shadow count.
- **ifcfg deprecation banner** — network section shows a PatternFly info Alert when legacy network-scripts connections are detected, with note text from the backend constant.
- **tmpfiles.d provisioning** — Containerfile emits `/usr/lib/tmpfiles.d/inspectah-var.conf` for unbacked /var directories with known ownership and mode. Directories without ownership data fall back to `RUN mkdir -p`. Ownership resolution follows a 5-step priority (root → materialized user → RPM-packaged → numeric with comment → numeric).
- **/var ownership display** — unbacked /var directories show ownership and mode (e.g., `0750 postgres:postgres`) in the HTML report, TUI, and refine web storage sections.
- **Orphan full-shadow synthesis** — services with `/etc/systemd/system/` shadows but no state divergence entry now get synthetic ServiceStateChange entries, making them real toggle targets through the full refine lifecycle (session, toggle, autosave, reload, export).
- **EL8 target image mapping** — RHEL 8 and CentOS Stream 8 hosts, which have no bootc base image of their own, now resolve to the EL9 target by default (`registry.redhat.io/rhel9/rhel-bootc:9.6`, `quay.io/centos-bootc/centos-bootc:stream9`), and EL8-to-EL9 migrations are classified as major upgrades. Default resolution stays pinned-minor-with-floor for all targets; use `--base-image` to track `:latest` explicitly.

### Fixed
- **Repo toggle for incomplete-provenance repos** — repos like EPEL with `incomplete` provenance were missing their toggle switch in the repo-first view, making them impossible to deselect. Aligned RepoGroupHeader with RepoBar to show the toggle for all non-unknown provenance repos.
- **Batch undo is now atomic** — "Include all" / "Exclude all" now undoes as a single step instead of requiring one undo click per item. Batch-toggle ops are recorded as a single timeline entry via a new `TimelineEntry::Batch` variant.
- **Bundler deprecation** — replaced deprecated `bundle install --deployment` with `bundle config set --local deployment 'true' && bundle install` in Containerfile rendering.
- **`--system-site-packages` in venv creation** — pip venvs with `system_site_packages: true` now include `--system-site-packages` in the rendered `python3 -m venv` command.
- **Renderer shell safety** — paths and package names in rendered Containerfile `RUN` lines are now single-quoted. Tokens containing single quotes or newlines are rejected with a warning comment.
- **Symlink containment in scan walkers** — all recursive directory walkers enforce scan-root containment and cycle detection via visited-inode tracking, preventing escape through directory symlinks in user-controlled trees.
- **Scan-root duplicate suppression** — `--scan-home` and `--scan-path` deduplication now uses component-aware path matching instead of string-prefix checks, so `/var/www2` is no longer incorrectly suppressed by `/var/www`.
- **`/var` directory depth-2 discovery** — `discover_var_directories()` now scans to `-maxdepth 2`, catching spec acceptance cases like `/var/lib/pgsql/data`. Deduplicates by keeping only leaf directories when both parent and child are discovered.
- **`/var` advisory wording corrected** — unbacked /var directories are not ephemeral on reboot (as previously stated); the actual problem is lack of declarative lifecycle management. Wording corrected across HTML report, TUI, and projection types.
- **Unreadable shadow file degradation** — bare shadow files in `/etc/systemd/system/` that cannot be read now trigger inspector degradation instead of silently producing empty content entries.
- **Unbacked /var paths in web adapter** — `web_storage_section()` now includes `unbacked_var_paths` as advisory items, making them visible in the React frontend.
- **Orphan full-shadow drop-ins in HTML report** — preset-matched services with full shadows (no `state_changes` entry) now appear as actionable service findings with rationale helper text in the HTML services table.
- **ifcfg deprecation note in TUI** — the `IFCFG_DEPRECATION_NOTE` advisory is now shown in the TUI Network section when connections use legacy network-scripts paths.
- **Full-shadow detection for preset-matched services** — services matching their preset (e.g., sshd enabled with preset=enable) were invisible to full-shadow detection because they went into `preset_matched_units`, not `state_changes`. An independent scan of `/etc/systemd/system/` now catches shadows regardless of which bucket the unit landed in.
- **Unbacked /var directories in Containerfile** — unbacked /var directories now emit `RUN mkdir -p` lines in the Containerfile output. Previously collected but only surfaced in the audit report.
- **Unbacked /var advisory in HTML report, refine, and TUI** — the unbacked /var advisory now renders in the HTML report storage section, is projected into RefStorage for the refine view, and appears as an advisory item in the TUI.
- **Complete network data in HTML report** — the HTML report now renders firewall zones, firewall direct rules, static routes, IP routes, IP rules, resolv.conf provenance, /etc/hosts additions, and proxy entries. Previously only NM connections were shown. The network item count now reflects all network data types.
- **Merge preserves finding semantics** — aggregate and fleet merge operations no longer collapse Advisory/Inventory findings back to boolean include/exclude. The `with_include()` method on `FindingKind` preserves non-actionable variants through merge.
- **TUI network items non-toggleable** — network connections, firewall zones, routes, and other network inventory items are now rendered as non-toggleable inventory rows in the TUI. Toggle guard and session-level validation reject inventory ItemId variants.
- **Batch-toggle slug routing** — `POST /api/batch-toggle/:group_slug` now uses `SectionGroup` slugs instead of ad-hoc group names. Reference-only groups are rejected.
- **Language package / unmanaged file double-counting** — system gems and npm manifest projects are no longer duplicated as unmanaged COPY lines in the Containerfile. All six language detection methods now feed the scan exclusion filter, and system gems use the actual gem directory path for prefix matching.

## [0.8.7-beta.1] - 2026-06-29

### Added
- **Language package replication (Tier 1)** — pip, npm, and gem environments are detected, rendered as executable Containerfile output, and exported with manifest files. Pip venvs are recreated faithfully; npm/gem use lockfile-copy. Confidence-based rendering: high=active, medium=commented-out, low=advisory.
- **Unmanaged file collection (Tier 2)** — `--include-unmanaged` catalogs files from /opt, /srv, /usr/local not owned by RPM or Tier 1 language packages. Includes provenance signals (mutability, writable mount, service working directory), size confirmation prompt (suppressible with `-y`/`--yes`), and per-file toggles in refine.
- **Repo-less RPM handling (Tier 3)** — packages with no repo source or a disabled/removed repo are detected automatically. Cached RPMs from `/var/cache/dnf/` are bundled; missing RPMs get a `MANUAL` annotation. Refine UI upload endpoint (`POST /api/upload-rpm`) allows manual RPM provision.
- **`-y`/`--yes` global CLI flag** — suppresses interactive prompts for CI/automation use.
- **`--exclude-path` scan flag** — repeatable path exclusion for unmanaged file collection.
- **Symlink-safe unmanaged scanning** — symlinks are detected without following, preserved as tar symlink entries, and rendered as `RUN ln -sf` directives in the Containerfile.
- **Refine UI: Language Packages section** — new decision section showing pip/npm/gem environments with per-environment toggles, confidence badges, package counts, and manifest basis labels. Keyboard shortcut 6.
- **Refine UI: Unmanaged Files section** — new decision section with directory grouping, per-item and per-group toggles, provenance signal badges (mutability, writable mount, service workdir), running size rollup, /var path warnings, and ArrowLeft/Right keyboard expand/collapse. Keyboard shortcut 7.
- **Refine UI: RPM upload modals** — single-file upload with NEVRA validation and batch upload with auto-matching, conflict detection, and matched/unmatched/conflicts view. Focus trap and focus return on close.
- **Refine UI: repo-less RPM row states** — packages without repo sources show upload icon instead of checkbox. Five states: cached_excluded, cached_included, needs_upload, uploaded_excluded, uploaded_included. Row-level aria-live announcements for state transitions.
- **Refine UI: `--include-unmanaged` discoverability** — sidebar hint when scan was run without `--include-unmanaged`, guiding users to re-run for unmanaged file coverage.
- **Global search for new sections** — language packages searchable by environment path, package name, and ecosystem. Unmanaged files searchable by path. Results navigate to correct section with reveal highlighting.
- **Compose stack reference** — detected compose files are listed as a reference-only comment block in the Containerfile with Quadlet migration guidance. Raw YAML exported under `compose/` in the tarball, subject to secret redaction.
- **Aggregate: language packages section** — aggregate mode now includes a Language Packages section with zone-based layout, prevalence badges, ecosystem/confidence metadata, package-list variant diffs, and searchable package names.
- **Aggregate: unmanaged files section** — aggregate mode now includes an Unmanaged Files section with zone-based layout, file type/size metadata, provenance signals in detail pane, content-hash variant comparison, and searchable file paths.

### Changed
- **Refine sidebar** — Language Packages and Unmanaged Files sections now appear unconditionally with explicit empty states instead of being hidden when empty
- **RPM upload feedback** — upload modals now show whether the RPM matched a repo-less package, with inline warnings for unmatched files, version-mismatch info display, and an export confirmation gate for unmatched uploads
- **RPM batch upload modal** — shows list of packages needing RPMs with live match progress before the drop zone, using a collapsible checklist with green/grey labels
- **Manifest redaction coverage** — redacted exports now scrub auth-bearing URLs from `requirements.txt`, `package.json`, `package-lock.json`, `Gemfile`, and `Gemfile.lock` in both sidecar files and `inspection-snapshot.json`.
- **RPM ownership check** — pip RPM filtering now uses `rpm -qf` path ownership proof instead of `python3-<name>` heuristic, preventing false suppression of user-managed packages.
- **Deduplication uses ecosystem+path** — language environment dedup key includes ecosystem, preventing same-path npm+gem projects from collapsing. System pip now collects all packages into a single environment entry.

### Fixed
- **RPM upload matching** — uploaded RPMs now match repo-less packages by name and architecture instead of requiring exact NEVRA filenames. Supports non-standard filenames from vendor downloads, COPR, and manual builds. Upload endpoint now returns match status (`matched`/`unmatched`) with canonical `name.arch`.
- **RPM repo-less false positives** — repo-less detection now uses case-insensitive substring matching between install-time short names (`AppStream`, `baseos`) and full repo IDs (`rhel-9-for-aarch64-appstream-rpms`). Previously ~50% of packages were falsely flagged on real RHEL systems.
- **Python venv detection for "venv" directories** — removed `venv` from PRUNE_DIRS so the venv walker can discover environments at the most common path (`/opt/myapp/venv/`).
- **npm detection for projects without lockfile** — added package.json manifest fallback scan for npm projects without `package-lock.json`. Detected with method `npm manifest` and confidence `low`.
- **Ruby gem detection for system-installed gems** — added system gem detection via `gem list --local` with RPM ownership filtering. Detected with method `gem system` and confidence `medium`/`low`.
- **Duplicate repo display** — package tables now use the same repo identifier as the config tree. Source repo short names are normalized to full repo IDs using `.repo` file section headers.
- **Pip venv paths normalized** — venv renderer now produces absolute paths (`/opt/myapp/venv`) instead of relative (`opt/myapp/venv`).
- **Refine exclusion honored** — high-confidence language environments with `include: false` no longer emit active `COPY`/`RUN` lines.
- **Export contract test method strings** — test fixtures updated to use canonical method constants.

## [0.8.6-beta.5] - 2026-06-27

### Added
- **Prevalence badge display toggle** — clicking any prevalence badge in aggregate mode toggles between fraction (45/50) and flat percentage (90%) display. Toggle is global — one click changes all badges.
- **Aggregate sidebar include/total counts** — decision sections in aggregate sidebar now show "N included / M total" in their badges, matching the per-section counts available in single-host mode.

### Changed
- **Aggregate stats bar simplified** — removed "N need review" / "All reviewed" labels from the aggregate stats bar header. Host count and total items remain.
- **Aggregate sidebar simplified** — removed "N/M confirmed" ack progress labels from sidebar nav items. Item count badges remain.

### Fixed
- **Dark mode prevalence badge contrast** — prevalence badges now have proper dark-mode color overrides instead of hardcoded light-mode colors.
- **Keyboard navigation in aggregate mode** — number keys (1-9) now jump to the correct aggregate sections. Previously they sent single-host section IDs, causing partial navigation failures.
- **Aggregate default selections** — packages and config files in aggregate mode now correctly default to excluded when not present on 100% of hosts. Previously all items defaulted to selected regardless of prevalence, requiring manual deselection of partial-prevalence items.
- **False "unredact hashes" offer** — User Artifact Preview no longer shows the redact/reveal banner when the displayed content has no redacted material. ContainerfilePanel "Reveal hashes" button also hidden when the Containerfile has no crypt(3) hashes.
- **Container row click target** — entire quadlet row is now clickable to expand/collapse the unit file content, not just the small chevron indicator. Follows the same pattern as package decision rows.
- **RHEL repo classification** — RHEL-style long repo IDs (e.g. `rhel-9-for-x86_64-baseos-rpms`) are now correctly classified as distro repos. Previously, only short CentOS-style IDs (`baseos`, `appstream`) were recognized, causing RHEL base repos to appear as toggleable third-party repos instead of always-on.
- **Ungrouped packages disappearing** — ungrouping a DNF package group now correctly surfaces individual members in the package list and Containerfile. Previously, non-leaf group members were filtered out by the leaf dependency filter after ungrouping. Removed the empty `# Ungrouped from "..."` Containerfile comment.
- **Inspectah COPR repo in config files** — inspectah's own COPR repo definition is now auto-excluded from config files and repo file output. The migration tool should never carry its own repo into the target image.
- **Group toggle removed** — removed the non-functional group-level toggle switch. Groups are managed via the ungroup button (dissolves into individual packages) or per-member actions.

## [0.8.6-beta.4] - 2026-06-22

### Added
- **Subscription cert expiry display** — scan output now shows entitlement certificate expiration date when `--preserve subscription` is used. Warns at <7 days remaining, errors if already expired. Also included in the generated README.
- **Anaconda gap classifier** — packages installed by the RHEL installer (Anaconda) are now classified as platform plumbing and auto-excluded from migration scope. Dramatically reduces migration noise by hiding installer-default packages.
- **Package group dependency visibility** — group members now show whether they're already in the base image. Summary labels distinguish "new" members from base-image members. Progressive disclosure replaces fixed truncation for long group member lists.
- **Version changes table** — context section now renders version changes as a grouped table (upgrades/downgrades with EVR formatting) instead of simple list.
- **Networking subsections** — context section networking split into clear subsections: Connections, Firewall, Routes & Rules, DNS & Hosts, Proxy.
- **Kernel & boot subsections** — context section kernel & boot split into Customizations vs Defaults/Context for clearer organization.
- **Service state display** — refine UI now shows both current (host) state and preset default alongside each service.
- **Container section quadlet content** — quadlet unit file content now viewable inline via expand/collapse in container section.
- **Sidebar subsection counts** — sidebar section counts correctly sum subsection items for subsection-only sections.
- **Accessibility improvements** — ContextList subsections use semantic headings and ARIA region landmarks.
- **Pull failure classification** — five error categories (registry unreachable, auth required, image not found, TLS/cert error, unknown) with tailored remediation guidance including disconnected-environment workarounds.
- Build metadata in version output — `inspectah version` and `--version` now show commit hash and build date
- Compile-time build script (`build.rs`) captures git revision and date

### Changed
- **CLI command rename** — `fleet` subcommand renamed to `aggregate` — all CLI commands, types, modules, and documentation updated
- **Mandatory baseline** — `--no-baseline` flag removed; baseline extraction is now required. Scans that cannot pull the target image exit with code 3 with remediation guidance. Use `--base-image` to override auto-resolution.
- **CLI flag rename** — `--baseline` flag renamed to `--target-image` (old flag still accepted as alias for compatibility).
- **Exit codes** — pull failures now exit with code 3 (previously the scan would continue with degraded output).
- **Scan progress output** redesigned as append-only streaming receipt.
- **Progress modes** simplified from three (rich/plain/flat) to two (pretty/flat).
- **Sub-step detail** moved behind `--verbose` flag.
- **Verbose mode** now works with both pretty and flat modes; flat mode respects `--verbose` (previously always showed sub-steps).
- **Tuned profiles** auto-enabled by default (previously required manual inclusion).
- **Schema version** bumped to 19. Tarballs from older schema versions are no longer loadable.

### Fixed
- **RPM performance** — massive speedup through batching: `dnf group info`, `rpm -qR`, and `--whatprovides` calls now batched into single invocations. Dramatically reduces scan time on hosts with many packages/groups.
- **Platform plumbing packages** hidden from refine view (installer-default packages no longer clutter migration scope).
- **User refinement operations** preserved after anaconda reclassification.
- **Config content truncation** removed (previously capped at 500 characters).
- **InstalledGroup members** filtered to installed-only packages (previously included uninstalled metadata).
- **Triage count badge** removed from UI (was noisy, not useful).
- **Subuid badge** removed from user cards.

### Removed
- **`--no-baseline` flag** — baseline is now mandatory.
- **`--progress rich` and `--progress plain` modes** (use `--progress pretty`).

### Known Issues
- RHEL-subscribed builds (`--preserve-subscription`) do not work when inspectah runs on non-RHEL hosts. The subscription material is host-specific and cannot be transferred across distributions.

## [0.8.5-beta.2] - 2026-06-05

### Added
- Unified include-default model for all 25 toggleable item types
- Locked items with reason badges in both web UI and TUI
- Shell completions auto-generated via clap_complete
- Experimental TUI mentioned in README
- CHANGELOG.md

### Changed
- Include flag is now authoritative for render overrides
- Fleet prevalence gate removed (now handled by aggregate merge)
- Single-host normalization moved to collectors
- Fleet handlers consume stored include values directly

### Fixed
- Validate `--ack-sensitive` before scanning instead of after full scan
- Progress display race condition causing duplicate spinner lines
- Triage diagram text clipping in expanded detail panels
- Triage diagram icon/text overlap in fleet category nodes
- Correct COPR username in README and docs
- Add missing kickstart file to README output tree
- Repair broken Getting Started link in docs
- MongoDB URL redaction preserves connection string structure
- Symlink resolution during /etc ownership classification
- FlatpakApp missing fleet field in test initializers

## [0.8.5-beta.1] - 2026-06-02

### Added
- PasswordHash pattern for secret detection
- PEM full-block matching for certificate detection
- False-positive value filtering for NSS/PAM tokens
- Comment-line filtering to pattern matching
- Documentation landing page for GitHub Pages site
- Experimental TUI for refine mentioned in README

### Changed
- Removed Homebrew install section from README (Rust CLI only)
- README rewritten for Rust CLI
- Removed shipped and Go-era specs and plans

### Fixed
- Clear fleet redaction_state properly
- Comment-line secret detection accuracy
- ExportDialog and App.routing test failures
- Serialized env-var tests to eliminate flaky race
- Null-safety and double-toggle bugs in ContextItem
- Test fixtures that produced noop operations

### Removed
- Pre-promotion compatibility shims

## [0.8.5-alpha.1] - 2026-06-01

### Added
- Project reference extraction system for cross-section analysis
- Network and storage reference extractors
- Container and kernel/boot reference extractors
- Service reference extractor
- Version change reference extractor
- Include field to RefinedTunedSelection
- Projection types module for reference-based refinement

## [0.8.4-alpha.1] - 2026-05-30

### Changed
- RPM-based dependency classification is now the primary path (replaced DNF-based resolution)

### Fixed
- Massive performance improvement: baseline filter now runs before DNF dependency resolution (reducing analysis time from 711 seconds to seconds)

## [0.8.3-alpha.2] - 2026-05-29

### Added
- Timing instrumentation to RPM inspector phases

### Fixed
- Baseline filter ordering bug - packages now filtered before DNF dependency resolution
- Build output now streams in real time instead of buffering

## [0.8.3-alpha.1] - 2026-05-29

### Added
- `inspectah build` command for building bootable container images
- `--preserve-subscription` flag to capture RHSM subscription material
- `--ack-sensitive` flag (renamed from `--acknowledge-sensitive`)
- Subscription fields to snapshot schema (v18)
- SubscriptionFile, SubscriptionSection, and EntitlementPair types
- SubscriptionInspector for RHSM material collection
- Integration tests for preserve-subscription feature
- Comprehensive documentation site using Jekyll and just-the-docs theme
- Diataxis-structured documentation (tutorials, how-to guides, reference, explanations)
- Six D3 diagrams embedded in documentation
- First-migration tutorial
- Contributing documentation
- CLI reference from help output
- Getting started tutorial
- Build and subscription documentation

### Changed
- Reframed project as distro-neutral FOSS tool
- Subscription files staged in tarball output
- Documentation moved to GitHub Pages

### Fixed
- Hardlink rejection in tarball extractor (security)
- Full symlink chain resolution in subscription inspector
- Fail fast when `--keep-context` target is non-empty
- Deterministic ambient/fallback proof tests
- Diagram centering on fullscreen enter/exit

## [0.8.2-alpha.2] - 2026-05-26

### Changed
- Variant file tree removed from tarball output
- Empty env files skipped in tarball output
- Schema placeholder file removed from tarball
- Non-universal divergent items demoted to informational

### Fixed
- User-toggled packages bypass leaf filter
- Leaf filter skipped for fleet snapshots in Containerfile
- Export dialog warning updated for promoted sections
- Version change display swapped to host → base
- Third-party repos use warning color
- Empty non-toggleable repos hidden from RepoBar
- `@commandline` repo shows 'not included' in RepoBar

## [0.8.2-alpha.1] - 2026-05-26

### Added
- System Tuning section (merged sysctls and tuned)
- Triage bucket type system
- `--verbose` and `--quiet` flags to scan command
- Consistent section headings to all content panes
- Top-level checkbox to UserCard header

### Changed
- Sysctls and tuned merged into unified System Tuning section
- Section promotion complete (all phases)
- Triage classification system implemented
- DECISION/CONTEXT_SECTIONS renamed to REVIEW/REFERENCE
- Default to strict intersection for package include in fleet

### Fixed
- Prevalence badge contrast improved
- Stock default tuned profiles suppressed from Containerfile
- Intersection default applied to all section types
- Cross-section state bleed and search collisions prevented
- Tuned profile include and prevalence in fleet view
- Projected include used for decision section toggles
- Fleet banner contrast for dark theme
- Expand chevron hidden for empty/whitespace-only detail
- `@commandline` repo made non-toggleable with friendly label
- Entire row clickable for expand/collapse
- Config noise - system-generated files filtered from unowned file detection
- RPM-owned file filtering uses sentinel format

## [0.8.1-alpha.2] - 2026-05-24

### Added
- Unified package/repository management across single and fleet views
- Accessibility contract: ARIA live regions, grid headers, focus management
- Fleet conflict popover and excluded zone states
- Containerfile change highlights with auto-scroll
- Reduced motion support for animations
- OS theme auto-detection with manual override
- Hostname popover to fleet StatsBar

### Changed
- Multi-line format for systemctl enable/disable/mask
- Leaf-package filter applied to fleet snapshots in Containerfile

### Fixed
- Focus handoff and keyboard navigation
- Attention badges removed from fleet item rows
- Banner text uses neutral dark color
- Variant selection decoupled from auto-review
- Info attention badge text contrast
- Main container stretches full viewport width
- Fleet content fills full viewport width
- Variant view renders inline below item row
- Search result selection highlight softened

## [0.8.0-alpha.4] - 2026-05-19

### Added
- Service intent inference with typed contract
- Service context subsections in UI
- Service omissions and advisories surfacing
- Owning package resolution during service collection
- Display implementations for service types
- `inspectah fleet` and `fleet init` commands
- Fleet aggregate Phase 1 functionality
- SSH tunnel hint when starting refine server
- Refine command shown after scan completes

### Changed
- Centralized service omission decisions
- Strict service deserialization enforced
- Masked service distinction in data model
- Preset unknown handling improved

### Fixed
- Omitted-row duplication and DOM identity conflicts
- Omission comments emitted correctly
- Duplicate package handling
- Baseline comparison uses plain package names
- Owning package guard rejects spaced output
- Service package truth helpers shared

## [0.7.0-go-final] - 2026-06-02

Final release of the Go implementation before the Rust rewrite.

---

[Unreleased]: https://github.com/marrusl/inspectah/compare/v0.9.0-beta.3...HEAD
[0.9.0-beta.3]: https://github.com/marrusl/inspectah/compare/v0.9.0-beta.2...v0.9.0-beta.3
[0.9.0-beta.2]: https://github.com/marrusl/inspectah/compare/v0.9.0-beta.1...v0.9.0-beta.2
[0.9.0-beta.1]: https://github.com/marrusl/inspectah/compare/v0.8.7-beta.1...v0.9.0-beta.1
[0.8.7-beta.1]: https://github.com/marrusl/inspectah/compare/v0.8.6-beta.5...v0.8.7-beta.1
[0.8.6-beta.5]: https://github.com/marrusl/inspectah/compare/v0.8.6-beta.4...v0.8.6-beta.5
[0.8.6-beta.4]: https://github.com/marrusl/inspectah/compare/v0.8.5-beta.2...v0.8.6-beta.4
[0.8.5-beta.2]: https://github.com/marrusl/inspectah/compare/v0.8.5-beta.1...v0.8.5-beta.2
[0.8.5-beta.1]: https://github.com/marrusl/inspectah/compare/v0.8.5-alpha.1...v0.8.5-beta.1
[0.8.5-alpha.1]: https://github.com/marrusl/inspectah/compare/v0.8.4-alpha.1...v0.8.5-alpha.1
[0.8.4-alpha.1]: https://github.com/marrusl/inspectah/compare/v0.8.3-alpha.2...v0.8.4-alpha.1
[0.8.3-alpha.2]: https://github.com/marrusl/inspectah/compare/v0.8.3-alpha.1...v0.8.3-alpha.2
[0.8.3-alpha.1]: https://github.com/marrusl/inspectah/compare/v0.8.2-alpha.2...v0.8.3-alpha.1
[0.8.2-alpha.2]: https://github.com/marrusl/inspectah/compare/v0.8.2-alpha.1...v0.8.2-alpha.2
[0.8.2-alpha.1]: https://github.com/marrusl/inspectah/compare/v0.8.1-alpha.2...v0.8.2-alpha.1
[0.8.1-alpha.2]: https://github.com/marrusl/inspectah/compare/v0.8.0-alpha.4...v0.8.1-alpha.2
[0.8.0-alpha.4]: https://github.com/marrusl/inspectah/compare/v0.7.0-go-final...v0.8.0-alpha.4
[0.7.0-go-final]: https://github.com/marrusl/inspectah/releases/tag/v0.7.0-go-final
