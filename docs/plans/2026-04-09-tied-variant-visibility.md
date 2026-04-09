# Tied Variant Visibility Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Eliminate silent data loss when fleet merge produces tied config/drop-in/quadlet/compose/env-file variants by picking a deterministic winner, adding model flags, and making ties visible across all output surfaces.

**Architecture:** Add `tie`/`tie_winner` boolean flags to five item-type models. Modify `_auto_select_variants()` to pick a winner by full SHA-256 digest sort instead of deselecting all. Add Level 1 content normalization at hash time. Update seven renderer surfaces (Containerfile comments, HTML report, refine UI, CLI output, audit report, readme, and new merge-notes.md).

**Tech Stack:** Python 3.11+, Pydantic v2 models, Jinja2 templates, pytest

**Spec:** `docs/specs/proposed/2026-04-09-tied-config-visibility-design.md`

---

### Task 1: Add `tie` and `tie_winner` Model Flags

**Files:**
- Modify: `src/yoinkc/schema.py:207-218` (ConfigFileEntry)
- Modify: `src/yoinkc/schema.py:242-249` (SystemdDropIn)
- Modify: `src/yoinkc/schema.py:423-429` (QuadletUnit)
- Modify: `src/yoinkc/schema.py:437-441` (ComposeFile)
- Test: `tests/test_fleet_merge.py`

Note: Non-RPM env files reuse `ConfigFileEntry` (see `schema.py:513`), so they get the flags automatically.

- [ ] **Step 1: Write failing test for tie flags on merged output**

In `tests/test_fleet_merge.py`, add at the top imports:

```python
from yoinkc.schema import ConfigSection, ConfigFileEntry
```

(ConfigSection and ConfigFileEntry are already imported if used by existing tests — verify and add only if missing.)

Add a new test class:

```python
class TestTieFlags:
    """Verify tie/tie_winner flags are set correctly after fleet merge."""

    def test_tied_variants_get_tie_flags(self):
        from yoinkc.fleet.merge import merge_snapshots

        # Two hosts with different content for the same path → tie
        s1 = _snap("host-1", config=ConfigSection(files=[
            ConfigFileEntry(path="/etc/test.conf", kind="unowned", content="variant-a"),
        ]))
        s2 = _snap("host-2", config=ConfigSection(files=[
            ConfigFileEntry(path="/etc/test.conf", kind="unowned", content="variant-b"),
        ]))
        merged = merge_snapshots([s1, s2], min_prevalence=0)

        variants = merged.config.files
        assert len(variants) == 2

        # Both should have tie=True
        assert all(v.tie for v in variants), "All tied variants must have tie=True"

        # Exactly one should be tie_winner=True and include=True
        winners = [v for v in variants if v.tie_winner]
        assert len(winners) == 1, "Exactly one variant should be tie_winner"
        assert winners[0].include is True, "Tie winner must have include=True"

        # The loser should have tie_winner=False and include=False
        losers = [v for v in variants if not v.tie_winner]
        assert len(losers) == 1
        assert losers[0].include is False

    def test_clear_winner_no_tie_flags(self):
        from yoinkc.fleet.merge import merge_snapshots

        # 2 hosts with content A, 1 host with content B → clear winner, no tie
        s1 = _snap("host-1", config=ConfigSection(files=[
            ConfigFileEntry(path="/etc/test.conf", kind="unowned", content="majority"),
        ]))
        s2 = _snap("host-2", config=ConfigSection(files=[
            ConfigFileEntry(path="/etc/test.conf", kind="unowned", content="majority"),
        ]))
        s3 = _snap("host-3", config=ConfigSection(files=[
            ConfigFileEntry(path="/etc/test.conf", kind="unowned", content="minority"),
        ]))
        merged = merge_snapshots([s1, s2, s3], min_prevalence=0)

        for v in merged.config.files:
            assert v.tie is False, "Clear winners should not have tie=True"
            assert v.tie_winner is False

    def test_three_way_tie_one_winner(self):
        from yoinkc.fleet.merge import merge_snapshots

        s1 = _snap("host-1", config=ConfigSection(files=[
            ConfigFileEntry(path="/etc/test.conf", kind="unowned", content="aaa"),
        ]))
        s2 = _snap("host-2", config=ConfigSection(files=[
            ConfigFileEntry(path="/etc/test.conf", kind="unowned", content="bbb"),
        ]))
        s3 = _snap("host-3", config=ConfigSection(files=[
            ConfigFileEntry(path="/etc/test.conf", kind="unowned", content="ccc"),
        ]))
        merged = merge_snapshots([s1, s2, s3], min_prevalence=0)

        variants = merged.config.files
        assert len(variants) == 3
        assert all(v.tie for v in variants)

        winners = [v for v in variants if v.tie_winner]
        assert len(winners) == 1, "3-way tie: exactly one winner"
        assert winners[0].include is True

        losers = [v for v in variants if not v.tie_winner]
        assert len(losers) == 2
        assert all(not v.include for v in losers)
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_fleet_merge.py::TestTieFlags -v`

Expected: FAIL — `ConfigFileEntry` has no attribute `tie`.

- [ ] **Step 3: Add tie and tie_winner fields to all four models**

In `src/yoinkc/schema.py`, add fields to each model:

For `ConfigFileEntry` (after line 218, before `fleet`):
```python
    tie: bool = False
    tie_winner: bool = False
```

For `SystemdDropIn` (after line 248, before `fleet`):
```python
    tie: bool = False
    tie_winner: bool = False
```

For `QuadletUnit` (after line 428, before `fleet`):
```python
    tie: bool = False
    tie_winner: bool = False
```

For `ComposeFile` (after line 440, before `fleet`):
```python
    tie: bool = False
    tie_winner: bool = False
```

- [ ] **Step 4: Run tests to verify they still fail (flags exist but aren't set)**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_fleet_merge.py::TestTieFlags -v`

Expected: FAIL — `tie` is `False` on tied variants (the merge logic doesn't set them yet).

- [ ] **Step 5: Commit model changes**

```bash
cd /Users/mrussell/Work/bootc-migration/yoinkc
git add src/yoinkc/schema.py tests/test_fleet_merge.py
git commit -m "$(cat <<'EOF'
feat(schema): add tie and tie_winner flags to variant models

Add boolean fields to ConfigFileEntry, SystemdDropIn, QuadletUnit, and
ComposeFile. These flags distinguish tie-auto-resolved items from
clear winners and below-threshold exclusions. Non-RPM env files reuse
ConfigFileEntry and inherit the flags automatically.

Flags are not yet set by merge logic — that follows in the next commit.

Assisted-by: Claude Code (Opus 4.6)
EOF
)"
```

---

### Task 2: Switch `_content_hash()` to Full SHA-256 and Add Normalization

**Files:**
- Modify: `src/yoinkc/fleet/merge.py:25-26` (_content_hash)
- Modify: `src/yoinkc/fleet/merge.py:347-352,365-369,426-431,456-460` (variant_fn call sites)
- Test: `tests/test_fleet_merge.py`

- [ ] **Step 1: Write failing test for whitespace normalization**

In `tests/test_fleet_merge.py`, add:

```python
class TestNormalization:
    """Level 1 normalization: trailing whitespace + line endings."""

    def test_trailing_whitespace_collapses_variants(self):
        from yoinkc.fleet.merge import merge_snapshots

        # Same content except trailing spaces on line 2
        content_a = "key=value\nsetting=on\n"
        content_b = "key=value\nsetting=on   \n"

        s1 = _snap("host-1", config=ConfigSection(files=[
            ConfigFileEntry(path="/etc/test.conf", kind="unowned", content=content_a),
        ]))
        s2 = _snap("host-2", config=ConfigSection(files=[
            ConfigFileEntry(path="/etc/test.conf", kind="unowned", content=content_b),
        ]))
        merged = merge_snapshots([s1, s2], min_prevalence=0)

        # Should collapse to 1 variant (not 2)
        assert len(merged.config.files) == 1
        assert merged.config.files[0].fleet.count == 2

    def test_line_ending_normalization(self):
        from yoinkc.fleet.merge import merge_snapshots

        content_unix = "key=value\nsetting=on\n"
        content_dos = "key=value\r\nsetting=on\r\n"

        s1 = _snap("host-1", config=ConfigSection(files=[
            ConfigFileEntry(path="/etc/test.conf", kind="unowned", content=content_unix),
        ]))
        s2 = _snap("host-2", config=ConfigSection(files=[
            ConfigFileEntry(path="/etc/test.conf", kind="unowned", content=content_dos),
        ]))
        merged = merge_snapshots([s1, s2], min_prevalence=0)

        assert len(merged.config.files) == 1
        assert merged.config.files[0].fleet.count == 2

    def test_genuine_content_difference_not_collapsed(self):
        from yoinkc.fleet.merge import merge_snapshots

        s1 = _snap("host-1", config=ConfigSection(files=[
            ConfigFileEntry(path="/etc/test.conf", kind="unowned", content="key=value1\n"),
        ]))
        s2 = _snap("host-2", config=ConfigSection(files=[
            ConfigFileEntry(path="/etc/test.conf", kind="unowned", content="key=value2\n"),
        ]))
        merged = merge_snapshots([s1, s2], min_prevalence=0)

        assert len(merged.config.files) == 2
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_fleet_merge.py::TestNormalization -v`

Expected: FAIL — trailing whitespace variants are not collapsed.

- [ ] **Step 3: Implement `_normalize_content()` and update `_content_hash()`**

In `src/yoinkc/fleet/merge.py`, replace the `_content_hash` function and add normalization:

```python
def _normalize_content(text: str) -> str:
    """Level 1 normalization: strip trailing whitespace per line, normalize line endings."""
    lines = text.replace("\r\n", "\n").replace("\r", "\n").split("\n")
    return "\n".join(line.rstrip() for line in lines)


def _content_hash(text: str) -> str:
    return hashlib.sha256(text.encode()).hexdigest()
```

Then update the four content-based `variant_fn` lambdas to normalize before hashing:

Config files (line ~350):
```python
variant_fn=lambda f: _content_hash(_normalize_content(f.content)),
```

Drop-ins (line ~368):
```python
variant_fn=lambda d: _content_hash(_normalize_content(d.content)),
```

Quadlet units (line ~429):
```python
variant_fn=lambda q: _content_hash(_normalize_content(q.content)),
```

Non-RPM env files (line ~459):
```python
variant_fn=lambda f: _content_hash(_normalize_content(f.content)),
```

Leave compose files unchanged — they use tuple-based variant grouping that already discards whitespace:
```python
variant_fn=lambda c: _content_hash(
    str(sorted((img.service, img.image) for img in c.images))
),
```

- [ ] **Step 4: Run normalization tests to verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_fleet_merge.py::TestNormalization -v`

Expected: PASS

- [ ] **Step 5: Run full test suite to check for regressions from full-hash change**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_fleet_merge.py -v`

Expected: PASS (existing tests should not depend on truncated hash length).

- [ ] **Step 6: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/yoinkc
git add src/yoinkc/fleet/merge.py tests/test_fleet_merge.py
git commit -m "$(cat <<'EOF'
feat(fleet): add Level 1 normalization and switch to full SHA-256

Add _normalize_content() to strip trailing whitespace and normalize
line endings before content hashing. This collapses noise variants
that differ only in trivial whitespace.

Switch _content_hash() from truncated 16-char hex to full 64-char
SHA-256 digest to eliminate collision risk for both variant grouping
and the upcoming tiebreaker sort.

Compose files are excluded from normalization since their variant
function uses sorted (service, image) tuples.

Assisted-by: Claude Code (Opus 4.6)
EOF
)"
```

---

### Task 3: Implement Deterministic Tiebreaker in `_auto_select_variants()`

**Files:**
- Modify: `src/yoinkc/fleet/merge.py:119-155` (_auto_select_variants)
- Test: `tests/test_fleet_merge.py`

- [ ] **Step 1: Write failing test for deterministic tiebreaker**

In `tests/test_fleet_merge.py`, add:

```python
class TestDeterministicTiebreaker:
    """Tiebreaker picks winner by full SHA-256 digest sort."""

    def test_same_winner_regardless_of_input_order(self):
        from yoinkc.fleet.merge import merge_snapshots

        content_a = "variant-alpha"
        content_b = "variant-beta"

        # Order 1: host-1 has alpha, host-2 has beta
        s1a = _snap("host-1", config=ConfigSection(files=[
            ConfigFileEntry(path="/etc/test.conf", kind="unowned", content=content_a),
        ]))
        s1b = _snap("host-2", config=ConfigSection(files=[
            ConfigFileEntry(path="/etc/test.conf", kind="unowned", content=content_b),
        ]))
        merged1 = merge_snapshots([s1a, s1b], min_prevalence=0)
        winner1 = [v for v in merged1.config.files if v.tie_winner][0].content

        # Order 2: reversed
        s2a = _snap("host-1", config=ConfigSection(files=[
            ConfigFileEntry(path="/etc/test.conf", kind="unowned", content=content_b),
        ]))
        s2b = _snap("host-2", config=ConfigSection(files=[
            ConfigFileEntry(path="/etc/test.conf", kind="unowned", content=content_a),
        ]))
        merged2 = merge_snapshots([s2a, s2b], min_prevalence=0)
        winner2 = [v for v in merged2.config.files if v.tie_winner][0].content

        assert winner1 == winner2, "Tiebreaker must be order-independent"

    def test_tiebreaker_picks_lowest_hash(self):
        import hashlib
        from yoinkc.fleet.merge import merge_snapshots, _normalize_content

        content_a = "aaa-content"
        content_b = "bbb-content"

        hash_a = hashlib.sha256(_normalize_content(content_a).encode()).hexdigest()
        hash_b = hashlib.sha256(_normalize_content(content_b).encode()).hexdigest()
        expected_winner = content_a if hash_a < hash_b else content_b

        s1 = _snap("host-1", config=ConfigSection(files=[
            ConfigFileEntry(path="/etc/test.conf", kind="unowned", content=content_a),
        ]))
        s2 = _snap("host-2", config=ConfigSection(files=[
            ConfigFileEntry(path="/etc/test.conf", kind="unowned", content=content_b),
        ]))
        merged = merge_snapshots([s1, s2], min_prevalence=0)
        winner = [v for v in merged.config.files if v.tie_winner][0]

        assert winner.content == expected_winner
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_fleet_merge.py::TestDeterministicTiebreaker -v`

Expected: FAIL — no variant has `tie_winner=True`.

- [ ] **Step 3: Implement tiebreaker in `_auto_select_variants()`**

Replace the function in `src/yoinkc/fleet/merge.py`:

```python
def _auto_select_variants(items: list) -> None:
    """Post-process content-variant item lists with tie-breaking auto-selection.

    Groups items by ``path``. Within each group:
    - Single variant: always selected (``include=True``).
    - Clear winner (strictly highest ``fleet.count``): winner selected, rest deselected.
    - Tie at the top: winner picked by lowest full SHA-256 digest; all tied
      variants get ``tie=True``, winner also gets ``tie_winner=True``.

    Items lacking ``path``, ``fleet``, or ``include`` attributes are skipped.
    """
    groups: dict[str, list] = {}
    order: list[str] = []
    for item in items:
        path = getattr(item, "path", None)
        if path is None or not hasattr(item, "fleet") or item.fleet is None:
            continue
        if not hasattr(item, "include"):
            continue
        if path not in groups:
            order.append(path)
            groups[path] = []
        groups[path].append(item)

    for path in order:
        variants = groups[path]
        if len(variants) == 1:
            variants[0].include = True
            continue
        variants.sort(key=lambda v: v.fleet.count, reverse=True)
        top_count = variants[0].fleet.count
        if variants[0].fleet.count == variants[1].fleet.count:
            # Tie at the top — collect all variants tied at max count
            tied = [v for v in variants if v.fleet.count == top_count]
            non_tied = [v for v in variants if v.fleet.count != top_count]

            # Sort tied variants by full content hash for deterministic pick
            tied.sort(key=lambda v: _content_hash(
                _normalize_content(v.content) if hasattr(v, "content")
                else str(sorted((img.service, img.image) for img in v.images))
            ))

            # Mark all tied variants
            for v in tied:
                v.tie = True
                v.tie_winner = False
                v.include = False

            # First in hash order wins
            tied[0].tie_winner = True
            tied[0].include = True

            # Non-tied variants below the top are just losers
            for v in non_tied:
                v.include = False
        else:
            variants[0].include = True
            for v in variants[1:]:
                v.include = False
```

- [ ] **Step 4: Run tiebreaker tests**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_fleet_merge.py::TestDeterministicTiebreaker tests/test_fleet_merge.py::TestTieFlags -v`

Expected: PASS

- [ ] **Step 5: Update existing `TestAutoSelectVariants` expectations**

The existing `test_auto_select_config_variants` test expects tied variants to ALL have `include=False`. With the tiebreaker, one tied variant now has `include=True`. Update the expected values:

In the parametrize list, change:
- `([2, 2], [False, False])` → `([2, 2], [True, False])` (winner gets True)
- `([1, 1, 1], [False, False, False])` → `([1, 1, 1], [True, False, False])` (one winner)
- `([3, 3, 1], [False, False, False])` → `([3, 3, 1], [True, False, False])` (top-two tied, one wins; third is a non-tied loser below top count)

Note: For `[3, 3, 1]`, the sort is by fleet.count descending. After tiebreaker, the two count-3 variants are tied — one wins. The count-1 variant remains `include=False` as a non-tied loser.

- [ ] **Step 6: Run full merge test suite**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_fleet_merge.py -v`

Expected: PASS

- [ ] **Step 7: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/yoinkc
git add src/yoinkc/fleet/merge.py tests/test_fleet_merge.py
git commit -m "$(cat <<'EOF'
feat(fleet): deterministic tiebreaker for variant auto-selection

When variants tie at the top fleet count, pick the winner by lowest
full SHA-256 digest (lexicographic sort). Set tie=True on all tied
variants and tie_winner=True on the winner.

Previously all tied variants were set to include=False, silently
dropping them from the output. Now exactly one variant is included
with full traceability via the tie flags.

Handles 2-way and N-way ties uniformly. Compose files use the
sorted (service, image) tuple digest for tiebreaking.

Assisted-by: Claude Code (Opus 4.6)
EOF
)"
```

---

### Task 4: Containerfile Inventory Comment — Tied Items Block

**Files:**
- Modify: `src/yoinkc/renderers/containerfile/_config_tree.py:319-437` (config_inventory_comment)
- Test: `tests/test_fleet_merge.py` (or a new test file for renderer output)

- [ ] **Step 1: Write failing test for tied items in Containerfile comment**

In `tests/test_fleet_merge.py`, add:

```python
class TestContainerfileTieComment:
    """Containerfile inventory comment includes tied items block."""

    def test_tied_config_appears_in_inventory_comment(self):
        from yoinkc.fleet.merge import merge_snapshots
        from yoinkc.renderers.containerfile._config_tree import config_inventory_comment

        s1 = _snap("host-1", config=ConfigSection(files=[
            ConfigFileEntry(path="/etc/test.conf", kind="unowned", content="variant-a"),
        ]))
        s2 = _snap("host-2", config=ConfigSection(files=[
            ConfigFileEntry(path="/etc/test.conf", kind="unowned", content="variant-b"),
        ]))
        merged = merge_snapshots([s1, s2], min_prevalence=0)

        lines = config_inventory_comment(merged, dhcp_paths=set())
        comment_text = "\n".join(lines)

        assert "Tied" in comment_text, "Should mention tied items"
        assert "etc/test.conf" in comment_text, "Should list the tied path"
        assert "merge-notes.md" in comment_text, "Should point to merge-notes.md"

    def test_no_tie_block_when_no_ties(self):
        from yoinkc.fleet.merge import merge_snapshots
        from yoinkc.renderers.containerfile._config_tree import config_inventory_comment

        # 2 hosts with identical content → no tie
        s1 = _snap("host-1", config=ConfigSection(files=[
            ConfigFileEntry(path="/etc/test.conf", kind="unowned", content="same"),
        ]))
        s2 = _snap("host-2", config=ConfigSection(files=[
            ConfigFileEntry(path="/etc/test.conf", kind="unowned", content="same"),
        ]))
        merged = merge_snapshots([s1, s2], min_prevalence=0)

        lines = config_inventory_comment(merged, dhcp_paths=set())
        comment_text = "\n".join(lines)

        assert "Tied" not in comment_text
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_fleet_merge.py::TestContainerfileTieComment -v`

Expected: FAIL — "Tied" not in comment text.

- [ ] **Step 3: Add tied items block to `config_inventory_comment()`**

In `src/yoinkc/renderers/containerfile/_config_tree.py`, add the following block inside `config_inventory_comment()`, after the existing config file categories (after the orphaned block around line 355) but before the repo files block (line 357):

```python
    # Tied items (across all variant-bearing sections)
    tied_items = []
    if snapshot.config and snapshot.config.files:
        for f in snapshot.config.files:
            if getattr(f, "tie", False) and not getattr(f, "tie_winner", False):
                # Find the winner for this path to get variant count
                pass  # counted below
        tied_configs = [f for f in snapshot.config.files if getattr(f, "tie_winner", False)]
        for f in tied_configs:
            path_variants = [v for v in snapshot.config.files if v.path == f.path]
            tied_items.append((f.path.lstrip("/"), "config", f.fleet, len(path_variants)))
    if snapshot.services and snapshot.services.drop_ins:
        tied_dropins = [d for d in snapshot.services.drop_ins if getattr(d, "tie_winner", False)]
        for d in tied_dropins:
            path_variants = [v for v in snapshot.services.drop_ins if v.path == d.path]
            tied_items.append((d.path.lstrip("/"), "drop-in", d.fleet, len(path_variants)))
    if snapshot.containers:
        if snapshot.containers.quadlet_units:
            tied_quads = [q for q in snapshot.containers.quadlet_units if getattr(q, "tie_winner", False)]
            for q in tied_quads:
                path_variants = [v for v in snapshot.containers.quadlet_units if v.path == q.path]
                tied_items.append((q.path.lstrip("/"), "quadlet", q.fleet, len(path_variants)))
        if snapshot.containers.compose_files:
            tied_compose = [c for c in snapshot.containers.compose_files if getattr(c, "tie_winner", False)]
            for c in tied_compose:
                path_variants = [v for v in snapshot.containers.compose_files if v.path == c.path]
                tied_items.append((c.path.lstrip("/"), "compose", c.fleet, len(path_variants)))
    if snapshot.non_rpm_software and snapshot.non_rpm_software.env_files:
        tied_envs = [f for f in snapshot.non_rpm_software.env_files if getattr(f, "tie_winner", False)]
        for f in tied_envs:
            path_variants = [v for v in snapshot.non_rpm_software.env_files if v.path == f.path]
            tied_items.append((f.path.lstrip("/"), "env", f.fleet, len(path_variants)))

    if tied_items:
        lines.append(f"# Tied items resolved by content-hash tiebreaker ({len(tied_items)}):")
        for path, item_type, fleet, variant_count in tied_items:
            lines.append(f"#   {path}  ({item_type}, {fleet.count}/{fleet.total} hosts each, {variant_count} variants)")
        lines.append("#   See merge-notes.md for tie details")
        lines.append("#   Review in report.html or run `yoinkc refine` to change selection")
```

- [ ] **Step 4: Run tests**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_fleet_merge.py::TestContainerfileTieComment -v`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/yoinkc
git add src/yoinkc/renderers/containerfile/_config_tree.py tests/test_fleet_merge.py
git commit -m "$(cat <<'EOF'
feat(containerfile): add tied items block to inventory comment

List tied items with path, type, fleet ratio, and variant count in
the Containerfile inventory comment. Points users to merge-notes.md
and report.html for details. Covers all five item types that pass
through _auto_select_variants().

Assisted-by: Claude Code (Opus 4.6)
EOF
)"
```

---

### Task 5: New `merge-notes.md` Renderer

**Files:**
- Create: `src/yoinkc/renderers/merge_notes.py`
- Modify: `src/yoinkc/renderers/__init__.py:22-58` (run_all — add render call)
- Test: `tests/test_fleet_merge.py`

- [ ] **Step 1: Write failing test for merge-notes.md generation**

In `tests/test_fleet_merge.py`, add:

```python
from pathlib import Path
import tempfile


class TestMergeNotes:
    """merge-notes.md is generated with tie and non-unanimous details."""

    def test_merge_notes_contains_tied_item(self):
        from yoinkc.fleet.merge import merge_snapshots
        from yoinkc.renderers.merge_notes import render_merge_notes

        s1 = _snap("host-1", config=ConfigSection(files=[
            ConfigFileEntry(path="/etc/test.conf", kind="unowned", content="variant-a"),
        ]))
        s2 = _snap("host-2", config=ConfigSection(files=[
            ConfigFileEntry(path="/etc/test.conf", kind="unowned", content="variant-b"),
        ]))
        merged = merge_snapshots([s1, s2], min_prevalence=0)

        with tempfile.TemporaryDirectory() as tmpdir:
            output_dir = Path(tmpdir)
            render_merge_notes(merged, output_dir)
            notes_path = output_dir / "merge-notes.md"
            assert notes_path.exists(), "merge-notes.md should be created"
            content = notes_path.read_text()
            assert "/etc/test.conf" in content
            assert "tie" in content.lower()
            assert "variant" in content.lower()

    def test_merge_notes_contains_non_unanimous_item(self):
        from yoinkc.fleet.merge import merge_snapshots
        from yoinkc.renderers.merge_notes import render_merge_notes

        # 2 hosts same, 1 different → clear winner at 2/3, non-unanimous
        s1 = _snap("host-1", config=ConfigSection(files=[
            ConfigFileEntry(path="/etc/test.conf", kind="unowned", content="majority"),
        ]))
        s2 = _snap("host-2", config=ConfigSection(files=[
            ConfigFileEntry(path="/etc/test.conf", kind="unowned", content="majority"),
        ]))
        s3 = _snap("host-3", config=ConfigSection(files=[
            ConfigFileEntry(path="/etc/test.conf", kind="unowned", content="minority"),
        ]))
        merged = merge_snapshots([s1, s2, s3], min_prevalence=0)

        with tempfile.TemporaryDirectory() as tmpdir:
            output_dir = Path(tmpdir)
            render_merge_notes(merged, output_dir)
            content = (output_dir / "merge-notes.md").read_text()
            assert "/etc/test.conf" in content
            assert "2/3" in content

    def test_no_merge_notes_when_all_unanimous(self):
        from yoinkc.fleet.merge import merge_snapshots
        from yoinkc.renderers.merge_notes import render_merge_notes

        s1 = _snap("host-1", config=ConfigSection(files=[
            ConfigFileEntry(path="/etc/test.conf", kind="unowned", content="same"),
        ]))
        s2 = _snap("host-2", config=ConfigSection(files=[
            ConfigFileEntry(path="/etc/test.conf", kind="unowned", content="same"),
        ]))
        merged = merge_snapshots([s1, s2], min_prevalence=0)

        with tempfile.TemporaryDirectory() as tmpdir:
            output_dir = Path(tmpdir)
            render_merge_notes(merged, output_dir)
            assert not (output_dir / "merge-notes.md").exists(), \
                "merge-notes.md should not be created when all items are unanimous"

    def test_merge_notes_absent_for_single_host(self):
        """Single-host snapshots have no fleet metadata → no merge notes."""
        from yoinkc.renderers.merge_notes import render_merge_notes

        snap = _snap("host-1", config=ConfigSection(files=[
            ConfigFileEntry(path="/etc/test.conf", kind="unowned", content="val"),
        ]))

        with tempfile.TemporaryDirectory() as tmpdir:
            output_dir = Path(tmpdir)
            render_merge_notes(snap, output_dir)
            assert not (output_dir / "merge-notes.md").exists()
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_fleet_merge.py::TestMergeNotes -v`

Expected: FAIL — `ModuleNotFoundError: No module named 'yoinkc.renderers.merge_notes'`

- [ ] **Step 3: Implement `render_merge_notes()`**

Create `src/yoinkc/renderers/merge_notes.py`:

```python
"""Render merge-notes.md — fleet merge ambiguity drill-down."""

import hashlib
from pathlib import Path
from typing import NamedTuple

from ..schema import InspectionSnapshot


class _VariantInfo(NamedTuple):
    path: str
    item_type: str
    fleet_count: int
    fleet_total: int
    content_hash: str
    is_winner: bool
    is_tie: bool


def _collect_variant_items(snapshot: InspectionSnapshot) -> list[_VariantInfo]:
    """Collect all non-unanimous variant items across all five item types."""
    items: list[_VariantInfo] = []

    def _add_items(item_list, item_type: str, content_fn):
        if not item_list:
            return
        # Group by path
        groups: dict[str, list] = {}
        for item in item_list:
            groups.setdefault(item.path, []).append(item)
        for path, variants in groups.items():
            if not any(v.fleet for v in variants):
                continue
            total = variants[0].fleet.total if variants[0].fleet else 0
            if total == 0:
                continue
            # Check if unanimous (single variant with full count)
            if len(variants) == 1 and variants[0].fleet.count == total:
                continue
            for v in variants:
                c_hash = hashlib.sha256(
                    content_fn(v).encode()
                ).hexdigest()[:16]
                items.append(_VariantInfo(
                    path=v.path,
                    item_type=item_type,
                    fleet_count=v.fleet.count if v.fleet else 0,
                    fleet_total=total,
                    content_hash=c_hash,
                    is_winner=v.include,
                    is_tie=getattr(v, "tie", False),
                ))

    if snapshot.config and snapshot.config.files:
        _add_items(snapshot.config.files, "config", lambda v: v.content)
    if snapshot.services and snapshot.services.drop_ins:
        _add_items(snapshot.services.drop_ins, "drop-in", lambda v: v.content)
    if snapshot.containers:
        if snapshot.containers.quadlet_units:
            _add_items(snapshot.containers.quadlet_units, "quadlet", lambda v: v.content)
        if snapshot.containers.compose_files:
            _add_items(
                snapshot.containers.compose_files, "compose",
                lambda v: str(sorted((img.service, img.image) for img in v.images)),
            )
    if snapshot.non_rpm_software and snapshot.non_rpm_software.env_files:
        _add_items(snapshot.non_rpm_software.env_files, "env", lambda v: v.content)

    return items


def render_merge_notes(snapshot: InspectionSnapshot, output_dir: Path) -> None:
    """Write merge-notes.md if there are non-unanimous or tied items."""
    items = _collect_variant_items(snapshot)
    if not items:
        return

    lines = [
        "# Fleet Merge Notes",
        "",
        "This file documents fleet merge decisions where hosts disagreed on file content.",
        "Review these items to verify the auto-selected variant is correct for your target image.",
        "",
    ]

    # Group by path for display
    tied: dict[str, list[_VariantInfo]] = {}
    non_unanimous: dict[str, list[_VariantInfo]] = {}

    for item in items:
        if item.is_tie:
            tied.setdefault(item.path, []).append(item)
        else:
            non_unanimous.setdefault(item.path, []).append(item)

    if tied:
        lines.append("## Tied Items (auto-resolved by content hash)")
        lines.append("")
        for path, variants in sorted(tied.items()):
            item_type = variants[0].item_type
            total = variants[0].fleet_total
            winner = next((v for v in variants if v.is_winner), None)
            lines.append(f"### `{path}` ({item_type})")
            lines.append("")
            lines.append(f"- **Variants:** {len(variants)}")
            lines.append(f"- **Fleet total:** {total} hosts")
            if winner:
                lines.append(f"- **Auto-selected:** hash `{winner.content_hash}` ({winner.fleet_count}/{total} hosts)")
            lines.append("")
            lines.append("| Variant hash | Hosts | Selected |")
            lines.append("|---|---|---|")
            for v in sorted(variants, key=lambda x: x.content_hash):
                selected = "**winner**" if v.is_winner else "—"
                lines.append(f"| `{v.content_hash}` | {v.fleet_count}/{v.fleet_total} | {selected} |")
            lines.append("")

    if non_unanimous:
        lines.append("## Non-Unanimous Items")
        lines.append("")
        lines.append("These items have a clear winner but were not present on all hosts.")
        lines.append("")
        for path, variants in sorted(non_unanimous.items()):
            item_type = variants[0].item_type
            total = variants[0].fleet_total
            winner = next((v for v in variants if v.is_winner), None)
            if winner:
                lines.append(f"- `{path}` ({item_type}): winner at {winner.fleet_count}/{total} hosts, "
                             f"{len(variants)} variant(s)")
            else:
                lines.append(f"- `{path}` ({item_type}): {len(variants)} variant(s), no winner selected")

    output_dir.mkdir(parents=True, exist_ok=True)
    (output_dir / "merge-notes.md").write_text("\n".join(lines) + "\n")
```

- [ ] **Step 4: Run tests**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_fleet_merge.py::TestMergeNotes -v`

Expected: PASS

- [ ] **Step 5: Wire into `run_all()` in renderers/__init__.py**

In `src/yoinkc/renderers/__init__.py`, add the import and call:

After the existing import of `write_redacted_dir` (or wherever imports are), add:
```python
from .merge_notes import render_merge_notes
```

In the `run_all()` function, add after `write_redacted_dir(snapshot, output_dir)` (line 48):
```python
    render_merge_notes(snapshot, output_dir)
```

- [ ] **Step 6: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/yoinkc
git add src/yoinkc/renderers/merge_notes.py src/yoinkc/renderers/__init__.py tests/test_fleet_merge.py
git commit -m "$(cat <<'EOF'
feat(renderers): add merge-notes.md for fleet ambiguity drill-down

New renderer produces merge-notes.md listing tied items (with variant
hash table) and non-unanimous items (with fleet ratios). Only created
for fleet merges with non-unanimous content. Covers config files,
drop-ins, quadlet units, compose files, and env files.

Assisted-by: Claude Code (Opus 4.6)
EOF
)"
```

---

### Task 6: Audit Report Disambiguation (`[TIE LOSER]` labels)

**Files:**
- Modify: `src/yoinkc/renderers/audit_report.py` (multiple `[EXCLUDED]` sites)
- Test: `tests/test_fleet_merge.py`

- [ ] **Step 1: Write failing test for audit report labels**

In `tests/test_fleet_merge.py`, add:

```python
class TestAuditReportDisambiguation:
    """Audit report distinguishes [EXCLUDED], [TIE LOSER], and [REDACTED]."""

    def test_tie_loser_labeled_in_audit_report(self):
        from yoinkc.fleet.merge import merge_snapshots
        from yoinkc.renderers.audit_report import render_audit_report
        from jinja2 import Environment, FileSystemLoader

        s1 = _snap("host-1", config=ConfigSection(files=[
            ConfigFileEntry(path="/etc/test.conf", kind="unowned", content="variant-a"),
        ]))
        s2 = _snap("host-2", config=ConfigSection(files=[
            ConfigFileEntry(path="/etc/test.conf", kind="unowned", content="variant-b"),
        ]))
        merged = merge_snapshots([s1, s2], min_prevalence=0)

        with tempfile.TemporaryDirectory() as tmpdir:
            output_dir = Path(tmpdir)
            templates_dir = Path(__file__).resolve().parent.parent / "src" / "yoinkc" / "templates"
            env = Environment(loader=FileSystemLoader(str(templates_dir)), autoescape=True)
            render_audit_report(merged, env, output_dir)
            content = (output_dir / "audit-report.md").read_text()

            assert "[TIE LOSER]" in content, "Tie losers should be labeled [TIE LOSER]"
            # The winner should NOT be labeled as excluded or tie loser
            # (it's included)

    def test_redacted_takes_precedence_over_tie(self):
        """A file that is both a tie winner and redacted should show [REDACTED]."""
        from yoinkc.fleet.merge import merge_snapshots
        from yoinkc.renderers.audit_report import render_audit_report
        from yoinkc.schema import RedactionFinding
        from jinja2 import Environment, FileSystemLoader

        s1 = _snap("host-1", config=ConfigSection(files=[
            ConfigFileEntry(path="/etc/secret.conf", kind="unowned", content="variant-a"),
        ]))
        s2 = _snap("host-2", config=ConfigSection(files=[
            ConfigFileEntry(path="/etc/secret.conf", kind="unowned", content="variant-b"),
        ]))
        merged = merge_snapshots([s1, s2], min_prevalence=0)

        # Add a redaction for the tied path
        merged.redactions.append(RedactionFinding(
            source="file", kind="excluded", path="/etc/secret.conf",
            reason="secret detected", remediation="provision",
        ))

        with tempfile.TemporaryDirectory() as tmpdir:
            output_dir = Path(tmpdir)
            templates_dir = Path(__file__).resolve().parent.parent / "src" / "yoinkc" / "templates"
            env = Environment(loader=FileSystemLoader(str(templates_dir)), autoescape=True)
            render_audit_report(merged, env, output_dir)
            content = (output_dir / "audit-report.md").read_text()

            # Redacted takes precedence — should not show [TIE LOSER]
            # for the excluded variant of a redacted file
            assert "[REDACTED]" in content or "[EXCLUDED]" in content  # existing behavior for redacted
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_fleet_merge.py::TestAuditReportDisambiguation -v`

Expected: FAIL — "[TIE LOSER]" not found in audit report.

- [ ] **Step 3: Add `_item_label()` helper to audit_report.py**

In `src/yoinkc/renderers/audit_report.py`, add a helper function near the top of the file (after imports):

```python
def _item_label(item, redacted_paths: set[str]) -> str:
    """Return the appropriate label prefix for an item.

    Precedence: [REDACTED] > [TIE LOSER] > [EXCLUDED] > ""
    """
    path = getattr(item, "path", "")
    if path in redacted_paths:
        return "[REDACTED] "
    if not item.include:
        if getattr(item, "tie", False) and not getattr(item, "tie_winner", False):
            return "[TIE LOSER] "
        return "[EXCLUDED] "
    return ""
```

Then build the `redacted_paths` set at the top of the main render function and pass it through. Replace each instance of:
```python
prefix = "[EXCLUDED] " if not X.include else ""
```
with:
```python
prefix = _item_label(X, redacted_paths)
```

for config files (line ~267), drop-ins (line ~243), quadlet units (line ~486), compose files (line ~494), and env files.

The `redacted_paths` set is built from:
```python
redacted_paths = {
    f.path for f in snapshot.redactions
    if hasattr(f, "source") and f.source == "file" and hasattr(f, "kind") and f.kind == "excluded"
}
```

Note: Not every `[EXCLUDED]` in the audit report is for variant items. Only replace the ones in sections that correspond to the five item types with tie flags. Leave package `[EXCLUDED]` labels unchanged.

- [ ] **Step 4: Run tests**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_fleet_merge.py::TestAuditReportDisambiguation -v`

Expected: PASS

- [ ] **Step 5: Run full test suite**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest -v`

Expected: PASS

- [ ] **Step 6: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/yoinkc
git add src/yoinkc/renderers/audit_report.py tests/test_fleet_merge.py
git commit -m "$(cat <<'EOF'
feat(audit-report): disambiguate [EXCLUDED] vs [TIE LOSER] vs [REDACTED]

Add _item_label() helper with precedence: redacted > tie loser >
excluded. Tie losers now show [TIE LOSER] instead of generic
[EXCLUDED] in the audit report. Redaction takes precedence for
files that are both redacted and tied.

Applies to config files, drop-ins, quadlet units, compose files,
and env files.

Assisted-by: Claude Code (Opus 4.6)
EOF
)"
```

---

### Task 7: CLI Output — Tie Summary Line

**Files:**
- Modify: `src/yoinkc/__main__.py:183-242` (_run_fleet)
- Test: `tests/test_fleet_merge.py`

- [ ] **Step 1: Write failing test for CLI tie summary**

In `tests/test_fleet_merge.py`, add:

```python
class TestCliTieSummary:
    """CLI prints tie summary when ties exist."""

    def test_tie_count_in_cli_output(self, capsys):
        from yoinkc.fleet.merge import merge_snapshots

        s1 = _snap("host-1", config=ConfigSection(files=[
            ConfigFileEntry(path="/etc/test.conf", kind="unowned", content="variant-a"),
        ]))
        s2 = _snap("host-2", config=ConfigSection(files=[
            ConfigFileEntry(path="/etc/test.conf", kind="unowned", content="variant-b"),
        ]))
        merged = merge_snapshots([s1, s2], min_prevalence=0)

        # Count ties the same way the CLI will
        tie_count = sum(
            1 for f in (merged.config.files or [])
            if getattr(f, "tie_winner", False)
        )
        assert tie_count == 1
```

- [ ] **Step 2: Implement tie summary in `_run_fleet()`**

In `src/yoinkc/__main__.py`, add a helper function:

```python
def _count_tied_winners(snapshot) -> int:
    """Count items with tie_winner=True across all variant-bearing sections."""
    count = 0
    if snapshot.config and snapshot.config.files:
        count += sum(1 for f in snapshot.config.files if getattr(f, "tie_winner", False))
    if snapshot.services and snapshot.services.drop_ins:
        count += sum(1 for d in snapshot.services.drop_ins if getattr(d, "tie_winner", False))
    if snapshot.containers:
        if snapshot.containers.quadlet_units:
            count += sum(1 for q in snapshot.containers.quadlet_units if getattr(q, "tie_winner", False))
        if snapshot.containers.compose_files:
            count += sum(1 for c in snapshot.containers.compose_files if getattr(c, "tie_winner", False))
    if snapshot.non_rpm_software and snapshot.non_rpm_software.env_files:
        count += sum(1 for f in snapshot.non_rpm_software.env_files if getattr(f, "tie_winner", False))
    return count
```

Then in `_run_fleet()`, after the existing print on line 232 (`Merged N hosts (threshold X%)`), add:

```python
    tie_count = _count_tied_winners(merged)
    if tie_count:
        s = "s" if tie_count != 1 else ""
        print(f"  {tie_count} item{s} with tied variants (auto-resolved by content hash)")
```

And similarly after the `--json-only` print on line 225, add the same logic.

- [ ] **Step 3: Run test**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_fleet_merge.py::TestCliTieSummary -v`

Expected: PASS

- [ ] **Step 4: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/yoinkc
git add src/yoinkc/__main__.py tests/test_fleet_merge.py
git commit -m "$(cat <<'EOF'
feat(cli): print tie summary after fleet merge

When ties exist, print a one-line summary after the merge status:
"N items with tied variants (auto-resolved by content hash)".
Zero ties = zero additional output.

Assisted-by: Claude Code (Opus 4.6)
EOF
)"
```

---

### Task 8: Readme Artifacts Table — merge-notes.md Row

**Files:**
- Modify: `src/yoinkc/renderers/readme.py:68-89` (artifacts table)
- Test: `tests/test_fleet_merge.py`

- [ ] **Step 1: Write failing test**

In `tests/test_fleet_merge.py`, add:

```python
class TestReadmeArtifacts:
    """merge-notes.md appears in readme artifacts for fleet merges."""

    def test_merge_notes_in_readme_for_fleet(self):
        from yoinkc.fleet.merge import merge_snapshots
        from yoinkc.renderers.readme import render_readme
        from jinja2 import Environment, FileSystemLoader

        s1 = _snap("host-1", config=ConfigSection(files=[
            ConfigFileEntry(path="/etc/test.conf", kind="unowned", content="variant-a"),
        ]))
        s2 = _snap("host-2", config=ConfigSection(files=[
            ConfigFileEntry(path="/etc/test.conf", kind="unowned", content="variant-b"),
        ]))
        merged = merge_snapshots([s1, s2], min_prevalence=0)

        with tempfile.TemporaryDirectory() as tmpdir:
            output_dir = Path(tmpdir)
            templates_dir = Path(__file__).resolve().parent.parent / "src" / "yoinkc" / "templates"
            env = Environment(loader=FileSystemLoader(str(templates_dir)), autoescape=True)
            render_readme(merged, env, output_dir)
            content = (output_dir / "README.md").read_text()
            assert "merge-notes.md" in content
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_fleet_merge.py::TestReadmeArtifacts -v`

Expected: FAIL — "merge-notes.md" not in README.

- [ ] **Step 3: Add merge-notes row to readme artifacts**

In `src/yoinkc/renderers/readme.py`, after the warnings row (around line 87), add a conditional row for fleet merges:

```python
    # Merge notes (fleet merges with non-unanimous items only)
    has_fleet = snapshot.fleet_metadata is not None
    if has_fleet:
        has_ties = False
        for section_items in [
            snapshot.config.files if snapshot.config else [],
            snapshot.services.drop_ins if snapshot.services else [],
            snapshot.containers.quadlet_units if snapshot.containers else [],
            snapshot.containers.compose_files if snapshot.containers else [],
            snapshot.non_rpm_software.env_files if snapshot.non_rpm_software else [],
        ]:
            if any(getattr(item, "tie", False) for item in section_items):
                has_ties = True
                break
        if has_ties:
            lines.append("| `merge-notes.md` | Fleet merge decisions — ties, non-unanimous items |")
```

Note: Check whether `snapshot.fleet_metadata` exists in the schema. If the fleet detection uses a different signal (like checking if any item has `fleet` set), adjust accordingly.

- [ ] **Step 4: Run test**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_fleet_merge.py::TestReadmeArtifacts -v`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/yoinkc
git add src/yoinkc/renderers/readme.py tests/test_fleet_merge.py
git commit -m "$(cat <<'EOF'
feat(readme): add merge-notes.md to artifacts table for fleet merges

Show merge-notes.md row in the README artifacts table when the
snapshot contains tied items from fleet merge.

Assisted-by: Claude Code (Opus 4.6)
EOF
)"
```

---

### Task 9: HTML Report — Tie Badges and Homogeneity Labels

**Files:**
- Modify: `src/yoinkc/renderers/html_report.py:688-725` (tie detection)
- Modify: `src/yoinkc/templates/report/_config.html.j2:16-18` (is_tied logic)
- Modify: `src/yoinkc/templates/report/_summary.html.j2:47-53` (tie count display)
- Modify: `src/yoinkc/templates/report/_services.html.j2` (drop-in variant logic)
- Modify: `src/yoinkc/templates/report/_containers.html.j2` (quadlet/compose variant logic)
- Modify: `src/yoinkc/templates/report/_js.html.j2` (client-side tie state)
- Modify: `src/yoinkc/templates/report/_editor_js.html.j2` (editor tie state)

This is the largest task. The core change: replace `is_tied = (not group_has_selected)` inference with explicit `tie`/`tie_winner` flag checks.

- [ ] **Step 1: Update `html_report.py` — switch tie detection to flag-based**

In `src/yoinkc/renderers/html_report.py`, find the section where `unresolved_ties` is computed (around lines 705-725). The current logic checks for variant groups where no item has `include=True`. Replace with flag-based detection:

```python
    # Count tie groups (groups where any variant has tie=True)
    auto_resolved_ties = 0
    # ... within the variant group loop:
    # Replace: is_tied = (not group_has_selected) and ...
    # With: Check if any variant in the group has tie=True
```

The renderer passes `unresolved_ties` to the template. After this change, pass:
- `auto_resolved_ties` — count of groups where `tie=True` and a `tie_winner=True` exists
- `unresolved_ties` — count of groups where `tie=True` but no variant has `include=True` (user deselected the auto-pick without choosing a replacement)

- [ ] **Step 2: Update `_config.html.j2` — use tie flags**

Replace line 18:
```jinja2
{%- set is_tied = (not group_has_selected) and (variants | length >= 2) and sorted_variants[0].item.fleet and sorted_variants[1].item.fleet and (sorted_variants[0].item.fleet.count == sorted_variants[1].item.fleet.count) %}
```

With:
```jinja2
{%- set is_tied = variants | selectattr('item.tie', 'equalto', true) | list | length > 0 %}
{%- set tie_winner = variants | selectattr('item.tie_winner', 'equalto', true) | first | default(none) %}
```

Update the badge rendering to show "tie winner (hash)" when `tie_winner` is not none, and style the badge with the homogeneity-based color.

For non-unanimous items (not tied, but fleet.count < fleet.total), add a fleet ratio label:
```jinja2
{%- if variant.item.fleet and variant.item.fleet.count < variant.item.fleet.total and not variant.item.tie %}
  <span class="badge badge-info">{{ variant.item.fleet.count }}/{{ variant.item.fleet.total }}</span>
{%- endif %}
```

- [ ] **Step 3: Update `_summary.html.j2` — show auto-resolved tie count**

Replace the `unresolved_ties` display with separate counts:
```jinja2
{% if auto_resolved_ties %}
  <li>{{ auto_resolved_ties }} tied item(s) (auto-resolved by content hash)</li>
{% endif %}
{% if unresolved_ties %}
  <li class="priority-manual">{{ unresolved_ties }} unresolved tie(s) — review recommended</li>
{% endif %}
```

- [ ] **Step 4: Update `_services.html.j2` and `_containers.html.j2`**

Apply the same `tie`/`tie_winner` flag pattern used in `_config.html.j2` to the variant group rendering in these templates. The exact code depends on how these templates currently render variants — follow the same pattern.

- [ ] **Step 5: Update `_js.html.j2` and `_editor_js.html.j2` — client-side parity**

Add `data-tie` and `data-tie-winner` attributes to variant elements rendered in Jinja, then update JS tie-state checks to use these attributes instead of inferring from include state. This ensures interactive variant toggling and tie badge rendering stay in sync.

In the JS, replace any logic that checks for "no selected variant in group" as a tie indicator with checks for the `data-tie` attribute.

- [ ] **Step 6: Run full test suite**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest -v`

Expected: PASS. If HTML report tests exist, they should pass with the updated tie logic.

- [ ] **Step 7: Manual smoke test**

If a test fleet snapshot is available, run `yoinkc fleet` against it and open the generated `report.html` to verify:
- Tied items show "tie winner (hash)" badge
- Non-unanimous items show fleet ratio labels
- Unanimous items have no badge
- Summary panel shows auto-resolved tie count

- [ ] **Step 8: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/yoinkc
git add src/yoinkc/renderers/html_report.py src/yoinkc/templates/report/
git commit -m "$(cat <<'EOF'
feat(report): flag-based tie detection and homogeneity labels

Replace include-inference tie detection with explicit tie/tie_winner
flag checks across all report templates. Add fleet ratio labels for
non-unanimous items and "tie winner (hash)" badges for auto-resolved
ties.

Updated files: html_report.py, _config.html.j2, _summary.html.j2,
_services.html.j2, _containers.html.j2, _js.html.j2, _editor_js.html.j2

Assisted-by: Claude Code (Opus 4.6)
EOF
)"
```

---

### Task 10: Refine UI — "(auto-selected: tied)" Label

**Files:**
- Modify: `src/yoinkc/templates/report/_config.html.j2` (variant display in refine mode)
- Modify: `src/yoinkc/templates/report/_editor_js.html.j2` (comparison view default)

This builds on Task 9's template changes. The refine UI reuses the same templates in `refine_mode=True`.

- [ ] **Step 1: Add auto-selected label to variant display**

In the variant rendering section of `_config.html.j2` (and equivalents for services/containers), where the selected variant is displayed, add:

```jinja2
{%- if variant.item.tie_winner %}
  <span class="badge badge-warning">(auto-selected: tied)</span>
{%- endif %}
```

This label appears only on the auto-picked winner, distinguishing it from fleet consensus.

- [ ] **Step 2: Set comparison view default for tied variants**

In `_editor_js.html.j2`, when a variant group has `data-tie="true"`, default the comparison view to show the tie winner diffed against the next tied variant (first non-winner with `data-tie="true"`). This gives the user the most useful comparison immediately.

- [ ] **Step 3: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/yoinkc
git add src/yoinkc/templates/report/
git commit -m "$(cat <<'EOF'
feat(refine): add (auto-selected: tied) label and comparison default

Show "(auto-selected: tied)" badge on auto-picked tie winners in
refine mode. Default comparison view to winner vs next tied variant
for immediate tie resolution.

Assisted-by: Claude Code (Opus 4.6)
EOF
)"
```

---

### Task 11: Integration Test — Full Round-Trip

**Files:**
- Test: `tests/test_fleet_merge.py`

- [ ] **Step 1: Write round-trip integration test**

```python
class TestTieRoundTrip:
    """Full pipeline: merge with ties → render → verify all surfaces."""

    def test_full_render_with_ties(self):
        from yoinkc.fleet.merge import merge_snapshots
        from yoinkc.renderers import run_all
        from jinja2 import Environment, FileSystemLoader

        s1 = _snap("host-1", config=ConfigSection(files=[
            ConfigFileEntry(path="/etc/app.conf", kind="unowned", content="setting=alpha"),
        ]))
        s2 = _snap("host-2", config=ConfigSection(files=[
            ConfigFileEntry(path="/etc/app.conf", kind="unowned", content="setting=beta"),
        ]))
        merged = merge_snapshots([s1, s2], min_prevalence=0)

        with tempfile.TemporaryDirectory() as tmpdir:
            output_dir = Path(tmpdir)
            run_all(merged, output_dir)

            # Containerfile has tie comment
            containerfile = (output_dir / "Containerfile").read_text()
            assert "Tied" in containerfile
            assert "etc/app.conf" in containerfile

            # merge-notes.md exists with tie details
            merge_notes = (output_dir / "merge-notes.md").read_text()
            assert "/etc/app.conf" in merge_notes
            assert "tie" in merge_notes.lower()

            # Config file is present in output (winner was included)
            config_file = output_dir / "config" / "etc" / "app.conf"
            assert config_file.exists(), "Tie winner should be written to config/"

            # audit-report.md has [TIE LOSER]
            audit = (output_dir / "audit-report.md").read_text()
            assert "[TIE LOSER]" in audit
```

- [ ] **Step 2: Run integration test**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_fleet_merge.py::TestTieRoundTrip -v`

Expected: PASS

- [ ] **Step 3: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/yoinkc
git add tests/test_fleet_merge.py
git commit -m "$(cat <<'EOF'
test(fleet): add round-trip integration test for tied variants

Verify the full pipeline: merge with ties → render all surfaces →
check Containerfile comment, merge-notes.md, config/ output, and
audit report [TIE LOSER] labels.

Assisted-by: Claude Code (Opus 4.6)
EOF
)"
```

---

### Task 12: Final Full Suite Run and Cleanup

- [ ] **Step 1: Run the complete test suite**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest -v`

Expected: All tests PASS.

- [ ] **Step 2: Run a quick manual smoke test if fleet test data is available**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m yoinkc fleet <test-fleet-dir>` (if test data exists)

Verify:
- CLI prints tie summary
- Output tarball contains merge-notes.md
- Containerfile has tied items comment block
- report.html shows tie badges

- [ ] **Step 3: Final commit if any cleanup was needed**

Only if Step 1 or 2 revealed issues that needed fixing.
