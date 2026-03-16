# Semantic Config Labels & System Properties

**Date:** 2026-03-15
**Status:** Proposed

## Problem

yoinkc detects config files but only classifies them by RPM lifecycle (`rpm_owned_modified`, `unowned`, `orphaned`). Files like `/etc/audit/rules.d/custom.rules` or `/etc/tmpfiles.d/myapp.conf` appear as generic entries with no semantic meaning. Users — especially those unfamiliar with what's on their systems — can't quickly identify what category of configuration they're looking at.

Additionally, several system properties (locale, timezone, kernel command line, alternatives) aren't captured at all. These are simple to detect and important for migration correctness.

## Scope

### Part A: Config File Categories (8 items)

Add a `category` field to `ConfigFileEntry` that classifies files by semantic meaning based on their path. This is a path-based label, not semantic analysis of file contents.

### Part B: System Properties (4 items)

Extend the kernel_boot inspector to detect locale, timezone, kernel command line, and alternatives. Render in the Kernel/Boot tab. This is a temporary home — a dedicated "System Properties" tab is planned for the future, but building a new inspector/tab/schema section for 4 items that will move is over-engineering.

## Part A: Config File Categories

### Schema

```python
class ConfigCategory(str, Enum):
    TMPFILES = "tmpfiles"
    ENVIRONMENT = "environment"
    AUDIT = "audit"
    LIBRARY_PATH = "library_path"
    JOURNAL = "journal"
    LOGROTATE = "logrotate"
    AUTOMOUNT = "automount"
    SYSCTL = "sysctl"
    OTHER = "other"
```

Add `category: ConfigCategory = ConfigCategory.OTHER` to `ConfigFileEntry`. Defaults to `OTHER` so existing snapshots deserialize without breaking.

`SCHEMA_VERSION` bumps from 7 to 8.

### Classification Logic

A `classify_config_path()` function in the config inspector using prefix matching — same pattern as the existing 210-line exclusion list:

```python
_CATEGORY_RULES: list[tuple[ConfigCategory, list[str]]] = [
    (ConfigCategory.TMPFILES, ["/etc/tmpfiles.d/"]),
    (ConfigCategory.ENVIRONMENT, ["/etc/environment", "/etc/profile.d/"]),
    (ConfigCategory.AUDIT, ["/etc/audit/rules.d/"]),
    (ConfigCategory.LIBRARY_PATH, ["/etc/ld.so.conf.d/"]),
    (ConfigCategory.JOURNAL, ["/etc/systemd/journald.conf.d/"]),
    (ConfigCategory.LOGROTATE, ["/etc/logrotate.d/"]),
    (ConfigCategory.AUTOMOUNT, ["/etc/auto.master", "/etc/auto."]),
    (ConfigCategory.SYSCTL, ["/etc/sysctl.d/", "/etc/sysctl.conf"]),
]

def classify_config_path(path: str) -> ConfigCategory:
    for category, prefixes in _CATEGORY_RULES:
        for prefix in prefixes:
            if path == prefix or path.startswith(prefix):
                return category
    return ConfigCategory.OTHER
```

Called at `ConfigFileEntry` creation time in the config inspector — all three detection paths (RPM-owned modified, unowned, orphaned).

Note: sysctl files get the basic `sysctl` label here. Smart flagging of individual sysctl keys (e.g., highlighting `net.ipv4.ip_forward`) is deferred to a separate spec.

### HTML Report

Add a "Category" column to the config files table:
- New `<th>` after the existing columns
- Cell shows a human-readable label (e.g., "Audit Rules", "Tmpfiles", "Sysctl")
- Uses PF6 label styling for visual consistency
- Column is sortable
- No structural change — flat table preserved

### Containerfile Impact

None. The Containerfile generator copies config files regardless of category. Category is informational only.

## Part B: System Properties

### Schema

```python
class AlternativeEntry(BaseModel):
    name: str      # e.g., "java"
    path: str      # e.g., "/usr/lib/jvm/java-17/bin/java"
    status: str    # "auto" or "manual"
```

Add to `KernelBootSection`:
```python
locale: Optional[str] = None
timezone: Optional[str] = None
kernel_cmdline: Optional[str] = None
alternatives: list[AlternativeEntry] = []
```

All fields optional with defaults — existing snapshots remain valid.

### Detection

yoinkc runs inside a container with the host filesystem mounted at `host_root`. Commands like `localectl` won't work — must use file-based detection.

| Property | Source | Method |
|----------|--------|--------|
| Locale | `{host_root}/etc/locale.conf` | Parse `LANG=` line |
| Timezone | `{host_root}/etc/localtime` | Read symlink target, strip `/usr/share/zoneinfo/` prefix |
| Kernel cmdline | `{host_root}/proc/cmdline` | Read entire file. Also check `{host_root}/etc/kernel/cmdline` and `{host_root}/etc/default/grub` for persistent config |
| Alternatives | `{host_root}/etc/alternatives/` | Read symlinks in directory, resolve targets. Parse name from symlink name, derive status from whether a manual override exists |

All detection is best-effort — missing files produce `None`/empty list, not errors.

### HTML Report

**Kernel/Boot tab** — add two new subsections below existing content:

1. **System Properties** — description list or small table:
   - Locale: value
   - Timezone: value
   - Kernel Command Line: value
   - Only renders if at least one property is present

2. **Alternatives** — table with columns: Name, Path, Status
   - Only renders if alternatives list is non-empty

Both subsections are informational — no include/exclude toggles, no triage interaction.

### Containerfile Impact

None. System properties are informational context. The Containerfile generator doesn't set locale/timezone/alternatives — those are host-level concerns outside the container image.

## What Does NOT Change

- **Config inspector detection logic** — same three detection strategies (RPM-owned modified, unowned, orphaned). Category is layered on top.
- **Config file exclusion list** — the 210 hardcoded exclusions are unchanged.
- **Triage sidebar** — config categories don't affect triage counts. System properties aren't triage items.
- **Fleet analysis** — `category` serializes to JSON and will be present in fleet snapshots, but no fleet-specific handling needed.

## Alternatives Considered

1. **External mapping file (YAML)** for config categories — over-engineered for 8 categories that only developers edit.
2. **Computed property on model** for category — mixes classification logic into data model, doesn't serialize cleanly.
3. **New `system_properties` inspector** — over-built for 4 items that will move to a dedicated tab later. Creates a full inspector/schema/template that gets refactored.
4. **Sysctl smart flagging in this spec** — deferred. Curating a list of migration-critical sysctl keys and annotating them is a different kind of work that deserves its own spec.
