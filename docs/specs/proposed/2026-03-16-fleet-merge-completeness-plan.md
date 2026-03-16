# Fleet Merge Completeness Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Complete fleet merge for selinux and non_rpm_software sections, suppress storage in fleet reports, fix `_strip_host_lists` privacy mode gaps.

**Architecture:** Follow existing merge patterns — `_merge_identity_items()` for identity-keyed items, `_merge_content_items()` for content-variant items, `_deduplicate_strings()` for string unions, first-snapshot pass-through for scalars. Schema additions are minimal (`.fleet` and `.include` fields on two models).

**Tech Stack:** Python 3.9+, Pydantic v2, Jinja2, pytest

**Spec:** `docs/specs/proposed/2026-03-16-fleet-merge-completeness-design.md`

---

## Chunk 1: Schema Changes & Non-RPM Software Merge

### Task 1: Schema Changes

**Files:**
- Modify: `src/yoinkc/schema.py:497-503` (SelinuxPortLabel)
- Modify: `src/yoinkc/schema.py:410-427` (NonRpmItem)
- Modify: `src/yoinkc/schema.py:536` (SCHEMA_VERSION)

- [ ] **Step 1: Add `fleet` and `include` to SelinuxPortLabel**

In `src/yoinkc/schema.py`, find the `SelinuxPortLabel` class (~line 497) and add:

```python
class SelinuxPortLabel(BaseModel):
    protocol: str = ""
    port: str = ""
    type: str = ""
    include: bool = True
    fleet: Optional[FleetPrevalence] = None
```

- [ ] **Step 2: Add `fleet` to NonRpmItem**

In `src/yoinkc/schema.py`, find the `NonRpmItem` class (~line 410). It already has `include: bool = True`. Add after it:

```python
    fleet: Optional[FleetPrevalence] = None
```

- [ ] **Step 3: Bump SCHEMA_VERSION**

In `src/yoinkc/schema.py` (~line 536), change:

```python
SCHEMA_VERSION = 9
```

- [ ] **Step 4: Run existing tests to confirm no regressions**

Run: `pytest tests/ -x -q`
Expected: All existing tests pass (new optional fields with defaults don't break anything).

- [ ] **Step 5: Commit**

```bash
git add src/yoinkc/schema.py
git commit -m "feat(schema): add fleet/include to SelinuxPortLabel, fleet to NonRpmItem

Bump SCHEMA_VERSION to 9. Prepares models for fleet merge of
selinux and non_rpm_software sections.

Assisted-by: Claude Code"
```

---

### Task 2: Non-RPM Software Merge

**Files:**
- Modify: `src/yoinkc/fleet/merge.py:176-401` (merge_snapshots)
- Test: `tests/test_fleet_merge.py`

- [ ] **Step 1: Write failing test — items merge by path**

In `tests/test_fleet_merge.py`, add imports if not present:

```python
from yoinkc.schema import NonRpmItem, NonRpmSoftwareSection
```

Add test:

```python
def test_non_rpm_items_merged_by_path():
    """Non-RPM items with same path across hosts are deduplicated."""
    item = NonRpmItem(path="/opt/app/bin/myapp", name="myapp", method="elf")
    s1 = _snap("host1", non_rpm_software=NonRpmSoftwareSection(items=[item]))
    s2 = _snap("host2", non_rpm_software=NonRpmSoftwareSection(items=[item]))
    merged = merge_snapshots([s1, s2], min_prevalence=100)
    assert merged.non_rpm_software is not None
    assert len(merged.non_rpm_software.items) == 1
    assert merged.non_rpm_software.items[0].fleet.count == 2
    assert merged.non_rpm_software.items[0].fleet.total == 2
    assert merged.non_rpm_software.items[0].include is True
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pytest tests/test_fleet_merge.py::test_non_rpm_items_merged_by_path -v`
Expected: FAIL — `merged.non_rpm_software is None` (not yet merged).

- [ ] **Step 3: Write failing test — different items preserved**

```python
def test_non_rpm_different_items_both_preserved():
    """Different non-RPM items on different hosts are both in merged output."""
    i1 = NonRpmItem(path="/opt/app1/bin/app1", name="app1", method="elf")
    i2 = NonRpmItem(path="/opt/app2/bin/app2", name="app2", method="pip")
    s1 = _snap("host1", non_rpm_software=NonRpmSoftwareSection(items=[i1]))
    s2 = _snap("host2", non_rpm_software=NonRpmSoftwareSection(items=[i2]))
    merged = merge_snapshots([s1, s2], min_prevalence=100)
    assert len(merged.non_rpm_software.items) == 2
    paths = {i.path for i in merged.non_rpm_software.items}
    assert paths == {"/opt/app1/bin/app1", "/opt/app2/bin/app2"}
```

- [ ] **Step 4: Write failing test — prevalence threshold filters items**

```python
def test_non_rpm_prevalence_threshold():
    """Items below min_prevalence get include=False."""
    item_common = NonRpmItem(path="/opt/common/bin/app", name="common", method="elf")
    item_rare = NonRpmItem(path="/opt/rare/bin/app", name="rare", method="elf")
    s1 = _snap("host1", non_rpm_software=NonRpmSoftwareSection(items=[item_common, item_rare]))
    s2 = _snap("host2", non_rpm_software=NonRpmSoftwareSection(items=[item_common]))
    merged = merge_snapshots([s1, s2], min_prevalence=100)
    by_path = {i.path: i for i in merged.non_rpm_software.items}
    assert by_path["/opt/common/bin/app"].include is True
    assert by_path["/opt/rare/bin/app"].include is False
```

- [ ] **Step 5: Write failing test — env_files content variant merge**

```python
from yoinkc.schema import ConfigFileEntry

def test_non_rpm_env_files_content_variants():
    """env_files with same path but different content produce variants."""
    ef1 = ConfigFileEntry(path="/opt/app/.env", content="DB_HOST=db1.example.com")
    ef2 = ConfigFileEntry(path="/opt/app/.env", content="DB_HOST=db2.example.com")
    s1 = _snap("host1", non_rpm_software=NonRpmSoftwareSection(items=[], env_files=[ef1]))
    s2 = _snap("host2", non_rpm_software=NonRpmSoftwareSection(items=[], env_files=[ef2]))
    merged = merge_snapshots([s1, s2], min_prevalence=100)
    assert len(merged.non_rpm_software.env_files) == 2
    # Both variants exist, neither meets 100% threshold
    for ef in merged.non_rpm_software.env_files:
        assert ef.fleet.count == 1
        assert ef.include is False
```

- [ ] **Step 6: Write failing test — env_files identical content deduped**

```python
def test_non_rpm_env_files_identical_deduped():
    """env_files with same path and content are deduplicated with correct prevalence."""
    ef = ConfigFileEntry(path="/opt/app/.env", content="DB_HOST=db.example.com")
    s1 = _snap("host1", non_rpm_software=NonRpmSoftwareSection(items=[], env_files=[ef]))
    s2 = _snap("host2", non_rpm_software=NonRpmSoftwareSection(items=[], env_files=[ef]))
    merged = merge_snapshots([s1, s2], min_prevalence=100)
    assert len(merged.non_rpm_software.env_files) == 1
    assert merged.non_rpm_software.env_files[0].fleet.count == 2
    assert merged.non_rpm_software.env_files[0].include is True
```

- [ ] **Step 7: Implement non_rpm_software merge in merge_snapshots()**

In `src/yoinkc/fleet/merge.py`, in `merge_snapshots()`, add a new section block before the `InspectionSnapshot(...)` constructor (around line 380). Follow the containers section pattern:

```python
    # --- Non-RPM Software ---
    non_rpm_section = None
    has_non_rpm = any(s.non_rpm_software for s in snapshots)
    if has_non_rpm:
        non_rpm_items = _merge_identity_items(
            _collect_section_lists(snapshots, "non_rpm_software", "items"),
            key_fn=lambda i: i.path,
            total=total, min_prevalence=min_prevalence, host_names=host_names,
        )
        non_rpm_env_files = _merge_content_items(
            _collect_section_lists(snapshots, "non_rpm_software", "env_files"),
            identity_fn=lambda f: f.path,
            variant_fn=lambda f: _content_hash(f.content),
            total=total, min_prevalence=min_prevalence, host_names=host_names,
        )
        non_rpm_section = NonRpmSoftwareSection(
            items=non_rpm_items,
            env_files=non_rpm_env_files,
        )
```

Add `NonRpmSoftwareSection` to the imports at the top of `merge.py`.

Add `non_rpm_software=non_rpm_section` to the `InspectionSnapshot(...)` constructor call (~line 384).

- [ ] **Step 8: Run tests to verify they pass**

Run: `pytest tests/test_fleet_merge.py -k "non_rpm" -v`
Expected: All 5 new tests pass.

- [ ] **Step 9: Run full test suite**

Run: `pytest tests/ -x -q`
Expected: All tests pass.

- [ ] **Step 10: Commit**

```bash
git add src/yoinkc/fleet/merge.py tests/test_fleet_merge.py
git commit -m "feat(fleet): merge non_rpm_software section in fleet aggregate

Items merged by path identity with prevalence. env_files use
content-variant merge (same pattern as config files).

Assisted-by: Claude Code"
```

---

## Chunk 2: SELinux Merge

### Task 3: SELinux Merge

**Files:**
- Modify: `src/yoinkc/fleet/merge.py` (merge_snapshots)
- Test: `tests/test_fleet_merge.py`

- [ ] **Step 1: Write failing test — port_labels identity merge**

In `tests/test_fleet_merge.py`, add imports:

```python
from yoinkc.schema import SelinuxPortLabel, SelinuxSection
```

Add test:

```python
def test_selinux_port_labels_merged():
    """Port labels with same protocol/port are deduplicated across hosts."""
    pl = SelinuxPortLabel(protocol="tcp", port="8080", type="http_port_t")
    s1 = _snap("host1", selinux=SelinuxSection(port_labels=[pl], mode="enforcing"))
    s2 = _snap("host2", selinux=SelinuxSection(port_labels=[pl], mode="enforcing"))
    merged = merge_snapshots([s1, s2], min_prevalence=100)
    assert merged.selinux is not None
    assert len(merged.selinux.port_labels) == 1
    assert merged.selinux.port_labels[0].fleet.count == 2
    assert merged.selinux.port_labels[0].fleet.total == 2
    assert merged.selinux.port_labels[0].include is True
```

- [ ] **Step 2: Write failing test — different ports preserved**

```python
def test_selinux_different_ports_preserved():
    """Different protocol/port combinations are all preserved."""
    pl1 = SelinuxPortLabel(protocol="tcp", port="8080", type="http_port_t")
    pl2 = SelinuxPortLabel(protocol="tcp", port="9090", type="http_port_t")
    s1 = _snap("host1", selinux=SelinuxSection(port_labels=[pl1], mode="enforcing"))
    s2 = _snap("host2", selinux=SelinuxSection(port_labels=[pl2], mode="enforcing"))
    merged = merge_snapshots([s1, s2], min_prevalence=100)
    assert len(merged.selinux.port_labels) == 2
    ports = {pl.port for pl in merged.selinux.port_labels}
    assert ports == {"8080", "9090"}
```

- [ ] **Step 3: Write failing test — port_labels prevalence threshold**

```python
def test_selinux_port_labels_prevalence():
    """Port labels below threshold get include=False."""
    pl = SelinuxPortLabel(protocol="tcp", port="8080", type="http_port_t")
    s1 = _snap("host1", selinux=SelinuxSection(port_labels=[pl], mode="enforcing"))
    s2 = _snap("host2", selinux=SelinuxSection(port_labels=[], mode="enforcing"))
    merged = merge_snapshots([s1, s2], min_prevalence=100)
    assert len(merged.selinux.port_labels) == 1
    assert merged.selinux.port_labels[0].include is False
    assert merged.selinux.port_labels[0].fleet.count == 1
```

- [ ] **Step 4: Write failing test — boolean_overrides dedup**

```python
def test_selinux_boolean_overrides_deduped():
    """Boolean overrides are deduplicated by name with fleet prevalence."""
    b1 = {"name": "httpd_can_network_connect", "current": "on", "default": "off"}
    s1 = _snap("host1", selinux=SelinuxSection(boolean_overrides=[b1], mode="enforcing"))
    s2 = _snap("host2", selinux=SelinuxSection(boolean_overrides=[b1], mode="enforcing"))
    merged = merge_snapshots([s1, s2], min_prevalence=100)
    assert len(merged.selinux.boolean_overrides) == 1
    assert merged.selinux.boolean_overrides[0]["fleet"]["count"] == 2
```

- [ ] **Step 5: Write failing test — string list unions**

```python
def test_selinux_string_lists_unioned():
    """String list fields are unioned across hosts."""
    s1 = _snap("host1", selinux=SelinuxSection(
        custom_modules=["mymod1"], fcontext_rules=["/opt/app(/.*)? system_u:object_r:httpd_sys_content_t:s0"],
        mode="enforcing",
    ))
    s2 = _snap("host2", selinux=SelinuxSection(
        custom_modules=["mymod2"], fcontext_rules=["/srv/data(/.*)? system_u:object_r:httpd_sys_content_t:s0"],
        mode="enforcing",
    ))
    merged = merge_snapshots([s1, s2], min_prevalence=100)
    assert set(merged.selinux.custom_modules) == {"mymod1", "mymod2"}
    assert len(merged.selinux.fcontext_rules) == 2
```

- [ ] **Step 6: Write failing test — scalar pass-through**

```python
def test_selinux_scalars_first_snapshot():
    """Scalar fields (mode, fips_mode) pass through from first snapshot."""
    s1 = _snap("host1", selinux=SelinuxSection(mode="enforcing", fips_mode=True))
    s2 = _snap("host2", selinux=SelinuxSection(mode="permissive", fips_mode=False))
    merged = merge_snapshots([s1, s2], min_prevalence=100)
    assert merged.selinux.mode == "enforcing"
    assert merged.selinux.fips_mode is True
```

- [ ] **Step 7: Write failing test — scalar disagreement still merges**

```python
def test_selinux_mode_disagreement_merges():
    """Hosts with different SELinux modes still produce a valid merged section."""
    s1 = _snap("host1", selinux=SelinuxSection(mode="enforcing"))
    s2 = _snap("host2", selinux=SelinuxSection(mode="disabled"))
    s3 = _snap("host3", selinux=SelinuxSection(mode="enforcing"))
    merged = merge_snapshots([s1, s2, s3], min_prevalence=100)
    assert merged.selinux is not None
    # First snapshot wins
    assert merged.selinux.mode == "enforcing"
```

- [ ] **Step 8: Implement SELinux merge in merge_snapshots()**

In `src/yoinkc/fleet/merge.py`, add a new section block in `merge_snapshots()`. Follow the network section pattern for port_labels, plus string unions and dict dedup:

```python
    # --- SELinux ---
    selinux_section = None
    has_selinux = any(s.selinux for s in snapshots)
    if has_selinux:
        port_labels = _merge_identity_items(
            _collect_section_lists(snapshots, "selinux", "port_labels"),
            key_fn=lambda p: f"{p.protocol}/{p.port}",
            total=total, min_prevalence=min_prevalence, host_names=host_names,
        )
        custom_modules = _deduplicate_strings(
            _collect_section_lists(snapshots, "selinux", "custom_modules"),
        )
        fcontext_rules = _deduplicate_strings(
            _collect_section_lists(snapshots, "selinux", "fcontext_rules"),
        )
        audit_rules = _deduplicate_strings(
            _collect_section_lists(snapshots, "selinux", "audit_rules"),
        )
        pam_configs = _deduplicate_strings(
            _collect_section_lists(snapshots, "selinux", "pam_configs"),
        )
        boolean_overrides = _deduplicate_dicts(
            _collect_section_lists(snapshots, "selinux", "boolean_overrides"),
            key_field="name",
            total=total, host_names=host_names,
        )
        # Scalars: pass-through from first snapshot with selinux data
        first_se = next(s.selinux for s in snapshots if s.selinux)
        selinux_section = SelinuxSection(
            mode=first_se.mode,
            fips_mode=first_se.fips_mode,
            port_labels=port_labels,
            custom_modules=custom_modules,
            fcontext_rules=fcontext_rules,
            audit_rules=audit_rules,
            pam_configs=pam_configs,
            boolean_overrides=boolean_overrides,
        )
```

Add `SelinuxSection` to the imports at the top of `merge.py`.

Add `selinux=selinux_section` to the `InspectionSnapshot(...)` constructor call.

- [ ] **Step 9: Run tests to verify they pass**

Run: `pytest tests/test_fleet_merge.py -k "selinux" -v`
Expected: All 7 new tests pass.

- [ ] **Step 10: Run full test suite**

Run: `pytest tests/ -x -q`
Expected: All tests pass.

- [ ] **Step 11: Commit**

```bash
git add src/yoinkc/fleet/merge.py tests/test_fleet_merge.py
git commit -m "feat(fleet): merge selinux section in fleet aggregate

Port labels merged by protocol/port identity with prevalence.
Boolean overrides deduplicated by name. String lists (custom_modules,
fcontext_rules, audit_rules, pam_configs) unioned. Scalars (mode,
fips_mode) pass through from first snapshot.

Assisted-by: Claude Code"
```

---

## Chunk 3: Privacy Mode Fix, Storage Suppression & Cleanup

### Task 4: Add Hosts to Dict-Based Fleet & Fix `_strip_host_lists`

**Files:**
- Modify: `src/yoinkc/fleet/merge.py:108-130` (_deduplicate_dicts)
- Modify: `src/yoinkc/fleet/merge.py:404-420` (_strip_host_lists)
- Test: `tests/test_fleet_merge.py`

**Context:** `_deduplicate_dicts()` currently stores fleet as `{"count": N, "total": T}` without a `hosts` list. This means `_strip_host_lists` has nothing to strip for dict-based items. We need to add `hosts` to the dict-based fleet output first, then update `_strip_host_lists` to handle all sections including dict-based items.

- [ ] **Step 1: Write failing test — boolean_overrides include hosts**

```python
def test_deduplicate_dicts_includes_hosts():
    """Dict-based fleet prevalence includes host list."""
    b1 = {"name": "httpd_can_network_connect", "current": "on", "default": "off"}
    s1 = _snap("host1", selinux=SelinuxSection(boolean_overrides=[b1], mode="enforcing"))
    s2 = _snap("host2", selinux=SelinuxSection(boolean_overrides=[b1], mode="enforcing"))
    merged = merge_snapshots([s1, s2], min_prevalence=100)
    fleet = merged.selinux.boolean_overrides[0]["fleet"]
    assert "hosts" in fleet
    assert set(fleet["hosts"]) == {"host1", "host2"}
```

- [ ] **Step 2: Update `_deduplicate_dicts` to include hosts**

In `src/yoinkc/fleet/merge.py`, find `_deduplicate_dicts()` (~line 108). Change the fleet dict assignment (~line 128) from:

```python
            item["fleet"] = {"count": len(entry["hosts"]), "total": total}
```

to:

```python
            item["fleet"] = {
                "count": len(entry["hosts"]),
                "total": total,
                "hosts": list(entry["hosts"]),
            }
```

- [ ] **Step 3: Run test to verify it passes**

Run: `pytest tests/test_fleet_merge.py::test_deduplicate_dicts_includes_hosts -v`
Expected: PASS

- [ ] **Step 4: Write failing test — selinux hosts stripped**

```python
def test_strip_host_lists_selinux():
    """--no-hosts strips host lists from selinux port_labels."""
    pl = SelinuxPortLabel(protocol="tcp", port="8080", type="http_port_t")
    s1 = _snap("host1", selinux=SelinuxSection(port_labels=[pl], mode="enforcing"))
    s2 = _snap("host2", selinux=SelinuxSection(port_labels=[pl], mode="enforcing"))
    merged = merge_snapshots([s1, s2], include_hosts=False)
    assert merged.selinux.port_labels[0].fleet.hosts == []
    assert merged.selinux.port_labels[0].fleet.count == 2  # count preserved
```

- [ ] **Step 5: Write failing test — non_rpm_software hosts stripped**

```python
def test_strip_host_lists_non_rpm():
    """--no-hosts strips host lists from non_rpm_software items."""
    item = NonRpmItem(path="/opt/app/bin/myapp", name="myapp", method="elf")
    s1 = _snap("host1", non_rpm_software=NonRpmSoftwareSection(items=[item]))
    s2 = _snap("host2", non_rpm_software=NonRpmSoftwareSection(items=[item]))
    merged = merge_snapshots([s1, s2], include_hosts=False)
    assert merged.non_rpm_software.items[0].fleet.hosts == []
    assert merged.non_rpm_software.items[0].fleet.count == 2
```

- [ ] **Step 6: Write failing test — users_groups hosts stripped (drive-by fix)**

```python
def test_strip_host_lists_users_groups():
    """--no-hosts strips host lists from users_groups (drive-by fix)."""
    from yoinkc.schema import UserGroupSection
    s1 = _snap("host1", users_groups=UserGroupSection(
        users=[{"name": "appuser", "uid": 1001}],
    ))
    s2 = _snap("host2", users_groups=UserGroupSection(
        users=[{"name": "appuser", "uid": 1001}],
    ))
    merged = merge_snapshots([s1, s2], include_hosts=False)
    # Dict-based fleet: hosts should be stripped
    assert merged.users_groups.users[0]["fleet"]["hosts"] == []
    assert merged.users_groups.users[0]["fleet"]["count"] == 2
```

- [ ] **Step 7: Update `_strip_host_lists` to handle new sections and dicts**

In `src/yoinkc/fleet/merge.py`, replace the `_strip_host_lists` function (lines 404-420). The current function iterates a hardcoded list of 6 section names and only handles Pydantic model `.fleet` attributes. Replace with:

```python
def _strip_host_lists(snapshot: InspectionSnapshot) -> None:
    """Remove per-item host lists from fleet metadata (privacy mode)."""
    for section_name in ["rpm", "config", "services", "network",
                         "scheduled_tasks", "containers",
                         "selinux", "non_rpm_software", "users_groups"]:
        section = getattr(snapshot, section_name, None)
        if section is None:
            continue
        for field_name in type(section).model_fields:
            items = getattr(section, field_name, None)
            if not isinstance(items, list):
                continue
            for item in items:
                # Pydantic model items
                if hasattr(item, "fleet") and item.fleet is not None:
                    item.fleet.hosts = []
                # Dict-based items (users, groups, boolean_overrides)
                elif isinstance(item, dict) and "fleet" in item:
                    fleet = item["fleet"]
                    if isinstance(fleet, dict) and "hosts" in fleet:
                        fleet["hosts"] = []
```

- [ ] **Step 8: Run tests to verify they pass**

Run: `pytest tests/test_fleet_merge.py -k "strip_host or deduplicate_dicts_includes" -v`
Expected: All 4 new tests pass plus the existing `test_strip_host_lists`.

- [ ] **Step 9: Run full test suite**

Run: `pytest tests/ -x -q`
Expected: All tests pass.

- [ ] **Step 10: Commit**

```bash
git add src/yoinkc/fleet/merge.py tests/test_fleet_merge.py
git commit -m "fix(fleet): add hosts to dict fleet, fix _strip_host_lists coverage

Add hosts list to _deduplicate_dicts fleet output (was count/total
only). Add selinux, non_rpm_software, users_groups to
_strip_host_lists. Handle both Pydantic .fleet and dict fleet keys.

Assisted-by: Claude Code"
```

---

### Task 5: Storage Tab Suppression

**Files:**
- Modify: `src/yoinkc/templates/report/_storage.html.j2`
- Test: `tests/test_fleet_merge.py` (or appropriate template test file)

- [ ] **Step 1: Write failing test — storage hidden in fleet reports**

Find the appropriate test file for template rendering. If fleet template tests exist in `test_fleet_merge.py` or a dedicated file, add there. Otherwise add to `test_fleet_merge.py`:

```python
def test_storage_suppressed_in_fleet_report():
    """Storage section is not rendered in fleet reports."""
    from yoinkc.schema import StorageSection, FstabEntry
    fstab = FstabEntry(device="/dev/sda1", mount_point="/", fstype="xfs", options="defaults")
    s1 = _snap("host1", storage=StorageSection(fstab_entries=[fstab]))
    s2 = _snap("host2", storage=StorageSection(fstab_entries=[fstab]))
    merged = merge_snapshots([s1, s2], min_prevalence=100)
    # Storage is NOT merged — should remain None in merged snapshot
    assert merged.storage is None
```

Note: This test verifies the merge engine doesn't include storage. The template guard (`{% if not fleet_meta %}`) is defense-in-depth. The "storage present when fleet_meta is not set" case is already covered by existing non-fleet render tests — no new test needed for that direction.

- [ ] **Step 2: Run test to verify it passes**

Run: `pytest tests/test_fleet_merge.py::test_storage_suppressed_in_fleet_report -v`
Expected: PASS — storage is already None in merged snapshots (never added to constructor).

- [ ] **Step 3: Add template guard for storage section**

In `src/yoinkc/templates/report/_storage.html.j2`, wrap the entire content with a fleet guard. At the very beginning of the file, add:

```jinja2
{% if not fleet_meta %}
```

At the very end of the file, add:

```jinja2
{% endif %}
```

This silently hides the storage tab/section when rendering fleet reports. Single-host reports are unaffected.

- [ ] **Step 4: Verify template renders correctly for non-fleet**

Run: `pytest tests/ -x -q`
Expected: All tests pass (existing non-fleet render tests still produce storage output).

- [ ] **Step 5: Commit**

```bash
git add src/yoinkc/templates/report/_storage.html.j2 tests/test_fleet_merge.py
git commit -m "feat(fleet): suppress storage tab in fleet reports

Storage data varies too wildly across hosts for useful fleet
aggregation. Silently hide the storage section when fleet_meta
is present. Single-host reports unaffected.

Assisted-by: Claude Code"
```

---

### Task 6: Final Verification

- [ ] **Step 1: Run full test suite**

Run: `pytest tests/ -v`
Expected: All tests pass, including all new fleet merge tests.

- [ ] **Step 2: Verify test count increased**

Count new tests added:
- Non-RPM: 5 tests (items merge, different items, prevalence, env_files variants, env_files dedup)
- SELinux: 7 tests (port labels merge, different ports, prevalence, boolean dedup, string unions, scalar pass-through, mode disagreement)
- Privacy mode: 4 tests (dict hosts, selinux strip, non_rpm strip, users_groups strip)
- Storage: 1 test (storage suppressed)
- **Total: 17 new tests**

- [ ] **Step 3: Verify no lint/type issues**

Run whatever linter/type checker the project uses (check `pyproject.toml` for configuration).
