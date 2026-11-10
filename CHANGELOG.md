# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- **Packages nothing in the image needs are listed as Not carried**: a package that was not user-installed on the source host, and that no package the image installs depends on, is no longer counted as part of the image. The scan tarball excludes it by default, and both the scan tarball and the refine export list it as `# Not carried: not user-installed on the source host, and nothing this image installs needs it (N)` in the Containerfile, one comment line per package, and as a `### Not carried (N)` table in the audit report with the package, its source repo and the units it had enabled. A package that owned an enabled unit is listed first and the Containerfile names the unit (`firewalld (firewalld.service was enabled)`). The test is the scan's leaf data: a dependency of a package you kept is still carried, because dnf installs it with that package. A kept package the image cannot install, one the Containerfile leaves as a manual step, does not count, so a dependency only it needs is not carried. Locked packages, packages installed at system install time, packages the baseline image ships, uploaded packages and repo-less packages keep their existing handling. When the scan has no leaf data, or the snapshot is a fleet aggregate, nothing is excluded and no list is shown. For a scan with a baseline and leaf data, an unchanged refine export installs the same packages as the scan tarball, except where refine applies its own rules: packages installed at system install time, repo-less packages, and a package only one of those needs, such as `dnf-plugins-core` when refine excludes the repo-less `epel-release` that pulls it in.

### Changed
- **The Added Packages count no longer includes packages that are not carried**: the `### Added Packages (N)` heading in the audit report counts the packages the image installs, so N drops by the size of the Not carried list. Those packages are named in the Not carried table instead.
- **A unit owned by a package that is not carried is omitted with that reason**: its `systemctl` lines are left out and the Containerfile records `# Omitted: firewalld.service (package 'firewalld' is not carried: not user-installed on the source host, and nothing this image installs needs it)`. A unit is omitted only when the existing presence rules already prove its owner absent, so a package with the same name at another architecture that the image does install keeps its unit.
- **`/var` directories of packages a kept package pulls in are no longer provisioned**: the `/var` evidence now counts a package that an installed package depends on as carried, so the Containerfile no longer creates a directory for it and the tmpfiles fragment no longer says its creator is missing.

### Fixed
- **Language package paths with shell characters render again**: a pip, npm or gem project whose path held `$`, `;`, `&`, `|` or a backtick, such as `/opt/app$canary/venv` or `/srv/a&b/webapp`, was skipped with `# WARNING: path contains unsafe characters` since v0.9.0-beta.2, although every place the path is written in the Containerfile single-quotes it. Such a path now renders its install as it did in v0.9.0-beta.1. A path holding a single quote or a newline is still skipped with the warning.
- **The missing-`readelf` warning no longer points at a container image**: when `readelf` is not installed the scan skips its search for compiled programs under the scan roots, and the warning told you to install binutils "in the inspectah container image", which no longer exists because the scan runs directly on the host. The warning now says what the skip means: hand-installed (non-RPM) compiled programs under the scan roots were not detected. It no longer suggests installing anything.
- **A package dnf installs alongside one you kept now counts as pulled in**: the dependency list recorded for each leaf package followed only plain hard requirements. It skipped requirements on a file, so `git` did not list `perl-interpreter` (needed through `/usr/bin/perl`) and `httpd` did not list `mailcap` (through `/etc/mime.types`). It skipped weak dependencies, which dnf installs by default on rhel-bootc 9 and 10, so `httpd` did not list `mod_http2`. And it skipped conditional (rich) dependencies such as `(cockpit-ws-selinux if selinux-policy-targeted)`. Packages missing from that list read as not needed by anything you kept, so an excluded one lost its `systemctl enable` line, or was left out of a disabled repo's transaction, although dnf installs it. The list now follows file requirements, Recommends, Supplements and conditional dependencies, judged against what is installed on the host. A Supplements that needs several packages present together, such as `(shared-lib and desktop)`, counts only for a leaf that brings all of them, not for every leaf that brings one. A version in a conditional dependency is checked, so `(plugin if engine >= 2 else fallback)` counts `fallback` on a host with only `engine` 1, and a version floor such as `(foo >= 2 if foo)` does not count `foo` when nothing else installed provides `foo`. `with` and `without` match one package at a time, as rpm defines them. If the scan cannot read which package owns each file, the list comes from dnf instead, or is marked unavailable, rather than silently missing every file requirement. Suggests are still not followed, since dnf does not install them by default. A snapshot written by an earlier version keeps its old lists until the host is scanned again.
- **A leaf package no longer depends on packages it does not need**: the scan resolved the capabilities that added packages require in batches of 50 and credited every provider in a batch to every capability in it, so unrelated packages were recorded as dependencies of each other. On a RHEL 9.8 host, 25 of 26 leaf packages had the same 114-package dependency list, with `tree`, `htop` and `wget` each depending on `nginx-core`, `httpd-core` and `rsyslog`. That list decides which excluded packages still count as present because something you kept depends on them, so an excluded package could keep its `systemctl enable` line, or be named in a disabled repo's transaction, when nothing installs it. Each capability now maps to its own providers only, which changes the dependency lists in a new scan and therefore which packages count as dependencies. A snapshot written by an earlier version keeps its old lists until the host is scanned again.
- **A multi-value sysctl set to its configured value no longer reports as drift**: the kernel prints values such as `net.ipv4.ip_local_port_range` tab-separated while sysctl.d files write them space-separated, and the two were compared as raw strings. Measured on a RHEL 9.8 host, `1024 65535` in the file against `1024<TAB>65535` at runtime was reported as an override, written into `99-inspectah-migrated.conf`, and the source file was then left out of the image, taking every other setting in it along. Values are now compared token by token, so only a real difference in value counts. The values stored in the snapshot are unchanged.
- **Unmanaged file exclusions no longer hide sibling paths**: `--exclude-path` entries and the language environment roots found elsewhere in the scan were matched as bare string prefixes in both unmanaged walks, so `--exclude-path /opt/app` also hid `/opt/app-data` and `/opt/app.conf`, and a language environment at `/usr/lib/node_modules/foo` hid a sibling `foo-cli`. Those files silently dropped out of the scan output, and the `/opt` ones out of the report and the image as well. An entry now covers the path itself and everything beneath it, and nothing else. A trailing slash on the entry is still accepted.
- **A pre-loaded base image now works on a disconnected host**: the scan ran `podman pull` on the baseline image every time, and podman contacts the registry on every pull even when the image is already stored. A host with no registry access that had the base image from `podman load` therefore stopped at exit 3 before any inspector ran, which is the air-gap workflow the docs describe. The scan now checks local storage first and uses the stored image without pulling, printing `Using local image` in place of the `Pulled` line, and the line before extraction now reads `Preparing baseline image` instead of `Pulling`. A stored image is not refreshed, so pull it again or remove it with `podman rmi` to scan against a newer build.
- **`--scan-home all` no longer scans the whole filesystem**: it selected every user with UID 1000 or higher, which on RHEL includes `nobody` (UID 65534, home `/`), so `/` became a scan root and every non-RPM walk ran over the entire host. `all` now selects UIDs 1000 through 59999, the range the users inspector already reports as regular users, and a home of `/` or an empty home is never added as a scan root, even for a user named explicitly. Paths found under a root of `/` (from `--scan-path /`) or under a home with a trailing slash no longer print as `//home/...`.
- **Excluding a package no longer leaves its service in `systemctl enable`**: the unit stayed in the enable list with an advisory that no surface showed, so excluding `nginx` in refine produced a Containerfile that failed at `Failed to enable unit, unit nginx.service does not exist`, with no warning anywhere. A unit whose owning package is excluded is now left out of `systemctl enable`, `disable` and `mask`, and the Containerfile records it as `# Omitted: nginx.service (package 'nginx' was excluded)`, which the Omitted Services list in refine shows as well. This applies to an aggregate (fleet) export as well, where a package left out by the prevalence default counts as excluded. A package the baseline is known to ship keeps its unit, and so does a package that something you kept depends on, since dnf installs it anyway: that covers a dependency refine starts excluded and one you exclude yourself. The dependency is read from the scan's leaf data, which records the dependencies of leaf packages only; when the scan has no leaf data, or one leaf package depends on another, an excluded package's unit is still left out, and you enable it by hand if dnf pulls the package in.
- **A package from a repo disabled on the source host installs from that repo**: a package installed from a repo the host keeps at `enabled=0`, such as `openh264` from `epel-cisco-openh264`, was rendered both in the plain `dnf install` list and in the repo-less MANUAL block, and the build stopped at `Unable to find a match: openh264`. When the export carries that repo's `.repo` file, and any `gpgkey=file://` key its section names, the package now installs once, in a transaction with `--enablerepo=<repo id>` (plus the CodeReady Builder flags when those are carried), and is left out of the repo-less block and the manual follow-up. Packages from several disabled repos share that one transaction, which enables all of their repos, and the cached and uploaded RPMs installed from file join it, so a package in any of them can depend on a package in another; no other transaction enables those repos. When the `.repo` file is not carried, because it was excluded in refine or never captured, the package appears only in the repo-less block. Such packages are selected the way the plain list selects, with one difference: a package that something you kept depends on is named in its repo's transaction whatever its own state, because the transaction of the package that needs it does not enable that repo. A dependency refine starts excluded, or one you exclude, is therefore still installed, and one whose dependents are all excluded is not named. Otherwise a package excluded in refine is left out entirely, so the scan tarball and an unchanged refine export install the same set. Group membership does not hold one back, because the group install does not enable its repo. This transaction runs after the repo-enabling packages and before the `dnf group install` and the main `dnf install` list, either of which can depend on it, so the group install now also comes after the repo-enabling packages. While a package installs from its repo, neither export carries its cached RPM under `repoless-packages/` any more, and an RPM you upload for it in refine is used: the package then installs from the upload instead of from the repo. When its repo is not carried and no RPM is, a `/var` directory the package owns is now provisioned rather than assumed to come with the package. No package the scan marked repo-less is added to the plain `dnf install` list any more.
- **An npm project missing a manifest no longer breaks the build**: a project with a `package-lock.json` was marked high confidence whether or not its `package.json` was there or could be read, and high confidence renders an active `COPY` of both files followed by `npm ci`. A lockfile with no `package.json` beside it therefore produced a `COPY` of a file the export never wrote, and `podman build` stopped there; `npm ci` would have refused the single file in any case. A project is now high confidence only when both manifests were read, and the Containerfile checks that both are held whatever confidence the snapshot records, so a snapshot written by an earlier version renders safely too. Anything less renders a commented block for you to finish, and its manifests are no longer staged under `language-packages/npm/`, since no active `COPY` would carry them. That block no longer offers commented `COPY` lines for files the export does not carry: it names any manifest that was not captured and says to put both in the project directory before running `npm ci`.
- **Refine no longer pre-excludes a package whose disabled repo ships in the export**: refine starts every repo-less package excluded, so that a package of unknown origin needs a deliberate include. A package from a repo the host keeps disabled was caught by the same rule, even though its `.repo` file, with a source, is right there in the export and the scan tarball installs it from that repo with `--enablerepo`. Such a package now keeps its ordinary default in refine, so opening refine and exporting without changes installs it the same way the scan tarball does. `@commandline` packages, packages whose repo file is missing or excluded, and packages from repos that no longer exist still start excluded. The package still has no row of its own in the refine package list.
- **A repo-enabling package whose repo the export dropped accepts the upload its FIXME asks for**: when the repo that serves a package such as `epel-release` is not carried by the export, the Containerfile tells you to re-include the repo or upload the RPM, but refine refused the upload, because it checked the package against the scan rather than against your current choices. The upload is now accepted, the package installs from it, and the FIXME goes away. If you re-include the repo afterwards, the package installs from the repo again and the upload is left out of the export.
- **Repo-less RPMs install before the packages and services that need them**: the block that installs a cached or uploaded RPM with `dnf localinstall` was placed with the non-RPM software, after `systemctl enable` and after the main `dnf install` list. A unit owned by such a package was enabled before the package that ships it was installed, and a package that depends on it was installed before it, so either step could fail the build. Each such RPM is now copied into the image and installed from that file by the same `dnf install` that installs the packages from repos disabled on the source host, so dnf resolves the two together in either direction. That step runs after the repo-enabling packages and before `dnf group install`, the main `dnf install` list and every service action, and it carries the CodeReady Builder flags when those are carried. The MANUAL and FIXME notes for repo-less packages the export cannot install follow it under `Repo-less RPM packages`.

### Security
- **The refine export keeps every file it extracts from the scan tarball inside the export directory**: the export copies included unmanaged files out of the source scan tarball, and both the file list and the tarball entries come from that tarball, so a crafted one could write anywhere the refine user can. Three routes reached outside the export directory: a path containing `..`, a symlink entry followed by a regular file at the same path (the file was written through the link), and a symlink entry at a directory followed by a file beneath it. The export now fails on an entry path with a `..`, root or `.` component, on a second entry at a path it already wrote, on any existing symlink or non-directory where it needs a directory, and on an existing file at the destination. An unmanaged hardlink entry, which used to become a symlink, now fails the export, as do device and FIFO entries. Unmanaged symlinks themselves are kept as they are, with their absolute targets, and the export never writes through one. The same path check now applies to fleet aggregate variant paths written under `aggregate/variants/`, where a `..` or a leading `//` also escaped the export directory: such a snapshot is refused as untrusted.
- **A line starting with `//` or `;` is no longer skipped by redaction in files where neither is comment syntax**: redaction leaves comment lines alone, and it treated a line as a comment if it began with `#`, `//` or `;`, in every file. In `fstab`, `sshd_config`, PAM files, audit rules and most `.conf` files only `#` starts a comment, so a line such as `//srv/share/password=hunter2` kept its secret in the export and produced no finding in `secrets-review.md`. `#` still marks a comment everywhere. `;` now does so only in `.ini`, `.cnf` and `.repo` files, in `smb.conf`, and in systemd's own files: units, quadlets and networkd files under a `systemd` directory, `.conf` drop-ins in their `*.d/` directories, and daemon configuration such as `/etc/systemd/journald.conf`. `//` does so only in `named.conf`. Those are the files whose parsers read them as comments; a `.conf` elsewhere is not assumed to.
- **The scan tarball's cached repo-less RPMs pass the RPM filename allowlist before they are written**: `inspectah scan` copied each cached repo-less RPM into `repoless-packages/` under a filename built from the package's name, version, release and arch as the host's rpmdb reported them, with no check. A package name such as `../evil` put the copy outside that directory, and the scan usually runs as root. The Containerfile already refused such a name and left a FIXME in place of its `COPY`; the writer now runs the same allowlist, does not stage the file, and prints `Warning: cached RPM for <name.arch> not bundled` so the missing RPM is not a silent drop. Upload the RPM in refine to install that package.
- **npm global package names are quoted on the `RUN npm install -g` line**: the names and versions on that line come from each global package's `package.json` on the scanned host and were written into the `RUN` unquoted. A blacklist of `;`, `$`, a backtick, `|` and `&` was the only guard, so `>`, `<`, `(`, `)`, `*`, `{`, `}`, `#`, a space or a tab still reached the build: a package named `pm2>/etc/passwd` truncated a file inside the build container. Each token is now single-quoted, as in `RUN npm install -g -- 'pm2@5.3.0' 'typescript'`, on the active line and on the commented one alike, and a name or version holding a single quote or a newline is still skipped with a warning.
- **An npm global package name can no longer pass npm an option**: quoting keeps a name out of the shell, but npm still read a token that starts with `-` as one of its own options, so a global package named `--prefix=/tmp/elsewhere` on the scanned host moved where the build installed every package on that line. The package list on the `RUN npm install -g` line, active or commented, now follows `--`, which ends npm's option parsing, so each token is read as a package.
- **Names taken from the snapshot can no longer write outside the export**: several export steps joined a name from the snapshot onto a directory in the export: a language-package manifest file name, a user name in `users/home/<name>/.ssh/authorized_keys`, and the names of timer and quadlet units. An absolute name replaced the directory and a name with `..` left it, so a crafted snapshot could overwrite a file such as another user's `authorized_keys` wherever the user running the scan or refine could write. Each of these names must now be a single plain file or directory name. One that is not is skipped and a warning is printed, and for a user's SSH keys the Containerfile carries a `# FIXME` in place of the `COPY`.

## [0.9.0-beta.5] - 2026-11-05

### Changed
- **The refine export is assembled in one place** — `RefineSession::export_tarball` is removed and replaced by `RefineSession::export_plan`, which returns an `ExportPlan` the caller renders with `ExportPlan::render`. `POST /api/tarball` built its own copy of the same argument list, so the route and the session could disagree about what an export is without any test being able to see it: a change that made the route discard the operator's original include map passed the entire test suite. There is now one assembly and one list to get wrong. The download is byte-for-byte what it was; this removes a way for the two paths to drift, it does not change what either produces today. Library callers using `export_tarball` should call `session.export_plan(generation)?.render(&path)`.

### Fixed
- **The unmanaged `/usr` walk no longer reports the kernel and libgcc** — the rpmdb records some packages under the pre-usrmerge paths (`kernel-core` under `/lib/modules`, libgcc under `/lib64`), while the walk sees `/usr/lib/modules` and `/usr/lib64`. Ownership was a plain string match, so every file of those packages read as unmanaged: on a measured RHEL 10 host the kernel module tree alone was 108.9 MB of the 121.9 MB the section reported, and it advised copying the kernel into the image. A path owned under `/lib`, `/lib64`, `/bin` or `/sbin` now also counts as owned under `/usr`. A host without usrmerge is unaffected.
- **Unmanaged files under `/srv` are no longer copied into the image**: on fedora-bootc, centos-bootc and rhel-bootc `/srv` is a symlink to `var/srv`, so the Containerfile's `COPY` wrote that content into `/var/srv`, where it was unpacked once on first boot and never changed by a later image update. An included `/srv` finding now gets a comment block in the Containerfile and a note in the audit report instead, naming the paths and saying to carry them with the `/var` strategy: restore them into `/var/srv` after deployment, or give them their own mount. The files are still bundled under `unmanaged/srv/`. `/opt` and `/usr/local` findings are unaffected.
- **The unmanaged `/usr` walk honors the same exclusions as the unmanaged files walk** — it filtered only by RPM ownership, so `/usr/local`, which the unmanaged files walk already covers, was reported a second time. The same file got two `COPY` lines and two include toggles that did not know about each other, and a symlink there (an npm global such as `/usr/local/bin/ng`) was positioned to fail the second bundler with `File exists`. Paths given with `--exclude-path` and the language environments already counted elsewhere were also reported again under `/usr`. The `/usr` walk now skips every unmanaged files scan root under `/usr`, taken from the same list that walk uses, and applies both exclusions with the same prefix match.

## [0.9.0-beta.4] - 2026-11-04

### Changed
- **File context rules are replayed as real commands** — custom `semanage fcontext` rules were collected as raw table rows, and every one of them failed the renderer's shell safety check, so the generated build carried a FIXME in place of each of the first ten rules, nothing at all for any beyond that, and a commented-out `restorecon -Rv /` suggestion at the end. They are now collected as typed records and emitted as `semanage fcontext` commands that are valid against the target's policy. Order follows the source host's policy store when `file_contexts.local` and `file_contexts.subs` can be read, and the sorted output of `semanage fcontext -l -C` otherwise, with a note in the build saying which. Each label and unlabel rule removes its own key from the target's local store, then adds it, or modifies it when the target policy already defines the key; a rule whose path the target already maps as an equivalence is skipped at build time with a FIXME, because deleting its key would delete that equivalence. An equivalence rule instead reads the target's own mapping for its subject: added when absent, left alone when identical, and the build stops with an instruction when the target maps it elsewhere. There is no build-time `restorecon`, because a bootc target labels the deployed tree from the image's own policy. `/srv` is a symlink to `/var/srv` on the RHEL bootc base and the policy carries no alias for it, so a `/var/srv` equivalence is added once, ahead of the first `/srv` rule that needs it, to make the rules reach the real path.
- **A repo-enabling package installs from the repo that served it** — a `-release` package installed outside any repo rendered twice, once as an active install and once as a manual block, and the build could resolve neither reliably. Such a package is now recognised only when the host's enabled repos actually served it, the serving repo ids travel in the snapshot, and the export keeps the active install only while one of those repo definitions is still carried, enabled, and keyed, and every repo file and GPG key the package itself provides is still included. When no proof survives, the package is demoted to the upload path with an instruction naming the repo. A CodeReady Builder section enabled on the source host is carried as `--enablerepo` on the four `dnf` transactions that resolve against it: the repo-enabling bootstrap, the package list, the group install, and the module-stream enable.
- **Repo and group controls act on their members** — a repo toggle was decided by repo-exclusion history and a group toggle by a check that read every declared member present in the package list and answered "already satisfied" for a group with no member present at all, so include-all on a repo whose history said enabled returned success and recorded nothing even with a member individually excluded, and a group exclude did the same whenever the check came back empty. Both now read the effective member set, the packages the operation would actually change, and an operation that would change nothing is refused with a message saying so. The repo switch reads the same counts and posts the direction it derives from them, in the same release, so no control it offers can post an op the server refuses. Its accessible name carries the repo id ahead of those counts, so two repos whose counts agree are still told apart by a screen reader navigating control to control.
- **`/etc` content is written from the Configuration Files decisions alone** — the export rewrote the kernel snippet directories after honouring those decisions, with no include check, so excluding a file under Configuration Files did not keep it out of the image, and RPM-owned unmodified `modprobe.d` files shipped in every build. There is now one source for `/etc` content. PAM files, audit rules, and custom tuned profiles become ordinary Configuration Files entries with their own include decision and their own rows in the reports.
- **A `/var` directory is backed only by a tmpfiles entry that creates it** — backing was decided by an unanchored recursive grep that walked up to `/var`, so a stock `d /var/lib` line marked every directory under it as backed and the unbacked-directory advisory never fired. Only a `d`, `D`, `v`, `q`, or `Q` entry at exactly that path, a unit `StateDirectory=`/`CacheDirectory=`/`LogsDirectory=`, or RPM ownership counts. The tmpfiles half of that is an exact-path lookup. The unit half is still a recursive text search with no line anchor and no boundary after the directory name, so a commented-out directive or a longer directory name under the same prefix satisfies it, and a vendor unit is matched even where an `/etc` override carrying no directive takes precedence over it. The export then checks whether that creator reaches the image and provisions the directory when it does not, or prints an instruction when it cannot tell. A unit named by two files, an `/etc` override and an RPM-owned vendor copy, is resolved by full path: excluding the override leaves the vendor unit as the creator rather than counting the directory as lost and adding a duplicate entry under whatever ownership the export guessed.
- **Uploaded RPMs keep their real filename and are proven before export** — an RPM whose filename did not match the host's exact version was matched, marked, and then silently left out of the build, so the build failed on a missing `COPY` source. Uploads are now staged under their own name through one allowlist, streamed to disk in bounded chunks rather than buffered whole, placed and recorded under one lock, and verified as a regular file inside the staging directory before the export copies them. A staged file that cannot be verified demotes its package with an instruction instead of failing the export.
- **The `/var` summary's outcome column uses the words the count above it uses** — a directory the export has to make itself was listed as "provisioned by this export" under a summary reading "1 unbacked", so the two halves of the same row carried opposite valence and shared no word the reader could match them on. "carried" was also the only one of the three still written in the code's own vocabulary, and it is the value that means "nothing to do here". Each outcome now leads with the word it is counted under: "backed: creator carried by this export", "unbacked: provisioned by this export", and "backing unconfirmed", which already matched.
- **Scan output drops the em dash** — a non-success inspector line read `FAIL — podman not found` and the subscription certificate warnings ended `— rebuild soon` and `— will not work on unregistered systems`. The reason now follows a colon (`FAIL: podman not found`, `skip: disabled`) and each subscription warning ends in its own sentence, matching the wording the generated README already used.

- **A `/var` directory is carried only when a unit that declares it reaches the image** — the collector grepped the unit directories to find which file declared a `StateDirectory=`, `CacheDirectory=` or `LogsDirectory=`, then kept the filename and threw the path away. Three things went with it: which of the two copies of that filename declared the directory, whether a second file declared it as well, and anything the search never looked at. The export then resolved the filename against RPM ownership, so a vendor package shipping a unit of that name counted as proof the directory gets created, whether or not that unit declares anything. Measured on a RHEL 9.8 host: an administrator's full shadow of `crond.service` adding `StateDirectory=crondstate`, excluded in the refine UI while `cronie` stays included, produced an export that called `/var/lib/crondstate` carried, wrote no tmpfiles entry for it and said nothing in the audit report. `cronie`'s own unit declares no directory at all, so the image would have had neither the creator nor the directory. The backing now records the full path of every file carrying the directive, and the export asks whether one of those files reaches the image: an included `/etc` unit or a carried vendor unit means carried, and an excluded shadow that is the only declarer means the export provisions the directory and names the creator it lost. The search also now covers `.mount` and `.swap` units beside `.service` and `.socket`, which is the full set `systemd.exec(5)` applies to, so a mount unit declaring a directory is no longer invisible.

### Removed
- **The experimental terminal UI** — `inspectah refine --tui` is gone, along with the `inspectah-tui` crate. `--tui` is no longer a recognised flag. The web UI is the only refine interface.
- **Dead `/var` fields and phantom kernel decisions** — `size_estimate` and `recommendation` were written by nothing and rendered as placeholder text on every `/var` row. Every loaded kernel module carried an include decision no surface read and no renderer honoured; they are inventory now. `non_default_modules` and `repo_providing_packages` were never populated.

### Security
- **RPM filenames are allowlisted before they reach a `COPY`, a `RUN`, or the filesystem** — an uploaded or cached RPM filename reaches four places in the generated build: a `COPY` source, a shell-form `RUN dnf localinstall`, the `rm` operand that follows it, and the commented copy of all three that a pre-excluded package carries. None of them was checked. The cached-RPM path built the filename from four snapshot fields and interpolated it straight into the `RUN`, so a scanned host reporting a package named `evil$(id)` produced a build that ran `id` as root, and the manual-resolution block put an unescaped package name in a comment, where a newline ended the comment and made the rest of the name an instruction. The upload handler had no allowlist either: it stripped the directory components off the name it was given and accepted whatever was left, so long as it ended in `.rpm`. There is now one ASCII allowlist, and one type that can only be built by passing it. The upload handler runs it and answers 400, the export runs it and demotes the package to a FIXME with a warning, the cached-RPM renderer and the source-tarball extraction run it and emit a FIXME instead of a `COPY`, and the upload-directory rescan on session resume runs it and leaves an unrecognised file unattached. A name the allowlist refuses is never sanitised into one it accepts. The export writes each uploaded package's file from the handle it verified rather than from a recorded path, and it is the only writer of that path.
- **Every host-sourced field of a `/var` provisioning line is allowlisted before it reaches a `RUN` or a tmpfiles.d entry** — the second instance of the same class, and the entry above does not cover it. A `/var` directory the export has to provision is named in a shell-form `RUN mkdir -p` when the scan captured no mode for it and in a tmpfiles.d `d` entry when it did, and nothing checked or quoted any operand of either. A snapshot carrying the path `/var/lib/app;id` produced a build that ran `id` as root. tmpfiles.d records are newline-separated and their fields whitespace-separated, so a newline in the path, the mode, the owner, the group, or the named creator each ended the record and started a second directive for systemd to apply at boot; the creator also reaches a Containerfile comment, where a newline ends the comment and promotes the rest to an instruction. The strongest of those needed no crafted tarball: the collector parsed `stat -c "%04a %u %g %U %G"` by splitting on the first four spaces, so the group name took the entire remainder of the output. That parse now requires exactly five whitespace-separated fields. The path and the three fields beside it are each proved by an ASCII allowlist carried in a type with no other constructor, and what a refusal costs differs by field: an account name degrades to the numeric id, which provisions the same ownership, while a refused path or mode has no stand-in that is still the host's and the artifact carries an instruction to create the directory by hand in place of the build step. The creator is escaped rather than refused, because it only labels a comment the operator is being asked to act on.

- **Rotate any credential a scanned host carried in a language-package manifest, a compose file or a quadlet unit, in every export taken with v0.8.7-beta.1 through v0.9.0-beta.3, and re-scan afterwards** — `inspectah scan` wrote every collected manifest into `inspection-snapshot.json` with no scrubbing of any kind. Upgrading does not close that: a credential already sitting in a tarball someone shared, in a fleet aggregate, or in a pushed image layer stays exposed, so rotation is the remedy here rather than a rebuild. All five formats were affected equally, `requirements.txt`, `package.json`, `package-lock.json`, `Gemfile` and `Gemfile.lock`, because the snapshot object never reached the scrubber at all: the redaction pass visited `.env` file content and git remote URLs and never `manifest_files`, and the renderer then serialized the snapshot whole. The sidecar under `language-packages/` was scrubbed correctly in the same export, two directories down from the copy that was not, and a project detected as `npm manifest`, a `package.json` with no lockfile beside it, gets no sidecar at all, so for those the unscrubbed snapshot was the only copy in the export. Three things kept it quiet. The scrub emitted no finding, so `secrets-review.md` said nothing about the credential and the README's `Secrets redacted` count did not include it, which on a host whose only secret was that credential meant the row was absent and the export looked clean. The snapshot was still eligible to be stamped `FullyRedacted`, because a manifest contributed nothing to weigh against it, and `FullyRedacted` is the one state the refine import path trusts to skip re-redaction, so a mislabelled snapshot stayed unscrubbed through a round trip. And the snapshot JSON is the artifact that goes in the tarball and that `aggregate` ingests across a fleet. The scrub now runs in the redaction pass, which owns the snapshot object that both export paths serialize, so neither path can be fixed without the other, and a manifest it scrubs emits a high-confidence finding, so the count the README prints and the state the import path trusts both describe what happened. The regression test asserts that no file anywhere under the export directory carries the fixture credential, rather than asserting about one named file. That qualification no longer needs making, because the line scanners it referred to are rewritten here too. **Eight span defects across six hand-rolled scrubbers are replaced by one primitive.** Every one was the same mistake: the scrubber found the right line and then took the wrong span, because it located the separator with a bare search from the start of the line instead of from the end of the key it had already matched. A compact `"publishConfig": { "registry": … }` gave up its credential because the first colon on the line belonged to a different member; `--index-url=URL` and `-i URL` are both valid requirements-file syntax and neither was recognised; a Gemfile `source` line with a quoted word in a trailing comment matched the comment's quotes; a `Gemfile.lock` line carrying two credentialed URLs kept the second; a compose `DATABASE_URL` whose value contained `?sslmode=require` split inside the query string. There is now one `value_span_after_key`, told which key the caller matched, searching forward from it, with four named syntaxes for the four forms its callers parse. **And a scrubber that cannot locate its value now says so.** That is the more important half. Previously a scrubber that failed to parse a line returned it unchanged and emitted nothing, so the snapshot stayed `FullyRedacted` over a credential nothing had examined and `secrets-review.md` was empty — measured on four separate inputs. A line routed to a scrubber by a key match whose value cannot then be located is recorded as an unresolved finding, which downgrades the snapshot to `PartiallyRedacted` and names the file and line for review. The finding deliberately carries no line text, because the premise is that the line may hold a credential nothing stripped and these findings are serialized into the export. A credential-free manifest comes back byte-identical, including its trailing newline, so the signal means what it says. **Three further fields had the same gap and are fixed with it.** A compose file's raw text and a `.timer` unit's text each reached `inspection-snapshot.json` the same way. A quadlet unit's text was worse than any of them: nothing scrubbed it anywhere, the quadlet collector raises no hint to hold the state down, and the renderer emits `COPY quadlet/ /etc/containers/systemd/`, so an `Environment=DB_PASSWORD=` line was copied into the built image at a path systemd reads at boot. Measured before and after inside a built image rather than only in the export. A `.timer`'s `Description=`, parsed into its own field, went the same way into the snapshot and the audit report. All four are now scrubbed where every other content field is, and two tests hold the class shut: adding a field to any content-bearing snapshot type fails to compile until it is classified, and a sweep plants a distinct secret in every one of them and asserts that exactly the fields declared exempt survive the export.

### Breaking
- **Snapshot schema 25** — removed fields, changed types, a new `shadowed_by` value, and a `/var` directory backing that records declaring paths rather than a unit filename make schema 22, 23 and 24 snapshots unloadable. `MIN_SCHEMA` rises with `SCHEMA_VERSION`. For 23 that is because the pass assigning a service-owned config file to a single owner runs at collect time, so loading an older snapshot would re-render from the state the pass exists to remove. For 24 it is more literal: the `unit` field is gone, so the payload does not deserialize, and the path it stood for is a collect-time observation no load-time migration could recover. Re-scan; aggregates re-aggregate from re-scanned hosts.

### Fixed
- **A redacted export's JSON manifests are still valid JSON** — the auth scrubber rebuilt each rewritten line with quotes that were already in the slices it was joining, so every value it touched came out as `""value""`. It ran on every `"resolved"` line of a `package-lock.json` and every `"registry"` line of a `package.json`, with or without credentials in them, so a redacted export's lockfile was unparseable throughout: 139 doubled quotes in the first one measured on a real host. `npm ci` reports that as a missing or too-old lockfile, which points nowhere near the export, and the build died at that step. Both export paths were affected once the manifest writer became shared, so a redacted CLI scan tarball corrupted its lockfile the same way the refine web download did. Unredacted scans were never affected. The regression test asserts the staged file parses as JSON; the suite that shipped this defect asserted only that the credential was gone and the host survived in the URL, both of which were true of the corrupt output.
- **A systemd drop-in has one include decision, not two** — a `*.service.d/*.conf` drop-in and an admin-written unit file in `/etc/systemd/system/` are recorded by two collectors, into `config.files` and into `services.drop_ins`, and the operator got a separate checkbox in each section. Only one of them governed whether the file reached the image. The export subtracts every service-owned path from the generic `/etc` tree without asking what the Configuration Files row said, while the Service Drop-ins writer skips excluded entries, so a file left checked under Configuration Files and unchecked under Service Drop-ins was written to neither tree and reached no image, with the checkbox still checked and no warning anywhere in the export. The reverse pair shipped a file the operator had unchecked. The two rows live in sidebar sections that are never on screen together and the search index did not cover the drop-in list, so nothing would prompt a comparison. The Configuration Files row for a service-owned path is now inventory, naming Service Drop-ins as its owner, and the server refuses a toggle on it from the web and CLI alike. The Service Drop-ins row is the one decision, and the export obeys it.
- **A shadowed Configuration Files row says where its control went** — making the row inventory stopped the two-checkbox contradiction, but in the web UI it left a row whose checkbox silently disappeared behind a bare "Inventory" badge, with nothing naming the control that replaced it. The badge is now the owner's name, "Controlled in Service Drop-ins" or "Merged into sysctl settings", beside the owner's current state and a link that jumps to the owning row with it focused. The join is the path string the shadow was computed from: `service_dropins[].path` for a service-owned file, `sysctls[].source` for one whose keys were absorbed. The Services section grew a group for drop-ins whose unit has no state change of its own, because a unit sitting at its preset state never reaches `state_changes` and its drop-in therefore rendered nowhere at all, which is the common case rather than an edge one. Global search now indexes the drop-in list by path and unit, so searching the path of a shadowed file reaches the row that decides it.
- **A service drop-in can be switched back on** — the Service Drop-ins toggle only moved one way. Including a drop-in was allowed only when its parent unit appeared in the services section as an included state change, but that section records a unit only when its enable state diverges from its preset; a unit sitting at its preset state is listed separately and has no entry there at all. For that case, which is the ordinary one, the include was refused permanently while the exclude was unguarded, so an operator who unchecked a drop-in lost the file for the rest of the session with no way back short of discarding the session. The Configuration Files row over the same path became inventory in this release, which left this the only control over the file. The refusal now tests for an actual exclusion of the parent rather than for the absence of a decision about it. Excluding a parent service still cascades onto its drop-ins and still blocks re-including one of them on its own, because that would undo the operator's parent decision for a single file. Measured on a RHEL 9.8 host and in the built image: a drop-in whose parent carries no `systemctl enable` line is correct output, not a hazard, because the image's own preset enables the unit exactly as the source host had it.
- **An admin-written unit file in `/etc/systemd/system` reaches the configuration inventory** — the walk's alias-symlink exclusion never asked whether the path was a symlink. It stripped `/etc/systemd/system/` and dropped anything with no `/` left, which is a nesting-depth test: drop-ins under `*.service.d/` survived it and whole unit files did not. An operator's full shadow of a vendor unit, and any wholly new unit written straight into that directory, were discarded from `config.files` for having a short path, while the alias symlinks the rule was named for were excluded only by coincidence. The check now tests the file type. A symlink there is systemd's own state, an alias from `systemctl enable` or a mask link to `/dev/null`, and both are already recorded by the services inspector; a regular file is admin-written content and is kept. Measured on a RHEL 9.8 host carrying synthetic drift: five unit files entered the inventory that a scan of the same host had been dropping, the two masked units stayed out, and the exported tree did not change, because a path the services section already carries is still written from there alone.

- **A failed RPM upload no longer reports success** — the refine web UI branched on the response status alone and never read the server's `{"error": "…"}` body, and the upload modal caught the resulting error and discarded it. Every refusal left the green "`foo.rpm` matches nginx" checkmark on screen with the Upload button live again, so an operator whose upload was rejected for its filename, its size, or a staging conflict saw nothing at all go wrong. Both upload routes now surface the server's reason: the modal replaces the checkmark with the refusal text and offers no retry, and the status code rides along in the message because two of the reasons ("internal error", and axum's "failed to read stream" for a body over the 500 MiB route limit) say nothing on their own. The DELETE route had the same defect and the same fix, so removing a package whose RPM came from the host's dnf cache now reports why instead of logging "Conflict".
- **Reference sections no longer lag behind the session's own decisions** — two surfaces answered from a projection of the session's original snapshot, built once and cached for the server's lifetime. Once a decision was made, the cached surfaces kept reporting the original include state: a Configuration Files exclusion dropped the file from the export while its Kernel and Boot mirror kept reporting it included, and the Storage view reported a /var directory as carried while the export wrote a provisioning line for it. A resumed session's first read reported an exclusion it had just replayed as still included, since the cache read the pristine snapshot rather than the replayed state. Both caches are gone: reference sections now project from the live session on each request.
- **Both export paths now carry every directory their own Containerfile copies** — the third and fourth instances of the same class as the tmpfiles entry below, and they are mirror images: each export path emitted a `COPY` the other path satisfied. The refine web download deleted `quadlet/` from the archive after the config tree had written it, on the stated grounds that the Containerfile referenced quadlets through `config/` paths, which it never did; so every download taken from a host running quadlets failed `podman build` at `COPY quadlet/ /etc/containers/systemd/`, in all four combinations of the operator's config and container toggles, with no export warning. In the other direction, the pip/npm/gem manifest writer lived in the refine crate alone, so the CLI scan tarball emitted `COPY language-packages/...` for a directory it never wrote and failed there instead. The manifest writer now sits beside the renderer that emits those `COPY` lines and both paths call it, and `quadlet/` is part of the refine export. A test over both paths asserts the general invariant: every `COPY` source the renderer emits exists in the export tree rendered alongside it.
- **The refine web download's Containerfile no longer references a tmpfiles fragment the archive doesn't contain** — the tarball downloaded from the refine UI emitted `COPY config/usr/lib/tmpfiles.d/inspectah-var.conf` in its Containerfile without writing that file into the archive, so `podman build` failed on a missing COPY source with nothing said beforehand. The CLI's own render path wrote the member correctly; only the web download's export path skipped it. The mismatch predates this phase, but it previously needed a scan verdict of an unbacked /var directory, which no decision in the UI could produce. This phase made it reachable by decision: excluding a tmpfiles.d fragment under Configuration Files now provisions the directory and emits the COPY line, so an ordinary UI action could produce a download that fails to build.
- **An unreadable directory under `/etc` marks the scan partial**: the main `/etc` walk recorded a permission failure on a subdirectory and then dropped it unless the whole walk came back empty, so on any real host the config inspector reported complete while that subtree's files were missing from the export. The failure now degrades the config inspector, and the scan reports partial. A host with any unreadable `/etc` subdirectory, for example `/etc/sssd`, now gets a partial scan where it used to get a complete one. This change shipped in 0.9.0-beta.4; the entry was added after that release.

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

[Unreleased]: https://github.com/marrusl/inspectah/compare/v0.9.0-beta.5...HEAD
[0.9.0-beta.5]: https://github.com/marrusl/inspectah/compare/v0.9.0-beta.4...v0.9.0-beta.5
[0.9.0-beta.4]: https://github.com/marrusl/inspectah/compare/v0.9.0-beta.3...v0.9.0-beta.4
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
