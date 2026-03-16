# Fleet Merge Completeness

**Date:** 2026-03-16
**Status:** Proposed
**Part of:** Fleet Refine (Spec 1 of 3)

## Problem

Fleet merge (`yoinkc-fleet aggregate`) currently omits three sections from
merged snapshots: storage, selinux, and non_rpm_software. This means fleet
reports are incomplete — users see blank sections for data that exists in the
source snapshots. More critically, enabling fleet refine mode (Spec 2) on an
incomplete snapshot would be misleading: users would refine a Containerfile
that silently drops SELinux port labels and non-RPM software.

This spec completes the merge for selinux and non_rpm_software, and
intentionally suppresses storage in fleet reports.

## Context: Fleet Refine Decomposition

Fleet refine is decomposed into three specs, built in order:

1. **Fleet Merge Completeness** (this spec) — complete the merge engine,
   suppress storage
2. **Fleet Refine Lifecycle** — toggles, reset, re-render, prevalence slider
   on fleet snapshots
3. **Fleet Config Editor with Variant Awareness** — variant comparison, pick
   canonical variant, diff view

Each spec is independently shippable. This spec has no UI changes beyond
storage suppression.

## Decisions

### Non-RPM Software Merge

NonRpmItem already has `.include: bool = True`. The identity key is `path` —
each non-RPM item is uniquely identified by its filesystem path.

**items:** Merge with `_merge_identity_items(key_fn=lambda i: i.path)`. Items
on the same path across hosts are deduplicated; prevalence count tracks how
many hosts have each item. Include/exclude is set by the `--min-prevalence`
threshold, same as all other identity-keyed items.

**env_files:** These are `List[ConfigFileEntry]`, which already has full fleet
support (`.fleet` field, `.include` toggle). Merge with
`_merge_content_items(identity_fn=lambda f: f.path, variant_fn=lambda f: _content_hash(f.content))` —
same pattern as config files. An env file at the same path with different
content across hosts produces separate content variants with independent
prevalence.

**Schema change:** Add `fleet: Optional[FleetPrevalence] = None` to
`NonRpmItem`.

### SELinux Merge

SELinux has a mix of field types requiring different merge strategies.

**port_labels (List[SelinuxPortLabel]):** Identity-based merge with
`key_fn=lambda p: f"{p.protocol}/{p.port}"`. A TCP/8080 label on 3 of 5 hosts
gets prevalence 3/5. If the same protocol/port has different SELinux types
across hosts, the first-seen type wins (same as package version in RPM merge).

**String list fields** (custom_modules, fcontext_rules, audit_rules,
pam_configs): Union with `_deduplicate_strings()`. No per-item prevalence —
these are simple set unions, same pattern as enabled_units/disabled_units in
the services section.

**boolean_overrides (List[dict]):** Deduplicate by boolean name with
`_deduplicate_dicts(key_field="name")`. Each boolean gets fleet prevalence
as a dict key (same pattern as users/groups).

**Scalar fields** (mode, fips_mode): Pass-through from first snapshot. These
are host-level settings that don't go into the Containerfile. If hosts
disagree on SELinux mode, that's informational but not actionable in the
migration spec. Same pattern as kernel_boot locale/timezone.

**Schema changes:**
- Add `fleet: Optional[FleetPrevalence] = None` to `SelinuxPortLabel`
- Add `include: bool = True` to `SelinuxPortLabel`

Adding `.include` now (rather than deferring to Spec 2) avoids touching the
model again and lets the merge engine set include/exclude by prevalence
threshold automatically.

### Storage Suppression

Storage data (fstab entries, mount points, LVM volumes, /var directories)
varies wildly across hosts. Unlike config files or packages, there's no
useful union or intersection — a fleet of 5 hosts might have 5 completely
different partition layouts. No good way to present this in a merged report.

**Decision:** Silently suppress the storage tab in fleet reports. Add
`{% if not fleet_meta %}` guard around the storage section in the HTML
template. No merge logic, no schema changes.

Single-host reports are unaffected.

### Privacy Mode (`--no-hosts`)

The `_strip_host_lists()` function in `merge.py` iterates a hardcoded list of
section names to clear `.fleet.hosts` when `--no-hosts` is passed. Currently
it covers rpm, config, services, network, scheduled_tasks, and containers.

Add `selinux` and `non_rpm_software` to that list so fleet reports generated
with `--no-hosts` don't leak hostnames in the newly merged sections.

Additionally, `_strip_host_lists()` currently only handles Pydantic model
items with a `.fleet` attribute. Dict-based items (boolean_overrides in
selinux, users/groups in users_groups) store fleet prevalence as a dict key
`item["fleet"]`, which `hasattr()` doesn't catch. The function needs to
handle both patterns: `hasattr(item, "fleet")` for models, `"fleet" in item`
for dicts.

Drive-by fix: `users_groups` is also missing from `_strip_host_lists()` — a
pre-existing bug. Fix it while we're in the function (same dict-handling
issue applies).

### Schema Version

Bump `SCHEMA_VERSION` to 9. Two new fields on existing models (NonRpmItem.fleet,
SelinuxPortLabel.fleet) plus one new field (SelinuxPortLabel.include).

## Testing

### SELinux Merge Tests
- port_labels: same protocol/port deduped across hosts, different ports preserved
- port_labels: prevalence count correct (3 of 5 hosts)
- port_labels: include set by min_prevalence threshold
- boolean_overrides: dedup by name, fleet dict attached
- String lists: union across hosts (custom_modules, fcontext_rules, etc.)
- Scalar pass-through: mode and fips_mode from first snapshot
- Scalar disagreement: hosts with different modes still merge (first wins)

### Non-RPM Software Merge Tests
- items: merge by path, prevalence count correct
- items: include set by min_prevalence threshold
- items: different items on different hosts both preserved
- env_files: content-variant merge (same path, different content)
- env_files: identical content deduped with correct prevalence

### Privacy Mode Tests
- `--no-hosts` strips host lists from selinux port_labels
- `--no-hosts` strips host lists from non_rpm_software items
- `--no-hosts` strips host lists from users_groups (drive-by fix)

### Storage Suppression Tests
- Storage section absent from HTML when fleet_meta is set
- Storage section present in HTML when fleet_meta is not set

## Implementation Notes

Function names in this spec (e.g., `_merge_identity_items`,
`_merge_content_items`) describe the merge pattern, not necessarily the exact
function name in `merge.py`. The implementation plan will map these to the
actual function signatures in the codebase.

## Out of Scope

- Refine mode changes (Spec 2)
- Toggle UI changes (Spec 2)
- Prevalence slider (Spec 2)
- Config editor changes (Spec 3)
- New CLI flags
- Storage merge logic
