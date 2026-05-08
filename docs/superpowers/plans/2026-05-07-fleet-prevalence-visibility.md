# Fleet Prevalence Visibility — Phase 1 Implementation Plan (revision 4)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add prevalence visibility to the fleet refine UI so users understand fleet consensus at a glance, with interactive threshold presets for classification adjustment.

**Architecture:** Backend propagates raw `Fleet` data to each triage manifest entry — no zone computation on the server. The client owns all threshold-driven classification: it computes prevalence zones from raw `fleet.count/fleet.total` against the user-selected threshold, renders badges, sorts within tier groups, and enriches headers. All threshold interaction is client-side only — no server round-trips, no `include` state mutations, no rebuild triggers.

**Merge-threshold coexistence:** Task 1 changes `--min-prevalence` to default to `0`, so the common path produces reports where all items arrive with `include=true`. However, users can still pass an explicit `--min-prevalence 50` (or any nonzero value), which causes the fleet merge to set `include=false` on items below that merge threshold. The client dropdown is additive to whatever state the merge produced — it visually classifies the items that survived the merge but never overrides their `include` state. Reports with `meta.fleet.min_prevalence > 0` display correctly: items excluded by the merge show their toggle off (as today), and the client threshold only affects visual badge coloring and sort order among the items present. The UI does not read or display `meta.fleet.min_prevalence` — it is not aware of whether the merge used a nonzero threshold.

**Tech Stack:** Go (renderer, CLI), vanilla JavaScript (report.html SPA), CSS (embedded in report.html)

**Spec:** `docs/specs/proposed/2026-05-07-fleet-prevalence-visibility-design.md`

**Revision 2 changes from round 1 review:**
- Removed `PrevalenceZone` from Go backend — client owns all zoning (Kit #4)
- Removed `secrets` from prevalence scope — current card builders and data seams don't support it (Thorn #1, Fern #2, Kit #2)
- Fixed all `go test` paths to `cmd/inspectah/` module root (Kit #1)
- Fixed threshold refresh to invalidate all applicable sections on change (Fern #1)
- Fixed sidebar: secondary fleet badge that never overwrites tier badge (Fern #3)
- Fixed sidebar threshold indicator DOM creation order (Kit #3, Thorn #3)
- Fixed Overview threshold handler to avoid blowing away the dropdown (Thorn #5)
- Consolidated frontend Tasks 3-9 into a single coherent Task 3 (Kit #5)
- Added automated E2E test for presentation-only threshold invariant (Thorn #4)

**Revision 3 changes from round 2 review:**
- Fixed tab-order contract: `toggle switch → prevalence badge → chevron` (3 stops, matching actual DOM — item name is not a focusable control) (Fern #1, Thorn #3, Kit #1)
- Promoted `fleet-threshold-no-dirty-state` E2E test from deferred into required Task 4 (Thorn #1, Kit #2)
- Added merge-threshold coexistence model for nonzero `--min-prevalence` reports (Thorn #2)
- Reframed grep verification as smoke check, not proof (Kit #2)
- Clarified fleet reason text uses new `.fleet-reason` element, not existing `detail-reason` slot (Fern warning #4)
- Clarified sidebar threshold placement: after review-progress bar (Fern warning #4)
- Added plan/spec drift reconciliation note (Fern warning #3)

**Plan/spec drift note:** This plan supersedes the approved spec on two points where implementation constraints forced scope changes: (1) `secrets` is removed from Phase 1 prevalence scope (spec says applicable; plan defers due to card builder / data seam gap), and (2) sidebar badges use a separate secondary `.fleet-review-badge` element instead of reusing the existing tier badge slot (spec language was flexible; plan picks the additive approach). The spec remains authoritative on all other points.

**Revision 4 changes from round 3 review:**
- Tightened `fleet-threshold-no-dirty-state` Playwright spec: `resetServer()` in `beforeAll`/`afterAll`, `isRefineMode()` guard, dropped `#rebuild-bar` class check (rebuild bar is always active in refine mode — `#changes-badge` is the real dirty-state signal) (Thorn #1)
- Added nonzero merge-threshold verification case in Task 2 tests (Thorn #2)

---

## File Structure

**Modified files:**

| File | Responsibility |
|------|---------------|
| `cmd/inspectah/internal/cli/passthrough.go` | Change `--min-prevalence` default 100 → 0 |
| `cmd/inspectah/internal/cli/fleet_test.go` | Test default prevalence value |
| `cmd/inspectah/internal/renderer/triage.go` | Add `Fleet` to `TriageItem`, propagate Fleet data from schema items |
| `cmd/inspectah/internal/renderer/triage_test.go` | Test Fleet propagation |
| `cmd/inspectah/internal/renderer/static/report.html` | CSS + JS: badges, dropdown, reason text, headers, sort, sidebar |

**Files NOT modified (per spec):**

| File | Reason |
|------|--------|
| `cmd/inspectah/internal/fleet/merge.go` | Merge algorithm unchanged |
| `cmd/inspectah/internal/schema/types.go` | `FleetPrevalence` struct already complete |
| `cmd/inspectah/internal/refine/server.go` | No new endpoints |

**Scope exclusions for Phase 1:**
- **Secrets section** — `buildSecretCard()` / `buildSecretDecidedCard()` use a different card pattern than `buildToggleCard()`, and `classifySecretItems()` does not propagate Fleet data. Secrets prevalence requires both a fleet-data source and secret-card rendering work; deferred to a follow-up.

---

### Task 1: Change --min-prevalence default to 0

**Files:**
- Modify: `cmd/inspectah/internal/cli/passthrough.go:33`
- Test: `cmd/inspectah/internal/cli/fleet_test.go`

- [ ] **Step 1: Write the failing test**

Add to `cmd/inspectah/internal/cli/fleet_test.go`:

```go
func TestFleetCmd_DefaultPrevalence(t *testing.T) {
	cmd := newFleetCmd(&GlobalOpts{Version: "0.7.0"})
	f := cmd.Flags().Lookup("min-prevalence")
	assert.NotNil(t, f)
	assert.Equal(t, "0", f.DefValue)
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/cli/ -run TestFleetCmd_DefaultPrevalence -v`

Expected: FAIL — `DefValue` is `"100"`, not `"0"`

- [ ] **Step 3: Change the default**

In `cmd/inspectah/internal/cli/passthrough.go`, in `registerFleetPassthrough`, change:

```go
// Before:
f.IntP("min-prevalence", "p", 100, "include items on >= N%% of hosts")

// After:
f.IntP("min-prevalence", "p", 0, "include items on >= N%% of hosts")
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/cli/ -run TestFleetCmd_DefaultPrevalence -v`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add cmd/inspectah/internal/cli/passthrough.go cmd/inspectah/internal/cli/fleet_test.go
git commit -m "feat(cli): change --min-prevalence default from 100 to 0

All items now arrive with include=true regardless of prevalence.
The presentation threshold (client-side dropdown) handles visual
classification. No items are silently excluded.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 2: Add Fleet to TriageItem and propagate

**Files:**
- Modify: `cmd/inspectah/internal/renderer/triage.go`
- Test: `cmd/inspectah/internal/renderer/triage_test.go`

The backend only propagates raw `Fleet` data. No `PrevalenceZone` computation — the client owns all threshold-driven classification from the raw `fleet.count/fleet.total` values.

- [ ] **Step 1: Write the failing tests**

Add to `cmd/inspectah/internal/renderer/triage_test.go`:

```go
func TestClassifySnapshot_FleetPropagation(t *testing.T) {
	fleet := &schema.FleetPrevalence{Count: 2, Total: 3, Hosts: []string{"h1", "h2"}}
	snap := &schema.InspectionSnapshot{
		SchemaVersion: schema.SchemaVersion,
		Meta: map[string]interface{}{
			"hostname": "fleet-merged",
			"fleet":    map[string]interface{}{"total_hosts": float64(3)},
		},
		Rpm: &schema.RpmSection{
			PackagesAdded: []schema.PackageEntry{
				{Name: "httpd", Arch: "x86_64", Version: "2.4", Release: "1.el9",
					State: schema.PackageStateAdded, Include: true, Fleet: fleet},
			},
		},
	}

	items := ClassifySnapshot(snap, nil)
	var found *TriageItem
	for i := range items {
		if items[i].Key == "pkg-httpd-x86_64" {
			found = &items[i]
			break
		}
	}
	if found == nil {
		t.Fatal("expected to find pkg-httpd-x86_64 in triage items")
	}
	if found.Fleet == nil {
		t.Fatal("expected Fleet to be propagated to TriageItem")
	}
	if found.Fleet.Count != 2 || found.Fleet.Total != 3 {
		t.Errorf("Fleet = {%d, %d}, want {2, 3}", found.Fleet.Count, found.Fleet.Total)
	}
}

func TestClassifySnapshot_SingleMachineNilFleet(t *testing.T) {
	snap := &schema.InspectionSnapshot{
		SchemaVersion: schema.SchemaVersion,
		Meta:          map[string]interface{}{"hostname": "single-host"},
		Rpm: &schema.RpmSection{
			PackagesAdded: []schema.PackageEntry{
				{Name: "httpd", Arch: "x86_64", Version: "2.4", Release: "1.el9",
					State: schema.PackageStateAdded, Include: true},
			},
		},
	}

	items := ClassifySnapshot(snap, nil)
	for _, item := range items {
		if item.Fleet != nil {
			t.Errorf("single-machine item %s has non-nil Fleet", item.Key)
		}
	}
}

func TestClassifySnapshot_FleetIdentityFromMap(t *testing.T) {
	snap := &schema.InspectionSnapshot{
		SchemaVersion: schema.SchemaVersion,
		Meta: map[string]interface{}{
			"hostname": "fleet-merged",
			"fleet":    map[string]interface{}{"total_hosts": float64(2)},
		},
		UsersGroups: &schema.UserGroupSection{
			Users: []map[string]interface{}{
				{
					"name": "testuser", "uid": float64(1001),
					"include": true,
					"fleet":   map[string]interface{}{"count": float64(2), "total": float64(2), "hosts": []interface{}{"h1", "h2"}},
				},
			},
		},
	}

	items := ClassifySnapshot(snap, nil)
	var found *TriageItem
	for i := range items {
		if items[i].Key == "user-testuser" {
			found = &items[i]
			break
		}
	}
	if found == nil {
		t.Fatal("expected to find user-testuser in triage items")
	}
	if found.Fleet == nil {
		t.Fatal("expected Fleet to be extracted from map for identity item")
	}
	if found.Fleet.Count != 2 || found.Fleet.Total != 2 {
		t.Errorf("Fleet = {%d, %d}, want {2, 2}", found.Fleet.Count, found.Fleet.Total)
	}
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/renderer/ -run "TestClassifySnapshot_Fleet|TestClassifySnapshot_SingleMachine" -v`

Expected: FAIL — `TriageItem` has no `Fleet` field

- [ ] **Step 3: Add Fleet field to TriageItem**

In `cmd/inspectah/internal/renderer/triage.go`, modify the `TriageItem` struct (lines 12-30). Add one field after `ParentUser`:

```go
type TriageItem struct {
	Section        string                  `json:"section"`
	Key            string                  `json:"key"`
	Tier           int                     `json:"tier"`
	Reason         string                  `json:"reason"`
	Name           string                  `json:"name"`
	Meta           string                  `json:"meta"`
	Group          string                  `json:"group,omitempty"`
	CardType       string                  `json:"card_type,omitempty"`
	DisplayOnly    bool                    `json:"display_only,omitempty"`
	Acknowledged   bool                    `json:"acknowledged,omitempty"`
	Deps           []string                `json:"deps,omitempty"`
	IsSecret       bool                    `json:"is_secret,omitempty"`
	SourcePath     string                  `json:"source_path,omitempty"`
	DefaultInclude bool                    `json:"default_include"`
	AlwaysIncluded bool                    `json:"always_included,omitempty"`
	UserPrivate    bool                    `json:"user_private,omitempty"`
	ParentUser     string                  `json:"parent_user,omitempty"`
	Fleet          *schema.FleetPrevalence `json:"fleet,omitempty"`
}
```

- [ ] **Step 4: Add extractFleetFromMap helper**

Add after the `mapInclude` function (around line 88):

```go
func extractFleetFromMap(m map[string]interface{}) *schema.FleetPrevalence {
	raw, ok := m["fleet"]
	if !ok || raw == nil {
		return nil
	}
	fleetMap, ok := raw.(map[string]interface{})
	if !ok {
		return nil
	}
	count, _ := fleetMap["count"].(float64)
	total, _ := fleetMap["total"].(float64)
	if total == 0 {
		return nil
	}
	var hosts []string
	if rawHosts, ok := fleetMap["hosts"].([]interface{}); ok {
		for _, h := range rawHosts {
			if s, ok := h.(string); ok {
				hosts = append(hosts, s)
			}
		}
	}
	return &schema.FleetPrevalence{
		Count: int(count),
		Total: int(total),
		Hosts: hosts,
	}
}
```

- [ ] **Step 5: Propagate Fleet in classifyPackages**

Add `Fleet: pkg.Fleet` to the PackageEntry TriageItem (~line 316):

```go
		item := TriageItem{
			Section:        "packages",
			Key:            fmt.Sprintf("pkg-%s-%s", pkg.Name, pkg.Arch),
			Tier:           tier,
			Reason:         reason,
			Name:           pkg.Name,
			Meta:           joinNonEmpty(" | ", pkg.Version+"-"+pkg.Release, pkg.Arch, pkg.SourceRepo),
			DefaultInclude: pkg.Include,
			Fleet:          pkg.Fleet,
		}
```

Add `Fleet: ms.Fleet` to the module stream TriageItem (~line 360):

```go
		items = append(items, TriageItem{
			Section:        "packages",
			Key:            fmt.Sprintf("ms-%s-%s", ms.ModuleName, ms.Stream),
			Tier:           2,
			Reason:         "Module stream package. Verify compatibility.",
			Name:           ms.ModuleName + ":" + ms.Stream,
			Meta:           strings.Join(ms.Profiles, ", "),
			DefaultInclude: ms.Include,
			Fleet:          ms.Fleet,
		})
```

- [ ] **Step 6: Propagate Fleet in classifyRuntime**

Service state changes (~line 543) — add `Fleet: svc.Fleet`:

```go
			items = append(items, TriageItem{
				Section: "runtime", Key: "svc-" + svc.Unit,
				Tier: tier, Reason: reason, Name: svc.Unit, Meta: meta,
				Group:          group,
				DefaultInclude: svc.Include,
				Fleet:          svc.Fleet,
			})
```

Cron jobs (~line 572) — add `Fleet: job.Fleet`:

```go
			items = append(items, TriageItem{
				Section: "runtime", Key: "cron-" + job.Path,
				Tier: 2, Reason: "Scheduled cron job.",
				Name: job.Path, Meta: job.Source,
				Group:          group,
				DefaultInclude: job.Include,
				Fleet:          job.Fleet,
			})
```

Systemd timers (~line 585) — add `Fleet: timer.Fleet`:

```go
			items = append(items, TriageItem{
				Section: "runtime", Key: "timer-" + timer.Name,
				Tier: 2, Reason: "Systemd timer unit.",
				Name: timer.Name, Meta: timer.OnCalendar,
				Group:          group,
				DefaultInclude: isIncluded(timer.Include),
				Fleet:          timer.Fleet,
			})
```

- [ ] **Step 7: Propagate Fleet in classifyIdentity**

Users (~line 720) — add `Fleet: extractFleetFromMap(u)`:

```go
			items = append(items, TriageItem{
				Section: "identity", Key: "user-" + name,
				Tier: tier, Reason: reason, Name: name,
				Meta:           fmt.Sprintf("UID %.0f", uid),
				DefaultInclude: mapInclude(u),
				Fleet:          extractFleetFromMap(u),
			})
```

Groups (~line 735) — add `Fleet: extractFleetFromMap(g)`:

```go
			item := TriageItem{
				Section: "identity", Key: "group-" + name,
				Tier: tier, Reason: reason, Name: name,
				Meta:           fmt.Sprintf("GID %.0f", gid),
				DefaultInclude: mapInclude(g),
				Fleet:          extractFleetFromMap(g),
			}
```

- [ ] **Step 8: Propagate Fleet in classifySystemItems**

Add `Fleet: <source>.Fleet` for items whose structs have Fleet. Add `Fleet: extractFleetFromMap(<map>)` for map-based items.

**NMConnection** (~line 812): `Fleet: conn.Fleet`
**FirewallZone** (~line 872): `Fleet: zone.Fleet`
**FstabEntry** (~line 882): `Fleet: entry.Fleet`
**SELinux booleans** (map, ~line 906): `Fleet: extractFleetFromMap(b)`
**SelinuxPortLabel** (~line 938): `Fleet: p.Fleet`

**No change needed** (struct has no Fleet field):
- `SysctlOverride` (kernel boot — first-snapshot-wins, not merged)
- `NonDefaultModule` (same)
- `StaticRouteFile` (no Fleet field)
- SELinux `CustomModules` (plain strings)

- [ ] **Step 9: Add nonzero merge-threshold coexistence test**

This test verifies that a snapshot produced with `--min-prevalence 50` (where the merge set `include=false` on below-threshold items) still carries Fleet data correctly, and that merge-excluded items retain their `include=false` state in the triage manifest.

Add to `cmd/inspectah/internal/renderer/triage_test.go`:

```go
func TestClassifySnapshot_NonzeroMergeThreshold(t *testing.T) {
	snap := &schema.InspectionSnapshot{
		SchemaVersion: schema.SchemaVersion,
		Meta: map[string]interface{}{
			"hostname": "fleet-merged",
			"fleet": map[string]interface{}{
				"total_hosts":    float64(3),
				"min_prevalence": float64(50),
			},
		},
		Rpm: &schema.RpmSection{
			PackagesAdded: []schema.PackageEntry{
				{Name: "httpd", Arch: "x86_64", Version: "2.4", Release: "1.el9",
					State: schema.PackageStateAdded, Include: true,
					Fleet: &schema.FleetPrevalence{Count: 3, Total: 3, Hosts: []string{"h1", "h2", "h3"}}},
				{Name: "rare-tool", Arch: "x86_64", Version: "1.0", Release: "1.el9",
					State: schema.PackageStateAdded, Include: false,
					Fleet: &schema.FleetPrevalence{Count: 1, Total: 3, Hosts: []string{"h1"}}},
			},
		},
	}

	items := ClassifySnapshot(snap, nil)

	var httpd, rare *TriageItem
	for i := range items {
		switch items[i].Key {
		case "pkg-httpd-x86_64":
			httpd = &items[i]
		case "pkg-rare-tool-x86_64":
			rare = &items[i]
		}
	}

	if httpd == nil || rare == nil {
		t.Fatal("expected both packages in triage items")
	}

	// Above-threshold item: include=true, Fleet propagated
	if !httpd.DefaultInclude {
		t.Error("httpd: expected DefaultInclude=true (above merge threshold)")
	}
	if httpd.Fleet == nil || httpd.Fleet.Count != 3 {
		t.Error("httpd: expected Fleet with Count=3")
	}

	// Below-threshold item: include=false (from merge), Fleet still propagated
	if rare.DefaultInclude {
		t.Error("rare-tool: expected DefaultInclude=false (merge-excluded)")
	}
	if rare.Fleet == nil || rare.Fleet.Count != 1 {
		t.Error("rare-tool: expected Fleet with Count=1 even though merge-excluded")
	}
}
```

- [ ] **Step 10: Run tests to verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/renderer/ -run "TestClassifySnapshot_Fleet|TestClassifySnapshot_SingleMachine|TestClassifySnapshot_NonzeroMerge" -v`

Expected: All 4 tests PASS

- [ ] **Step 11: Run full renderer test suite**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/renderer/ -v -count=1`

Expected: All existing tests PASS (new field is `omitempty`, backward compatible)

- [ ] **Step 12: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add cmd/inspectah/internal/renderer/triage.go cmd/inspectah/internal/renderer/triage_test.go
git commit -m "feat(renderer): add Fleet to TriageItem and propagate

Propagate fleet prevalence data from schema items to triage
manifest entries. Client owns all threshold-driven zoning from
raw fleet.count/fleet.total values.

Fleet data propagated for: packages, module streams, services,
cron jobs, systemd timers, network connections, firewall zones,
fstab entries, SELinux booleans (from map), SELinux port labels.

Items from first-snapshot-wins sections (kernel boot) and items
without fleet support (static routes, custom modules) correctly
have nil Fleet.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 3: Frontend prevalence UI

**Files:**
- Modify: `cmd/inspectah/internal/renderer/static/report.html`

All frontend prevalence work lands in a single coherent pass. This task modifies `renderOverview()`, `renderSidebar()`, `updateBadge()`, `buildToggleCard()`, `renderTriageSection()`, and supporting helpers as one unit to avoid partial integration states.

- [ ] **Step 1: Add prevalence CSS**

In `report.html`, before the closing `</style>` tag (the second one, after all SPA-specific CSS), insert:

```css
    /* Prevalence badge */
    .prevalence-badge {
      font-size: 0.75rem;
      padding: 0.125rem 0.5rem;
      border-radius: 4px;
      cursor: pointer;
      display: inline-block;
      min-width: 24px;
      min-height: 24px;
      line-height: 24px;
      text-align: center;
      user-select: none;
      background: rgba(139, 148, 158, 0.15);
      margin-left: 0.5rem;
      vertical-align: middle;
    }
    .prevalence-badge:hover { opacity: 0.8; }
    .prevalence-badge:focus-visible {
      outline: 2px solid var(--pf-t--global--color--brand--default, #0066cc);
      outline-offset: 2px;
    }
    .prevalence-badge.zone-unanimous { color: #8b949e; }
    .prevalence-badge.zone-above { color: #d29922; }
    .prevalence-badge.zone-below { color: #d29922; font-weight: 600; }

    .pf-v6-theme-dark .prevalence-badge.zone-unanimous { color: #8b949e; }
    .pf-v6-theme-dark .prevalence-badge.zone-above { color: #e3b341; }
    .pf-v6-theme-dark .prevalence-badge.zone-below { color: #e3b341; font-weight: 600; }

    .sidebar-threshold {
      font-size: 0.8rem;
      opacity: 0.7;
      margin-top: 0.25rem;
      padding: 0.25rem 1rem;
    }
    .threshold-select {
      font-size: 0.85rem;
      padding: 0.25rem 0.5rem;
      border-radius: 4px;
      background: var(--pf-t--global--background--color--secondary-default);
      color: var(--pf-t--global--text--color--primary);
      border: 1px solid var(--pf-t--global--border--color--default);
    }
    .fleet-reason {
      font-size: 0.8rem;
      opacity: 0.8;
      margin-top: 0.25rem;
      font-style: italic;
    }
    #prevalence-format-announce {
      position: absolute;
      width: 1px;
      height: 1px;
      padding: 0;
      margin: -1px;
      overflow: hidden;
      clip: rect(0, 0, 0, 0);
      border: 0;
    }
    .fleet-review-badge {
      font-size: 0.65rem;
      padding: 0 4px;
      border-radius: 6px;
      margin-left: 4px;
      opacity: 0.6;
      background: rgba(210, 153, 34, 0.15);
      color: #d29922;
    }
```

- [ ] **Step 2: Add aria-live region**

After `<div id="main-content"` opening tag, immediately before its first child, add:

```html
    <div id="prevalence-format-announce" aria-live="polite" role="status"></div>
```

- [ ] **Step 3: Add App state**

In `var App = {` (~line 2025), after the last existing property, add:

```js
  prevalenceThreshold: 1.0,
  prevalenceFormat: 'count',
```

- [ ] **Step 4: Add all prevalence helper functions**

After the `groupByTier` function (~line 2505, just before `// ── Overview Section ──`), add the full helper block:

```js
// ── Fleet Prevalence Helpers ──
function isFleetSnapshot() {
  return App.snapshot && App.snapshot.meta && App.snapshot.meta.fleet;
}

function isApplicableForPrevalence(sectionId) {
  return sectionId === 'packages' || sectionId === 'runtime' ||
         sectionId === 'identity' || sectionId === 'system';
}

function computePrevalenceZone(item, threshold) {
  if (!item.fleet) return '';
  if (item.fleet.count === item.fleet.total) return 'unanimous';
  var ratio = item.fleet.count / item.fleet.total;
  if (ratio >= threshold) return 'above';
  return 'below';
}

function formatPrevalenceBadge(item) {
  if (!item.fleet) return '';
  if (App.prevalenceFormat === 'percent') {
    return Math.round((item.fleet.count / item.fleet.total) * 100) + '%';
  }
  return item.fleet.count + '/' + item.fleet.total;
}

function formatPrevalenceAriaLabel(item) {
  if (!item.fleet) return '';
  if (App.prevalenceFormat === 'percent') {
    return Math.round((item.fleet.count / item.fleet.total) * 100) + ' percent';
  }
  return item.fleet.count + ' of ' + item.fleet.total + ' hosts';
}

function findManifestItem(key) {
  for (var i = 0; i < App.triageManifest.length; i++) {
    if (App.triageManifest[i].key === key) return App.triageManifest[i];
  }
  return null;
}

function updateAllPrevalenceBadges() {
  var badges = document.querySelectorAll('.prevalence-badge');
  for (var i = 0; i < badges.length; i++) {
    var key = badges[i].getAttribute('data-item-key');
    var item = findManifestItem(key);
    if (item && item.fleet) {
      var zone = computePrevalenceZone(item, App.prevalenceThreshold);
      badges[i].textContent = formatPrevalenceBadge(item);
      badges[i].className = 'prevalence-badge' + (zone ? ' zone-' + zone : '');
      badges[i].setAttribute('aria-label',
        'Fleet prevalence: ' + formatPrevalenceAriaLabel(item) + '. Activate to toggle format.');
    }
  }
}

function getFleetReviewCount(sectionId) {
  if (!isApplicableForPrevalence(sectionId)) return 0;
  var items = getManifestItemsForSection(sectionId);
  var count = 0;
  for (var i = 0; i < items.length; i++) {
    if (computePrevalenceZone(items[i], App.prevalenceThreshold) === 'below') count++;
  }
  return count;
}

function generateFleetReason(item) {
  if (!item.fleet) return '';
  var count = item.fleet.count;
  var total = item.fleet.total;
  if (count === total) {
    return 'Present on all ' + total + ' hosts';
  }
  var zone = computePrevalenceZone(item, App.prevalenceThreshold);
  var suffix = (zone === 'below') ? ' — review recommended' : '';
  if (count === 1 && item.fleet.hosts && item.fleet.hosts.length === 1) {
    return 'Present on ' + count + '/' + total + ' hosts (' + item.fleet.hosts[0] + ' only)' + suffix;
  }
  return 'Present on ' + count + '/' + total + ' hosts' + suffix;
}

function getThresholdPresetLabel(threshold) {
  var presets = {1: 'Unanimous (100%)', 0.8: 'Strong consensus (80%)',
                 0.5: 'Majority (50%)', 0: 'Any presence'};
  return presets[threshold] || (Math.round(threshold * 100) + '%');
}

function updateSidebarThreshold() {
  var el = document.getElementById('sidebar-threshold');
  if (!el) return;
  el.textContent = 'Threshold: ' + getThresholdPresetLabel(App.prevalenceThreshold);
}

function invalidateApplicableSections() {
  MIGRATION_SECTIONS.forEach(function(s) {
    if (isApplicableForPrevalence(s.id)) {
      var container = document.getElementById('section-' + s.id);
      if (container) container.innerHTML = '';
    }
  });
}
```

- [ ] **Step 5: Add threshold dropdown to renderOverview**

In `renderOverview()` (~line 2501), after `container.appendChild(heading)` and before the stats grid creation (`var stats = document.createElement('div')`), add:

```js
  // Threshold presets dropdown (fleet mode only)
  if (isFleetSnapshot()) {
    var thresholdDiv = document.createElement('div');
    thresholdDiv.style.cssText = 'display:flex;align-items:center;gap:0.5rem;margin-bottom:1rem;';

    var thresholdLabel = document.createElement('label');
    thresholdLabel.setAttribute('for', 'threshold-select');
    thresholdLabel.textContent = 'Prevalence threshold:';
    thresholdLabel.style.fontSize = '0.85rem';
    thresholdDiv.appendChild(thresholdLabel);

    var thresholdSelect = document.createElement('select');
    thresholdSelect.id = 'threshold-select';
    thresholdSelect.className = 'threshold-select';
    var presets = [
      {label: 'Unanimous (100%)', value: '1'},
      {label: 'Strong consensus (80%)', value: '0.8'},
      {label: 'Majority (50%)', value: '0.5'},
      {label: 'Any presence', value: '0'}
    ];
    for (var pi = 0; pi < presets.length; pi++) {
      var opt = document.createElement('option');
      opt.value = presets[pi].value;
      opt.textContent = presets[pi].label;
      if (parseFloat(presets[pi].value) === App.prevalenceThreshold) opt.selected = true;
      thresholdSelect.appendChild(opt);
    }
    thresholdSelect.onchange = function() {
      App.prevalenceThreshold = parseFloat(this.value);
      invalidateApplicableSections();
      updateAllBadges();
      updateSidebarThreshold();
    };
    thresholdDiv.appendChild(thresholdSelect);
    container.appendChild(thresholdDiv);
  }
```

Note: the handler does NOT call `renderOverview()`. The dropdown remains in the DOM untouched — only other sections are invalidated. This preserves focus on the `<select>` after the change.

- [ ] **Step 6: Add prevalence badge to buildToggleCard**

In `buildToggleCard()` (~line 3274), after the `metaEl` (`.toggle-card-meta`) is appended to `content`, and before the flatpak annotation check (`if (item.key.indexOf('flatpak-') === 0)`), add:

```js
  // Prevalence badge (fleet mode, applicable sections only)
  if (item.fleet && isApplicableForPrevalence(sectionId)) {
    var prevBadge = document.createElement('span');
    var zone = computePrevalenceZone(item, App.prevalenceThreshold);
    prevBadge.className = 'prevalence-badge' + (zone ? ' zone-' + zone : '');
    prevBadge.textContent = formatPrevalenceBadge(item);
    prevBadge.setAttribute('role', 'button');
    prevBadge.setAttribute('tabindex', '0');
    prevBadge.setAttribute('data-item-key', item.key);
    prevBadge.setAttribute('aria-label',
      'Fleet prevalence: ' + formatPrevalenceAriaLabel(item) + '. Activate to toggle format.');
    prevBadge.onclick = function(e) {
      e.stopPropagation();
      App.prevalenceFormat = App.prevalenceFormat === 'count' ? 'percent' : 'count';
      updateAllPrevalenceBadges();
      var announce = document.getElementById('prevalence-format-announce');
      if (announce) {
        announce.textContent = App.prevalenceFormat === 'percent'
          ? 'Showing percentages' : 'Showing host counts';
      }
    };
    prevBadge.addEventListener('keydown', function(e) {
      if (e.key === 'Enter' || e.key === ' ') {
        e.preventDefault();
        prevBadge.click();
      }
    });
    content.appendChild(prevBadge);
  }
```

- [ ] **Step 7: Add fleet reason text and host list in detail pane**

In `buildToggleCard()`, in the detail pane after `detailMeta.textContent = item.meta` and before the tier-3 warning block (`if (item.tier === 3)`), add. Note: this creates a NEW `.fleet-reason` element — it does not reuse the existing `.detail-reason` slot, which carries the triage classifier reason. The fleet reason is supplementary prevalence context appended after the existing metadata:

```js
  // Fleet reason text + host list
  if (item.fleet && isApplicableForPrevalence(sectionId)) {
    var fleetReason = document.createElement('div');
    fleetReason.className = 'fleet-reason';
    fleetReason.textContent = generateFleetReason(item);
    detail.appendChild(fleetReason);

    // Host list with missing hosts
    var fleetMeta = App.snapshot.meta && App.snapshot.meta.fleet;
    if (fleetMeta && fleetMeta.host_title_map && item.fleet.hosts) {
      var allHosts = Object.keys(fleetMeta.host_title_map);
      var presentSet = {};
      for (var hi = 0; hi < item.fleet.hosts.length; hi++) {
        presentSet[item.fleet.hosts[hi]] = true;
      }
      var missing = [];
      for (var ai = 0; ai < allHosts.length; ai++) {
        if (!presentSet[allHosts[ai]]) {
          var displayName = fleetMeta.host_title_map[allHosts[ai]] || allHosts[ai];
          missing.push(displayName);
        }
      }
      var presentNames = item.fleet.hosts.map(function(h) {
        return (fleetMeta.host_title_map && fleetMeta.host_title_map[h]) || h;
      });
      var hostText = 'Hosts: ' + presentNames.join(', ');
      if (missing.length > 0) {
        hostText += ' (missing: ' + missing.join(', ') + ')';
      }
      var hostEl = document.createElement('div');
      hostEl.className = 'detail-meta';
      hostEl.style.fontSize = '0.8rem';
      hostEl.textContent = hostText;
      detail.appendChild(hostEl);
    }
  }
```

- [ ] **Step 8: Add section header review counts**

In `renderTriageSection()` (~line 5647), replace the plain `heading.textContent = label` (for non-nonrpm sections) with:

```js
  if (sectionId !== 'nonrpm' && isFleetSnapshot() && isApplicableForPrevalence(sectionId)) {
    var allItemsForCount = getManifestItemsForSection(sectionId);
    var itemCount = allItemsForCount.length;
    var reviewCount = getFleetReviewCount(sectionId);
    if (reviewCount > 0) {
      heading.textContent = label + ' (' + itemCount + ' items, ' + reviewCount + ' to review)';
    } else {
      heading.textContent = label + ' (' + itemCount + ' items)';
    }
  } else if (sectionId !== 'nonrpm') {
    heading.textContent = label;
  }
```

- [ ] **Step 9: Add prevalence-aware sort within tier groups**

In `renderTriageSection()`, after the `tierSlots` array definition and before the `for` loop that iterates `tierSlots`, add:

```js
  // Prevalence-aware sort within each tier group (fleet mode, applicable sections)
  if (isFleetSnapshot() && isApplicableForPrevalence(sectionId)) {
    for (var ts = 0; ts < tierSlots.length; ts++) {
      tierSlots[ts].items.sort(function(a, b) {
        var zoneA = computePrevalenceZone(a, App.prevalenceThreshold);
        var zoneB = computePrevalenceZone(b, App.prevalenceThreshold);

        var zonePriority = {'below': 0, 'above': 1, 'unanimous': 2, '': 3};
        var pa = zonePriority[zoneA] !== undefined ? zonePriority[zoneA] : 3;
        var pb = zonePriority[zoneB] !== undefined ? zonePriority[zoneB] : 3;
        if (pa !== pb) return pa - pb;

        var ratioA = (a.fleet && a.fleet.total > 0) ? a.fleet.count / a.fleet.total : 1;
        var ratioB = (b.fleet && b.fleet.total > 0) ? b.fleet.count / b.fleet.total : 1;

        if (zoneA === 'below') return ratioA - ratioB;
        return ratioB - ratioA;
      });
    }
  }
```

- [ ] **Step 10: Add sidebar threshold indicator**

In `renderSidebar()` (~line 2247), after `nav.appendChild(list)` and before the keyboard navigation listener, add. The indicator is placed after the nav list, which positions it below the review-progress bar in the sidebar visual hierarchy:

```js
  // Threshold indicator (fleet mode only) — below review progress in sidebar
  if (isFleetSnapshot()) {
    var thresholdSpan = document.createElement('div');
    thresholdSpan.id = 'sidebar-threshold';
    thresholdSpan.className = 'sidebar-threshold';
    thresholdSpan.setAttribute('aria-live', 'polite');
    thresholdSpan.textContent = 'Threshold: ' + getThresholdPresetLabel(App.prevalenceThreshold);
    nav.parentElement.appendChild(thresholdSpan);
  }
```

Note: text content is set directly on the element before it's appended — NOT via `updateSidebarThreshold()` which depends on `getElementById` (element isn't in the DOM yet at this point). Subsequent updates after threshold changes go through `updateSidebarThreshold()`.

- [ ] **Step 11: Add secondary fleet badge to sidebar**

In `updateBadge()` (~line 2416), after the final `else { badge.style.display = 'none'; }` block that hides the tier badge, add:

```js
  // Fleet mode: secondary fleet review badge (never replaces tier badge)
  var fleetBadge = document.getElementById('fleet-badge-' + sectionId);
  if (isFleetSnapshot() && isApplicableForPrevalence(sectionId)) {
    var fleetReviewCount = getFleetReviewCount(sectionId);
    if (!fleetBadge) {
      // Create fleet badge element on first call
      var navLink = badge.parentElement;
      if (navLink) {
        fleetBadge = document.createElement('span');
        fleetBadge.id = 'fleet-badge-' + sectionId;
        fleetBadge.className = 'fleet-review-badge';
        navLink.appendChild(fleetBadge);
      }
    }
    if (fleetBadge) {
      if (fleetReviewCount > 0) {
        fleetBadge.textContent = fleetReviewCount;
        fleetBadge.style.display = '';
        fleetBadge.setAttribute('aria-label', fleetReviewCount + ' items below prevalence threshold');
      } else {
        fleetBadge.style.display = 'none';
      }
    }
  } else if (fleetBadge) {
    fleetBadge.style.display = 'none';
  }
```

This creates a separate `<span class="fleet-review-badge">` after the tier badge. The tier badge is never touched. The fleet badge is styled lighter/smaller via the `.fleet-review-badge` CSS class.

- [ ] **Step 12: Run full test suite**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/renderer/ -v -count=1`

Expected: All tests PASS

- [ ] **Step 13: Manual browser test — fleet tarball**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && go run ./cmd/inspectah refine <path-to-fleet-tarball>`

Verify:
- [ ] Prevalence badges on toggle cards (packages, runtime, identity, system)
- [ ] NO badges on secrets, config, containers, nonrpm
- [ ] Badges show "N/M" format, click toggles ALL badges to "%"
- [ ] Badge click does NOT expand card (stopPropagation)
- [ ] Screen reader format announcement via aria-live
- [ ] Threshold dropdown on Overview with 4 presets
- [ ] Changing threshold: applicable sections re-render on next navigation
- [ ] Changing threshold: sidebar badges and threshold indicator update immediately
- [ ] Changing threshold: focus stays on `<select>` (Overview not blown away)
- [ ] Section headers show "X items, Y to review" in applicable sections
- [ ] Expanded detail shows fleet reason text with correct patterns
- [ ] Expanded detail shows host list with missing hosts
- [ ] Sidebar threshold indicator shows preset label
- [ ] Sidebar fleet-review-badge shows below-threshold count separately from tier badge
- [ ] No rebuild triggered by threshold change
- [ ] No include state changes from threshold
- [ ] Dark mode: badges, dropdown, sidebar all visible

- [ ] **Step 14: Manual browser test — single-machine tarball**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && go run ./cmd/inspectah refine <path-to-single-machine-tarball>`

Verify:
- [ ] No prevalence badges anywhere
- [ ] No threshold dropdown
- [ ] No sidebar threshold indicator
- [ ] No fleet-review-badge
- [ ] All existing functionality unchanged

- [ ] **Step 15: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add cmd/inspectah/internal/renderer/static/report.html
git commit -m "feat(report): add fleet prevalence UI

Prevalence badges on toggle cards for applicable sections
(packages, runtime, identity, system). Badge shows N/M hosts
(click toggles to %). stopPropagation prevents card expand.

Threshold presets dropdown on Overview: Unanimous (100%),
Strong consensus (80%), Majority (50%), Any presence. Handler
invalidates all applicable sections for fresh render on
navigation — no stale pre-rendered content. Focus stays on
select (Overview DOM not recreated).

Section headers show 'X items, Y to review' based on threshold.
Prevalence-aware sort within tier groups.

Sidebar: threshold indicator with preset label, secondary
fleet-review-badge alongside (never replacing) tier badges.

Reason text in detail pane: threshold-aware with single-host
naming and host list with missing hosts.

Full accessibility: aria-label, role=button, keyboard Enter/Space,
aria-live format announcements, focus preservation.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 4: Automated verification

**Files:**
- All modified files from Tasks 1-3

- [ ] **Step 1: Run full Go test suite from module root**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./... -count=1`

Expected: All tests PASS across all packages

- [ ] **Step 2: Smoke check — threshold code does not reference state-mutation functions (code audit)**

This is a lightweight smoke check, not proof. The real proof is the E2E browser test in Step 5.

The threshold `onchange` handler calls only `invalidateApplicableSections()`, `updateAllBadges()`, and `updateSidebarThreshold()`. Quick audit that none reference include-state or change-counter functions:

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
grep -A5 'function invalidateApplicableSections' cmd/inspectah/internal/renderer/static/report.html
grep -A5 'function updateSidebarThreshold' cmd/inspectah/internal/renderer/static/report.html
```

Expected: `invalidateApplicableSections` only calls `container.innerHTML = ''`. `updateSidebarThreshold` only sets `el.textContent`. Neither references `updateSnapshotInclude`, `changeCount`, or rebuild signals.

- [ ] **Step 3: Keyboard and accessibility audit**

Using keyboard navigation in the browser:
- [ ] Tab order per toggle card row: toggle switch → prevalence badge → expand chevron (3 interactive stops — item name is display text, not a focusable control, matching the existing "two tab stops per row" contract extended by one for the badge)
- [ ] Enter/Space on badge toggles format globally
- [ ] Badge click does NOT expand card (stopPropagation verified)
- [ ] `#prevalence-format-announce` updates on format toggle
- [ ] Threshold dropdown accessible via keyboard (Tab, Enter, Arrow keys)
- [ ] `#sidebar-threshold` has `aria-live="polite"` and updates on threshold change

- [ ] **Step 4: Verify reason text patterns**

In a fleet tarball refine session, expand toggle cards and verify:
- 100% prevalence: `Present on all 3 hosts`
- Sub-threshold (default 100%): `Present on 2/3 hosts — review recommended`
- Single host: `Present on 1/3 hosts (hostname only) — review recommended`
- Above threshold (after lowering to 50%): `Present on 2/3 hosts` (no suffix)

- [ ] **Step 5: Write and run `fleet-threshold-no-dirty-state` E2E browser test**

Create `tests/e2e-go/tests/fleet-threshold-no-dirty-state.spec.ts`. This test follows the repo's existing refine-harness conventions: `resetServer()` in `beforeAll`/`afterAll` for clean state isolation, `waitForBoot()` for SPA readiness, and `isRefineMode()` guard to confirm refine-server liveness before exercising interactive controls. The dirty-state signal is `#changes-badge` (not `#rebuild-bar`, which is always active in refine mode).

```typescript
/**
 * Proves the presentation-only threshold invariant:
 * changing the threshold dropdown must NOT mutate include state,
 * increment the change counter, or lose focus.
 *
 * Follows the same harness pattern as rebuild-cycle.spec.ts:
 * resetServer() for state isolation, waitForBoot() + isRefineMode()
 * for readiness.
 */
import { test, expect } from '@playwright/test';
import { waitForBoot, navigateToSection, isRefineMode, resetServer } from './helpers';

test.describe('fleet threshold is presentation-only', () => {
  test.beforeAll(async () => { await resetServer(); });
  test.afterAll(async () => { await resetServer(); });

  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    await waitForBoot(page);
    const refine = await isRefineMode(page);
    expect(refine).toBe(true);
  });

  test('threshold change does not dirty state', async ({ page }) => {
    await navigateToSection(page, 'overview');

    const select = page.locator('#threshold-select');
    await expect(select).toBeVisible();

    // #changes-badge is the real dirty-state signal
    const changesBadge = page.locator('#changes-badge');
    await expect(changesBadge).toBeHidden();

    // Capture snapshot byte-for-byte before threshold change
    const snapshotBefore = await page.evaluate(() => {
      return JSON.stringify((window as any).App.snapshot);
    });

    // Change threshold to "Majority (50%)"
    await select.selectOption('0.5');

    // Assert: changes badge stays hidden (no dirty state)
    await expect(changesBadge).toBeHidden();

    // Assert: snapshot is byte-for-byte unchanged (no include mutations)
    const snapshotAfter = await page.evaluate(() => {
      return JSON.stringify((window as any).App.snapshot);
    });
    expect(snapshotAfter).toBe(snapshotBefore);

    // Assert: focus stays on the threshold select
    const focusedId = await page.evaluate(() => document.activeElement?.id);
    expect(focusedId).toBe('threshold-select');

    // Cycle through remaining presets — same invariant holds
    for (const value of ['0.8', '0', '1']) {
      await select.selectOption(value);
      await expect(changesBadge).toBeHidden();

      const snap = await page.evaluate(() => {
        return JSON.stringify((window as any).App.snapshot);
      });
      expect(snap).toBe(snapshotBefore);
    }
  });

  test('threshold change updates presentation on navigation', async ({ page }) => {
    await navigateToSection(page, 'overview');

    const select = page.locator('#threshold-select');
    await expect(select).toBeVisible();

    // Change to "Any presence" (0) — no items should be below threshold
    await select.selectOption('0');

    // Navigate to packages — section was invalidated, re-renders fresh
    await navigateToSection(page, 'packages');
    const heading = page.locator('#heading-packages');
    await expect(heading).toBeVisible();

    // At threshold 0, no items are "below", so no "to review" suffix
    const headingText = await heading.textContent();
    expect(headingText).not.toContain('to review');

    // Navigate back to Overview — dropdown should still show "Any presence"
    await navigateToSection(page, 'overview');
    const selectedValue = await page.locator('#threshold-select').inputValue();
    expect(selectedValue).toBe('0');
  });
});
```

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/tests/e2e-go && npx playwright test fleet-threshold-no-dirty-state.spec.ts`

Expected: Both tests PASS

- [ ] **Step 6: Commit E2E test**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add tests/e2e-go/tests/fleet-threshold-no-dirty-state.spec.ts
git commit -m "test(e2e): add fleet-threshold-no-dirty-state browser proof

Automated Playwright test proving the presentation-only threshold
invariant: threshold changes do not increment change counter,
mutate snapshot include values, activate rebuild UI, or lose
focus. Also verifies that navigation after threshold change
produces fresh section renders with updated prevalence counts.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Deferred items

### Secrets prevalence (follow-up)

The spec lists `secrets` as an applicable section, but the current implementation routes secret items through `buildSecretCard()` / `buildSecretDecidedCard()` (not `buildToggleCard()`), and `classifySecretItems()` does not propagate Fleet data. Adding secrets prevalence requires:
1. A fleet-data source for secret items (likely derived from the backing config entry via `source_path`)
2. Badge rendering in the secret card builders

### Additional E2E browser tests (follow-up)

The spec defines additional E2E tests in `tests/e2e-go/tests/`. The `fleet-threshold-no-dirty-state` test is already required in Task 4. The remaining tests are follow-up coverage:

| Test | Assertion |
|---|---|
| `fleet-prevalence-badge-visible` | Fleet report toggle cards in applicable sections show `N/M hosts` badge |
| `fleet-prevalence-badge-absent-config` | Config section cards do NOT show prevalence badges |
| `fleet-prevalence-badge-absent-single` | Single-machine report shows no prevalence badges |
| `fleet-badge-format-toggle` | Clicking any badge toggles ALL badges between count/percent |
| `fleet-badge-no-expand` | Clicking badge does NOT expand/collapse card |

The manual tests in Task 3 Steps 13-14 cover these assertions until the E2E tests are built.
