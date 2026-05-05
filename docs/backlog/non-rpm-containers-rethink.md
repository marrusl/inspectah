---
title: Non-RPM software + containers triage rethink
priority: P2
status: needs-brainstorm
created: 2026-05-04
---

# Non-RPM Software + Containers Triage Rethink

## Problem

The non-RPM and containers sections need investigation and design work. Current state is not well thought out.

## Known Issues

### False positive pip packages
System packages installed via RPM (dnf, setools, selinux, distro, etc.) are detected as "pip packages" because they have `.dist-info` directories. These are not operator-installed software — they're RPM contents that happen to have pip metadata. The inspector needs to cross-reference against the RPM database to filter these out.

### Unclear build strategy
When non-RPM items are "included," it's not clear what the Containerfile should do:
- **Binaries in /usr/local/bin:** COPY from captured filesystem? Or document for manual install?
- **Python venvs:** COPY the whole venv? Or `RUN pip install -r requirements.txt`?
- **npm apps:** COPY node_modules? Or `RUN npm install`?
- **Go binaries:** COPY the binary? Multi-stage build?
- Each strategy has different tradeoffs for image size, reproducibility, and maintenance.

### Quadlet units
4 quadlet units detected. When included, they COPY via config tree and systemd auto-starts them. But:
- Should the referenced container images be pre-pulled during build?
- How does this interact with registry auth at build time vs. runtime?
- Are quadlet units always portable, or do some reference host-specific paths/networks?

### Triage UX
- What does "include" mean for each non-RPM type? The action differs by type.
- Should the triage card explain what will happen (COPY vs. RUN vs. manual)?
- How do we handle items that can't be automatically reproduced in a Containerfile?

## Participants for Brainstorm
- Collins (image mode architecture — what bootc expects)
- Kit (implementation — what the renderer can produce)
- Fern (UX — how to present choices that have different consequences per type)
- Ember (strategy — how competitors handle this)
