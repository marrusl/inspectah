# Fleet Prevalence UI Design

## Goal

Surface fleet metadata (prevalence counts, host lists, content variants) in the yoinkc HTML report so users can understand and refine fleet-aggregated snapshots.

## Prerequisites

- Fleet analysis engine (`yoinkc-fleet aggregate`) is implemented and produces merged snapshots with `FleetPrevalence` on items and `FleetMeta` in `meta["fleet"]`.

## Design Decisions

| Decision | Answer |
|---|---|
| Report mode | Fleet-aware: banner + badges in same template, gated on `meta["fleet"]` |
| Badge style | Fraction + color bar; click toggles to percentage |
| Banner | Moderate: host count, threshold, hostnames, include/exclude summary |
| Content variants | Grouped under one expandable row per path |
| Filtering/sorting | None for v1; badges are informational only |
| Host detail | PF6 popover on click showing host list |
| Architecture | Renderer-enriched: renderer pre-processes, template renders |

## Out of Scope

- Prevalence-based filtering/sorting (future fleet-refine work)
- `yoinkc-fleet aggregate` producing tarballs directly (separate spec)
- Package version spread display (separate spec)
- Fleet-specific triage logic
- Users/groups fleet badges (they use `List[dict]` not typed models; fleet data is stored as a plain `"fleet"` dict key — rendering this requires different handling and is deferred)
- Reset button interaction with variant groups (reset restores include/exclude state only; expand/collapse is view state, not snapshot state)

---

## Section 1: Fleet Detection and Banner

### Detection

The renderer checks `snapshot.meta.get("fleet")`. If present and parseable as `FleetMeta`, the report enters fleet mode. All fleet-specific rendering is gated on a `fleet_meta` template variable. Non-fleet snapshots render exactly as today.

### Renderer Changes

`_build_context()` gains a `fleet_meta` key:

```python
fleet_raw = snapshot.meta.get("fleet")
if fleet_raw:
    fleet_meta = FleetMeta(**fleet_raw)
else:
    fleet_meta = None
```

### Banner

A PF6 card rendered at the top of the report (after the OS header, before the first section):

- Title: "Fleet Analysis"
- Body line 1: "{total_hosts} hosts merged at {min_prevalence}% prevalence threshold"
- Body line 2: host list (comma-separated from `fleet_meta.source_hosts`)
- Body line 3: "{included_count} items included, {excluded_count} excluded" — computed from existing `counts` context variable (sum of all section counts minus excluded)

Rendered only when `{% if fleet_meta %}`.

---

## Section 2: Prevalence Badges

### Sections Receiving Badges

All sections that iterate typed models with `fleet` fields get the "Fleet" column:

| Section | List | Template iteration style |
|---|---|---|
| RPM packages (added) | `packages_added` | Direct snapshot iteration |
| RPM packages (base image) | `base_image_only` | Direct snapshot iteration |
| Repo files | `repo_files` | Direct snapshot iteration |
| Services (state changes) | `state_changes` | Direct snapshot iteration |
| Systemd drop-ins | `drop_ins` | Direct snapshot iteration |
| Firewall zones | `firewall_zones` | Direct snapshot iteration |
| Scheduled tasks (timers) | `generated_timer_units` | Direct snapshot iteration |
| Scheduled tasks (cron) | `cron_jobs` | Direct snapshot iteration |
| Config files | `files` | Pre-processed via `_prepare_config_files()` |
| Quadlet units | `quadlet_units` | Direct snapshot iteration |
| Compose files | `compose_files` | Card layout (not table rows) |

**Special handling required:**

- **Config files:** `_prepare_config_files()` converts `ConfigFileEntry` to plain dicts, stripping the `fleet` field. The function must be updated to pass through `fleet` data (add `"fleet": entry.fleet` to the output dict).
- **Compose files:** Use a card layout, not table rows. The prevalence bar renders inside the card header rather than as a table cell. Same data, different placement.

### Color Classes

Based on percentage (count/total). Guard against `total == 0` (return `pf-m-blue`):

| Percentage | Color | PF6 Class |
|---|---|---|
| 100% | Blue | `pf-m-blue` |
| 50-99% | Gold | `pf-m-gold` |
| < 50% | Red | `pf-m-red` |

### Renderer Pre-processing

Register a Jinja2 filter `fleet_color` on the Jinja2 `Environment` (created in `renderers/__init__.py:run_all()` or in `html_report.render()` before template invocation):

```python
def _fleet_color(fleet: FleetPrevalence | None) -> str:
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

### Template Rendering

Each item table gains a conditional prevalence column header when in fleet mode:

```html
{% if fleet_meta %}
<th>Fleet</th>
{% endif %}
```

Each item row gains a conditional prevalence cell:

```html
{% if fleet_meta %}
<td class="fleet-prevalence">
  {% if item.fleet %}
  <div class="fleet-bar"
       data-count="{{ item.fleet.count }}"
       data-total="{{ item.fleet.total }}"
       data-hosts="{{ item.fleet.hosts | join(', ') }}">
    <div class="fleet-bar-fill {{ item.fleet | fleet_color }}"
         style="width: {{ (item.fleet.count * 100 // item.fleet.total) if item.fleet.total > 0 else 100 }}%"></div>
    <span class="fleet-bar-label">{{ item.fleet.count }}/{{ item.fleet.total }}</span>
  </div>
  {% endif %}
</td>
{% endif %}
```

Note: Jinja2 dot notation (`item.fleet`) resolves both object attributes and dict keys, so the same template syntax works for both Pydantic models (direct iteration) and pre-processed dicts (config files).

### JavaScript Interactivity

Two handlers:

1. **Click `.fleet-bar-label`**: Toggle display between fraction ("2/3") and percentage ("67%"). Reads `data-count` and `data-total` from parent `.fleet-bar`. Must call `event.stopPropagation()` to prevent the popover handler on the parent `.fleet-bar` from also firing.

2. **Click `.fleet-bar`**: Open a PF6 popover listing the hosts from `data-hosts`. If `data-hosts` is empty, show "Host list not available" (occurs when `--no-hosts` was used). Simple popover with host names as a list. Closes on click outside.

### CSS

Minimal, scoped under `.fleet-prevalence`:

- `.fleet-bar`: fixed width (e.g., 60px), background `#e0e0e0`, border-radius, overflow hidden, cursor pointer
- `.fleet-bar-fill`: height 100%, background color from PF6 palette (blue/gold/red)
- `.fleet-bar-label`: small text alongside the bar

---

## Section 3: Content Variant Grouping

### Affected Sections

Only content-bearing sections where the fleet merge produces multiple items for the same path:

- Config files (`config.files`)
- Quadlet units (`containers.quadlet_units`)
- Systemd drop-ins (`services.drop_ins`)

**Not grouped:** Compose files use a card layout (one card per file). Variant compose files render as separate cards, each with its own prevalence bar in the card header. No grouping needed since cards are visually distinct.

### Renderer Pre-processing

For config files, extend `_prepare_config_files()` to group items by path when in fleet mode. For quadlet units and drop-ins, add new grouping logic in `_build_context()`.

The grouped structure maps paths to lists of items with their original snapshot indices:

```python
variant_groups[path] = [
    {"item": item_or_dict, "snap_index": original_index},
    ...
]
# Sorted by prevalence (highest first within each group)
```

Groups with 1 item render as normal rows. Groups with 2+ items render as grouped rows.

The grouped structure is passed to the template as context variables (e.g., `config_variant_groups`, `quadlet_variant_groups`, `dropin_variant_groups`).

### Template Rendering

**Grouped row (collapsed):**
- Path shown once
- Checkbox and prevalence bar for the highest-prevalence variant
- `data-snap-section`, `data-snap-list`, `data-snap-index` point to the highest-prevalence variant's original index
- Expand toggle badge: "N variants"

**Expanded child rows:**
- Each variant gets its own row with:
  - Checkbox (include/exclude) with its own `data-snap-index` pointing to the variant's original snapshot index
  - Prevalence bar
  - Host list (via popover)
  - "View" link to file viewer
- The parent row's checkbox state reflects the highest-prevalence variant. Child row checkboxes are independent.

Expand/collapse follows the existing tree-view pattern (toggle `pf-m-expanded` class).

### Non-fleet Mode

No grouping occurs. Config files, quadlet units, and drop-ins render exactly as today: one row per file.

---

## Section 4: Testing Strategy

### Automated Tests (Python)

Added to existing `tests/test_renderer_outputs.py`:

| Test | Asserts |
|---|---|
| `test_fleet_banner_present` | Snapshot with `meta["fleet"]` renders banner with host count and threshold |
| `test_fleet_banner_absent` | Normal snapshot has no fleet banner markup |
| `test_fleet_prevalence_badge` | Items with `.fleet` set render prevalence bar with correct count/total |
| `test_fleet_prevalence_absent` | Items without `.fleet` render no prevalence markup |
| `test_fleet_variant_grouping` | Config items sharing a path render as grouped row with expand toggle |
| `test_fleet_color_class` | Jinja2 filter: 100% = blue, 50-99% = gold, <50% = red, total=0 = blue |
| `test_fleet_config_passthrough` | `_prepare_config_files` preserves `fleet` field in output dicts |
| `test_fleet_empty_hosts_popover` | Items with empty `hosts` list render `data-hosts=""` |

### Manual Browser Testing

- Click prevalence label toggles between fraction and percentage
- Click prevalence bar opens popover with host list
- Click prevalence bar with no hosts shows "Host list not available"
- Expand variant group reveals child rows with individual checkboxes
- Child row checkboxes have correct `data-snap-index` values
- Non-fleet report has zero visual differences from current behavior

---

## Future Work (Separate Specs)

- `yoinkc-fleet aggregate` producing tarballs directly (render pipeline integration)
- Package version spread display across fleet
- Prevalence-based interactive filtering/sorting in yoinkc-refine
- Fleet-specific triage logic (e.g., "items on <50% of hosts flagged as fixme")
- Layered image hierarchy (common base + role-specific variants)
- Users/groups fleet prevalence badges
