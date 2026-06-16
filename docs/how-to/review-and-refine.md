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

- A scan output tarball (`.tar.gz`) from a previous `inspectah scan` run

## Choose your interface

inspectah refine offers two interfaces: a **web UI** (default) and a
**terminal UI**. Both provide the same refinement capabilities -- section
navigation, item toggling, search, export, and session persistence.

### Web UI (default)

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

### Terminal UI

For environments without a browser (SSH sessions, headless servers, or
personal preference), use the terminal UI:

```bash
inspectah refine --tui ./scan-output.tar.gz
```

The TUI renders directly in your terminal using a keyboard-driven
interface. Press `?` to see available keybindings. The TUI shares the
same session persistence as the web UI -- switching between them
preserves your progress.

## Navigate the dashboard

The landing page shows system metadata (hostname, OS version, system type)
and a summary of findings across all sections. The dashboard organizes
findings into these sections:

| Section | What it contains |
|---------|-----------------|
| Packages | RPM packages found on the host |
| Config Files | Modified configuration files in `/etc` |
| Repositories | Enabled RPM repositories |
| Users & Groups | Local user and group accounts |
| Services | systemd service units and their state |
| Quadlets | Podman quadlet container definitions |
| Flatpaks | Flatpak applications |
| Sysctl | Kernel tunable overrides |
| Tuned | Performance tuning profiles |

Not every section appears in every scan. Sections only show up when
the scan found relevant data on the host.

The stats bar at the top summarizes each section's item counts
(total, included, excluded) and shows how many items still need review.

## Understand triage indicators

Each item gets a triage classification that tells you what it means for
your migration:

**Baseline** -- The item matches the base image. It ships with the OS and
does not need to appear in your Containerfile. Baseline items are excluded
from the Containerfile by default.

**Site** -- The item was added or configured by your organization. It belongs
in the Containerfile to reproduce your environment. Site items are included
by default.

**Investigate** -- The item's provenance is unclear or it has an unusual
characteristic that needs human judgment. Examples include locally installed
packages (no repository), packages with unknown source repositories,
version downgrades, and items at security-sensitive paths.

Each triage tag also carries a reason explaining why it was classified that
way (for example, "Matches base image package" or "Locally installed RPM").

## Include and exclude items

Toggle any item between included and excluded to control whether it appears
in the generated Containerfile. The UI reflects these changes immediately
in the Containerfile preview pane.

Including an item adds it to the Containerfile. Excluding it removes it.
You can override the automatic triage classification in either direction --
include a Baseline item you want to pin, or exclude a Site item you have
decided not to carry forward.

When you toggle a repository, all packages sourced from that repository
are affected as a batch.

### Locked items

Some items display as locked -- their include/exclude state is visible but
the toggle is disabled. Locked items carry a reason explaining why they
cannot be changed (e.g., "Baseline package" or "Required dependency").
These represent non-negotiable decisions where toggling would produce an
invalid Containerfile.

In the web UI, locked items appear with a lock icon and a reason badge.
In the TUI, locked items are marked with a lock indicator and cannot be
selected for toggling.

## Handle version changes

When the scan detects that a package version differs from the base image,
the version change is shown with its direction (upgrade or downgrade).
Upgrades are normal maintenance. Downgrades are flagged for investigation
since they may indicate a pinned version that needs explicit handling in
the Containerfile.

## Manage users and groups

User accounts discovered on the host have additional options beyond
include/exclude:

- **Strategy** -- Choose how to handle each user in the Containerfile
  (create via `useradd`, skip, or manage externally)
- **Password handling** -- Select the password approach for included
  users (lock, set a hash, or omit)

The user preview shows how each user will appear in the generated
Containerfile based on your selections.

## Review sensitive data

When the scan encounters security-sensitive content (paths that may contain
credentials, private keys, or other secrets), the session is flagged as
sensitive. Items at sensitive paths receive an Investigate classification
with a reason indicating the sensitivity.

Redaction state tracks what level of scrubbing was applied during the scan:

- **Fully redacted** -- All sensitive values replaced with placeholders
- **Partially redacted** -- Some sensitive values retained
- **Sensitive retained** -- Sensitive values present in the data

Review these items carefully before including them. The triage reason
explains why each path was flagged so you can decide whether to include
it, exclude it, or handle it through a separate secrets management approach.

## Export refined results

When you are satisfied with your selections, export the results. The
export produces a tarball containing:

- The refined Containerfile reflecting all your include/exclude decisions
- Updated scan data with your triage overrides applied

The exported Containerfile is byte-identical to what the preview pane shows
in the UI, so what you see is what you get.

## Work with aggregate data

When refining a aggregate scan (multiple hosts merged into one tarball), the
dashboard switches to aggregate mode. Instead of single-host triage buckets,
items are grouped by prevalence zones:

**Consensus** -- The item appears on all or nearly all hosts. Treat it
like a Baseline item in single-host mode.

**Near consensus** -- The item appears on most hosts but not all. Similar
to Site classification -- it likely belongs in the Containerfile but
verify whether the partial coverage is intentional.

**Divergent** -- The item differs significantly across hosts. These need
investigation to decide which variant to include.

The aggregate summary shows the total host count and highlights items with
variant conflicts -- cases where the same item exists in different versions
or configurations across hosts. Each variant shows its host count so you
can see how widespread each version is.

For items with multiple variants, you can:

- **Select a variant** -- Choose which host's version to use
- **Edit a variant** -- Modify the content directly
- **Discard a variant** -- Remove a variant from consideration

Repository source conflicts are also surfaced when a package is sourced
from different repositories on different hosts.

## Manage your session

### Undo and redo

Every change you make is tracked. Use undo and redo to step back and
forward through your edit history. The operation history shows a log
of all changes made in the current session.

### Resume a previous session

Session progress is saved automatically alongside the tarball in a file
named `.inspectah-session-<basename>.json` (for example,
`.inspectah-session-hostname-20260527-143000.json` for a tarball named
`hostname-20260527-143000.tar.gz`).

When you run `inspectah refine` on the same tarball again, it resumes
where you left off with all your previous include/exclude decisions intact.

To start fresh and discard saved progress:

```bash
inspectah refine --fresh scan-output.tar.gz
```

### Autosave

The session state is persisted automatically as you work. You do not need
to save manually. If you close the browser and reopen the refine server
on the same tarball, your work is preserved.
