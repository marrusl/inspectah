# Locale/Timezone Containerfile Rendering

**Priority:** Low
**Status:** Backlog

## Problem

The kernel/boot inspector detects locale (`LANG`, `LC_*`) and timezone
settings from the source system, but the containerfile renderer does not
emit any instructions to reproduce them. The migrated image gets the base
image defaults, which may differ from the source system.

## Impact

A system with `LANG=ja_JP.UTF-8` or `TZ=America/New_York` would silently
lose those settings after migration. Most bootc deployments configure this
at deploy time (Ignition, cloud-init), but it's a fidelity gap for the
Containerfile output.

## Likely Shape

Render `RUN localectl set-locale LANG=...` and
`RUN ln -sf /usr/share/zoneinfo/<tz> /etc/localtime` (or `timedatectl`)
when the source system's settings differ from the base image defaults.
Advisory or commented-out may be appropriate since deploy-time config is
the expected pattern for image mode.
