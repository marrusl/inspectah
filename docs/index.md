---
title: Home
layout: home
nav_order: 1
---

# inspectah

inspectah scans package-mode RHEL, CentOS Stream, and Fedora hosts and generates the artifacts you need to migrate them to image mode -- Containerfiles, package lists, config trees, and systemd service maps. It analyzes what is installed beyond the base image so you know exactly what to carry forward.

## Quick start

```bash
# Scan a host
sudo inspectah scan

# Refine the output interactively
inspectah refine <snapshot.tar.gz>
```

See [Getting Started](tutorials/) for a full walkthrough.

## CLI reference

The current CLI surface:

| Command | Description |
|---------|-------------|
| `inspectah scan` | Scan the current system and produce a migration snapshot |
| `inspectah refine` | Interactively refine scan output and re-render artifacts |
| `inspectah fleet init` | Initialize a fleet directory from host snapshots |
| `inspectah fleet aggregate` | Aggregate snapshots into a fleet-wide specification |
| `inspectah version` | Print version, commit, and build date |

See [CLI Reference](reference/) for the full flag tables.

## Documentation

{: .fs-5 }

**[Tutorials](tutorials/)** -- Step-by-step guides to get started with inspectah, from first scan to fleet aggregation.

**[How-To Guides](how-to/)** -- Task-oriented recipes for specific workflows like building a bootc image from inspectah output.

**[Reference](reference/)** -- Complete CLI flags, output formats, and schema documentation.

**[Explanation](explanation/)** -- Architecture, design decisions, and how baseline subtraction works under the hood.

**[Contributing](contributing-index/)** -- How to build from source, run tests, and submit changes.
