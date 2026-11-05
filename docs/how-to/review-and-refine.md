---
title: Review and Refine Findings
parent: How-To Guides
nav_order: 1
---

# Review and Refine Findings

After scanning a host, use the refine UI to review findings, adjust
classifications, and produce a refined Containerfile ready for your
image build.

## Prerequisites

- A scan output tarball (`.tar.gz`) from a previous `inspectah scan` run,
  made by the same inspectah release you run `refine` with. `refine` loads
  a snapshot only when its schema version matches the binary's exactly. A
  tarball from a different release stops with an `unsupported schema version`
  error; re-scan with the current release.

## Launch the refine UI

```bash
inspectah refine ./scan-output.tar.gz
```

The server starts on port 8642 and opens your browser automatically.
To use a different port:

```bash
inspectah refine --port 9000 ./scan-output.tar.gz
```

To suppress the browser launch:

```bash
inspectah refine --open false ./scan-output.tar.gz
```

## Navigate the refine UI

The UI opens on Packages. The sidebar header shows the host name, OS name,
and OS version, and below it the sidebar lists eight groups in a fixed
order. A group with one section is a plain link. A group with several is
expandable, and its sections are listed beneath it.

| Group | Sections | What it contains |
|-------|----------|------------------|
| Packages | Packages | RPM packages, the Repositories bar, package groups, and a Version Changes panel |
| System Configuration | Configuration Files, Kernel & Boot, Security & Access Control | Modified files under `/etc`, kernel and boot settings including sysctl overrides and the tuned profile, and SELinux |
| Services & Scheduling | Services, Scheduled Tasks, Containers | systemd units and drop-ins, cron, at, and timer jobs, Quadlet units, and Flatpaks |
| Users & Identity | Users & Groups | Local non-system user accounts |
| Network | Network | Reference only |
| Storage | Storage | Reference only, including the `/var` directory summary |
| Software & Files | Non-RPM Software, Language Packages, Unmanaged Files | Software outside the package manager, pip, npm, and gem environments, and files from `/opt`, `/srv`, and `/usr/local` |
| Secrets & Subscription | Secrets, Subscription | Placeholders today: each says "Not yet implemented." |

Every section is always in the sidebar. A section with nothing to show says
so in the content area, for example "No service state changes detected."
Software & Files also tells you when the scan did not collect unmanaged
files and to re-run with `--include-unmanaged` to review them.

Each section link carries a count badge. The badge is blue on sections where
you make decisions and grey on reference sections, where you can read the
findings but not change them. Reference sections are included in the export
as they are.

The stats bar at the top shows a Packages counter and a Configs counter,
each as `N included / M excluded`. When a counter has rows with no
decision it adds `/ K advisory`; the count is left out at zero. `K` counts
the rows you cannot toggle, Advisory and Inventory alike. The stats bar
also holds Undo, Redo, Reset to defaults, and Export.

### Find items and move around

| Key | Action |
|-----|--------|
| `1` to `8` | Jump to the sidebar group in that position. In a group with several sections it opens the first one you decide on. |
| `/` | Filter the items in the current section |
| `Ctrl+K` | Search across sections |
| `Ctrl+Z`, `Ctrl+Shift+Z` | Undo, redo |
| `Ctrl+E` | Show or hide the Containerfile preview |
| `Ctrl+Shift+E` | Open the Export dialog |
| `?` | List the keyboard shortcuts |

`Cmd` works in place of `Ctrl`. The single-key shortcuts (`1` to `8`, `/`,
`?`) do nothing while focus is in a text field, and no shortcut acts while a
dialog is open.

## Understand triage indicators

Each item gets a triage classification that tells you what it means for
your migration:

**Baseline**: The item matches the base image. It ships with the OS and
does not need to appear in your Containerfile. Baseline items are excluded
from the Containerfile by default.

**Site**: The item was added or configured by your organization. It belongs
in the Containerfile to reproduce your environment. Site items are included
by default.

**Investigate**: The item's provenance is unclear or it has an unusual
characteristic that needs human judgment. Examples include locally installed
packages (no repository), packages with unknown source repositories,
version downgrades, and items at security-sensitive paths.

Each triage tag also carries a reason explaining why it was classified that
way (for example, "Matches base image package" or "Locally installed RPM").
An item that was excluded by default and is not locked shows a short label
for the reason: "Installer default", "Unclear provenance", "No repo source",
or "Dependency".

### Findings you can and cannot decide

The triage classification says how much attention an item needs. A second
property says whether you can decide it at all. See
[Triage Classification](../reference/triage-classification.md) for the full
model.

| Row shows | Meaning |
|-----------|---------|
| A checkbox | Actionable. You decide whether it goes into the image. |
| "Advisory: Modernization" or "Advisory: Cross-tree symlink", with a rationale beneath the row | Advisory. inspectah is reporting something, not asking for a decision. There is no checkbox. The finding is listed in the audit report, and its file is not copied into the image. |
| "Inventory", or a badge naming the row that owns the decision | Inventory. The row is shown for reference. There is no checkbox. |

A configuration file that a service drop-in or a sysctl override already
carries is Inventory, so the decision is made once. Its badge reads
"Controlled in Service Drop-ins" or "Merged into sysctl settings", with the
owning row's state beside it and a "Go to owner" link. A unit at its preset
state that has a drop-in is listed under "Other drop-in overrides" in
Services.

If a request to change an Advisory or Inventory row does reach the server,
the server refuses it with HTTP 422 and a message. The refine UI does not
offer the control in the first place.

## Include and exclude items

Toggle any actionable item between included and excluded to control whether
it appears in the generated Containerfile. The UI reflects these changes
immediately in the Containerfile preview pane.

Including an item adds it to the Containerfile. Excluding it removes it.
You can override the automatic triage classification in either direction:
include a Baseline item you want to pin, or exclude a Site item you have
decided not to carry forward.

### Repositories

The Repositories bar above the package list has one line per repository. Not
every repository has a switch, and a switch does not always reach every
package the repository serves. A repository gets a switch only when all of
these hold:

- It is not a distro repository.
- Its repo definition was found on the host and is enabled. The definition
  is either complete, or incomplete, in which case the line also reads "repo
  definition incomplete".
- At least one of its packages is actionable and not locked.

Every other repository shows state text in place of a switch:

| State text | Repository |
|------------|------------|
| "always included" | A distro repository |
| "disabled on host" | The repo definition is present but disabled |
| "repo id not found in any .repo file" | Packages name the repository, but no repo definition on the host does, usually because it was removed or renamed after install |
| "installed outside any repo" | A pseudo repository or an empty repo id. The `@commandline` repository, for packages installed from a local file, is titled "Local / Manual installs". |
| "nothing to decide: all members are locked" | The repository is eligible, but every one of its packages is locked or not actionable |

A switch acts only on the repository's actionable, unlocked packages.
Locked packages are never touched. The switch label counts those packages:
"N of M packages included", or "M packages excluded" when none is included.
When the Packages view hides some of them behind its display filters, the
label ends with ", K not listed". The switch is on only when every one of
those packages is included, so a repository with a partial selection shows
as off. Turning it off excludes all of them, and turning it on includes all
of them.

The server refuses a repository or package group operation that would
change nothing, with a message instead of a recorded change, for example
"nothing to change: all 8 effective members of 'epel' are already excluded".

After you exclude a repository, its packages are listed under "From excluded
repos" at the end of the Packages view. Each has its own checkbox, so you can
bring one package back without re-including the repository; the repository's
label then shows the partial count. A package with no serving repository has
an Upload button in place of the checkbox, here as in the main list. Use it
to supply the RPM, as described in [Handle Repo-less RPMs](repoless-rpms.md).

### Package groups

A package group installed with `dnf group install` appears as one row with
its package count. Expand it to see its members, or choose "ungroup" to
dissolve it into individual package rows. The row has no group-level switch;
you change a group through its members. A group whose members are each
locked or from the base image reads "nothing to decide: all members are
locked or from base", and a group action on it is refused.

### Locked items

Some items display as locked: their include/exclude state is visible but
the checkbox is disabled, and a grey badge gives the reason, or reads
"LOCKED" when there is none. Locked items are set before refine opens the
snapshot. They represent decisions where toggling would produce an invalid
Containerfile, for example host state such as `/etc/fstab` entries that
cannot move into an image.

### Change a whole section at once

Sections where you decide have an Actions menu beside the section name
(Include all, Exclude all). The menu acts on every actionable, unlocked item in
that section, and one undo reverses the whole change. Excluding every package
asks you to confirm first. There is no menu on Users & Identity, and none on
the reference sections.

To discard every include and exclude change, use Reset to defaults in the
stats bar. It is available once you have made a change, and asks you to
confirm.

## Handle version changes

When the scan detects that a package version differs from the base image,
the change is listed in the Version Changes panel at the bottom of the
Packages view. The panel is collapsed until you expand it. It lists the
package with its host version and target version, in two groups:
downgrades first, then upgrades. Upgrades are normal maintenance.
Downgrades are flagged for investigation since they may indicate a pinned
version that needs explicit handling in the Containerfile.

## Review language packages

Each pip, npm, or gem environment under Language Packages has an include
checkbox, a confidence label, and how it was detected. Environments that
contain C extensions or use the system site-packages directory carry a "C
extensions" or "system site-packages" badge.

npm global environments expand to list their packages. Each package has a
pin checkbox that fixes the Containerfile line at the detected version, and
each of these environments has a "Pin all" button, even when it holds a
single package. With a package row focused, `Enter` or `Space` toggles its
pin. A section search that matches a package expands its environment and
moves focus to the first match.

## Review /var directories

The Storage view lists the `/var` directories the scan found as one
collapsed summary row, titled with the directory count. The row's state text
counts the directories this export does not back with a creator: "N
unbacked", "M unconfirmed", or both ("N unbacked, M unconfirmed"), or "all
backed by tmpfiles.d, unit directives, or package ownership" when there are
none. Expand the row to see one line per
directory. Each line gives, in order:

- The path
- The mode and owner, or "ownership not recorded"
- What created it on the host: "tmpfiles.d" with the fragment path in
  parentheses, "StateDirectory=", "CacheDirectory=" or "LogsDirectory="
  with the declaring unit in parentheses, "package-owned" with the package
  in parentheses, or "nothing declares it"
- What the export does about it: "backed: creator carried by this export",
  "unbacked: provisioned by this export", or "backing unconfirmed"

Directories that need attention are listed first. Each unbacked or
unconfirmed directory also gets its own row under the summary, showing its
path and, where it applies, a state such as `backing unconfirmed`. A carried
directory whose mode or owner differs from the default gets an Inventory
row.

## Manage users and groups

User accounts discovered on the host have additional options beyond
include/exclude:

- **Containerfile strategy**: Choose how each user is handled: Skip, or
  create the user with `useradd`.
- **Password**: Choose "No password", "Keep existing password" (offered
  only when the scan preserved the user's hash), or "Set new password",
  which you type and confirm in the browser.

The Users & Groups view produces Kickstart and Blueprint TOML artifacts for
the output tarball, and the artifact preview shows how each user will appear
based on your selections.

## Review sensitive data

Redaction runs during the scan, before the tarball is written. It covers
file content in configuration, repo, drop-in, and unit files, the kernel
command line, container environment variables, Quadlet unit text, compose
files, cron, at, and timer commands and timer unit content, language-package
manifests, and sudoers and shadow entries. The full list is in the
[Redaction engine](../reference/inspector-coverage.md#redaction-engine)
reference.

The redaction state records how complete the scrubbing was:

- **Fully redacted**: Every finding was resolved by redaction, or there
  were none.
- **Partially redacted**: At least one finding is unresolved. A line that
  a scrubber matched by key but could not parse is reported this way, with
  its line number, instead of being passed as clean.
- **Sensitive retained**: You kept sensitive data on purpose with
  `--preserve` or `--no-redaction`.

Review unresolved lines by hand before you share the export.

Items at sensitive paths receive an Investigate classification with a reason
indicating the sensitivity. The triage reason explains why each path was
flagged so you can decide whether to include it, exclude it, or handle it
through a separate secrets management approach.

When the snapshot retains sensitive data, or you set a new password for a
user, the Export dialog says "Sensitive data detected" and asks you to
acknowledge it before it lets you export.

## Export refined results

When you are satisfied with your selections, choose Export, or press
`Ctrl+Shift+E`. The dialog reports how many packages and configuration files
are excluded. It reports advisory findings apart from exclusions: "Advisory
findings (N)" counts Advisory and Inventory rows together, and the note
that their files are not copied into the image holds for Advisory rows
only, since an Inventory row's file travels with the row that owns the
decision. The browser saves `inspectah-refine-output.tar.gz`. The archive
inside unpacks to `inspectah-<hostname>-refined/`, which contains:

- The refined Containerfile reflecting all your include/exclude decisions
- Updated scan data with your triage overrides applied
- The audit report

One difference exists today between the preview pane and the Export
dialog: package groups render as their individual members in the exported
Containerfile instead of a single `dnf group install` line. The image that
results installs the same packages either way; only the Containerfile's
wording differs.

## Work with aggregate data

When refining an aggregate scan (multiple hosts merged into one tarball), the
dashboard switches to aggregate mode. An aggregate tarball loads under the
same exact-match rule as a single-host one. Instead of single-host triage
buckets, items are grouped by prevalence zones:

**Consensus**: The item appears on all or nearly all hosts. Treat it
like a Baseline item in single-host mode.

**Near consensus**: The item appears on most hosts but not all. Similar
to Site classification: it likely belongs in the Containerfile but
verify whether the partial coverage is intentional.

**Divergent**: The item differs significantly across hosts. These need
investigation to decide which variant to include.

The aggregate summary shows the total host count and highlights items with
variant conflicts: cases where the same item exists in different versions
or configurations across hosts. Each variant shows its host count so you
can see how widespread each version is.

For items with multiple variants, you can:

- **Select a variant**: Choose which host's version to use
- **Edit a variant**: Modify the content directly
- **Discard a variant**: Remove a variant from consideration

Repository source conflicts are also surfaced when a package is sourced
from different repositories on different hosts.

## Manage your session

### Undo and redo

Every change you make is tracked. Use the Undo and Redo buttons in the stats
bar, or `Ctrl+Z` and `Ctrl+Shift+Z`, to step back and forward through your
edit history. A section-wide "Include all" or "Exclude all" counts as one
step.

### Resume a previous session

Session progress is saved automatically alongside the tarball in a file
named `.inspectah-session-<basename>.json` (for example,
`.inspectah-session-hostname-20260527-143000.json` for a tarball named
`hostname-20260527-143000.tar.gz`).

When you run `inspectah refine` on the same tarball again and a saved
session exists, it shows the number of saved operations and asks whether to
resume, start fresh, or quit. Resuming restores all your previous
include/exclude decisions.

To start fresh and discard saved progress, pass `--fresh`. If a saved
session exists, `refine` asks you to confirm before it discards it:

```bash
inspectah refine --fresh scan-output.tar.gz
```

### Autosave

The session state is persisted automatically as you work. You do not need
to save manually. If you close the browser and reopen the refine server
on the same tarball, your work is preserved.
