# Triage UX Fixes Implementation Plan

*Revision 3 — fixes semod-* routing, current_value field, sidecar proof (now through RunRefine), browser checklist, contradictory snippets, expandedDep reset.*

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reduce package noise to leaf-only triage, surface version changes, and move SELinux to the System & Security section.

**Architecture:** Normalize leaf package defaults at extraction time (before sidecar). Classifier filters to leaf packages in single-machine mode and populates dep data from `LeafDepTree`. New `classifyVersionChanges` function for display-only version change items. SELinux moved from `classifyIdentity` to `classifySystemItems` with truthful surface types. JS adds dep drill-down chevron and passive-item accounting exclusion.

**Tech Stack:** Go (schema, classifier, refine server), vanilla JS (report.html SPA), Go unit tests.

**Spec:** `docs/specs/proposed/2026-05-03-triage-ux-fixes.md` (revision 4, approved)

**Repo:** `/Users/mrussell/Work/bootc-migration/inspectah/` on `go-port` branch.

**Working directory convention:** All `Run:` commands and `git add`/`git commit` commands execute from the repo root (`/Users/mrussell/Work/bootc-migration/inspectah/`). Test commands use `cd cmd/inspectah &&` prefix to enter the Go module root.

---

## File Structure

### Go changes
- **Modify:** `cmd/inspectah/internal/renderer/triage.go` — add `Deps` field to `TriageItem`, add `extractDeps` helper, update `classifyPackages` for leaf-only filtering, add `classifyVersionChanges`, move SELinux from `classifyIdentity` to `classifySystemItems`
- **Modify:** `cmd/inspectah/internal/renderer/triage_test.go` — tests for leaf filtering, deps normalization, version changes, SELinux section move
- **Modify:** `cmd/inspectah/internal/refine/server.go` — reorder normalization before sidecar creation, add `NormalizeLeafDefaults` call
- **Modify:** `cmd/inspectah/internal/refine/server_test.go` — sidecar-path proof for leaf normalization
- **Modify:** `cmd/inspectah/internal/renderer/html.go` — add `NormalizeLeafDefaults` call for static report path
- **Modify:** `cmd/inspectah/internal/renderer/html_test.go` — golden HTML tests for leaf deps and version changes

### JS changes
- **Modify:** `cmd/inspectah/internal/renderer/static/report.html` — dep drill-down in accordion rows, `isPassiveItem` accounting exclusion, `MIGRATION_SECTIONS` label change, `semod-*` acknowledged handlers, SELinux-specific notification copy in `buildNotificationCard`, `restoreFocus` dep-chevron support, `App.expandedDep` with reset on accordion rerender

---

### Task 1: Add Deps field to TriageItem and extractDeps helper

**Files:**
- Modify: `cmd/inspectah/internal/renderer/triage.go`
- Test: `cmd/inspectah/internal/renderer/triage_test.go`

- [ ] **Step 1: Write failing tests for extractDeps**

```go
func TestExtractDeps(t *testing.T) {
	tests := []struct {
		name     string
		depTree  map[string]interface{}
		leafName string
		want     []string
	}{
		{"nil tree", nil, "vim", nil},
		{"missing key", map[string]interface{}{"bash": []interface{}{"readline"}}, "vim", nil},
		{"nil value", map[string]interface{}{"vim": nil}, "vim", nil},
		{"empty array interface", map[string]interface{}{"vim": []interface{}{}}, "vim", nil},
		{"valid interface slice", map[string]interface{}{
			"vim": []interface{}{"vim-common", "gpm-libs", "vim-filesystem"},
		}, "vim", []string{"vim-common", "gpm-libs", "vim-filesystem"}},
		{"string slice (Go-native)", map[string]interface{}{
			"vim": []string{"vim-common", "gpm-libs"},
		}, "vim", []string{"vim-common", "gpm-libs"}},
		{"empty string slice", map[string]interface{}{"vim": []string{}}, "vim", nil},
		{"mixed types in interface slice", map[string]interface{}{
			"vim": []interface{}{"vim-common", 42, "gpm-libs"},
		}, "vim", []string{"vim-common", "gpm-libs"}},
		{"wrong type value", map[string]interface{}{"vim": "not-a-slice"}, "vim", nil},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := extractDeps(tt.depTree, tt.leafName)
			assert.Equal(t, tt.want, got)
		})
	}
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd cmd/inspectah && go test ./internal/renderer/ -run "TestExtractDeps" -v`
Expected: FAIL — `extractDeps` not defined.

- [ ] **Step 3: Add Deps field to TriageItem and extractDeps helper**

In `cmd/inspectah/internal/renderer/triage.go`, add `Deps` field to `TriageItem` after `Acknowledged`:

```go
Deps           []string `json:"deps,omitempty"`
```

Add the `extractDeps` helper:

```go
func extractDeps(depTree map[string]interface{}, leafName string) []string {
	if depTree == nil {
		return nil
	}
	raw, ok := depTree[leafName]
	if !ok || raw == nil {
		return nil
	}
	if strSlice, ok := raw.([]string); ok {
		if len(strSlice) == 0 {
			return nil
		}
		return strSlice
	}
	arr, ok := raw.([]interface{})
	if !ok {
		return nil
	}
	deps := make([]string, 0, len(arr))
	for _, v := range arr {
		if s, ok := v.(string); ok {
			deps = append(deps, s)
		}
	}
	if len(deps) == 0 {
		return nil
	}
	return deps
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd cmd/inspectah && go test ./internal/renderer/ -run "TestExtractDeps" -v`
Expected: PASS

- [ ] **Step 5: Run full renderer test suite**

Run: `cd cmd/inspectah && go test ./internal/renderer/ -v`
Expected: All PASS.

- [ ] **Step 6: Commit**

```bash
git add cmd/inspectah/internal/renderer/triage.go cmd/inspectah/internal/renderer/triage_test.go
git commit -m "triage: add Deps field to TriageItem and extractDeps helper

Handles both []interface{} (JSON-decoded) and []string (Go-native)
LeafDepTree shapes. Nil, empty, missing key, and mixed-type cases
all return nil.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 2: Leaf-only package filtering in classifyPackages

**Files:**
- Modify: `cmd/inspectah/internal/renderer/triage.go`
- Test: `cmd/inspectah/internal/renderer/triage_test.go`

- [ ] **Step 1: Write failing tests for leaf-only filtering**

```go
func TestClassifyPackages_LeafOnly_SingleMachine(t *testing.T) {
	leafNames := []string{"vim", "htop"}
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		LeafPackages: &leafNames,
		AutoPackages: &[]string{"vim-common", "gpm-libs"},
		LeafDepTree: map[string]interface{}{
			"vim":  []interface{}{"vim-common", "gpm-libs"},
			"htop": []interface{}{},
		},
		PackagesAdded: []schema.PackageEntry{
			{Name: "vim", Arch: "x86_64", Include: true, SourceRepo: "appstream", Version: "9.1", Release: "1.el9"},
			{Name: "vim-common", Arch: "x86_64", Include: false, SourceRepo: "appstream", Version: "9.1", Release: "1.el9"},
			{Name: "gpm-libs", Arch: "x86_64", Include: false, SourceRepo: "appstream", Version: "1.20", Release: "1.el9"},
			{Name: "htop", Arch: "x86_64", Include: true, SourceRepo: "epel", Version: "3.3", Release: "1.el9"},
		},
	}

	items := classifyPackages(snap, make(map[string]bool), false)

	assert.Equal(t, 2, len(items), "should only have 2 leaf packages, not 4")

	vim := findItem(items, "pkg-vim-x86_64")
	require.NotNil(t, vim)
	assert.Equal(t, "repo:appstream", vim.Group)
	assert.Equal(t, []string{"vim-common", "gpm-libs"}, vim.Deps)

	htop := findItem(items, "pkg-htop-x86_64")
	require.NotNil(t, htop)
	assert.Nil(t, htop.Deps, "htop has empty deps, should be nil")

	assert.Nil(t, findItem(items, "pkg-vim-common-x86_64"))
	assert.Nil(t, findItem(items, "pkg-gpm-libs-x86_64"))
}

func TestClassifyPackages_LeafOnly_FleetStillShowsAll(t *testing.T) {
	leafNames := []string{"vim"}
	snap := schema.NewSnapshot()
	snap.Meta["fleet"] = map[string]interface{}{"source_hosts": []interface{}{"h1"}}
	snap.Rpm = &schema.RpmSection{
		LeafPackages: &leafNames,
		PackagesAdded: []schema.PackageEntry{
			{Name: "vim", Arch: "x86_64", Include: true, SourceRepo: "appstream"},
			{Name: "vim-common", Arch: "x86_64", Include: false, SourceRepo: "appstream"},
		},
	}

	items := classifyPackages(snap, make(map[string]bool), true)
	assert.Equal(t, 2, len(items), "fleet mode should show ALL packages including deps")
}

func TestClassifyPackages_NoLeafPackages_ShowsAll(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		PackagesAdded: []schema.PackageEntry{
			{Name: "vim", Arch: "x86_64", Include: true, SourceRepo: "appstream"},
			{Name: "vim-common", Arch: "x86_64", Include: true, SourceRepo: "appstream"},
		},
	}

	items := classifyPackages(snap, make(map[string]bool), false)
	assert.Equal(t, 2, len(items), "without LeafPackages, all packages should appear")
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd cmd/inspectah && go test ./internal/renderer/ -run "TestClassifyPackages_LeafOnly" -v`
Expected: FAIL — leaf filtering not implemented.

- [ ] **Step 3: Update classifyPackages with leaf-only filtering**

In `classifyPackages`, after `baselineNames` setup and before the `PackagesAdded` loop, add:

```go
var leafSet map[string]bool
leafOnly := !isFleet && snap.Rpm.LeafPackages != nil
if leafOnly {
	leafSet = make(map[string]bool)
	for _, name := range *snap.Rpm.LeafPackages {
		leafSet[name] = true
	}
}
```

At the top of the `PackagesAdded` loop, after the secrets check, add:

```go
if leafOnly && !leafSet[pkg.Name] {
	continue
}
```

After creating the `item` TriageItem (before the `!isFleet` grouping block), add deps:

```go
if leafOnly {
	item.Deps = extractDeps(snap.Rpm.LeafDepTree, pkg.Name)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd cmd/inspectah && go test ./internal/renderer/ -v`
Expected: All PASS.

- [ ] **Step 5: Commit**

```bash
git add cmd/inspectah/internal/renderer/triage.go cmd/inspectah/internal/renderer/triage_test.go
git commit -m "triage: filter to leaf-only packages in single-machine mode

When !isFleet and LeafPackages is populated, only leaf packages
appear in the triage manifest. Deps populated from LeafDepTree.
Fleet mode and no-LeafPackages fallback show all packages.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 3: Normalize leaf defaults in refine server and HTML renderer

**Files:**
- Modify: `cmd/inspectah/internal/refine/server.go`
- Modify: `cmd/inspectah/internal/renderer/html.go`
- Modify: `cmd/inspectah/internal/renderer/triage.go` (add `NormalizeLeafDefaults` as exported)
- Test: `cmd/inspectah/internal/renderer/triage_test.go`

- [ ] **Step 1: Write failing test for NormalizeLeafDefaults**

```go
func TestNormalizeLeafDefaults(t *testing.T) {
	t.Run("sets leaf includes to true", func(t *testing.T) {
		leafNames := []string{"vim", "htop"}
		snap := schema.NewSnapshot()
		snap.Rpm = &schema.RpmSection{
			LeafPackages: &leafNames,
			PackagesAdded: []schema.PackageEntry{
				{Name: "vim", Arch: "x86_64", Include: false},
				{Name: "vim-common", Arch: "x86_64", Include: false},
				{Name: "htop", Arch: "x86_64", Include: false},
			},
		}

		NormalizeLeafDefaults(snap)

		assert.True(t, snap.Rpm.PackagesAdded[0].Include, "leaf vim should be true")
		assert.False(t, snap.Rpm.PackagesAdded[1].Include, "dep vim-common should stay false")
		assert.True(t, snap.Rpm.PackagesAdded[2].Include, "leaf htop should be true")
	})

	t.Run("skips fleet snapshots", func(t *testing.T) {
		leafNames := []string{"vim"}
		snap := schema.NewSnapshot()
		snap.Meta["fleet"] = map[string]interface{}{"source_hosts": []interface{}{"h1"}}
		snap.Rpm = &schema.RpmSection{
			LeafPackages: &leafNames,
			PackagesAdded: []schema.PackageEntry{
				{Name: "vim", Arch: "x86_64", Include: false},
			},
		}

		NormalizeLeafDefaults(snap)
		assert.False(t, snap.Rpm.PackagesAdded[0].Include, "fleet snapshot should not be normalized")
	})

	t.Run("no-op when LeafPackages nil", func(t *testing.T) {
		snap := schema.NewSnapshot()
		snap.Rpm = &schema.RpmSection{
			PackagesAdded: []schema.PackageEntry{
				{Name: "vim", Arch: "x86_64", Include: false},
			},
		}

		NormalizeLeafDefaults(snap)
		assert.False(t, snap.Rpm.PackagesAdded[0].Include)
	})
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd cmd/inspectah && go test ./internal/renderer/ -run "TestNormalizeLeafDefaults" -v`
Expected: FAIL — `NormalizeLeafDefaults` not defined.

- [ ] **Step 3: Add NormalizeLeafDefaults (exported) to triage.go**

```go
func NormalizeLeafDefaults(snap *schema.InspectionSnapshot) {
	if snap.Rpm == nil || snap.Rpm.LeafPackages == nil || isFleetSnapshot(snap) {
		return
	}
	leafSet := make(map[string]bool)
	for _, name := range *snap.Rpm.LeafPackages {
		leafSet[name] = true
	}
	for i := range snap.Rpm.PackagesAdded {
		if leafSet[snap.Rpm.PackagesAdded[i].Name] {
			snap.Rpm.PackagesAdded[i].Include = true
		}
	}
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd cmd/inspectah && go test ./internal/renderer/ -run "TestNormalizeLeafDefaults" -v`
Expected: PASS

- [ ] **Step 5: Update RunRefine to normalize BEFORE sidecar**

In `cmd/inspectah/internal/refine/server.go`, find the sidecar creation block (around line 110-119). The current order is:

1. Save sidecar from raw snapshot bytes
2. Normalize snapshot (NormalizeSnapshot)

Change to:

1. Load, normalize leaf defaults AND NormalizeSnapshot
2. Save working snapshot
3. Save sidecar from the NORMALIZED snapshot bytes

```go
// Normalize snapshot BEFORE sidecar — leaf defaults and nil *bool fixes.
// Both the sidecar and working copy must agree on initial include state.
if snap, err := schema.LoadSnapshot(snapPath); err == nil {
	renderer.NormalizeLeafDefaults(snap)
	schema.NormalizeSnapshot(snap)
	schema.SaveSnapshot(snap, snapPath)
}

// Create immutable sidecar from the NORMALIZED snapshot
sidecarPath := filepath.Join(tmpDir, "original-inspection-snapshot.json")
if _, err := os.Stat(sidecarPath); os.IsNotExist(err) {
	if snapData, err := os.ReadFile(snapPath); err == nil {
		os.WriteFile(sidecarPath, snapData, 0444)
	}
}
```

Remove the old sidecar-first block and the old normalize-after block.

Add import for `renderer` package: `"github.com/marrusl/inspectah/cmd/inspectah/internal/renderer"`

- [ ] **Step 6: Update RenderHTMLReport to normalize for static reports**

In `cmd/inspectah/internal/renderer/html.go`, add `NormalizeLeafDefaults(snap)` BEFORE `snapJSON, err := json.Marshal(snap)`:

```go
NormalizeLeafDefaults(snap)

snapJSON, err := json.Marshal(snap)
```

- [ ] **Step 7: Run full test suite**

Run: `cd cmd/inspectah && go test ./internal/renderer/ ./internal/refine/ -v`
Expected: All PASS.

- [ ] **Step 8: Commit**

```bash
git add cmd/inspectah/internal/renderer/triage.go cmd/inspectah/internal/renderer/triage_test.go cmd/inspectah/internal/refine/server.go cmd/inspectah/internal/renderer/html.go
git commit -m "fix: normalize leaf defaults before sidecar creation

NormalizeLeafDefaults sets Include=true on leaf packages. Runs
before sidecar is saved so both copies agree on initial state.
isItemDecided correctly reports untouched leaves as undecided.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 4: Add classifyVersionChanges

**Files:**
- Modify: `cmd/inspectah/internal/renderer/triage.go`
- Test: `cmd/inspectah/internal/renderer/triage_test.go`

- [ ] **Step 1: Write failing tests**

```go
func TestClassifyVersionChanges_SingleMachine(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		VersionChanges: []schema.VersionChange{
			{Name: "bash", Arch: "x86_64", HostVersion: "5.2.26", BaseVersion: "5.2.32", Direction: schema.VersionChangeUpgrade},
			{Name: "openssl", Arch: "x86_64", HostVersion: "3.2.2", BaseVersion: "3.2.1", Direction: schema.VersionChangeDowngrade},
		},
	}

	items := classifyVersionChanges(snap, false)
	assert.Equal(t, 2, len(items))

	bash := findItem(items, "verchg-bash-x86_64")
	require.NotNil(t, bash)
	assert.Equal(t, "packages", bash.Section)
	assert.Equal(t, 1, bash.Tier)
	assert.True(t, bash.DisplayOnly)
	assert.Equal(t, "sub:version-upgrades", bash.Group)
	assert.Contains(t, bash.Meta, "→")

	openssl := findItem(items, "verchg-openssl-x86_64")
	require.NotNil(t, openssl)
	assert.Equal(t, "sub:version-downgrades", openssl.Group)
}

func TestClassifyVersionChanges_Fleet_ReturnsNil(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		VersionChanges: []schema.VersionChange{
			{Name: "bash", Arch: "x86_64", Direction: schema.VersionChangeUpgrade},
		},
	}

	items := classifyVersionChanges(snap, true)
	assert.Nil(t, items)
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd cmd/inspectah && go test ./internal/renderer/ -run "TestClassifyVersionChanges" -v`
Expected: FAIL

- [ ] **Step 3: Add classifyVersionChanges and wire into classifyAll**

```go
func classifyVersionChanges(snap *schema.InspectionSnapshot, isFleet bool) []TriageItem {
	if snap.Rpm == nil || len(snap.Rpm.VersionChanges) == 0 || isFleet {
		return nil
	}
	var items []TriageItem
	for _, vc := range snap.Rpm.VersionChanges {
		group := "sub:version-upgrades"
		if vc.Direction == schema.VersionChangeDowngrade {
			group = "sub:version-downgrades"
		}
		items = append(items, TriageItem{
			Section:     "packages",
			Key:         "verchg-" + vc.Name + "-" + vc.Arch,
			Tier:        1,
			Reason:      fmt.Sprintf("Package %s from %s to %s.", vc.Direction, vc.HostVersion, vc.BaseVersion),
			Name:        vc.Name,
			Meta:        vc.HostVersion + " → " + vc.BaseVersion,
			Group:       group,
			DisplayOnly: true,
		})
	}
	return items
}
```

In `classifyAll`, add after `classifySecretItems`:

```go
items = append(items, classifyVersionChanges(snap, isFleet)...)
```

- [ ] **Step 4: Run tests**

Run: `cd cmd/inspectah && go test ./internal/renderer/ -v`
Expected: All PASS.

- [ ] **Step 5: Commit**

```bash
git add cmd/inspectah/internal/renderer/triage.go cmd/inspectah/internal/renderer/triage_test.go
git commit -m "triage: add version changes display-only accordion

classifyVersionChanges creates tier-1 display-only items grouped
by direction (upgrades/downgrades) in the packages section.
Single-machine only — fleet returns nil.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 5: Move SELinux from identity to system

**Files:**
- Modify: `cmd/inspectah/internal/renderer/triage.go`
- Test: `cmd/inspectah/internal/renderer/triage_test.go`

- [ ] **Step 1: Write failing tests**

Note: SELinux boolean overrides use `current_value` (not `value`) on the live branch. Test fixtures must match this shape.

```go
func TestClassifySELinux_InSystemSection(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Selinux = &schema.SelinuxSection{
		BooleanOverrides: []map[string]interface{}{
			{"name": "httpd_can_network_connect", "current_value": "on", "include": true},
		},
		CustomModules: []string{"myapp"},
		PortLabels: []schema.SelinuxPortLabel{
			{Protocol: "tcp", Port: "8443", Type: "http_port_t", Include: true},
		},
	}

	items := ClassifySnapshot(snap, nil)

	sebool := findItem(items, "sebool-httpd_can_network_connect")
	require.NotNil(t, sebool)
	assert.Equal(t, "system", sebool.Section)
	assert.Equal(t, "sub:selinux", sebool.Group)
	assert.Equal(t, 2, sebool.Tier)
	assert.Equal(t, "on", sebool.Meta)

	semod := findItem(items, "semod-myapp")
	require.NotNil(t, semod)
	assert.Equal(t, "system", semod.Section)
	assert.Equal(t, "", semod.Group, "semod-* must be ungrouped to route to buildNotificationCard")
	assert.Equal(t, 3, semod.Tier)
	assert.Equal(t, "notification", semod.CardType)

	seport := findItem(items, "seport-tcp-8443")
	require.NotNil(t, seport)
	assert.Equal(t, "system", seport.Section)
	assert.Equal(t, "sub:selinux", seport.Group)

	for _, item := range items {
		if item.Section == "identity" {
			assert.NotContains(t, item.Key, "sebool-")
			assert.NotContains(t, item.Key, "semod-")
			assert.NotContains(t, item.Key, "seport-")
		}
	}
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd cmd/inspectah && go test ./internal/renderer/ -run "TestClassifySELinux_InSystem" -v`
Expected: FAIL — SELinux items still in "identity".

- [ ] **Step 3: Remove SELinux from classifyIdentity**

In `classifyIdentity`, remove the three SELinux loops (BooleanOverrides, CustomModules, PortLabels) and their `if snap.Selinux != nil {` wrapper. Keep users and groups loops.

- [ ] **Step 4: Add SELinux classification to classifySystemItems**

At the end of `classifySystemItems` (before `return items`), add:

```go
if snap.Selinux != nil {
	for _, b := range snap.Selinux.BooleanOverrides {
		name, _ := b["name"].(string)
		val, _ := b["current_value"].(string)
		if name == "" {
			continue
		}
		group := ""
		if !isFleet {
			group = "sub:selinux"
		}
		items = append(items, TriageItem{
			Section: "system", Key: "sebool-" + name,
			Tier: 2, Reason: "SELinux boolean changed from default.",
			Name: name, Meta: val,
			DefaultInclude: mapInclude(b),
			Group: group,
		})
	}
	for _, m := range snap.Selinux.CustomModules {
		// semod-* is UNGROUPED — it must route to buildNotificationCard,
		// which only renders ungrouped items. Do NOT set Group here.
		items = append(items, TriageItem{
			Section: "system", Key: "semod-" + m,
			Tier: 3, Reason: "Custom SELinux policy module. inspectah cannot yet generate semodule installation commands — manual Containerfile steps required.",
			Name: m,
			DefaultInclude: true,
			CardType: "notification",
		})
	}
	for _, p := range snap.Selinux.PortLabels {
		group := ""
		if !isFleet {
			group = "sub:selinux"
		}
		items = append(items, TriageItem{
			Section: "system", Key: fmt.Sprintf("seport-%s-%s", p.Protocol, p.Port),
			Tier: 2, Reason: "Custom SELinux port label.",
			Name:           fmt.Sprintf("%s/%s -> %s", p.Protocol, p.Port, p.Type),
			DefaultInclude: p.Include,
			Group: group,
		})
	}
}
```

- [ ] **Step 5: Run tests**

Run: `cd cmd/inspectah && go test ./internal/renderer/ -v`
Expected: All PASS.

- [ ] **Step 6: Commit**

```bash
git add cmd/inspectah/internal/renderer/triage.go cmd/inspectah/internal/renderer/triage_test.go
git commit -m "triage: move SELinux from identity to system section

sebool and seport are output-affecting toggles grouped as
sub:selinux. semod is ungrouped notification card (routes to
buildNotificationCard, renderer emits FIXME stubs).

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 6: JS — section label, passive accounting, semod-* handlers, notification copy

**Files:**
- Modify: `cmd/inspectah/internal/renderer/static/report.html`

- [ ] **Step 1: Update MIGRATION_SECTIONS label**

Find `{id: 'system',     label: 'System',` and change to:
```javascript
{id: 'system',     label: 'System & Security', tracked: true},
```

- [ ] **Step 2: Add isPassiveItem helper**

Add near `isItemDecided`:
```javascript
function isPassiveItem(item) {
  return item.display_only && item.group && item.key.indexOf('verchg-') === 0;
}
```

- [ ] **Step 3: Update section footer to exclude passive items**

In `renderTriageSection`, find the footer's decided count loop. Replace it with:
```javascript
var decidedCount = 0;
var accountableTotal = 0;
for (var fc = 0; fc < allItems.length; fc++) {
  if (isPassiveItem(allItems[fc])) continue;
  accountableTotal++;
  if (isItemDecided(allItems[fc]) || (allItems[fc].display_only && getSnapshotAcknowledged(allItems[fc].key))) decidedCount++;
}
```

Update the stats text to use `accountableTotal`:
```javascript
statsSpan.textContent = decidedCount + ' / ' + accountableTotal + ' decided';
```

- [ ] **Step 4: Update updateBadge to exclude passive items**

In `updateBadge`, add `if (isPassiveItem(items[i])) continue;` at the top of the loop body, before the `isItemDecided` check.

- [ ] **Step 5: Add semod-* handlers to getSnapshotAcknowledged/setSnapshotAcknowledged**

In `getSnapshotAcknowledged`, add before the final `return false`:
```javascript
if (key.indexOf('semod-') === 0) {
  return App.decisions[key] || false;
}
```

In `setSnapshotAcknowledged`, add before the final closing brace:
```javascript
if (key.indexOf('semod-') === 0) {
  return;
}
```

- [ ] **Step 6: Update buildNotificationCard with SELinux-specific copy**

In `buildNotificationCard`, find the description text (`desc.textContent = 'Provide a reproducible build-time source...'`). Replace with section-aware copy:

```javascript
var desc = document.createElement('div');
desc.style.cssText = 'font-size:13px;opacity:0.7;margin-bottom:10px;';
if (item.section === 'system' && item.key.indexOf('semod-') === 0) {
  desc.textContent = 'To include this module in the image, add COPY and semodule -i steps to your Containerfile manually.';
} else {
  desc.textContent = 'Provide a reproducible build-time source and add the installation to your Containerfile.';
}
full.appendChild(desc);
```

Also find the collapsed card warning text (`cWarn.textContent = 'no repo — manual install required'`). Update:

```javascript
if (item.section === 'system' && item.key.indexOf('semod-') === 0) {
  cWarn.textContent = 'custom module — manual Containerfile steps required';
} else if (item.card_type === 'notification' && item.section === 'containers') {
  cWarn.textContent = 'unclear provenance — manual follow-up required';
} else {
  cWarn.textContent = 'no repo — manual install required';
}
```

- [ ] **Step 7: Add dep-chevron to restoreFocus**

In `restoreFocus`, add a new target type case:
```javascript
} else if (targetType === 'dep-chevron') {
  var row = container.querySelector('[data-dep-key="' + targetId + '"]');
  if (row) target = row.querySelector('.dep-chevron');
}
```

- [ ] **Step 8: Run Go tests**

Run: `cd cmd/inspectah && go test ./internal/renderer/ -v`
Expected: All PASS.

- [ ] **Step 9: Commit**

```bash
git add cmd/inspectah/internal/renderer/static/report.html
git commit -m "report: System & Security label, passive accounting, semod handlers

Rename system section. Version-change items excluded from badge and
footer counts. semod-* gets session-only acknowledged proxy and
SELinux-specific notification copy. dep-chevron focus support.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 7: JS — dependency drill-down in accordion rows

**Files:**
- Modify: `cmd/inspectah/internal/renderer/static/report.html`

- [ ] **Step 1: Add App.expandedDep state**

In the `var App = {` block, add:
```javascript
expandedDep: null,
```

- [ ] **Step 2: Update buildOutputAccordion to reset expandedDep on rerender**

In `buildOutputAccordion`, find the header onclick handler. Add `App.expandedDep = null;` BEFORE the `renderTriageSection` call:

```javascript
header.onclick = function() {
    App.expandedDep = null;
    var expanded = App.tierExpanded['group-' + groupName];
    App.tierExpanded['group-' + groupName] = !expanded;
    var sectionId = items[0] ? items[0].section : '';
    if (sectionId) {
        renderTriageSection(sectionId);
        restoreFocus(sectionId, 'accordion-header', groupName);
    }
};
```

Also in `toggleAccordionGroup`, add `App.expandedDep = null;` at the top of the function.

- [ ] **Step 3: Update buildOutputAccordion expanded content with dep disclosure**

In `buildOutputAccordion`, find the expanded content `for` loop that creates `<tr>` rows. After `var tr = document.createElement('tr');` add:
```javascript
tr.setAttribute('data-dep-key', item.key);
```

After `tdName.textContent = item.name;` add the dep badge, chevron, and static-mode inline disclosure:
```javascript
if (item.deps && item.deps.length > 0) {
  var depBadge = document.createElement('span');
  depBadge.className = 'dep-badge';
  depBadge.textContent = item.deps.length + ' deps';
  tdName.appendChild(depBadge);

  if (App.mode !== 'static') {
    var depChevron = document.createElement('button');
    depChevron.className = 'dep-chevron';
    depChevron.type = 'button';
    var isDepExpanded = App.expandedDep === item.key;
    depChevron.setAttribute('aria-expanded', isDepExpanded ? 'true' : 'false');
    depChevron.setAttribute('aria-controls', 'deps-' + item.key.replace(/[^a-zA-Z0-9-]/g, '_'));
    depChevron.setAttribute('aria-label', (isDepExpanded ? 'Hide' : 'Show ' + item.deps.length) + ' dependencies for ' + item.name);
    depChevron.textContent = isDepExpanded ? '▾' : '▸';
    (function(itemKey, sId) {
      depChevron.onclick = function(e) {
        e.stopPropagation();
        App.expandedDep = (App.expandedDep === itemKey) ? null : itemKey;
        renderTriageSection(sId);
        restoreFocus(sId, 'dep-chevron', itemKey);
      };
    })(item.key, item.section);
    tdName.appendChild(depChevron);
  } else {
    var depStatic = document.createElement('span');
    depStatic.className = 'dep-list-static';
    depStatic.textContent = item.deps.join(', ');
    tdName.appendChild(depStatic);
  }
}
```

After `table.appendChild(tr);`, add the expanded dep row:
```javascript
if (item.deps && item.deps.length > 0 && App.expandedDep === item.key && App.mode !== 'static') {
  var depTr = document.createElement('tr');
  depTr.className = 'dep-row';
  var depTd = document.createElement('td');
  depTd.setAttribute('colspan', '3');
  var depUl = document.createElement('ul');
  depUl.setAttribute('aria-label', 'Dependencies for ' + item.name);
  depUl.id = 'deps-' + item.key.replace(/[^a-zA-Z0-9-]/g, '_');
  for (var d = 0; d < item.deps.length; d++) {
    var depLi = document.createElement('li');
    depLi.textContent = item.deps[d];
    depUl.appendChild(depLi);
  }
  depTd.appendChild(depUl);
  depTr.appendChild(depTd);
  table.appendChild(depTr);
}
```

- [ ] **Step 4: Add CSS for dep drill-down**

In the `<style>` section, add:
```css
.dep-badge {
  font-size: 11px;
  opacity: 0.5;
  margin-left: 8px;
}
.dep-chevron {
  background: none;
  border: none;
  color: inherit;
  font-size: 11px;
  cursor: pointer;
  padding: 0 4px;
  opacity: 0.5;
  margin-left: 4px;
}
.dep-chevron:hover, .dep-chevron:focus {
  opacity: 1;
}
.dep-row ul {
  margin: 4px 0 8px 24px;
  padding: 0;
  list-style: disc;
  font-size: 12px;
  opacity: 0.6;
  columns: 2;
  column-gap: 24px;
}
.dep-row li {
  padding: 1px 0;
}
.dep-list-static {
  display: block;
  font-size: 11px;
  opacity: 0.4;
  margin-top: 2px;
}
```

- [ ] **Step 5: Run Go tests**

Run: `cd cmd/inspectah && go test ./internal/renderer/ -v`
Expected: All PASS.

- [ ] **Step 6: Commit**

```bash
git add cmd/inspectah/internal/renderer/static/report.html
git commit -m "report: dependency drill-down in accordion rows

Chevron + dep count badge on leaf rows. Click expands read-only
dep sub-list (one at a time). Static mode shows deps inline.
expandedDep resets on accordion collapse/toggle. Two-column layout.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 8: Golden HTML tests + sidecar-path proof

**Files:**
- Modify: `cmd/inspectah/internal/renderer/html_test.go`
- Modify: `cmd/inspectah/internal/refine/server_test.go`

- [ ] **Step 1: Write golden test for leaf deps**

```go
func TestHTMLReportGoldenLeafDeps(t *testing.T) {
	leafNames := []string{"vim", "htop"}
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		LeafPackages: &leafNames,
		LeafDepTree: map[string]interface{}{
			"vim":  []interface{}{"vim-common", "gpm-libs"},
			"htop": nil,
		},
		PackagesAdded: []schema.PackageEntry{
			{Name: "vim", Arch: "x86_64", Include: false, SourceRepo: "appstream", Version: "9.1", Release: "1.el9"},
			{Name: "vim-common", Arch: "x86_64", Include: false, SourceRepo: "appstream", Version: "9.1", Release: "1.el9"},
			{Name: "gpm-libs", Arch: "x86_64", Include: false, SourceRepo: "appstream", Version: "1.20", Release: "1.el9"},
			{Name: "htop", Arch: "x86_64", Include: false, SourceRepo: "epel", Version: "3.3", Release: "1.el9"},
		},
	}

	dir := t.TempDir()
	err := RenderHTMLReport(snap, dir, HTMLReportOptions{})
	require.NoError(t, err)

	reportBytes, err := os.ReadFile(filepath.Join(dir, "report.html"))
	require.NoError(t, err)
	manifestJSON := extractTriageManifest(t, string(reportBytes))
	var items []TriageItem
	require.NoError(t, json.Unmarshal([]byte(manifestJSON), &items))

	assert.Equal(t, 2, len(items))

	vim := findItem(items, "pkg-vim-x86_64")
	require.NotNil(t, vim)
	assert.Equal(t, []string{"vim-common", "gpm-libs"}, vim.Deps)
	assert.True(t, vim.DefaultInclude, "leaf should have DefaultInclude true after normalization")

	htop := findItem(items, "pkg-htop-x86_64")
	require.NotNil(t, htop)
	assert.Nil(t, htop.Deps)
	assert.True(t, htop.DefaultInclude)
}
```

- [ ] **Step 2: Write golden test for version changes**

```go
func TestHTMLReportGoldenVersionChanges(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		VersionChanges: []schema.VersionChange{
			{Name: "bash", Arch: "x86_64", HostVersion: "5.2.26", BaseVersion: "5.2.32", Direction: schema.VersionChangeUpgrade},
		},
	}

	dir := t.TempDir()
	err := RenderHTMLReport(snap, dir, HTMLReportOptions{})
	require.NoError(t, err)

	reportBytes, _ := os.ReadFile(filepath.Join(dir, "report.html"))
	manifestJSON := extractTriageManifest(t, string(reportBytes))
	var items []TriageItem
	require.NoError(t, json.Unmarshal([]byte(manifestJSON), &items))

	bash := findItem(items, "verchg-bash-x86_64")
	require.NotNil(t, bash)
	assert.Equal(t, "packages", bash.Section)
	assert.True(t, bash.DisplayOnly)
	assert.Equal(t, "sub:version-upgrades", bash.Group)
}
```

- [ ] **Step 3: Write refine-server sidecar proof test**

This test proves the approval-critical fix: after `RunRefine` processes a tarball with leaf packages having `Include: false`, the sidecar and working snapshot both have `Include: true` for leaves, and `ClassifySnapshot(snap, original)` produces `DefaultInclude: true`.

**Critical requirement:** This test MUST exercise the real `RunRefine` function (or a shared helper that `RunRefine` calls) — not a test-local reenactment of the normalize-before-sidecar ordering. The proof is only valid if it goes through the same code path as production.

Add to `cmd/inspectah/internal/refine/server_test.go`. READ the test file first — the existing tests at lines 292-340 show how to call `RunRefine` with a test tarball and inspect the output directory.

```go
func TestRunRefine_LeafNormalizationSidecarAgreement(t *testing.T) {
	// Create a tarball with a snapshot containing LeafPackages and Include=false
	leafNames := []string{"vim"}
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		LeafPackages: &leafNames,
		PackagesAdded: []schema.PackageEntry{
			{Name: "vim", Arch: "x86_64", Include: false, SourceRepo: "appstream", Version: "9.1", Release: "1.el9"},
			{Name: "vim-common", Arch: "x86_64", Include: false, SourceRepo: "appstream", Version: "9.1", Release: "1.el9"},
		},
	}

	// Build a test tarball from this snapshot
	// (follow existing patterns in server_test.go for creating test tarballs —
	// look for how other tests write snapshot + report.html to a temp dir,
	// then tar it up and pass the path to RunRefine)
	tarball := buildTestTarball(t, snap)

	// Run the REAL RunRefine against the test tarball.
	// Use a short-lived server with immediate shutdown — we only need
	// the extraction/normalization/sidecar pipeline, not the HTTP server.
	// If RunRefine blocks on serving, use the existing test pattern of
	// running it in a goroutine with a context/cancel or errCh, then
	// inspecting the output directory after the initial render completes.
	outputDir := runRefineAndGetOutputDir(t, tarball)

	// Load both working and sidecar snapshots from the output directory
	workingSnap, err := schema.LoadSnapshot(
		filepath.Join(outputDir, "inspection-snapshot.json"))
	require.NoError(t, err)
	sidecarSnap, err := schema.LoadSnapshot(
		filepath.Join(outputDir, "original-inspection-snapshot.json"))
	require.NoError(t, err)

	// Both must agree: leaf vim has Include=true
	assert.True(t, workingSnap.Rpm.PackagesAdded[0].Include,
		"working snapshot leaf should be Include=true after RunRefine normalization")
	assert.True(t, sidecarSnap.Rpm.PackagesAdded[0].Include,
		"sidecar leaf should be Include=true (RunRefine normalizes before sidecar)")

	// Dep vim-common stays false in both
	assert.False(t, workingSnap.Rpm.PackagesAdded[1].Include)
	assert.False(t, sidecarSnap.Rpm.PackagesAdded[1].Include)

	// ClassifySnapshot with original produces DefaultInclude=true for leaf
	items := renderer.ClassifySnapshot(workingSnap, sidecarSnap)
	vim := findTriageItem(items, "pkg-vim-x86_64")
	require.NotNil(t, vim)
	assert.True(t, vim.DefaultInclude,
		"DefaultInclude must be true — both copies agree after RunRefine pipeline")
}
```

**Implementation notes for the implementer:**
- `buildTestTarball(t, snap)` and `runRefineAndGetOutputDir(t, tarball)` are test helpers the implementer must create. READ `server_test.go` carefully — the existing test patterns (lines 292-340) show how to call `RunRefine` in a goroutine, wait for the initial render, then inspect the output. The key requirement is that the test goes through `RunRefine` itself, not a manual reenactment.
- `findTriageItem` may need to be defined if the refine test file doesn't already have it (it exists as `findItem` in `triage_test.go` — same package won't work across packages, so define a local copy).
- If `RunRefine` requires a `ReRenderFn`, provide a minimal one from the existing test patterns.

- [ ] **Step 4: Run tests**

Run: `cd cmd/inspectah && go test ./internal/renderer/ -run "TestHTMLReportGoldenLeaf" -v`
Run: `cd cmd/inspectah && go test ./internal/renderer/ -run "TestHTMLReportGoldenVersion" -v`
Run: `cd cmd/inspectah && go test ./internal/refine/ -run "TestRefineServer_LeafNormalization" -v`
Expected: All PASS

- [ ] **Step 5: Commit**

```bash
git add cmd/inspectah/internal/renderer/html_test.go cmd/inspectah/internal/refine/server_test.go
git commit -m "test: golden HTML + sidecar proof for leaf normalization

Verify rendered manifest has deps on leaf items, DefaultInclude
normalized to true after sidecar creation, and version changes
as display-only grouped items.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 9: Browser smoke test — manual verification

**Files:** None (manual testing)

- [ ] **Step 1: Build and run**

```bash
cd cmd/inspectah && go build -o ../../inspectah-darwin-arm64 .
cd /Users/mrussell/Work/bootc-migration/inspectah
./inspectah-darwin-arm64 refine cmd/inspectah/inspectah-Fedora43-20260504-050250.tar.gz
```

Open the URL in a browser.

- [ ] **Step 2: Verify leaf package triage**

- [ ] Packages section shows ~57 leaf packages, NOT 510
- [ ] Accordion toggles show ON by default (not dimmed/disabled)
- [ ] Leaf package rows show dep count badge (e.g., "12 deps")
- [ ] Clicking chevron expands dep sub-list below the row
- [ ] Opening a second chevron collapses the first (one at a time)
- [ ] Zero-dep leaves have no chevron or badge
- [ ] Dep expansion resets when accordion is collapsed/re-expanded

- [ ] **Step 3: Verify static mode deps**

Open `report.html` directly as `file://`:
- [ ] No chevrons rendered
- [ ] Dep names shown inline as comma-separated list after package name
- [ ] Dep count badge visible

- [ ] **Step 4: Verify version changes**

- [ ] Version upgrades accordion appears in Packages section (tier 1)
- [ ] Accordion shows "N items — informational" subtitle
- [ ] Version changes do NOT affect section footer decided count
- [ ] Version changes do NOT affect sidebar badge

- [ ] **Step 5: Verify SELinux in System & Security**

- [ ] System section label shows "System & Security"
- [ ] SELinux accordion appears with `sebool-*` and `seport-*` items
- [ ] `semod-*` renders as individual notification card (NOT inside accordion)
- [ ] `semod-*` notification shows SELinux-specific copy ("manual Containerfile steps required"), NOT package copy
- [ ] `semod-*` Acknowledge button works, collapses to "custom module — manual Containerfile steps required"
- [ ] `semod-*` undo works

- [ ] **Step 6: Verify keyboard navigation**

- [ ] Tab reaches dep chevron in accordion
- [ ] Enter/Space toggles dep disclosure
- [ ] Focus returns to chevron after toggle re-render

- [ ] **Step 7: Fix any issues found**

Commit targeted fixes for issues discovered during manual testing.

---

### Task 10: Final — full test suite, vet, and build

**Files:** None (verification only)

- [ ] **Step 1: Run full Go test suite**

```bash
cd cmd/inspectah && go test ./... -v -count=1
```
Expected: All packages PASS, zero FAIL.

- [ ] **Step 2: Run go vet**

```bash
cd cmd/inspectah && go vet ./...
```
Expected: No issues.

- [ ] **Step 3: Build darwin binary**

```bash
cd cmd/inspectah && go build -o ../../inspectah-darwin-arm64 .
```

- [ ] **Step 4: Build linux binary**

```bash
cd cmd/inspectah && GOOS=linux GOARCH=arm64 go build -o ../../inspectah-linux-arm64 .
```
