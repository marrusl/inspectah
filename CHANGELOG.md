# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed
- **Breaking: snapshot schema is now 30**: `refine` and `aggregate` load only schema 30. Re-scan each host and re-aggregate. The scan now records what each added package needs. `selinux.boolean_overrides` now lists the booleans set persistently, each `{"name", "value"}`. New fields `selinux.disabled_modules` and `selinux.boolean_conflicts` hold the modules disabled with `semodule -d` and, in an aggregate, the booleans hosts set differently.
- **Packages that own an enabled unit are carried by default**: a package not user-installed on the source host that owns a unit enabled there, such as `firewalld` or `cronie`, is now installed by the scan tarball and refine exports. One that needs a disabled repo, a repo you exclude in refine, a package no enabled repo serves, or possibly a package the host lacked, is listed as Not carried with the reason, as is an owner you exclude.
- **Debug builds report `commit dev, built dev`**: `inspectah --version` and `inspectah version` from a debug build, such as plain `cargo build`, no longer show the git commit and build date, so the CLI is not recompiled after every commit. Version metadata in release builds and packaged binaries is unchanged.

### Fixed
- **Each architecture of a package keeps its own source repo**: on a multilib host, `libfoo.x86_64` installed from a local RPM no longer takes the repo of `libfoo.i686` from AppStream, so the export no longer installs it, or a package that needs it, by name from a repo that does not serve it. A package whose repo cannot be told apart from another architecture's is treated as having no recorded repo.
- **A unit command no longer fails the build when dnf skips its package**: when the image may or may not get a unit's package, such as one only another package needs or recommends, the Containerfile runs that unit's `systemctl` command, or the `enable` of a timer that starts it, only if the unit files are in the image. The audit report lists these commands. Before, they could fail the build or be left out.
- **Not carried no longer lists a package the image may still install**: one of the alternatives a package accepts, or one a conditional dependency needs once the export leaves another package out, now stays in the image by default with its unit command. Before, they were listed as Not carried and lost their `systemctl` commands.
- **Including a package in refine carries what it needs**: a package you include in the Packages view now takes the packages it needs out of the Not carried group, and the Containerfile keeps their recorded `systemctl` commands. On a fleet this includes a package only some hosts have. Before, dnf installed those packages while they stayed listed and lost their unit commands.
- **A fleet export names enabled-unit owners omitted by its package merge**: the fleet Containerfile's `# Not carried:` block, the audit report's `### Not carried` table, the refine Export dialog and `inspectah aggregate` now name a package that owns a unit enabled on any host and that the merge left out, such as `firewalld`. The Containerfile no longer enables that package's unit, which failed the build. Before, the fleet dropped such packages without a word.
- **SELinux booleans set with `setsebool -P` are carried**: the scan kept only booleans whose running value differed from the stored one, which is every runtime-only change and none of the persistent ones, and the Containerfile never set them. It now reads the persistent settings and sets each with `RUN setsebool -P`.
- **SELinux modules disabled with `semodule -d` are reported**: the scan ignored the policy store's record of disabled modules, so a base module you turned off was not mentioned and a disabled custom module was listed for installation as an active one. The Containerfile now names the disabled modules in a `FIXME` with a commented-out `semodule -d` line, and lists a disabled custom module only there. A custom or disabled module whose name `semodule` would refuse, such as one holding shell syntax, is named quoted in a `FIXME` of its own and is in no command line.
- **A fleet boolean set differently on different hosts is no longer set twice**: an aggregate where one host set a boolean on and another off rendered both `setsebool -P` commands, so host order chose the image's value. The Containerfile now names the boolean and the hosts on each side in a `FIXME` and sets neither.
- **A relocated SELinux policy store is found when `semanage.conf` quotes it or sets it twice**: `store-root = "/etc/selinux"` was read with its quotes, and the first of two assignments won where libsemanage uses the last, so the scan could read a stale store or none.
- **SELinux scan warnings no longer copy `booleans.local` lines or the policy store's path**: a line the scan could not read was quoted in the warning, and a warning about the store named its path, which comes from `semanage.conf` and `/etc/selinux/config` and keeps any text written after it on the line. Warnings are not redacted. They now give the line number and the reason, name the store as the one those files select, and give an error by its kind.
- **SELinux module discovery reports a directory it could not read in full**, instead of listing the modules it did read as if they were all, and an administrator's module named `extra_varrun` or `extra_binsbin` at a priority other than 400 is reported. A boolean whose name is not an SELinux boolean name, such as `*`, gets a `FIXME` instead of a `setsebool` line.
- **Edits to `%ghost` config files under `/etc` are collected**: rpm never checks a `%ghost` file's content, so an admin's `/etc/sssd/sssd.conf` was lost from the image. The scan now lists ghost files on the host as modified configs, except files another section records, such as `/etc/fstab` and the `.repo` files the repository section reads. Host identity and credentials stay out: `/etc/machine-id`, SSH host keys, and key files such as `/etc/chrony.keys`, which a scan warning names.
- **A named pipe under `/etc` no longer hangs the configuration scan**: the configuration scan could open a named pipe it found under `/etc`, including one rpm reported only for its permissions, and opening a named pipe that nothing writes to waits forever. It now checks the file type before every read and skips named pipes, sockets and device files, and links to them. A file replaced while the scan runs is not covered.
- **SSSD configuration keeps mode 0600 in the image**: SSSD refuses to start on a `sssd.conf` or `conf.d` snippet that is not mode 0600, and `COPY` gives a file the build context's mode. When the export carries either, the Containerfile now sets 0600 after copying the configuration.
- **The authselect profile is carried as `authselect select`**: the Containerfile now selects the host's authselect profile and features with `RUN authselect select <profile> <features> --force`. A hand-edited PAM or nsswitch file authselect generated is listed as an advisory, since selecting the profile writes it again. A custom profile is carried whole, or the Containerfile names its missing files instead of selecting it.
- **`/etc/audit/audit.rules` is carried unless the audit units are as packaged**: RHEL supports loading this file with `auditctl -R` instead of building it with `augenrules`, and the scan dropped it, leaving the image with no audit rules. It is now left out only when `/etc/audit/rules.d` exists and a completed `rpm -Va` shows the audit loader units exactly as packaged, since the image builds it again. Anything else, including a failed `rpm -Va`, carries it with a warning saying why.
- **`/etc/default/grub` is reported, never copied**: the scan now shows the file as an advisory with its content, because a bootc image does not regenerate its boot loader configuration from it. Move kernel arguments the image needs to a `/usr/lib/bootc/kargs.d/*.toml` file.
- **Fleet view no longer jumps to an item you left**: after opening an item from the fleet banner or search, switching to Packages straight away and then returning to its section scrolled to, highlighted and focused that old item. Choosing a section yourself now cancels the pending jump.
- **Keys typed into refine's search boxes stay there after a section change**: pressing Ctrl+K or `/` right after opening a section, from the keyboard or the mobile navigation menu, could lose the query, because the section then moved focus into its own content and the rest of what you typed went there. A search box you have just focused now keeps focus.
- **Screen readers read refine's section navigation as one list**: the sidebar's sections, including those with an Include all and Exclude all menu, are now list items of a single navigation list, so assistive technology announces each entry and the number of entries correctly.
- **Custom SELinux policy modules are detected again**: the scan read modules from a path RHEL 9 and 10 do not use, so modules installed with `semodule -i` were never reported. It now reads the policy store and the Containerfile names each module, with the `semodule` commands to carry it.
- **The `/srv` note gives the symlink target as `/var/srv`**: the Containerfile comment and the audit report note for unmanaged files under `/srv` said `/srv` is a symlink to `var/srv`, the raw relative link text, which reads as a path missing its leading slash. Both now name the absolute target, `/var/srv`.

### Security
- **SSSD bind passwords are redacted**: `ldap_default_authtok` and other `*_authtok` values in collected configuration are now redacted whole, spaces and quotes included. Before, an SSSD LDAP bind password in `sssd.conf` would have shipped in the export.
- **Paths through a linked directory are checked under both names**: with a symbolic link in a directory above it, such as `/etc/app/state` linking to `/etc`, a path could reach a key file, `/etc/shadow` or `/etc/machine-id` under a name no exclusion matched. Every exclusion now applies to both names, and a warning names any packaged path left out. Other configuration, packaged or not, is carried where the application reads it; a link to a directory above it is followed once.
- **Configuration warnings no longer quote file content**: the crypto policy warnings quoted the first line of `/etc/crypto-policies/config`, so a link from there to a credentials or key file put a secret in the scan and both exports, and a failed `authselect check` was quoted although it can repeat `authselect.conf`. Warnings are not redacted. They now give paths and fixed text only, and the policy file is read only as collected configuration is.
- **Encrypted private keys are redacted**: a PEM block labeled `ENCRYPTED PRIVATE KEY`, or any other label ending in `PRIVATE KEY`, inside a collected configuration file is now redacted like the RSA, EC and OpenSSH key blocks were.

## [0.9.0-beta.6] - 2026-11-20

### Added
- **The refine Export dialog shows the export's warnings**: when an export has warnings, the dialog stays open as "Downloaded, with warnings" and lists each one, notices as information and anything you asked for that is missing as a warning. Before download, a collapsed notice names not-carried packages that own an enabled unit and links to the Not carried group. Before, an uploaded RPM the export could not verify was dropped without a word.
- **Refine lists the packages it does not carry and includes one with a click**: the Packages view has a "Not carried" group below the package list, packages that own an enabled unit first. Include on a row installs the package, enables its units again and stops provisioning its `/var` directories, and moves it to the package list, where you can exclude it again. Before, you had to know a package's name to carry it.
- **Packages nothing in the image needs are listed as Not carried**: a package not user-installed on the source host that no package the image installs depends on is no longer part of the image. The scan tarball leaves it out by default and lists it in the Containerfile (`# Not carried:`) and in the audit report's `### Not carried (N)` table. Dependencies of kept packages are still carried. Without leaf data, or on a fleet aggregate, nothing is excluded.
- **Snapshots record the `gpgkey=` path that named each GPG key**: a key a repo file names with dnf variables is recorded at its expanded path, and the new `gpgkey_references` field keeps the path as written, so the repo file can be matched to its key. Snapshots without the field still load.
- **Unit state for sockets, timers and other unit kinds**: the scan now records and carries enabled, disabled and masked state for sockets, paths, timers, targets and slices, and for package-owned mount, automount and swap units, not only services. A socket a package ships disabled and you enabled, such as `cockpit.socket`, is now enabled in the image. Before, the state of every non-service unit was lost.
- **Advisories for your own mount, automount and swap units**: a storage unit you wrote gets comment lines in the Containerfile's unit state section and never a `systemctl` line. Where they apply, they give its state, whether its file is copied, what its mount point means on a bootc host (`/usr`, `/var` links, `/opt`), and the command to add by hand for an enabled or masked unit. A missing `nofail` is noted when it can be determined.
- **The export README explains carried unit states**: when the Containerfile carries `enable`, `disable` or `mask` for a host unit, the README's new Unit state section explains that states survive first boot only while the base image's empty `/etc/machine-id` stays empty. Removing it or writing `uninitialized` to it lets first boot apply presets, which can undo `enable` and `disable` lines.
- **Refine shows each unit's kind and why a command is left out**: in single-host sessions, Services rows show the unit kind and, when the export writes no command, the reason in the Containerfile's `# Omitted:` or `# FIXME:` words. Mount, automount and swap units you wrote are listed with their advisories, and a withheld configuration file or drop-in shows why under its row. The fleet view does not show these yet.
- **A converted cron timer has an include toggle in refine**: in single-host sessions, each timer or `@reboot` service converted from cron has a toggle on its Scheduled Tasks row. Excluding it removes the generated timer, its service, their `COPY` and the `enable` from the export; excluding the cron job removes every unit generated from it, and the row then has no toggle. Before, excluding one was accepted and changed nothing.
- **Refine says why an unmanaged file under `/etc/systemd/system/` is not exported**: the Unmanaged Files row of a withheld file shows the reason under the row, in the Containerfile's words.
- **Hand-made unit links are reported with the line that recreates them**: a link under `/etc/systemd/system/` that the unit's `[Install]` section does not make, such as a hand-made `.wants`, `.requires` or `.upholds` link or an alias, no longer counts as the unit being enabled. The Containerfile gets a comment advisory, with the `systemctl add-wants` or `add-requires` line that recreates the link, and a link the scan cannot report gets a scan warning saying why.
- **The source host's default target is carried**: when `/etc/systemd/system/default.target` links to a target, the Containerfile sets it with `RUN systemctl set-default -- <target>` before the other unit commands. When the target's file is excluded or its package is not in the image, an `# Omitted:` line says so and nothing is set. A fleet aggregate sets the target most hosts use, only when every host sets it.
- **Refine shows unit links, `Also=` members and the default target**: in single-host sessions, the Services list shows each reported link and each unit with an unreliable `[Install]` as a row with no toggle, with the Containerfile's advisory. The default target is the first row, with its own toggle and the reason when the export leaves it out. The fleet view does not show these rows.
- **Enabled template instances are carried when their identified configuration is in the export**: the scan now records enabled, masked and preset-disabled instances such as `getty@tty2.service`. The Containerfile enables one only when each `/etc` file the scan identified as read by it is in the export: paths with `%i` or `%I`, and the `wg-quick@` and OpenVPN files. Other files it reads are not checked. Otherwise an `# Omitted:` line says why and what to do. See [Systemd unit state](docs/reference/output-artifacts.md#systemd-unit-state).

### Changed
- **Breaking: snapshot schema is now 28**: `refine` and `aggregate` load only schema 28, and older snapshots do not load. Re-scan each host and re-aggregate. The `services` section is now `units`.
- **Breaking: the refine API and saved sessions use `units`**: the API and unit operations now use unit names instead of service names. If an older saved session cannot load, `refine` keeps it; start over with `--fresh`.
- **Breaking: refine export warnings carry a severity**: the `x-export-warnings` header from `POST /api/tarball` is now a JSON array of `{"kind": ..., "text": ...}` objects, `kind` being `failure` (something you asked for is missing) or `notice`, instead of plain strings. A script that reads the header must expect objects.
- **`inspectah scan` names packages it leaves out that owned an enabled unit**: after writing the tarball, the scan prints one stderr line, for example `Notice: 5 packages that own a unit enabled on the source host are not carried: audit, audit-rules, cronie and 2 more.`, pointing at the Not carried table and Containerfile block. It also prints with `--quiet`, never on stdout, and not with `--inspect-only`.
- **The refine export notes packages it leaves out that owned an enabled unit**: `x-export-warnings` carries one `notice` with the count and the first three names, pointing at the Not carried table and block. Include the packages in refine if the image needs them.
- **The scan tarball copies RPM-owned config files you modified**: a config file of an installed package that `rpm -Va` reports as changed is now included by default, as in a refine export with no decisions, so the two agree. The audit report lists them under Modified RPM-Owned Files. Exclude any you do not want in refine.
- **A config file the scan could not read is reported instead of copied empty**: a file over 256 KiB, unreadable or not text used to be collected empty, and an export wrote an empty file over the image's copy. It is now a `content_not_captured` advisory with its reason in the audit report, HTML report and refine, and neither scan nor refine copies it. Add it to the image by hand if you need it.
- **`inspectah aggregate` no longer treats an unreadable config file as matching an empty copy**: a host that could not read a file is now its own `content_not_captured` entry, never the selected variant, and no copy of the file is included by default. The aggregate no longer writes an empty file over another host's copy, and a refine export of it no longer writes an empty `.content` file.
- **Refine is documented as a workstation tool**: copy the scan tarball to your workstation and run `inspectah refine` there. Running it on the scanned host with a port forward is possible but not recommended: refine has no per-session authentication, so other local users can reach its port. Forward to `127.0.0.1`, not `localhost`. `inspectah refine` no longer prints the `If accessing remotely: ssh -L ...` line.
- **A GPG key the base image already ships is not copied over it**: when a base image package owns the key and `rpm -V` reports it unmodified on the source host, the scan records it as excluded with that reason and no warning. The Containerfile shows `# Not copied:` and the audit report a `### GPG Keys Not Copied (N)` table. Keys only the host's packages own, such as EPEL's, and modified or unowned keys are still copied.
- **The Added Packages count excludes packages that are not carried**: N in the audit report's `### Added Packages (N)` drops by the size of the Not carried list, which names those packages instead.
- **A unit owned by a package that is not carried is omitted with that reason**: its `systemctl` lines are left out, and the Containerfile records `# Omitted: firewalld.service (package 'firewalld' is not carried: ...)`. A same-named package at another architecture that the image installs keeps its unit.
- **Dependencies of kept packages no longer get `/var` directories provisioned**: such packages now count as carried, so the Containerfile no longer creates their directories and the tmpfiles fragment no longer says their creator is missing.
- **Sysctl files are carried as the host had them**: each file under `/etc/sysctl.d/` and a modified `/etc/sysctl.conf` is now an ordinary Configuration Files entry, copied unchanged under its own name, so the image applies the same settings in the same order. inspectah no longer rewrites picked keys into a file of its own. Files in `/usr/lib/sysctl.d/` and `/run/sysctl.d/` are not copied.
- **Runtime sysctl drift is reported, not carried**: a key whose running value differs from what the files request is listed in the audit report, HTML report and refine, with the file and line that set it. The image gets the files, not the running values.
- **System Tuning shows sysctl as a read-only panel**: keys are no longer per-key decisions in refine; each `/etc` sysctl file is included or excluded by its Configuration Files row. In single-host sessions the panel shows each key's effective value, running value and source line, drift, and files not carried with the reason. Fleet sessions say the explanation is per host and keep each file's variants in Configuration Files.
- **The sysctl explanation follows systemd's precedence and states only what it knows**: files resolve as systemd-sysctl applies them, so a later line wins and a `-<key>` line excludes a key. The audit report's `#### Sysctl` table, formerly `#### Sysctl Overrides`, shows file and runtime values. When a file cannot be read or redaction changed a value, the reports state no values or drift for the host and name the file.
- **An RPM-owned config symlink re-pointed to `/dev/null` is carried as an empty file**: `rpm -Va` reports such a link only through its link flag, so the scan dropped it and the image lost the mask. It is now carried as an empty file of the same name, as for a masked `/etc/sysctl.d/99-sysctl.conf`. A link re-pointed to any other file is still not carried.
- **A full unit shadow shows on its drop-in row only**: when `/etc/systemd/system/` holds a replacement for a vendor unit file and the unit is otherwise at its preset, refine no longer adds a state row for it, and the export no longer writes a `systemctl enable` or `disable` for it. The shadow file is still copied; its include toggle is on the drop-in row.
- **The reports call the unit section "Unit State Changes"**: it was "Service State Changes". The HTML report's anchor is now `#units` instead of `#services`, its filter box reads "Filter Units", and the aggregate detail pane shows Kind "Unit" for unit rows.
- **Unit names are checked against systemd's naming rules**: a unit name must be 1 to 255 bytes of ASCII letters, digits and `:_.\@-` and end in a known unit type suffix. A snapshot holding another name does not load, a refine operation naming one is refused with the rule, and the scan warns about such a name and leaves it out.
- **Unit commands run after the files they name, in one section**: the Containerfile writes `enable`, `disable` and `mask` under `# === Systemd Unit State ===`, replacing `# === Service Enablement ===`, after the configuration and drop-in `COPY` lines. Each verb is one `RUN systemctl <verb> --` with one unit per line, ordered by kind and name.
- **A unit command the export cannot reproduce is left out with the reason**: the unit state section writes `# Omitted: <unit> (<reason>)` instead of a command that would fail or do something the host did not, such as enabling a local unit whose file is excluded, or masking a unit whose file is copied in at its name. Refine shows the same reason on the row. See [Systemd unit state](docs/reference/output-artifacts.md#systemd-unit-state).
- **Units the image needs are never switched off**: a `disable` or `mask` of `bootc-*`, `ostree-*` or `systemd-remount-fs.service`, and a `mask` of `tmp.mount`, are not carried. The Containerfile writes a `# FIXME:` saying the image needs the unit, and refine shows the same reason. An `enable` of any of them is carried.
- **Including a tuned profile no longer forces `tuned.service` on**: the kernel and boot section writes the profile and leaves the service to its own unit row, so a `disable` or `mask` the host carried is kept. Before, it always wrote `systemctl enable tuned.service`, which undid a carried `disable` and failed the build on a `mask`.
- **Package updaters that run in place are excluded by default**: `dnf-automatic*.timer` and `packagekit-offline-update.service` are recorded and excluded, with the reason that an image-based host is updated by a new image, and the Containerfile names each in an `# Omitted:` line. Include one in refine to carry it anyway. `packagekit-offline-update.service` is no longer locked as incompatible with image mode.
- **A local or shadowed unit's command follows its file**: a unit whose file you wrote or replaced in `/etc/systemd/system/` has its command carried when that file is copied. An enabled local unit gets `enable`, a disabled one no `disable`. A shadow is compared with its preset, except that a preset-enabled shadow adding `[Install]` links gets an `enable` after its `COPY`, and `Also=` differences can add a state row. A fleet aggregate omits it when hosts differ. See [Systemd unit state](docs/reference/output-artifacts.md#systemd-unit-state).
- **Units the scan cannot place get a warning instead of a command**: a unit whose file or `[Install]` drop-in is in another directory on systemd's search path (`/usr/local/lib/systemd/system/`, `/run/systemd/system/`, `/etc/systemd/system.control/` and so on), an alias, and a link to a unit file elsewhere each get a warning naming the unit and path, and no state change. A link to `/dev/null` or a zero-length file is a mask.
- **Manual follow-up lines give the advice as its own sentence**: the Containerfile's `# TODO:` line for a locally installed or repo-less package now ends the reason with a full stop, as in `# TODO: 'tool.x86_64' was installed locally (state: local_install). Provide a .rpm or custom repo.` A script that matches the old line needs the new text.
- **The scan progress line for units reads "Units"**: it read "Services", and its count now covers every unit kind.
- **A unit inspectah generates never replaces one collected from the host**: when a cron-converted timer or `@reboot` service, its paired service, or `flatpak-provision.service` shares a name with a collected unit, nothing generated is written or enabled. A `# FIXME:` names the colliding unit. Exclude it and its file in refine to carry the generated one; a unit with no row to exclude means writing the job's timer and service by hand under other names.
- **Generated units are enabled after their own `COPY`**: `flatpak-provision.service` is enabled in the container workloads section, and generated cron timers and `@reboot` services in the Scheduled Tasks section, only when the export writes the file. Refine shows a declined generated unit's reason on its row, in the Containerfile's words.
- **A scheduled timer row links to its unit row**: in refine, an existing timer's Scheduled Tasks row names its unit (`<name>.timer`) and links to that unit's row when it has one. The row has no include toggle.
- **The Containerfile says which local timers it carries**: the Scheduled Tasks comment now reads `# Local timers carried with their configuration or drop-in row (N): ...` for timers whose file the export writes, and `# Local timers not carried (M): <name> (its unit file is not in the export)` for the rest, instead of listing every local timer.
- **A local or shadowed timer's file has one control**: a timer whose file is in `/etc/systemd/system/` is copied by its Configuration Files row, or its Service Drop-ins row for a copy of a vendor timer, and by nothing else. Excluding that row leaves the file out, and the timer's `enable` or `disable` is left out with the reason.
- **Breaking: Scheduled Tasks rows have new ids in `/api/snapshot/sections`**: rows that share a name no longer share an id. A script that reads these ids must be updated.
- **Enablement is read from a unit's links, not from systemctl's word**: `systemctl list-unit-files` reports `enabled` for any link to a unit, whoever made it. The scan now reads the links the unit's effective `[Install]` section names, against the links under `/etc/systemd/system/` and those packages ship. A unit is enabled when every named link is present and one is under `/etc`. Links the scan cannot read degrade the scan, and affected units get no state.
- **An `[Install]` section the scan cannot rely on is advised, not carried**: a unit whose section holds an `UpheldBy=`, a specifier the scan does not expand, an unreadable file or a name it does not admit gets no `enable`, and its links are reported with that reason. When it also has an `[Install]` file under `/etc/systemd/system/` or assigns `Also=`, one advisory says the image's links can differ from the source host's, and no `disable` is written.
- **A unit at its preset whose `/etc` files change `[Install]` is carried or advised**: links a full shadow or `/etc` drop-in gave a preset-enabled unit were lost on the image without a word. When the files only add links, the unit now gets an `enable` after the `COPY`. When they drop or replace a link, each difference gets an advisory with the `add-wants`, `add-requires` or `rm` line to run by hand, and no command.
- **A command whose `Also=` members differ on the source host is an advisory**: `systemctl enable` and `disable` also act on every unit named in `Also=`. The command is carried only when every member's links on the source host match what it does and each member with a row is carried with the same verb; otherwise the Containerfile names every member and the lines to add by hand.
- **Refine's Services section is now Systemd units, grouped by kind**: in single-host sessions, rows sit in collapsible groups by kind, such as Services, Sockets and Timers. A Show bar filters them, and each row shows its state, preset, and an outcome label (FIXME, Advisory or Omitted) with one reason. Search and keyboard navigation reach rows across groups. An empty section reads "No unit differs from its preset, and no drop-in was found." The fleet view still says "Services".
- **Language-package commands quote a host path only when it needs it**: the pip, npm and gem lines of the Containerfile no longer single-quote a path made only of letters, digits and `@%+=:,./_-`, so `RUN cd '/opt/webapp' && npm ci --production` is now `RUN cd /opt/webapp && npm ci --production`. Any other path is still quoted. Bundler's `deployment 'true'` is now `deployment true`.

### Removed
- **Breaking: `99-inspectah-migrated.conf` is no longer written**: the scan tarball and the refine export no longer contain a `sysctl/` directory, and the Containerfile has no `COPY sysctl/` line. The host's own sysctl files carry the settings, at their source paths under `config/`, such as `config/etc/sysctl.d/` and `config/etc/sysctl.conf`. A script that reads the old path must read those.
- **Breaking: the scheduled timer toggle in refine is gone**: an existing timer's row had a toggle that applied nothing. A saved refine session that uses it does not load: start over with `--fresh`.

### Fixed
- **`inspectah aggregate` accepts hosts that have no hostname**: scans of unnamed hosts, such as DHCP-named cloud instances, were refused as `duplicate hostname: <unknown>`. Each now aggregates as its own host, listed as `<unnamed N, scanned <time>>`. The same snapshot given twice is still refused.
- **A scan names a host whose `/etc/hostname` is empty**: the snapshot carried no hostname, so the refine header was blank and the export was named `inspectah-inspectah-refined.tar.gz`. The scan now falls back to the host's transient name and records its source.
- **An SELinux port label excluded in refine stays out of the export**: the Containerfile wrote `RUN semanage port -a` for every port label found and installed `policycoreutils-python-utils` for it. An excluded port label now renders neither and is not counted in the block's comment.
- **The refine download installs package groups as the preview shows**: the downloaded Containerfile installed a renderable group's members one by one instead of using `dnf group install`. The download now matches the preview.
- **The source host's machine-id is never copied into the image**: rendered unit states rely on the image keeping the base image's empty `/etc/machine-id`. An imported snapshot could offer the file as a config file, drop-in, repo file or unmanaged file. Every such offer is now left out, the Containerfile names it in a `# FIXME:`, and refine shows why under the row.
- **The image no longer carries the source host's Insights and subscription identity**: the scan now leaves out the files insights-client and subscription-manager write at registration (`/etc/insights-client/machine-id`, `.registered`, `/etc/rhsm/facts/insights-client.facts`, `/etc/pki/product/` certificates), so deployed machines no longer claim the source host's registration. Admin-written configuration such as Insights `tags.yaml` is still carried.
- **A disabled local timer is no longer enabled in the image**: a local timer is now enabled only when the host had it enabled, and its file is copied only by its own row. Before, every timer in `/etc/systemd/system/` was enabled, whatever its state on the host.
- **A timer whose trigger the export does not carry is not enabled**: the scan now records each timer's trigger (its `Unit=`, or the same-named `.service`) and the drop-ins that decide it. The timer's `enable` is left out with an `# Omitted:` line when a local trigger or one of those drop-ins is not in the export and nothing else provides it, or `Unit=` cannot be resolved. `disable` and `mask` are unchanged.
- **An unmanaged file is never written under `/etc/systemd/system/`**: an imported snapshot could offer one that replaced a unit file already decided. Such files are no longer bundled, extracted or copied; the Containerfile names each in a `# FIXME:` saying a unit file belongs under Configuration Files.
- **Excluding a repo in refine drops its `.repo` file and GPG key**: excluding `epel` removed its packages but still copied `epel.repo` and EPEL's key, because disabled sections such as `[epel-debuginfo]` counted as still in use. A `.repo` file is now copied while one of its enabled sections is included, and a key while an included, enabled section names it. Including `epel` again brings both back.
- **An EL10 export that uses EPEL builds**: EPEL 10's `epel.repo` names its key with `$releasever_major`, which the scan read literally and skipped, so the build failed at `dnf install` with `Curl error (37)`. The scan now expands dnf variables in `gpgkey=` paths as dnf does on that host and copies the key. A key it still cannot collect is a warning naming the repo file and what to do.
- **The stock Red Hat release key is read as a key**: `RPM-GPG-KEY-redhat-release` opens with a paragraph of text before its armored header, so the scan recorded it as not a key on every RHEL host and printed a stderr line per `redhat.repo` section. A key file is now accepted when any line begins with the armored header; files holding a private key block are still refused.
- **A GPG key that several repo sections name is recorded once**: a RHEL host's snapshot held one copy of the stock Red Hat key per `redhat.repo` section (209 on RHEL 10.2). Each key path is now recorded once, with every `gpgkey=` path that named it.
- **The Containerfile no longer copies a repo directory the export did not write**: `COPY config/etc/yum.repos.d/` was emitted even when every included repo file had a path the export refuses, which failed the build. The copy and the `Custom Repositories (N)` count now follow the files actually written.
- **Language package paths with shell characters render again**: a pip, npm or gem project whose path held `$`, `;`, `&`, `|` or a backtick was skipped with `# WARNING: path contains unsafe characters` since v0.9.0-beta.2. Such a path now renders its install again. A path holding a single quote or a newline is still skipped.
- **The missing-`readelf` warning no longer points at a container image**: the scan runs on the host, so the warning no longer says to install binutils "in the inspectah container image". It now says that hand-installed compiled programs under the scan roots were not detected.
- **Package dependency lists are accurate**: a leaf package's dependencies now follow file requirements, weak dependencies and conditional (rich) dependencies, and no longer credit unrelated packages to each other (on RHEL 9.8, 25 of 26 leaf packages shared one 114-package list). Excluded packages that dnf still installs keep their `systemctl enable` line, and packages nothing installs lose it. Re-scan to refresh lists in older snapshots.
- **A multi-value sysctl set to its configured value no longer reports as drift**: tab-separated runtime values such as `net.ipv4.ip_local_port_range` were compared as raw strings against space-separated file values. Values are now compared token by token.
- **Path exclusions and provenance no longer hide sibling paths**: `--exclude-path /opt/app` also hid `/opt/app-data`, a service working in `/opt/app` flagged `/opt/application`, and `/var/lib/flatpak2` was dropped on ostree hosts. Each now matches only the path and what lies beneath it. A trailing slash is still accepted.
- **A pre-loaded base image now works on a disconnected host**: the scan ran `podman pull` every time, so a host without registry access that had the image from `podman load` stopped at exit 3. The scan now uses a stored image without pulling and prints `Using local image`. A stored image is not refreshed: pull it again or `podman rmi` it to scan against a newer build.
- **`--scan-home all` no longer scans the whole filesystem**: it selected every UID of 1000 or higher, including `nobody` (UID 65534, home `/`). It now selects UIDs 1000 through 59999, and a home of `/` or an empty home is never a scan root. A home or `--scan-path` with a `..` segment is refused.
- **Excluding a package no longer leaves its service in `systemctl enable`**: excluding `nginx` in refine produced a Containerfile that failed at `unit nginx.service does not exist`. A unit whose package is excluded is now omitted from `enable`, `disable` and `mask` with `# Omitted: nginx.service (package 'nginx' was excluded)`, in fleet exports too. A unit stays when the baseline ships its package or something you kept depends on it.
- **A package from a repo disabled on the source host installs from that repo**: a package from a repo kept at `enabled=0`, such as `openh264` from `epel-cisco-openh264`, was listed twice and the build failed. When the export carries the `.repo` file and its keys, the package now installs once with `--enablerepo=<repo id>`; otherwise it stays in the repo-less block.
- **An npm project missing a manifest no longer breaks the build**: a `package-lock.json` without a readable `package.json` rendered an active `COPY` of a file the export never wrote. A project is now high confidence only when both were read, and anything less renders a commented block telling you to put both in the project directory before `npm ci`.
- **Refine no longer pre-excludes a package whose disabled repo ships in the export**: a package from a repo the host keeps disabled, whose `.repo` file is in the export, now keeps its ordinary default, so an unchanged refine export installs it with `--enablerepo` as the scan tarball does. `@commandline` packages and packages whose repo file is missing or excluded still start excluded.
- **Host text in `README.md` and `audit-report.md` renders literally**: a host value such as a path or unit name holding `*`, `[`, `<`, a backtick, a pipe or a newline was written as Markdown and could add a link or HTML or break a table row. These values now render as code spans. The HTML report already escaped them.
- **A repo defined in two files is judged by the copy dnf uses**: refine's repo status and the audit's "Carried by Export" now follow the `.repo` file dnf reads first. A repo-enabling package that copy cannot serve gets a FIXME instead of a failing install.
- **Excluding a repo in refine no longer excludes its case variant**: dnf treats `site` and `SITE` as two repos, but excluding one also excluded the other's packages, `.repo` file and key. Each now follows its own toggle, and CodeReady Builder repos that differ only by case are both enabled.
- **A repo-enabling package whose repo the export dropped accepts the upload its FIXME asks for**: refine refused an RPM upload for a package such as `epel-release` when its repo was not carried, although the FIXME said to upload one. The upload is now accepted and the FIXME goes away; re-including the repo later leaves the upload out.
- **Repo-less RPMs install before the packages and services that need them**: the `dnf localinstall` block ran after `systemctl enable` and the main `dnf install`, so either step could fail the build. Each cached or uploaded RPM is now copied in and installed by the same `dnf install` as packages from repos disabled on the source host, before groups, the main list and every service action.
- **Removing an uploaded RPM says why it was refused**: when refine will not delete an uploaded RPM, such as one the snapshot records outside this session's upload directory, it returned a bare `500 internal error`. It now returns a `409` naming the file and the reason, never a host path. Full detail goes to refine's standard error with control characters escaped.
- **The web UI's development server no longer has its changes refused**: `npm run dev` forwarded requests with the page's own origin, so refine refused every change. It now presents refine's origin; other origins are still refused. The UI `inspectah refine` serves is unaffected.
- **Refine shows a failed user change on the user card**: when changing a user's Containerfile strategy or password choice failed, the card came back unchanged and the error went only to the browser console. The card now says which change failed and to try again, and screen readers announce it.
- **Closing the RPM upload dialog no longer abandons the upload**: closing it mid-upload did not stop the request, so a second upload could start and the first's result appear for another package. The dialog now cannot be closed until the upload finishes, and a double click sends one upload.
- **Refine keeps a saved session it cannot load**: `inspectah refine` used to delete an unreadable saved session and start fresh, printing only `Discarded previous session.`. It now stops with an error naming the file and why, leaves it in place, and says `--fresh` discards it. `--fresh` and the "Start fresh" answer still discard it.
- **An aggregate lists a unit once when its hosts disagree about its state**: a unit enabled on some hosts and disabled on others was in both the enabled and disabled lists. It now has one inventory entry with the state most hosts have.
- **A masked unit is no longer also recorded as an empty full shadow**: a mask in `/etc/systemd/system/` (a link to `/dev/null` or a zero-length file, also through links such as `/etc/alternatives`) was copied as an empty file, or read as an alias so the vendor unit came back in the image. A mask is now one masked state, carried only as `systemctl mask`.
- **`dbus-broker.service` is no longer dropped**: the scan skipped every unit named `dbus-*`. Only `dbus-org.` aliases are skipped now.
- **A unit file that is not valid UTF-8 no longer marks the units inspection incomplete**: the unit gets the advisory for an unreliable `[Install]` and no `enable` or `disable`, and a mask over it is still carried. A file under `/etc` is reported as content not captured on its Configuration Files row.
- **A file the export cannot write fails the export**: a staged file that could not be written was skipped with a stderr warning while the Containerfile still copied it. Any failed write is now an error naming the file. Two files at one path with different content are withheld with a `# FIXME:`. On case-insensitive filesystems such as macOS, two included paths differing only by case fail the export naming both: exclude one.
- **What refers to a staged file follows the export's decision**: a generated or local unit file the export withholds is neither copied nor enabled, where the build used to fail at a `COPY` or an enable naming a file the export did not write. A `/var` directory whose tmpfiles.d fragment or unit file is withheld is provisioned, and the GPG Keys heading and firewall zone comment count only staged files.
- **Excluding a unit's `[Install]` drop-in leaves its command out**: excluding the drop-in in refine kept the unit's `enable`, which then read an `[Install]` the source host did not have. The export now leaves the command out with an `# Omitted:` line when such a drop-in is excluded or the scan did not record it.
- **`.upholds` links are no longer copied as configuration files**: the configuration walk followed links in `*.upholds/` and the export copied a plain file where the link was. They are now skipped and reported as unit links.
- **Section search works in the Systemd units section**: pressing `/` there opened a search that never appeared. It now filters rows by what they show, including a link's relation and a mount unit's mount point.
- **A locked unit row names the reason instead of a slug**: the badge and checkbox label of `dnf-makecache.service`, `dnf-makecache.timer` and `packagekit.service` read `service-image-mode-incompatible`. They now read "Incompatible with image mode".
- **The Containerfile panel footer in single-host refine covers every decision**: it said the preview reflects package and config decisions only. It now reads "Preview reflects every include and exclude decision you make in refine."
- **Redaction replaces only the secret, not its key**: `proxy_password = hunter2` becomes `proxy_password = REDACTED_PASSWORD_1`, so the stock `rhsm.conf` still parses. An empty value is left alone. A quoted or XML value can still lose its closing quote or tag. A JDBC URL carrying a credential keeps only its scheme.
- **A database URL without a password no longer pairs with a later line**: a PostgreSQL, MongoDB, Redis or JDBC URL could match across line breaks, so `postgres://db/app` followed later by `contact: ops@example.com` redacted ` ops` as a password. These patterns now match within one line.
- **`secrets-review.md` gives the line each secret is on**: the Line column was one too high for a secret whose match did not start its line, such as an indented `password = ...`.
- **`secrets-review.md` names the tokens redaction writes**: it said `[REDACTED-*]`, which appears nowhere in the export. It now gives the real form, `REDACTED_<KIND>_<N>`, and the markers used for URL credentials.
- **A malformed SELinux port label renders a FIXME instead of a command**: a port label `semanage port` does not take, such as port `70000`, rendered a `RUN semanage port` line the build could not run. It now gets a `# FIXME` and no longer installs the SELinux policy utilities on its own.
- **A demoted repo-enabling package gets one piece of advice it can follow**: after you excluded a repo and re-included a package such as `epel-release`, the Containerfile said to include its `.repo` file under Configuration Files, which has no row for it, or to re-include a repo when that could not work, and a `# MANUAL:` block contradicted both. The `# FIXME:` now offers the RPM upload, and the repo to re-include, with its packages, only when that serves the package.
- **The tuned profile FIXME names only a row refine can include**: when the active custom profile's `tuned.conf` was left out on an advisory or locked Configuration Files row, the `# FIXME:` said to include it there, which refine refuses. It now says to activate a stock profile, and offers the row only when refine can include it.
- **A refine export from a stale tab says so**: when another tab or window had changed the session since the Export dialog opened, clicking Export closed the dialog with no download and no message. The dialog now stays open with its Stale state alert and the current generation; click Export again to download the current state.
- **Refine's Version Changes panel opens**: the panel below the Packages list was held shut, so the packages whose host version differs from the base image's, downgrades included, could not be seen. It now opens and closes when clicked.
- **Typing in the refine Packages search keeps the whole word**: each search change moved focus to the first matching row, so after the first letter the rest of the word went to that row and `buildah` filtered on `b`. Focus now stays in the search box; press the down arrow to move to the first shown row.
- **Row shortcuts work on refine package and container rows**: the `?` overlay lists `j`, `k`, `g`, `G` and the arrow keys for moving between rows and `x` or Space for toggling, but package rows ignored all of them and container rows ignored the moves. Both now take them as Configuration Files rows do. A package row has no detail to open, so `Enter` does nothing there; on a group row it opens the group.
- **Modifier keys no longer act on refine Configuration Files rows**: with a row focused, `x` or Space with Ctrl, Cmd or Alt held changed its decision, and Ctrl+K moved focus to another row as well as opening global search. The row now ignores `x` and Space with those keys held, and Ctrl+K only opens global search.

### Security
- **The refine export keeps every file it extracts inside the export directory**: a crafted scan tarball could write anywhere the refine user can, through a `..` path, a symlink entry followed by a file at the same path, or a file beneath a symlinked directory. The export now fails on such entries, on hardlink, device and FIFO entries and on duplicate paths, and applies the same check to fleet aggregate variant paths, refusing that snapshot as untrusted.
- **A line starting with `//` or `;` is no longer skipped by redaction where neither is comment syntax**: in `fstab`, `sshd_config`, PAM files and most `.conf` files only `#` starts a comment, so `//srv/share/password=hunter2` kept its secret in the export with no finding in `secrets-review.md`. `;` now marks a comment only in `.ini`, `.cnf`, `.repo`, `smb.conf` and systemd files, and `//` only in `named.conf`.
- **The scan tarball's cached repo-less RPMs pass the RPM filename allowlist before they are written**: a package name such as `../evil` put the copy outside `repoless-packages/`, and the scan usually runs as root. Such a file is now not staged and the scan prints `Warning: cached RPM for <name.arch> not bundled`. Upload the RPM in refine to install that package.
- **npm global package names are quoted and cannot pass npm an option**: names and versions on the `RUN npm install -g` line came from the scanned host's `package.json` files with only a partial blacklist, so a package named `pm2>/etc/passwd` truncated a file in the build container. Any token with other characters is now single-quoted, the list follows `--`, and a name or version holding a single quote or newline is skipped with a warning.
- **Names taken from the snapshot can no longer write outside the export**: a language-package manifest name, a user name in `users/home/<name>/.ssh/authorized_keys`, and timer and quadlet unit names were joined onto export directories, so a crafted snapshot could overwrite files such as another user's `authorized_keys`. Each must now be a single plain name. Otherwise it is skipped with a warning, and for SSH keys the Containerfile carries a `# FIXME` instead of the `COPY`.
- **Refine enforces its archive limits on the size it actually extracts, and bounds decompression**: a PAX record could make an entry declared as 0 bytes unpack to gigabytes, and a small compressible tarball with one huge long-name or PAX record could exhaust memory. Refine now counts the extractor's size and stops after about 590 MiB decompressed. `inspection-snapshot.json` must be a regular file in the archive, not a symlink to `/dev/zero` or a FIFO.
- **Snapshot values can no longer end a comment line or start a Containerfile instruction**: package names, paths and scheduled-task names from the snapshot went into `#` comments in the Containerfile and other generated files, so a newline ended the comment and the rest ran as an instruction. Line breaks and control characters are now escaped. A pip, npm or gem project path or npm global package name holding a line break is skipped with a `# WARNING:`.
- **User fields are checked before they reach a command**: a home such as `/home/alice; id > /tmp/x #` or a hash containing a quote ran a second command in the build. User and group names must be valid account names, the shell and home plain absolute paths, and the hash a crypt(3) string. A failing user gets a `# FIXME` in place of its `useradd`, password and SSH key lines; a bad hash costs only that password.
- **Snapshot values can no longer add an instruction through a `COPY`, `FROM` or `dnf group install` line**: an unmanaged path with a newline, a target image reference, or a group name with `$(...)` reached those lines unchecked. Each is now validated, and a refused value gets a `# FIXME` naming it. A config file, repo file, key, zone or drop-in whose path holds a line break, control character or `..` is not written to the export.
- **Snapshot values can no longer add a directive to the flatpak provisioning unit**: the unit that installs flatpak apps on first boot, which runs as root, wrote remote names, URLs, app ids and branches as they were. Names, ids and branches may now hold only letters, digits, `.`, `_` and `-`, and a URL may not hold whitespace, `%`, `$`, quotes, a backslash or `;`. A refused remote or app gets a `# FIXME` in the unit.
- **Snapshot values can no longer add a kickstart directive, a blueprint key or a command at install time**: `/etc/hosts` additions, DHCP connection names, and user names, groups, homes, shells, hashes and keys were written into the kickstart suggestion and users blueprint unchecked. Hosts lines are now single-quoted with `printf`, blueprint strings are escaped as TOML requires, and users are checked as in the Containerfile, so both carry the same users. A refused value gets a `# FIXME`.
- **A name from the snapshot can no longer be read as an option or a comment on a `RUN` line**: package, module, stream, version-lock, SELinux boolean and port label values could start with `-` or `#`, such as `--nogpgcheck`. Each is now refused with a `# FIXME` naming it, and a package refused for any reason now gets one. Units are written after `--`, and a GPG key path must be absolute.
- **Refine rejects requests not addressed to a loopback name**: a web page open in your browser can no longer read the refine API, including preserved password hashes, through DNS rebinding. Refine answers 403 to any request whose `Host` is not `127.0.0.1`, `localhost` or `[::1]`, so SSH forwards to another local port still work.
- **Credentials in podman output shown during a scan are redacted**: the baseline pull output redacted only the first bearer token, URL `user:password@` and `Authorization:` value, and never a URL's query string or a registry's `token` fields. Every occurrence is now replaced with `[REDACTED]`.
- **An AWS access key id after a comment line, or right after another key id, is redacted**: a key id on the line below a `#` comment, or the second of two key ids one character apart, kept its value in the export with no finding in `secrets-review.md`.
- **Quoted credential values are redacted whole**: `"password": "x"` in JSON was not redacted, and `password: "two words"` kept everything after the first space. The whole quoted value is now replaced, so it loses its quotes and a JSON file no longer parses.

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

[Unreleased]: https://github.com/marrusl/inspectah/compare/v0.9.0-beta.6...HEAD
[0.9.0-beta.6]: https://github.com/marrusl/inspectah/compare/v0.9.0-beta.5...v0.9.0-beta.6
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
