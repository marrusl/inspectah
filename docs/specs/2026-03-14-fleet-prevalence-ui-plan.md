# Fleet Prevalence UI Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Surface fleet prevalence metadata (counts, host lists, content variants) in the yoinkc HTML report when rendering fleet-merged snapshots.

**Architecture:** The renderer pre-processes fleet data (`FleetMeta` from `meta["fleet"]`, `FleetPrevalence` on items) and passes enriched context to the Jinja2 template. A `fleet_color` Jinja2 filter computes PF6 color classes. The template conditionally renders a fleet banner, prevalence bars on item rows, and grouped variant rows — all gated on `fleet_meta` being non-None. JavaScript handles fraction/percentage toggling and host list popovers.

**Tech Stack:** Python 3 (Pydantic, Jinja2), HTML/CSS (PatternFly 6), vanilla JavaScript

**Spec:** `docs/specs/2026-03-14-fleet-prevalence-ui-design.md`

---

## Chunk 1: Foundation (Filter, Banner, Config Passthrough)

### Task 1: Fleet Color Filter

**Files:**
- Modify: `src/yoinkc/renderers/html_report.py` (add `_fleet_color` function, register as Jinja2 filter)
- Test: `tests/test_renderer_outputs.py`

- [ ] **Step 1: Write the failing test for fleet_color filter**

Add a new test class `TestFleetColor` to `tests/test_renderer_outputs.py`:

```python
from yoinkc.schema import FleetPrevalence

class TestFleetColor:
    """Tests for the _fleet_color Jinja2 filter."""

    def test_full_prevalence_returns_blue(self):
        from yoinkc.renderers.html_report import _fleet_color
        fleet = FleetPrevalence(count=3, total=3)
        assert _fleet_color(fleet) == "pf-m-blue"

    def test_majority_prevalence_returns_gold(self):
        from yoinkc.renderers.html_report import _fleet_color
        fleet = FleetPrevalence(count=2, total=3)
        assert _fleet_color(fleet) == "pf-m-gold"

    def test_fifty_percent_returns_gold(self):
        from yoinkc.renderers.html_report import _fleet_color
        fleet = FleetPrevalence(count=50, total=100)
        assert _fleet_color(fleet) == "pf-m-gold"

    def test_minority_prevalence_returns_red(self):
        from yoinkc.renderers.html_report import _fleet_color
        fleet = FleetPrevalence(count=1, total=3)
        assert _fleet_color(fleet) == "pf-m-red"

    def test_none_returns_blue(self):
        from yoinkc.renderers.html_report import _fleet_color
        assert _fleet_color(None) == "pf-m-blue"

    def test_zero_total_returns_blue(self):
        from yoinkc.renderers.html_report import _fleet_color
        fleet = FleetPrevalence(count=0, total=0)
        assert _fleet_color(fleet) == "pf-m-blue"
```

- [ ] **Step 2: Run test to verify it fails**

Run: `python -m pytest tests/test_renderer_outputs.py::TestFleetColor -v`
Expected: FAIL with `ImportError` (function does not exist yet)

- [ ] **Step 3: Implement `_fleet_color` and register as Jinja2 filter**

In `src/yoinkc/renderers/html_report.py`, add near the top (after imports):

```python
def _fleet_color(fleet) -> str:
    """Jinja2 filter: return PF6 color class based on fleet prevalence."""
    if not fleet or fleet.total == 0:
        return "pf-m-blue"
    pct = fleet.count * 100 // fleet.total
    if pct >= 100:
        return "pf-m-blue"
    elif pct >= 50:
        return "pf-m-gold"
    else:
        return "pf-m-red"
```

In the `render()` function, register the filter **after** the `env.overlay()` block (which conditionally creates a new environment when `env.loader is None`), and **before** `ctx = _build_context(...)`:

```python
env.filters["fleet_color"] = _fleet_color
```

- [ ] **Step 4: Run test to verify it passes**

Run: `python -m pytest tests/test_renderer_outputs.py::TestFleetColor -v`
Expected: PASS (all 6 tests)

- [ ] **Step 5: Commit**

```
feat(html): add fleet_color Jinja2 filter for prevalence badges

Compute PF6 color class from FleetPrevalence: blue (100%),
gold (50-99%), red (<50%). Guards against None and zero total.

Assisted-by: Claude Code
```

---

### Task 2: Fleet Banner (Context + Template)

**Files:**
- Modify: `src/yoinkc/renderers/html_report.py` (`_build_context` adds `fleet_meta`)
- Modify: `src/yoinkc/templates/report.html.j2` (add fleet banner)
- Test: `tests/test_renderer_outputs.py`

- [ ] **Step 1: Write the failing tests for fleet banner**

Add to `tests/test_renderer_outputs.py`:

```python
class TestFleetBanner:
    """Tests for fleet banner rendering."""

    def _render_with_fleet_meta(self, tmp_path):
        """Render a snapshot with fleet metadata and return HTML."""
        snap = InspectionSnapshot(
            schema_version=1,
            os_release=OsRelease(
                name="Red Hat Enterprise Linux", version_id="9.4",
                id="rhel", platform_id="platform:el9",
            ),
            meta={
                "fleet": {
                    "source_hosts": ["web-01", "web-02", "web-03"],
                    "total_hosts": 3,
                    "min_prevalence": 67,
                }
            },
            rpm=RpmSection(
                packages_added=[
                    PackageEntry(name="httpd", version="2.4.57", release="1.el9", arch="x86_64",
                                 fleet=FleetPrevalence(count=3, total=3, hosts=["web-01", "web-02", "web-03"])),
                ],
            ),
        )
        env = Environment(autoescape=True)
        html_report.render(snap, env, tmp_path)
        return (tmp_path / "report.html").read_text()

    def test_fleet_banner_present(self, tmp_path):
        html = self._render_with_fleet_meta(tmp_path)
        assert "Fleet Analysis" in html
        assert "3 hosts merged" in html
        assert "67%" in html
        assert "web-01" in html
        assert "web-02" in html
        assert "web-03" in html
        assert "included" in html  # include/exclude summary

    def test_fleet_banner_absent(self, tmp_path):
        snap = InspectionSnapshot(
            schema_version=1,
            os_release=OsRelease(
                name="Red Hat Enterprise Linux", version_id="9.4",
                id="rhel", platform_id="platform:el9",
            ),
            rpm=RpmSection(
                packages_added=[
                    PackageEntry(name="httpd", version="2.4.57", release="1.el9", arch="x86_64"),
                ],
            ),
        )
        env = Environment(autoescape=True)
        html_report.render(snap, env, tmp_path)
        html = (tmp_path / "report.html").read_text()
        assert "Fleet Analysis" not in html
```

- [ ] **Step 2: Run test to verify it fails**

Run: `python -m pytest tests/test_renderer_outputs.py::TestFleetBanner -v`
Expected: FAIL (`fleet_meta` not in context, or banner HTML not in output)

- [ ] **Step 3: Add `fleet_meta` to `_build_context()`**

In `src/yoinkc/renderers/html_report.py`, in `_build_context()`:

```python
from yoinkc.schema import FleetMeta

# Near the top of _build_context, after snapshot is available:
fleet_raw = (snapshot.meta or {}).get("fleet")
fleet_meta = FleetMeta(**fleet_raw) if fleet_raw else None

# Add to the return dict:
"fleet_meta": fleet_meta,
```

- [ ] **Step 4: Add fleet banner to template**

In `src/yoinkc/templates/report.html.j2`, after the OS header section and before the first content section, add:

```html
{% if fleet_meta %}
<div class="pf-v6-c-card pf-m-compact fleet-banner" style="margin-bottom: 16px;">
  <div class="pf-v6-c-card__body">
    <div class="text-bold" style="font-size: 1.1em; margin-bottom: 4px;">Fleet Analysis</div>
    <div>{{ fleet_meta.total_hosts }} hosts merged at {{ fleet_meta.min_prevalence }}% prevalence threshold</div>
    <div style="color: var(--pf-v6-global--Color--200);">{{ fleet_meta.source_hosts | join(', ') }}</div>
    <div style="margin-top: 4px;">{{ counts.n_included }} items included, {{ counts.n_excluded }} excluded</div>
  </div>
</div>
{% endif %}
```

Note: `counts` is already in the template context from `_summary_counts()`. If `n_included`/`n_excluded` keys don't exist, compute them in `_build_context` from the existing counts dict and add them. Check what keys `_summary_counts()` actually returns and adapt accordingly — the goal is to show the total included vs excluded item counts across all sections.

- [ ] **Step 5: Run test to verify it passes**

Run: `python -m pytest tests/test_renderer_outputs.py::TestFleetBanner -v`
Expected: PASS (both tests)

- [ ] **Step 6: Run full test suite**

Run: `python -m pytest -x -q`
Expected: All tests pass, no regressions

- [ ] **Step 7: Commit**

```
feat(html): add fleet detection and summary banner

Parse FleetMeta from meta["fleet"] in _build_context. Render a PF6
card with host count, threshold, hostnames, and include/exclude
summary. Gated on fleet_meta presence — no change for non-fleet
reports.

Assisted-by: Claude Code
```

---

### Task 3: Config Files Fleet Passthrough

**Files:**
- Modify: `src/yoinkc/renderers/html_report.py` (`_prepare_config_files` passes `fleet`)
- Test: `tests/test_renderer_outputs.py`

- [ ] **Step 1: Write the failing test**

Add to `tests/test_renderer_outputs.py`:

```python
from yoinkc.schema import ConfigFileKind

class TestFleetConfigPassthrough:
    """Test that _prepare_config_files preserves fleet data."""

    def test_fleet_field_preserved(self):
        from yoinkc.renderers.html_report import _prepare_config_files
        fleet = FleetPrevalence(count=2, total=3, hosts=["web-01", "web-02"])
        snap = InspectionSnapshot(
            schema_version=1,
            os_release=OsRelease(
                name="Red Hat Enterprise Linux", version_id="9.4",
                id="rhel", platform_id="platform:el9",
            ),
            config=ConfigSection(
                files=[
                    ConfigFileEntry(
                        path="/etc/httpd/conf/httpd.conf",
                        kind=ConfigFileKind.UNOWNED,
                        content="ServerName localhost",
                        include=True,
                        fleet=fleet,
                    ),
                ],
            ),
        )
        result = _prepare_config_files(snap)
        assert len(result) == 1
        assert result[0]["fleet"] is not None
        assert result[0]["fleet"].count == 2
        assert result[0]["fleet"].total == 3

    def test_fleet_field_none_when_absent(self):
        from yoinkc.renderers.html_report import _prepare_config_files
        snap = InspectionSnapshot(
            schema_version=1,
            os_release=OsRelease(
                name="Red Hat Enterprise Linux", version_id="9.4",
                id="rhel", platform_id="platform:el9",
            ),
            config=ConfigSection(
                files=[
                    ConfigFileEntry(
                        path="/etc/httpd/conf/httpd.conf",
                        kind=ConfigFileKind.UNOWNED,
                        content="ServerName localhost",
                        include=True,
                    ),
                ],
            ),
        )
        result = _prepare_config_files(snap)
        assert len(result) == 1
        assert result[0]["fleet"] is None
```

- [ ] **Step 2: Run test to verify it fails**

Run: `python -m pytest tests/test_renderer_outputs.py::TestFleetConfigPassthrough -v`
Expected: FAIL (`"fleet"` key not in dict)

- [ ] **Step 3: Add `fleet` to `_prepare_config_files` output**

In `src/yoinkc/renderers/html_report.py`, in `_prepare_config_files()`, add `"fleet": entry.fleet` to the dict that is appended for each config file entry. Find the dict construction and add the key alongside the existing keys (`path`, `kind`, `flags`, `diff_html`, `snap_index`, `include`).

- [ ] **Step 4: Run test to verify it passes**

Run: `python -m pytest tests/test_renderer_outputs.py::TestFleetConfigPassthrough -v`
Expected: PASS (both tests)

- [ ] **Step 5: Run full test suite**

Run: `python -m pytest -x -q`
Expected: All tests pass

- [ ] **Step 6: Commit**

```
feat(html): pass fleet data through _prepare_config_files

Add "fleet" key to the output dicts so prevalence badges can render
on config file rows. Preserves None when no fleet data is present.

Assisted-by: Claude Code
```

---

## Chunk 2: Prevalence Badges (Template + CSS + JavaScript)

**Prerequisite:** Chunk 1 must be committed before starting Chunk 2. Task 4 depends on Task 3's fleet passthrough for config files.

### Task 4: Prevalence Badges in Template

**Files:**
- Modify: `src/yoinkc/templates/report.html.j2` (add Fleet column to all item tables)
- Test: `tests/test_renderer_outputs.py`

- [ ] **Step 1: Write the failing tests**

Add to `tests/test_renderer_outputs.py`:

```python
class TestFleetPrevalenceBadge:
    """Tests for fleet prevalence badges on item rows."""

    def _render_fleet_snapshot(self, tmp_path):
        """Render a fleet snapshot with prevalence data on items."""
        snap = InspectionSnapshot(
            schema_version=1,
            os_release=OsRelease(
                name="Red Hat Enterprise Linux", version_id="9.4",
                id="rhel", platform_id="platform:el9",
            ),
            meta={
                "fleet": {
                    "source_hosts": ["web-01", "web-02", "web-03"],
                    "total_hosts": 3,
                    "min_prevalence": 67,
                }
            },
            rpm=RpmSection(
                packages_added=[
                    PackageEntry(name="httpd", version="2.4.57", release="1.el9", arch="x86_64",
                                 fleet=FleetPrevalence(count=3, total=3, hosts=["web-01", "web-02", "web-03"])),
                    PackageEntry(name="debug-tools", version="1.0", release="1.el9", arch="x86_64",
                                 include=False,
                                 fleet=FleetPrevalence(count=1, total=3, hosts=["web-03"])),
                ],
            ),
        )
        env = Environment(autoescape=True)
        html_report.render(snap, env, tmp_path)
        return (tmp_path / "report.html").read_text()

    def test_prevalence_bar_present(self, tmp_path):
        html = self._render_fleet_snapshot(tmp_path)
        assert "fleet-bar" in html
        assert 'data-count="3"' in html
        assert 'data-total="3"' in html

    def test_prevalence_bar_absent_without_fleet(self, tmp_path):
        snap = InspectionSnapshot(
            schema_version=1,
            os_release=OsRelease(
                name="Red Hat Enterprise Linux", version_id="9.4",
                id="rhel", platform_id="platform:el9",
            ),
            rpm=RpmSection(
                packages_added=[
                    PackageEntry(name="httpd", version="2.4.57", release="1.el9", arch="x86_64"),
                ],
            ),
        )
        env = Environment(autoescape=True)
        html_report.render(snap, env, tmp_path)
        html = (tmp_path / "report.html").read_text()
        assert "fleet-bar" not in html
        assert "fleet-prevalence" not in html

    def test_color_class_applied(self, tmp_path):
        html = self._render_fleet_snapshot(tmp_path)
        assert "pf-m-blue" in html   # 3/3 = 100%
        assert "pf-m-red" in html    # 1/3 = 33%

    def test_hosts_in_data_attribute(self, tmp_path):
        html = self._render_fleet_snapshot(tmp_path)
        assert "web-01, web-02, web-03" in html

    def test_empty_hosts_renders_empty_data_attr(self, tmp_path):
        snap = InspectionSnapshot(
            schema_version=1,
            os_release=OsRelease(
                name="Red Hat Enterprise Linux", version_id="9.4",
                id="rhel", platform_id="platform:el9",
            ),
            meta={
                "fleet": {
                    "source_hosts": ["web-01"],
                    "total_hosts": 1,
                    "min_prevalence": 100,
                }
            },
            rpm=RpmSection(
                packages_added=[
                    PackageEntry(name="httpd", version="2.4.57", release="1.el9", arch="x86_64",
                                 fleet=FleetPrevalence(count=1, total=1, hosts=[])),
                ],
            ),
        )
        env = Environment(autoescape=True)
        html_report.render(snap, env, tmp_path)
        html = (tmp_path / "report.html").read_text()
        assert 'data-hosts=""' in html
```

- [ ] **Step 2: Run test to verify it fails**

Run: `python -m pytest tests/test_renderer_outputs.py::TestFleetPrevalenceBadge -v`
Expected: FAIL (no `fleet-bar` markup in HTML)

- [ ] **Step 3: Add prevalence badges to template**

In `src/yoinkc/templates/report.html.j2`, for each section's table, add a conditional `<th>Fleet</th>` header and a prevalence cell to each item row. Use a Jinja2 macro to avoid repeating the badge markup across all sections:

Define a macro near the top of the template (after CSS, before content):

```html
{% macro fleet_cell(item) %}
{% if fleet_meta %}
<td class="fleet-prevalence">
  {% if item.fleet %}
  <div class="fleet-bar"
       data-count="{{ item.fleet.count }}"
       data-total="{{ item.fleet.total }}"
       data-hosts="{{ item.fleet.hosts | join(', ') }}">
    <div class="fleet-bar-track">
      <div class="fleet-bar-fill {{ item.fleet | fleet_color }}"
           style="width: {{ (item.fleet.count * 100 // item.fleet.total) if item.fleet.total > 0 else 100 }}%"></div>
    </div>
    <span class="fleet-bar-label">{{ item.fleet.count }}/{{ item.fleet.total }}</span>
  </div>
  {% endif %}
</td>
{% endif %}
{% endmacro %}
```

Then in each section's table:
- Add `{% if fleet_meta %}<th>Fleet</th>{% endif %}` to the header row
- Add `{{ fleet_cell(item) }}` to each item row (use `item` variable name appropriate to the loop — e.g., `pkg`, `f`, `sc`, `u`, `c`, etc.)

For **compose files** (card layout, not table rows), render the prevalence bar inside the card header div instead of as a table cell:

```html
{% if fleet_meta and c.fleet %}
<div class="fleet-prevalence" style="display: inline-block; margin-left: 8px;">
  <div class="fleet-bar"
       data-count="{{ c.fleet.count }}"
       data-total="{{ c.fleet.total }}"
       data-hosts="{{ c.fleet.hosts | join(', ') }}">
    <div class="fleet-bar-track">
      <div class="fleet-bar-fill {{ c.fleet | fleet_color }}"
           style="width: {{ (c.fleet.count * 100 // c.fleet.total) if c.fleet.total > 0 else 100 }}%"></div>
    </div>
    <span class="fleet-bar-label">{{ c.fleet.count }}/{{ c.fleet.total }}</span>
  </div>
</div>
{% endif %}
```

**All 12 insertion points** (covering the spec's 11 sections — packages_added has two loops):
1. RPM packages_added (leaf package tree view) — add to each `<tr>` in the leaf package loop
2. RPM packages_added (unclassified table) — add to each `<tr>`
3. RPM base_image_only — add to each `<tr>`
4. Repo files — add to each `<tr>`
5. Config files — add to each `<tr>` in `config_files_rendered` loop
6. Services state_changes — add to each `<tr>`
7. Services drop_ins — add to each `<tr>`
8. Firewall zones — add to each `<tr>`
9. Scheduled tasks generated_timer_units — add to each `<tr>`
10. Scheduled tasks cron_jobs — add to each `<tr>`
11. Quadlet units — add to each `<tr>`
12. Compose files — add to each card div (inline, not table cell)

- [ ] **Step 4: Run test to verify it passes**

Run: `python -m pytest tests/test_renderer_outputs.py::TestFleetPrevalenceBadge -v`
Expected: PASS (all 5 tests)

- [ ] **Step 5: Run full test suite**

Run: `python -m pytest -x -q`
Expected: All tests pass

- [ ] **Step 6: Commit**

```
feat(html): add fleet prevalence badges to all item sections

Surface per-item fleet prevalence as visual bars with color-coded
fill on all 12 insertion points (11 spec sections). Uses a Jinja2
macro to keep badge markup DRY.

Assisted-by: Claude Code
```

---

### Task 5: Fleet CSS Styles

**Files:**
- Modify: `src/yoinkc/templates/report.html.j2` (add CSS in the `<style>` block)

- [ ] **Step 1: Add fleet CSS to the template's `<style>` block**

Add scoped styles for fleet prevalence. Uses a nested-div approach where `.fleet-bar-track` is the fixed-width background bar and `.fleet-bar-fill` is the colored fill inside it:

```css
/* Fleet prevalence badges */
.fleet-prevalence {
  width: 90px;
  min-width: 90px;
}
.fleet-bar {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  cursor: pointer;
}
.fleet-bar-track {
  width: 60px;
  height: 6px;
  background: #e0e0e0;
  border-radius: 3px;
  overflow: hidden;
}
.fleet-bar-fill {
  height: 100%;
  border-radius: 3px;
}
.fleet-bar-fill.pf-m-blue {
  background: var(--pf-v6-global--palette--blue-300, #0066cc);
}
.fleet-bar-fill.pf-m-gold {
  background: var(--pf-v6-global--palette--gold-400, #f0ab00);
}
.fleet-bar-fill.pf-m-red {
  background: var(--pf-v6-global--palette--red-100, #c9190b);
}
.fleet-bar-label {
  font-size: 11px;
  color: var(--pf-v6-global--Color--200, #6a6e73);
  white-space: nowrap;
  cursor: pointer;
  user-select: none;
}
.fleet-banner {
  border-left: 4px solid var(--pf-v6-global--palette--blue-300, #0066cc);
}
/* Fleet popover */
.fleet-popover {
  position: absolute;
  z-index: 1000;
  background: var(--pf-v6-global--BackgroundColor--100, #fff);
  border: 1px solid var(--pf-v6-global--BorderColor--100, #d2d2d2);
  border-radius: 4px;
  padding: 8px 12px;
  box-shadow: 0 4px 8px rgba(0,0,0,0.1);
  font-size: 13px;
  max-width: 300px;
}
.fleet-popover ul {
  list-style: none;
  margin: 0;
  padding: 0;
}
.fleet-popover li {
  padding: 2px 0;
}
/* Fleet variant grouping */
.fleet-variant-children {
  display: none;
}
```

- [ ] **Step 2: Verify existing tests still pass**

Run: `python -m pytest -x -q`
Expected: All tests pass (CSS-only change has no functional impact)

- [ ] **Step 3: Commit**

```
style(html): add fleet prevalence CSS styles

Styles for prevalence bars (track + fill), popover, banner accent,
and variant group expand/collapse. Uses PF6 CSS variables with
fallback hex values.

Assisted-by: Claude Code
```

---

### Task 6: JavaScript Interactivity

**Files:**
- Modify: `src/yoinkc/templates/report.html.j2` (add JS in the `<script>` block)

- [ ] **Step 1: Add fraction/percentage toggle handler**

In the template's `<script>` block, add:

```javascript
/* Fleet: toggle fraction/percentage on prevalence labels */
document.querySelectorAll('.fleet-bar-label').forEach(function(label) {
  label.addEventListener('click', function(e) {
    e.stopPropagation();  // prevent popover from opening
    var bar = this.closest('.fleet-bar');
    if (!bar) return;
    var count = parseInt(bar.getAttribute('data-count'), 10);
    var total = parseInt(bar.getAttribute('data-total'), 10);
    var showing = this.getAttribute('data-showing') || 'fraction';
    if (showing === 'fraction') {
      var pct = total > 0 ? Math.round(count * 100 / total) : 100;
      this.textContent = pct + '%';
      this.setAttribute('data-showing', 'percent');
    } else {
      this.textContent = count + '/' + total;
      this.setAttribute('data-showing', 'fraction');
    }
  });
});
```

- [ ] **Step 2: Add host list popover handler**

```javascript
/* Fleet: host list popover on bar click */
var activePopover = null;
document.querySelectorAll('.fleet-bar').forEach(function(bar) {
  bar.addEventListener('click', function(e) {
    // Close any existing popover
    if (activePopover) {
      activePopover.remove();
      activePopover = null;
    }
    var hosts = this.getAttribute('data-hosts');
    var popover = document.createElement('div');
    popover.className = 'fleet-popover';
    if (!hosts) {
      popover.textContent = 'Host list not available';
    } else {
      var ul = document.createElement('ul');
      hosts.split(', ').forEach(function(h) {
        if (h.trim()) {
          var li = document.createElement('li');
          li.textContent = h.trim();
          ul.appendChild(li);
        }
      });
      if (ul.children.length === 0) {
        popover.textContent = 'Host list not available';
      } else {
        popover.appendChild(ul);
      }
    }
    this.style.position = 'relative';
    this.appendChild(popover);
    activePopover = popover;
  });
});

/* Close popover on click outside */
document.addEventListener('click', function(e) {
  if (activePopover && !e.target.closest('.fleet-bar')) {
    activePopover.remove();
    activePopover = null;
  }
});
```

- [ ] **Step 3: Verify existing tests still pass**

Run: `python -m pytest -x -q`
Expected: All tests pass (JS-only change has no functional impact on server-side rendering)

- [ ] **Step 4: Commit**

```
feat(html): add fleet prevalence toggle and host popover JS

Click label to toggle fraction/percentage display (with
stopPropagation to prevent popover). Click bar to show host list
popover. Empty hosts shows "Host list not available".

Assisted-by: Claude Code
```

---

## Chunk 3: Content Variant Grouping

### Task 7: Variant Grouping Renderer Logic + Template

**Files:**
- Modify: `src/yoinkc/renderers/html_report.py` (add `_group_variants` helper, update `_build_context`)
- Modify: `src/yoinkc/templates/report.html.j2` (grouped variant rendering)
- Test: `tests/test_renderer_outputs.py`

- [ ] **Step 1: Write the failing tests**

Add to `tests/test_renderer_outputs.py`:

```python
class TestFleetVariantGrouping:
    """Tests for content variant grouping in fleet mode."""

    def test_variant_grouping_renders_expand_toggle(self, tmp_path):
        """Config items sharing a path render as a grouped row with expand toggle."""
        snap = InspectionSnapshot(
            schema_version=1,
            os_release=OsRelease(
                name="Red Hat Enterprise Linux", version_id="9.4",
                id="rhel", platform_id="platform:el9",
            ),
            meta={
                "fleet": {
                    "source_hosts": ["web-01", "web-02", "web-03"],
                    "total_hosts": 3,
                    "min_prevalence": 67,
                }
            },
            config=ConfigSection(
                files=[
                    ConfigFileEntry(
                        path="/etc/httpd/conf/httpd.conf",
                        kind=ConfigFileKind.UNOWNED,
                        content="ServerName web-01",
                        include=True,
                        fleet=FleetPrevalence(count=2, total=3, hosts=["web-01", "web-02"]),
                    ),
                    ConfigFileEntry(
                        path="/etc/httpd/conf/httpd.conf",
                        kind=ConfigFileKind.UNOWNED,
                        content="ServerName web-03",
                        include=False,
                        fleet=FleetPrevalence(count=1, total=3, hosts=["web-03"]),
                    ),
                ],
            ),
        )
        env = Environment(autoescape=True)
        html_report.render(snap, env, tmp_path)
        html = (tmp_path / "report.html").read_text()
        assert "2 variants" in html
        assert "fleet-variant-group" in html

    def test_no_grouping_without_fleet(self, tmp_path):
        """Without fleet data, config files render as individual rows."""
        snap = InspectionSnapshot(
            schema_version=1,
            os_release=OsRelease(
                name="Red Hat Enterprise Linux", version_id="9.4",
                id="rhel", platform_id="platform:el9",
            ),
            config=ConfigSection(
                files=[
                    ConfigFileEntry(
                        path="/etc/httpd/conf/httpd.conf",
                        kind=ConfigFileKind.UNOWNED,
                        content="ServerName localhost",
                        include=True,
                    ),
                ],
            ),
        )
        env = Environment(autoescape=True)
        html_report.render(snap, env, tmp_path)
        html = (tmp_path / "report.html").read_text()
        assert "fleet-variant-group" not in html

    def test_single_item_path_not_grouped(self, tmp_path):
        """A config path with only one item renders as a normal row."""
        snap = InspectionSnapshot(
            schema_version=1,
            os_release=OsRelease(
                name="Red Hat Enterprise Linux", version_id="9.4",
                id="rhel", platform_id="platform:el9",
            ),
            meta={
                "fleet": {
                    "source_hosts": ["web-01"],
                    "total_hosts": 1,
                    "min_prevalence": 100,
                }
            },
            config=ConfigSection(
                files=[
                    ConfigFileEntry(
                        path="/etc/httpd/conf/httpd.conf",
                        kind=ConfigFileKind.UNOWNED,
                        content="ServerName localhost",
                        include=True,
                        fleet=FleetPrevalence(count=1, total=1),
                    ),
                ],
            ),
        )
        env = Environment(autoescape=True)
        html_report.render(snap, env, tmp_path)
        html = (tmp_path / "report.html").read_text()
        assert "fleet-variant-group" not in html
```

- [ ] **Step 2: Run test to verify it fails**

Run: `python -m pytest tests/test_renderer_outputs.py::TestFleetVariantGrouping -v`
Expected: FAIL (no variant grouping markup)

- [ ] **Step 3: Implement `_group_variants` helper**

In `src/yoinkc/renderers/html_report.py`, add:

```python
from collections import OrderedDict

def _group_variants(items, path_key="path"):
    """Group items by path for variant display. Returns OrderedDict[path, list[dict]].

    Each entry in the list has {"item": item_or_dict, "snap_index": int}.
    Groups are sorted by prevalence (highest first).
    """
    groups = OrderedDict()
    for idx, item in enumerate(items):
        path = item[path_key] if isinstance(item, dict) else getattr(item, path_key)
        if path not in groups:
            groups[path] = []
        groups[path].append({"item": item, "snap_index": idx})

    # Sort variants within each group by prevalence (highest first)
    for path, variants in groups.items():
        variants.sort(
            key=lambda v: _variant_prevalence(v["item"]),
            reverse=True,
        )
    return groups


def _variant_prevalence(item):
    """Extract prevalence count for sorting. Returns 0 if no fleet data."""
    fleet = item.get("fleet") if isinstance(item, dict) else getattr(item, "fleet", None)
    return fleet.count if fleet else 0
```

- [ ] **Step 4: Update `_build_context` to pass variant groups**

In `_build_context()`, after computing `config_files_rendered`, add:

```python
# Variant grouping for fleet mode
if fleet_meta:
    config_variant_groups = _group_variants(config_files_rendered, path_key="path")
    quadlet_variant_groups = _group_variants(
        snapshot.containers.quadlet_units, path_key="path"
    ) if snapshot.containers and snapshot.containers.quadlet_units else OrderedDict()
    dropin_variant_groups = _group_variants(
        snapshot.services.drop_ins, path_key="path"
    ) if snapshot.services and snapshot.services.drop_ins else OrderedDict()
else:
    config_variant_groups = None
    quadlet_variant_groups = None
    dropin_variant_groups = None
```

Add to the return dict:
```python
"config_variant_groups": config_variant_groups,
"quadlet_variant_groups": quadlet_variant_groups,
"dropin_variant_groups": dropin_variant_groups,
```

- [ ] **Step 5: Update template for variant grouping**

In `src/yoinkc/templates/report.html.j2`, replace the config files iteration with grouped rendering when in fleet mode. Use `v.snap_index` from the wrapper dict (never `v.item.snap_index`) for `data-snap-index`:

```html
{% if fleet_meta and config_variant_groups %}
  {# Fleet mode: grouped by path #}
  {% for path, variants in config_variant_groups.items() %}
    {% if variants | length > 1 %}
      {# Grouped row (collapsed) #}
      {% set primary = variants[0] %}
      <tr class="fleet-variant-group{% if not primary.item.include %} excluded{% endif %}"
          data-snap-section="config" data-snap-list="files"
          data-snap-index="{{ primary.snap_index }}">
        <td class="pf-v6-c-table__check">
          <span class="pf-v6-c-check pf-m-standalone include-cb-wrap">
            <input type="checkbox" class="pf-v6-c-check__input include-cb"
                   {{ 'checked' if primary.item.include else '' }}/>
          </span>
        </td>
        <td>
          <code>{{ path }}</code>
          <span class="pf-v6-c-label pf-m-compact pf-m-outline fleet-variant-toggle"
                style="margin-left: 8px; cursor: pointer;">
            <span class="pf-v6-c-label__content">{{ variants | length }} variants</span>
          </span>
        </td>
        {{ fleet_cell(primary.item) }}
      </tr>
      {# Variant child rows (hidden by default via CSS) #}
      <tr class="fleet-variant-children">
        <td colspan="10">
          <table class="pf-v6-c-table pf-m-compact" style="margin: 0;">
            {% for v in variants %}
            <tr{% if not v.item.include %} class="excluded"{% endif %}
                data-snap-section="config" data-snap-list="files"
                data-snap-index="{{ v.snap_index }}">
              <td class="pf-v6-c-table__check">
                <span class="pf-v6-c-check pf-m-standalone include-cb-wrap">
                  <input type="checkbox" class="pf-v6-c-check__input include-cb"
                         {{ 'checked' if v.item.include else '' }}/>
                </span>
              </td>
              <td><code>{{ path }}</code> <span style="color: var(--pf-v6-global--Color--200);">(variant {{ loop.index }})</span></td>
              {{ fleet_cell(v.item) }}
              <td>
                {% if v.item is mapping and v.item.diff_html %}
                  {{ v.item.diff_html }}
                {% endif %}
              </td>
            </tr>
            {% endfor %}
          </table>
        </td>
      </tr>
    {% else %}
      {# Single item — render as normal row #}
      {% set f = variants[0].item %}
      {# ... existing single config file row markup, using f as the item ... #}
    {% endif %}
  {% endfor %}
{% else %}
  {# Non-fleet: existing config file rendering unchanged #}
  {%- for f in config_files_rendered %}
    {# ... existing markup ... #}
  {%- endfor %}
{% endif %}
```

Add JavaScript for the variant expand/collapse toggle (uses `display` toggling, not class toggle, since child rows default to `display: none` via CSS):

```javascript
/* Fleet: variant group expand/collapse */
document.querySelectorAll('.fleet-variant-toggle').forEach(function(badge) {
  badge.addEventListener('click', function() {
    var childRow = this.closest('tr').nextElementSibling;
    if (childRow && childRow.classList.contains('fleet-variant-children')) {
      if (childRow.style.display === 'table-row') {
        childRow.style.display = '';  // revert to CSS default (none)
      } else {
        childRow.style.display = 'table-row';
      }
    }
  });
});
```

Apply the same pattern for quadlet units and drop-ins if variant groups exist for those sections.

- [ ] **Step 6: Run test to verify it passes**

Run: `python -m pytest tests/test_renderer_outputs.py::TestFleetVariantGrouping -v`
Expected: PASS (all 3 tests)

- [ ] **Step 7: Run full test suite**

Run: `python -m pytest -x -q`
Expected: All tests pass

- [ ] **Step 8: Commit**

```
feat(html): add content variant grouping for fleet mode

Group config files, quadlet units, and drop-ins by path when fleet
data is present. Multiple variants for the same path collapse into
a single row with an expand toggle. Each variant has its own
checkbox and prevalence bar.

Assisted-by: Claude Code
```

---

## Post-Implementation Verification

After all tasks are complete:

- [ ] **Manual browser test:** Render a fleet-merged snapshot and open `report.html`:
  1. Fleet banner appears at top with host count, threshold, hostnames
  2. Prevalence bars appear on all item rows with correct colors
  3. Click prevalence label — toggles between "2/3" and "67%"
  4. Click prevalence bar — popover shows host names
  5. Click outside popover — it closes
  6. Config files with variants show grouped rows with "N variants" badge
  7. Click variant badge — child rows expand/collapse
  8. Non-fleet report (regular single-host snapshot) — zero visual differences
  9. Reset button still works correctly

- [ ] **Generate test fleet snapshot:**
  ```bash
  source /Users/mrussell/Work/bootc-migration/yoinkc/.venv/bin/activate
  yoinkc-fleet aggregate ./fleet-test/ -p 50 -o merged.json
  yoinkc --from-snapshot merged.json --output-dir /tmp/fleet-output
  open /tmp/fleet-output/report.html
  ```
