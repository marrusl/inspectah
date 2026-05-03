# Single-Machine Triage Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reduce single-machine triage from 300-400+ individual decision cards to grouped accordion views with toggle switches, keeping individual cards only for items needing genuine attention.

**Architecture:** Keep existing 3-tier classification. Add `Group`, `CardType`, `DisplayOnly`, and `Acknowledged` fields to `TriageItem`. The classifier populates these in single-machine mode (`Meta["fleet"]` absent). The JS groups items by `Group` into accordion cards. Ungrouped items render as individual cards (decision or notification). Display-only surfaces use Acknowledge/Skip language.

**Tech Stack:** Go (schema types, classifier), vanilla JS (report.html SPA), Go unit tests (table-driven), golden-file tests.

**Spec:** `docs/specs/proposed/2026-05-03-single-machine-triage-redesign.md` (revision 4, approved)

**Repo:** `/Users/mrussell/Work/bootc-migration/inspectah/` on `go-port` branch.

---

## File Structure

### Go changes
- **Modify:** `cmd/inspectah/internal/schema/types.go` — add `Acknowledged` field to 5 types
- **Modify:** `cmd/inspectah/internal/schema/snapshot.go` — normalize `Acknowledged` in `NormalizeSnapshot`, bump schema version
- **Modify:** `cmd/inspectah/internal/renderer/triage.go` — add fields to `TriageItem`, update all classifiers, add helpers
- **Modify:** `cmd/inspectah/internal/renderer/triage_test.go` — add tests for new fields
- **Modify:** `cmd/inspectah/internal/renderer/html.go` — no structural change, triage manifest already serialized

### JS changes
- **Modify:** `cmd/inspectah/internal/renderer/static/report.html` — new accordion/notification components, grouped rendering, toggle state machine, static mode

### Test data
- **Create:** `cmd/inspectah/internal/renderer/testdata/golden-output-accordion.html`
- **Create:** `cmd/inspectah/internal/renderer/testdata/golden-display-accordion.html`
- **Create:** `cmd/inspectah/internal/renderer/testdata/golden-notification-card.html`

---

### Task 1: Add Acknowledged field to schema types

**Files:**
- Modify: `cmd/inspectah/internal/schema/types.go`
- Test: `cmd/inspectah/internal/schema/types_test.go`

- [ ] **Step 1: Write failing test for Acknowledged JSON round-trip**

```go
func TestPackageEntryAcknowledgedJSON(t *testing.T) {
	pkg := PackageEntry{
		Name:         "custom-agent",
		Version:      "1.0",
		Release:      "1",
		Arch:         "x86_64",
		Include:      true,
		Acknowledged: true,
	}
	data, err := json.Marshal(pkg)
	require.NoError(t, err)
	assert.Contains(t, string(data), `"acknowledged":true`)

	var decoded PackageEntry
	require.NoError(t, json.Unmarshal(data, &decoded))
	assert.True(t, decoded.Acknowledged)
}

func TestNMConnectionAcknowledgedJSON(t *testing.T) {
	conn := NMConnection{
		Name:         "eth0",
		Type:         "ethernet",
		Acknowledged: true,
	}
	data, err := json.Marshal(conn)
	require.NoError(t, err)

	var decoded NMConnection
	require.NoError(t, json.Unmarshal(data, &decoded))
	assert.True(t, decoded.Acknowledged)
}

func TestFstabEntryAcknowledgedJSON(t *testing.T) {
	entry := FstabEntry{
		MountPoint:   "/data",
		Fstype:       "xfs",
		Acknowledged: true,
	}
	data, err := json.Marshal(entry)
	require.NoError(t, err)

	var decoded FstabEntry
	require.NoError(t, json.Unmarshal(data, &decoded))
	assert.True(t, decoded.Acknowledged)
}

func TestRunningContainerAcknowledgedJSON(t *testing.T) {
	c := RunningContainer{
		Name:         "nginx",
		Image:        "nginx:latest",
		Acknowledged: true,
	}
	data, err := json.Marshal(c)
	require.NoError(t, err)

	var decoded RunningContainer
	require.NoError(t, json.Unmarshal(data, &decoded))
	assert.True(t, decoded.Acknowledged)
}

func TestNonRpmItemAcknowledgedJSON(t *testing.T) {
	item := NonRpmItem{
		Path:         "/usr/local/bin/custom",
		Method:       "binary",
		Include:      true,
		Acknowledged: true,
	}
	data, err := json.Marshal(item)
	require.NoError(t, err)

	var decoded NonRpmItem
	require.NoError(t, json.Unmarshal(data, &decoded))
	assert.True(t, decoded.Acknowledged)
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && go test ./cmd/inspectah/internal/schema/ -run "Acknowledged" -v`
Expected: FAIL — `Acknowledged` field does not exist on any type.

- [ ] **Step 3: Add Acknowledged field to all 5 types**

In `cmd/inspectah/internal/schema/types.go`, add `Acknowledged bool` to each type:

```go
// PackageEntry — add after Include field:
Acknowledged bool `json:"acknowledged,omitempty"`

// NMConnection — add after Include field:
Acknowledged bool `json:"acknowledged,omitempty"`

// FstabEntry — add after Include field:
Acknowledged bool `json:"acknowledged,omitempty"`

// RunningContainer — add after Include field:
Acknowledged bool `json:"acknowledged,omitempty"`

// NonRpmItem — add after Include field:
Acknowledged bool `json:"acknowledged,omitempty"`
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && go test ./cmd/inspectah/internal/schema/ -run "Acknowledged" -v`
Expected: PASS

- [ ] **Step 5: Add mapAcknowledged helper and NormalizeSnapshot update**

In `cmd/inspectah/internal/schema/types.go`, add a helper parallel to `mapInclude`:

This is not needed — the `Acknowledged` field is only on typed structs (not untyped maps). Users and groups use untyped maps but groups are output-affecting (not display-only), so they don't need `Acknowledged`. SELinux booleans use untyped maps but are also output-affecting. No `mapAcknowledged` is needed.

In `cmd/inspectah/internal/schema/snapshot.go`, the `NormalizeSnapshot` function does NOT need to set `Acknowledged` defaults — the zero value (`false`) is correct for "not yet acknowledged."

- [ ] **Step 6: Run full schema test suite**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && go test ./cmd/inspectah/internal/schema/ -v`
Expected: All tests PASS, including existing ones.

- [ ] **Step 7: Commit**

```bash
git add cmd/inspectah/internal/schema/types.go cmd/inspectah/internal/schema/types_test.go
git commit -m "schema: add Acknowledged field to 5 snapshot types

Add Acknowledged bool to PackageEntry, NMConnection, FstabEntry,
RunningContainer, and NonRpmItem for notification/display-only
review state persistence.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 2: Add new fields to TriageItem and fleet detection helper

**Files:**
- Modify: `cmd/inspectah/internal/renderer/triage.go`
- Test: `cmd/inspectah/internal/renderer/triage_test.go`

- [ ] **Step 1: Write failing test for isFleetSnapshot**

```go
func TestIsFleetSnapshot(t *testing.T) {
	single := schema.NewSnapshot()
	assert.False(t, isFleetSnapshot(single), "single-machine snapshot should not be fleet")

	fleet := schema.NewSnapshot()
	fleet.Meta["fleet"] = map[string]interface{}{
		"source_hosts":   []interface{}{"host1", "host2"},
		"total_hosts":    float64(2),
		"min_prevalence": float64(50),
	}
	assert.True(t, isFleetSnapshot(fleet), "snapshot with fleet metadata should be fleet")
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && go test ./cmd/inspectah/internal/renderer/ -run "TestIsFleetSnapshot" -v`
Expected: FAIL — `isFleetSnapshot` not defined.

- [ ] **Step 3: Add new TriageItem fields and isFleetSnapshot helper**

In `cmd/inspectah/internal/renderer/triage.go`, update the `TriageItem` struct:

```go
type TriageItem struct {
	Section        string `json:"section"`
	Key            string `json:"key"`
	Tier           int    `json:"tier"`
	Reason         string `json:"reason"`
	Name           string `json:"name"`
	Meta           string `json:"meta"`
	Group          string `json:"group,omitempty"`
	CardType       string `json:"card_type,omitempty"`
	DisplayOnly    bool   `json:"display_only,omitempty"`
	Acknowledged   bool   `json:"acknowledged,omitempty"`
	IsSecret       bool   `json:"is_secret,omitempty"`
	SourcePath     string `json:"source_path,omitempty"`
	DefaultInclude bool   `json:"default_include"`
}
```

Add the fleet detection helper:

```go
func isFleetSnapshot(snap *schema.InspectionSnapshot) bool {
	if snap.Meta == nil {
		return false
	}
	_, ok := snap.Meta["fleet"]
	return ok
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && go test ./cmd/inspectah/internal/renderer/ -run "TestIsFleetSnapshot" -v`
Expected: PASS

- [ ] **Step 5: Run full renderer test suite to verify no regressions**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && go test ./cmd/inspectah/internal/renderer/ -v`
Expected: All existing tests PASS. New fields have zero values so existing TriageItem construction is unaffected.

- [ ] **Step 6: Commit**

```bash
git add cmd/inspectah/internal/renderer/triage.go cmd/inspectah/internal/renderer/triage_test.go
git commit -m "triage: add Group, CardType, DisplayOnly, Acknowledged fields

Extend TriageItem with grouping and surface-ownership fields for
single-machine triage redesign. Add isFleetSnapshot helper that
checks Meta[\"fleet\"] presence.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 3: Update classifyPackages for single-machine grouping

**Files:**
- Modify: `cmd/inspectah/internal/renderer/triage.go`
- Test: `cmd/inspectah/internal/renderer/triage_test.go`

- [ ] **Step 1: Write failing tests for package grouping**

```go
func TestClassifyPackages_SingleMachine_GroupByRepo(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		BaselinePackageNames: strSlicePtr([]string{"bash", "coreutils"}),
		PackagesAdded: []schema.PackageEntry{
			{Name: "bash", Arch: "x86_64", Include: true, SourceRepo: "baseos"},
			{Name: "vim", Arch: "x86_64", Include: true, SourceRepo: "appstream"},
			{Name: "htop", Arch: "x86_64", Include: true, SourceRepo: "epel"},
			{Name: "custom", Arch: "x86_64", Include: true, State: "local_install"},
		},
	}

	items := classifyPackages(snap, make(map[string]bool), false)

	// bash: tier 1 (baseline match), group = "repo:baseos"
	bash := findItem(items, "pkg-bash-x86_64")
	require.NotNil(t, bash)
	assert.Equal(t, 1, bash.Tier)
	assert.Equal(t, "repo:baseos", bash.Group)
	assert.Equal(t, "", bash.CardType)
	assert.False(t, bash.DisplayOnly)

	// vim: tier 2 (standard repo, not in base), group = "repo:appstream"
	vim := findItem(items, "pkg-vim-x86_64")
	require.NotNil(t, vim)
	assert.Equal(t, 2, vim.Tier)
	assert.Equal(t, "repo:appstream", vim.Group)

	// htop: tier 2 (third-party), group = "repo:epel"
	htop := findItem(items, "pkg-htop-x86_64")
	require.NotNil(t, htop)
	assert.Equal(t, 2, htop.Tier)
	assert.Equal(t, "repo:epel", htop.Group)

	// custom: tier 3 (local install), no group, notification card
	custom := findItem(items, "pkg-custom-x86_64")
	require.NotNil(t, custom)
	assert.Equal(t, 3, custom.Tier)
	assert.Equal(t, "", custom.Group)
	assert.Equal(t, "notification", custom.CardType)
}

func TestClassifyPackages_Fleet_NoGroups(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		PackagesAdded: []schema.PackageEntry{
			{Name: "vim", Arch: "x86_64", Include: true, SourceRepo: "appstream"},
		},
	}

	items := classifyPackages(snap, make(map[string]bool), true)

	vim := findItem(items, "pkg-vim-x86_64")
	require.NotNil(t, vim)
	assert.Equal(t, "", vim.Group, "fleet mode should not populate Group")
	assert.Equal(t, "", vim.CardType)
}

func TestClassifyPackages_NoRepo_Acknowledged(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		PackagesAdded: []schema.PackageEntry{
			{Name: "custom", Arch: "x86_64", Include: true, State: "local_install", Acknowledged: true},
		},
	}

	items := classifyPackages(snap, make(map[string]bool), false)
	custom := findItem(items, "pkg-custom-x86_64")
	require.NotNil(t, custom)
	assert.True(t, custom.Acknowledged)
	assert.Equal(t, "notification", custom.CardType)
}

// Helper to find an item by key
func findItem(items []TriageItem, key string) *TriageItem {
	for i := range items {
		if items[i].Key == key {
			return &items[i]
		}
	}
	return nil
}

// Helper for baseline package names
func strSlicePtr(s []string) *[]string {
	return &s
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && go test ./cmd/inspectah/internal/renderer/ -run "TestClassifyPackages_SingleMachine\|TestClassifyPackages_Fleet\|TestClassifyPackages_NoRepo_Ack" -v`
Expected: FAIL — `classifyPackages` signature doesn't accept `isFleet` parameter.

- [ ] **Step 3: Update classifyPackages to accept isFleet and populate Group/CardType**

Update the function signature and body in `triage.go`:

```go
func classifyPackages(snap *schema.InspectionSnapshot, secrets map[string]bool, isFleet bool) []TriageItem {
	if snap.Rpm == nil {
		return nil
	}
	var items []TriageItem
	baselineNames := make(map[string]bool)
	if snap.Rpm.BaselinePackageNames != nil {
		for _, n := range *snap.Rpm.BaselinePackageNames {
			baselineNames[n] = true
		}
	}

	for _, pkg := range snap.Rpm.PackagesAdded {
		if secrets[pkg.Name] {
			continue
		}
		tier, reason := classifyPackage(pkg, baselineNames)
		item := TriageItem{
			Section:        "packages",
			Key:            fmt.Sprintf("pkg-%s-%s", pkg.Name, pkg.Arch),
			Tier:           tier,
			Reason:         reason,
			Name:           pkg.Name,
			Meta:           joinNonEmpty(" | ", pkg.Version+"-"+pkg.Release, pkg.Arch, pkg.SourceRepo),
			DefaultInclude: pkg.Include,
		}

		if !isFleet {
			if tier == 3 && (string(pkg.State) == "local_install" || string(pkg.State) == "no_repo") {
				item.CardType = "notification"
				item.Acknowledged = pkg.Acknowledged
				item.Reason = "No repository source available. inspectah cannot reconstruct installation steps for this package."
			} else if pkg.SourceRepo != "" {
				item.Group = "repo:" + strings.ToLower(pkg.SourceRepo)
			}
		}

		items = append(items, item)
	}

	for _, ms := range snap.Rpm.ModuleStreams {
		if ms.BaselineMatch {
			continue
		}
		items = append(items, TriageItem{
			Section:        "packages",
			Key:            fmt.Sprintf("ms-%s-%s", ms.ModuleName, ms.Stream),
			Tier:           2,
			Reason:         "Module stream package. Stream choice affects dependency tree. Verify compatibility.",
			Name:           ms.ModuleName + ":" + ms.Stream,
			Meta:           strings.Join(ms.Profiles, ", "),
			DefaultInclude: ms.Include,
			// No Group — module streams always get individual cards
		})
	}
	return items
}
```

- [ ] **Step 4: Update classifyAll to pass isFleet to classifyPackages**

```go
func classifyAll(snap *schema.InspectionSnapshot, isFleet bool) []TriageItem {
	secretPaths := buildSecretPathSet(snap)

	var items []TriageItem
	items = append(items, classifyPackages(snap, secretPaths, isFleet)...)
	items = append(items, classifyConfigFiles(snap, secretPaths, isFleet)...)
	items = append(items, classifyRuntime(snap, secretPaths, isFleet)...)
	items = append(items, classifyContainerItems(snap, secretPaths, isFleet)...)
	items = append(items, classifyIdentity(snap, secretPaths, isFleet)...)
	items = append(items, classifySystemItems(snap, secretPaths, isFleet)...)
	items = append(items, classifySecretItems(snap, secretPaths)...)
	return items
}
```

Update `ClassifySnapshot` to detect fleet mode and pass it through:

```go
func ClassifySnapshot(snap *schema.InspectionSnapshot, original *schema.InspectionSnapshot) []TriageItem {
	isFleet := isFleetSnapshot(snap)
	items := classifyAll(snap, isFleet)

	if original != nil {
		origItems := classifyAll(original, isFleet)
		origMap := make(map[string]bool)
		for _, oi := range origItems {
			origMap[oi.Key] = oi.DefaultInclude
		}
		for i := range items {
			if val, ok := origMap[items[i].Key]; ok {
				items[i].DefaultInclude = val
			}
		}
	}

	return items
}
```

Temporarily stub the other classifier signatures to accept `isFleet` (they'll be updated in subsequent tasks):

```go
func classifyConfigFiles(snap *schema.InspectionSnapshot, secrets map[string]bool, isFleet bool) []TriageItem {
	// ... existing body unchanged for now, isFleet ignored ...
}
// Same for classifyRuntime, classifyContainerItems, classifyIdentity, classifySystemItems
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && go test ./cmd/inspectah/internal/renderer/ -v`
Expected: All tests PASS — new tests pass, existing tests unaffected (fleet detection returns false for test snapshots without Meta["fleet"]).

- [ ] **Step 6: Commit**

```bash
git add cmd/inspectah/internal/renderer/triage.go cmd/inspectah/internal/renderer/triage_test.go
git commit -m "triage: group packages by repo in single-machine mode

classifyPackages now populates Group (repo:<name>) for single-machine
snapshots. No-repo packages get CardType=notification. Module streams
stay as individual cards. Fleet mode is unchanged.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 4: Update classifyConfigFiles for single-machine grouping

**Files:**
- Modify: `cmd/inspectah/internal/renderer/triage.go`
- Test: `cmd/inspectah/internal/renderer/triage_test.go`

- [ ] **Step 1: Write failing tests**

```go
func TestClassifyConfigFiles_SingleMachine_Grouping(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Config = &schema.ConfigSection{
		Files: []schema.ConfigFileEntry{
			{Path: "/etc/ssh/sshd_config", Kind: schema.ConfigFileKindRpmOwnedDefault, Include: true},
			{Path: "/etc/httpd/conf/httpd.conf", Kind: schema.ConfigFileKindRpmOwnedModified, Include: true},
			{Path: "/etc/systemd/system/foo.service.d/override.conf", Kind: "systemd_dropin", Include: true},
			{Path: "/etc/custom/app.conf", Kind: "", Include: true},
		},
	}

	items := classifyConfigFiles(snap, make(map[string]bool), false)

	// RPM-owned-default: grouped as "kind:unchanged"
	ssh := findItem(items, "cfg-/etc/ssh/sshd_config")
	require.NotNil(t, ssh)
	assert.Equal(t, "kind:unchanged", ssh.Group)

	// RPM-owned-modified: no group (individual card)
	httpd := findItem(items, "cfg-/etc/httpd/conf/httpd.conf")
	require.NotNil(t, httpd)
	assert.Equal(t, "", httpd.Group)

	// Drop-in: grouped as "kind:drop-in"
	dropin := findItem(items, "cfg-/etc/systemd/system/foo.service.d/override.conf")
	require.NotNil(t, dropin)
	assert.Equal(t, "kind:drop-in", dropin.Group)

	// Custom: no group (individual card)
	custom := findItem(items, "cfg-/etc/custom/app.conf")
	require.NotNil(t, custom)
	assert.Equal(t, "", custom.Group)
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && go test ./cmd/inspectah/internal/renderer/ -run "TestClassifyConfigFiles_SingleMachine" -v`
Expected: FAIL — Group field not populated.

- [ ] **Step 3: Update classifyConfigFiles**

```go
func classifyConfigFiles(snap *schema.InspectionSnapshot, secrets map[string]bool, isFleet bool) []TriageItem {
	if snap.Config == nil {
		return nil
	}
	var items []TriageItem
	for _, f := range snap.Config.Files {
		if secrets[f.Path] {
			continue
		}
		if isQuadletPath(f.Path) {
			continue
		}
		tier, reason := classifyConfigFile(f)
		item := TriageItem{
			Section:        "config",
			Key:            "cfg-" + f.Path,
			Tier:           tier,
			Reason:         reason,
			Name:           f.Path,
			Meta:           joinNonEmpty(" | ", string(f.Kind), string(f.Category)),
			DefaultInclude: f.Include,
		}

		if !isFleet {
			switch f.Kind {
			case schema.ConfigFileKindRpmOwnedDefault, "baseline_match":
				item.Group = "kind:unchanged"
			case "systemd_dropin":
				item.Group = "kind:drop-in"
			// RPM-owned-modified and custom/untracked: no group (individual cards)
			}
		}

		items = append(items, item)
	}
	return items
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && go test ./cmd/inspectah/internal/renderer/ -v`
Expected: All PASS.

- [ ] **Step 5: Commit**

```bash
git add cmd/inspectah/internal/renderer/triage.go cmd/inspectah/internal/renderer/triage_test.go
git commit -m "triage: group configs by kind in single-machine mode

Unchanged configs group as kind:unchanged, drop-ins as kind:drop-in.
Modified and custom configs stay as individual cards for conscious
ownership decisions.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 5: Update classifyRuntime for single-machine grouping

**Files:**
- Modify: `cmd/inspectah/internal/renderer/triage.go`
- Test: `cmd/inspectah/internal/renderer/triage_test.go`

- [ ] **Step 1: Write failing tests**

```go
func TestClassifyRuntime_SingleMachine_Grouping(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Services = &schema.ServiceSection{
		StateChanges: []schema.ServiceStateChange{
			{Unit: "sshd.service", CurrentState: "enabled", DefaultState: "enabled", Include: true},
			{Unit: "httpd.service", CurrentState: "enabled", DefaultState: "disabled", Include: true},
			{Unit: "dnf-makecache.timer", CurrentState: "enabled", DefaultState: "enabled", Include: true},
		},
	}
	snap.ScheduledTasks = &schema.ScheduledTaskSection{
		CronJobs: []schema.CronJob{
			{Path: "/etc/cron.d/backup", Source: "custom", Include: true},
		},
	}

	items := classifyRuntime(snap, make(map[string]bool), false)

	// Default-state service: grouped
	sshd := findItem(items, "svc-sshd.service")
	require.NotNil(t, sshd)
	assert.Equal(t, "sub:services-default", sshd.Group)
	assert.Equal(t, 1, sshd.Tier)

	// Changed-state service: grouped
	httpd := findItem(items, "svc-httpd.service")
	require.NotNil(t, httpd)
	assert.Equal(t, "sub:services-changed", httpd.Group)
	assert.Equal(t, 2, httpd.Tier)

	// dnf-makecache: tier 3, no group, individual card
	dnf := findItem(items, "svc-dnf-makecache.timer")
	require.NotNil(t, dnf)
	assert.Equal(t, 3, dnf.Tier)
	assert.Equal(t, "", dnf.Group)
	assert.Contains(t, dnf.Reason, "package management at runtime")

	// Cron job: grouped
	cron := findItem(items, "cron-/etc/cron.d/backup")
	require.NotNil(t, cron)
	assert.Equal(t, "sub:cron", cron.Group)
}

func TestClassifyRuntime_ImageModeIncompatible(t *testing.T) {
	tests := []struct {
		unit string
	}{
		{"dnf-makecache.service"},
		{"dnf-makecache.timer"},
		{"packagekit.service"},
	}
	for _, tt := range tests {
		t.Run(tt.unit, func(t *testing.T) {
			snap := schema.NewSnapshot()
			snap.Services = &schema.ServiceSection{
				StateChanges: []schema.ServiceStateChange{
					{Unit: tt.unit, CurrentState: "enabled", DefaultState: "enabled", Include: true},
				},
			}
			items := classifyRuntime(snap, make(map[string]bool), false)
			svc := findItem(items, "svc-"+tt.unit)
			require.NotNil(t, svc)
			assert.Equal(t, 3, svc.Tier)
			assert.Equal(t, "", svc.Group)
		})
	}
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && go test ./cmd/inspectah/internal/renderer/ -run "TestClassifyRuntime_SingleMachine\|TestClassifyRuntime_ImageMode" -v`
Expected: FAIL

- [ ] **Step 3: Update classifyRuntime**

```go
var imageModeIncompatibleServices = map[string]bool{
	"dnf-makecache.service": true,
	"dnf-makecache.timer":   true,
	"packagekit.service":    true,
}

func classifyRuntime(snap *schema.InspectionSnapshot, secrets map[string]bool, isFleet bool) []TriageItem {
	var items []TriageItem
	if snap.Services != nil {
		for _, svc := range snap.Services.StateChanges {
			if secrets[svc.Unit] {
				continue
			}

			// Image-mode incompatible services: always tier 3, no group
			if !isFleet && imageModeIncompatibleServices[svc.Unit] {
				items = append(items, TriageItem{
					Section: "runtime", Key: "svc-" + svc.Unit,
					Tier: 3, Reason: "This service assumes package management at runtime, which is unavailable in image mode. Consider disabling or removing it from the image.",
					Name: svc.Unit, Meta: svc.CurrentState,
					DefaultInclude: svc.Include,
				})
				continue
			}

			isDefault := svc.CurrentState == svc.DefaultState
			tier := 2
			reason := fmt.Sprintf("Service state changed (%s -> %s).", svc.DefaultState, svc.CurrentState)
			group := ""
			if isDefault {
				tier = 1
				reason = "Service in default state."
				if !isFleet {
					group = "sub:services-default"
				}
			} else if !isFleet {
				group = "sub:services-changed"
			}
			meta := svc.CurrentState
			if svc.OwningPackage != nil {
				meta += " | " + *svc.OwningPackage
			}
			items = append(items, TriageItem{
				Section: "runtime", Key: "svc-" + svc.Unit,
				Tier: tier, Reason: reason, Name: svc.Unit, Meta: meta,
				DefaultInclude: svc.Include, Group: group,
			})
		}
	}
	if snap.ScheduledTasks != nil {
		for _, job := range snap.ScheduledTasks.CronJobs {
			group := ""
			if !isFleet {
				group = "sub:cron"
			}
			items = append(items, TriageItem{
				Section: "runtime", Key: "cron-" + job.Path,
				Tier: 2, Reason: "Scheduled cron job.",
				Name: job.Path, Meta: job.Source,
				DefaultInclude: job.Include, Group: group,
			})
		}
		for _, timer := range snap.ScheduledTasks.SystemdTimers {
			group := ""
			if !isFleet {
				group = "sub:timers"
			}
			items = append(items, TriageItem{
				Section: "runtime", Key: "timer-" + timer.Name,
				Tier: 2, Reason: "Systemd timer unit.",
				Name: timer.Name, Meta: timer.OnCalendar,
				DefaultInclude: isIncluded(timer.Include), Group: group,
			})
		}
	}
	return items
}
```

- [ ] **Step 4: Run tests**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && go test ./cmd/inspectah/internal/renderer/ -v`
Expected: All PASS.

- [ ] **Step 5: Commit**

```bash
git add cmd/inspectah/internal/renderer/triage.go cmd/inspectah/internal/renderer/triage_test.go
git commit -m "triage: group runtime items, flag image-mode incompatible services

Services grouped by state (default/changed), cron/timers by type.
dnf-makecache and packagekit flagged as tier 3 with image-mode
incompatibility warning.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 6: Update classifyContainerItems for single-machine grouping

**Files:**
- Modify: `cmd/inspectah/internal/renderer/triage.go`
- Test: `cmd/inspectah/internal/renderer/triage_test.go`

- [ ] **Step 1: Write failing tests**

```go
func TestClassifyContainerItems_SingleMachine_Grouping(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Containers = &schema.ContainerSection{
		QuadletUnits: []schema.QuadletUnit{
			{Name: "webapp.container", Image: "webapp:latest", Include: true},
		},
		RunningContainers: []schema.RunningContainer{
			{Name: "webapp", Image: "webapp:latest"},
			{Name: "orphan", Image: "orphan:latest"},
		},
	}
	snap.NonRpmSoftware = &schema.NonRpmSoftwareSection{
		Items: []schema.NonRpmItem{
			{Path: "/opt/agent/bin/agent", Method: "binary", Include: true},
		},
	}

	items := classifyContainerItems(snap, make(map[string]bool), false)

	// Quadlet: grouped
	quadlet := findItem(items, "quadlet-webapp.container")
	require.NotNil(t, quadlet)
	assert.Equal(t, "sub:quadlet", quadlet.Group)
	assert.False(t, quadlet.DisplayOnly)

	// Running container with quadlet backing: display-only
	webapp := findItem(items, "container-webapp")
	require.NotNil(t, webapp)
	assert.True(t, webapp.DisplayOnly)

	// Running container without quadlet: display-only
	orphan := findItem(items, "container-orphan")
	require.NotNil(t, orphan)
	assert.True(t, orphan.DisplayOnly)
	assert.Equal(t, 3, orphan.Tier)

	// Non-RPM binary: notification card
	agent := findItem(items, "nonrpm-/opt/agent/bin/agent")
	require.NotNil(t, agent)
	assert.Equal(t, "notification", agent.CardType)
	assert.Contains(t, agent.Reason, "provenance")
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && go test ./cmd/inspectah/internal/renderer/ -run "TestClassifyContainerItems_SingleMachine" -v`
Expected: FAIL

- [ ] **Step 3: Update classifyContainerItems**

```go
func classifyContainerItems(snap *schema.InspectionSnapshot, secrets map[string]bool, isFleet bool) []TriageItem {
	var items []TriageItem
	if snap.Containers != nil {
		quadletNames := make(map[string]bool)
		for _, q := range snap.Containers.QuadletUnits {
			quadletNames[q.Name] = true
			group := ""
			if !isFleet {
				group = "sub:quadlet"
			}
			items = append(items, TriageItem{
				Section: "containers", Key: "quadlet-" + q.Name,
				Tier: 2, Reason: "Quadlet file with container unit.",
				Name: q.Name, Meta: q.Image,
				DefaultInclude: q.Include, Group: group,
			})
		}
		for _, c := range snap.Containers.RunningContainers {
			tier, reason := 2, "Running container with quadlet backing."
			if !quadletNames[c.Name] {
				tier, reason = 3, "Running container without quadlet backing. This is runtime state — it will not be reproduced in the image. Consider converting to a Quadlet unit for image-mode compatibility."
			}
			item := TriageItem{
				Section: "containers", Key: "container-" + c.Name,
				Tier: tier, Reason: reason, Name: c.Name, Meta: c.Image,
				DefaultInclude: isIncluded(c.Include),
			}
			if !isFleet {
				item.DisplayOnly = true
				item.Acknowledged = c.Acknowledged
			}
			items = append(items, item)
		}
	}
	if snap.NonRpmSoftware != nil {
		for _, nri := range snap.NonRpmSoftware.Items {
			if secrets[nri.Path] {
				continue
			}
			name := nri.Path
			if name == "" {
				name = nri.Name
			}
			item := TriageItem{
				Section: "containers", Key: "nonrpm-" + name,
				Tier: 3, Reason: "Non-RPM binary with unclear provenance.",
				Name: name, Meta: nri.Method,
				DefaultInclude: nri.Include,
			}
			if !isFleet {
				item.CardType = "notification"
				item.Acknowledged = nri.Acknowledged
				item.Reason = "inspectah cannot determine the provenance or installation method for this binary. To include it in the image, provide a reproducible build-time source and add it to your Containerfile."
			}
			items = append(items, item)
		}
	}
	return items
}
```

- [ ] **Step 4: Run tests**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && go test ./cmd/inspectah/internal/renderer/ -v`
Expected: All PASS.

- [ ] **Step 5: Commit**

```bash
git add cmd/inspectah/internal/renderer/triage.go cmd/inspectah/internal/renderer/triage_test.go
git commit -m "triage: group containers, mark display-only, notification cards

Quadlets grouped as sub:quadlet. Running containers are display-only.
Non-RPM binaries become notification cards with provenance warning.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 7: Update classifyIdentity and classifySystemItems

**Files:**
- Modify: `cmd/inspectah/internal/renderer/triage.go`
- Test: `cmd/inspectah/internal/renderer/triage_test.go`

- [ ] **Step 1: Write failing tests**

```go
func TestClassifyIdentity_SingleMachine_NoGroups(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.UsersGroups = &schema.UserGroupSection{
		Users: []map[string]interface{}{
			{"name": "admin", "uid": float64(1001), "include": true},
		},
		Groups: []map[string]interface{}{
			{"name": "developers", "gid": float64(1001), "include": true},
		},
	}

	items := classifyIdentity(snap, make(map[string]bool), false)
	for _, item := range items {
		assert.Equal(t, "", item.Group, "identity items should never be grouped")
		assert.False(t, item.DisplayOnly, "identity items are output-affecting")
	}
}

func TestClassifySystemItems_SingleMachine_Grouping(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.KernelBoot = &schema.KernelBootSection{
		SysctlOverrides: []schema.SysctlOverride{
			{Key: "vm.swappiness", Runtime: "10", Include: true},
		},
	}
	snap.Network = &schema.NetworkSection{
		Connections: []schema.NMConnection{
			{Name: "eth0", Type: "ethernet"},
		},
	}
	snap.Storage = &schema.StorageSection{
		FstabEntries: []schema.FstabEntry{
			{MountPoint: "/data", Fstype: "xfs"},
			{MountPoint: "/var", Fstype: "xfs"},
			{MountPoint: "/usr/local", Fstype: "xfs"},
		},
	}

	items := classifySystemItems(snap, make(map[string]bool), false)

	// Sysctl: grouped, output-affecting
	sysctl := findItem(items, "sysctl-vm.swappiness")
	require.NotNil(t, sysctl)
	assert.Equal(t, "sub:sysctl", sysctl.Group)
	assert.False(t, sysctl.DisplayOnly)

	// Network: grouped, display-only
	eth0 := findItem(items, "conn-eth0")
	require.NotNil(t, eth0)
	assert.Equal(t, "sub:network", eth0.Group)
	assert.True(t, eth0.DisplayOnly)

	// Fstab /data: grouped (not risky), display-only
	data := findItem(items, "fstab-/data")
	require.NotNil(t, data)
	assert.Equal(t, "sub:fstab", data.Group)
	assert.True(t, data.DisplayOnly)

	// Fstab /var: individual card (risky mount), display-only
	varMount := findItem(items, "fstab-/var")
	require.NotNil(t, varMount)
	assert.Equal(t, "", varMount.Group)
	assert.True(t, varMount.DisplayOnly)

	// Fstab /usr/local: individual card (risky mount under /usr), display-only
	usrLocal := findItem(items, "fstab-/usr/local")
	require.NotNil(t, usrLocal)
	assert.Equal(t, "", usrLocal.Group)
	assert.True(t, usrLocal.DisplayOnly)
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && go test ./cmd/inspectah/internal/renderer/ -run "TestClassifyIdentity_SingleMachine\|TestClassifySystemItems_SingleMachine" -v`
Expected: FAIL

- [ ] **Step 3: Update classifyIdentity — add isFleet param, no grouping**

```go
func classifyIdentity(snap *schema.InspectionSnapshot, secrets map[string]bool, isFleet bool) []TriageItem {
	// Body is unchanged — identity items never get Group, CardType, or DisplayOnly.
	// The isFleet parameter is accepted for signature consistency but not used.
	// ... existing body stays the same ...
}
```

- [ ] **Step 4: Update classifySystemItems with grouping and risky fstab detection**

```go
var riskyMountPrefixes = []string{"/", "/boot", "/var", "/sysroot", "/usr", "/etc"}

func isRiskyMount(mountPoint string) bool {
	for _, prefix := range riskyMountPrefixes {
		if mountPoint == prefix {
			return true
		}
		if prefix != "/" && strings.HasPrefix(mountPoint, prefix+"/") {
			return true
		}
	}
	return false
}

func isUnstableDevicePath(device string) bool {
	return strings.HasPrefix(device, "/dev/sd") || strings.HasPrefix(device, "/dev/hd")
}

func classifySystemItems(snap *schema.InspectionSnapshot, secrets map[string]bool, isFleet bool) []TriageItem {
	var items []TriageItem
	if snap.KernelBoot != nil {
		for _, s := range snap.KernelBoot.SysctlOverrides {
			group := ""
			if !isFleet {
				group = "sub:sysctl"
			}
			items = append(items, TriageItem{
				Section: "system", Key: "sysctl-" + s.Key,
				Tier: 2, Reason: "Custom sysctl parameter.",
				Name: s.Key, Meta: s.Runtime,
				DefaultInclude: s.Include, Group: group,
			})
		}
		for _, m := range snap.KernelBoot.NonDefaultModules {
			group := ""
			if !isFleet {
				group = "sub:kmod"
			}
			items = append(items, TriageItem{
				Section: "system", Key: "kmod-" + m.Name,
				Tier: 2, Reason: "Kernel module loaded.",
				Name: m.Name, Meta: m.UsedBy,
				DefaultInclude: m.Include, Group: group,
			})
		}
	}
	if snap.Network != nil {
		for _, conn := range snap.Network.Connections {
			group := ""
			displayOnly := false
			if !isFleet {
				group = "sub:network"
				displayOnly = true
			}
			items = append(items, TriageItem{
				Section: "system", Key: "conn-" + conn.Name,
				Tier: 2, Reason: "Network connection configuration.",
				Name: conn.Name, Meta: conn.Type,
				DefaultInclude: isIncluded(conn.Include),
				Group: group, DisplayOnly: displayOnly,
				Acknowledged: conn.Acknowledged,
			})
		}
		for _, zone := range snap.Network.FirewallZones {
			group := ""
			if !isFleet {
				group = "sub:firewall"
			}
			items = append(items, TriageItem{
				Section: "system", Key: "fw-" + zone.Name,
				Tier: 2, Reason: "Custom firewall zone.",
				Name:           zone.Name,
				DefaultInclude: zone.Include, Group: group,
			})
		}
	}
	if snap.Storage != nil {
		for _, entry := range snap.Storage.FstabEntries {
			item := TriageItem{
				Section: "system", Key: "fstab-" + entry.MountPoint,
				Tier: 2, Reason: "Non-default mount point.",
				Name: entry.MountPoint, Meta: entry.Fstype,
				DefaultInclude: isIncluded(entry.Include),
			}
			if !isFleet {
				item.DisplayOnly = true
				item.Acknowledged = entry.Acknowledged
				if isRiskyMount(entry.MountPoint) || isUnstableDevicePath(entry.Device) {
					item.Reason = "Mount interacts with bootc filesystem model. Handle at deploy time."
					// No group — individual card for risky mounts
				} else {
					item.Group = "sub:fstab"
				}
			}
			items = append(items, item)
		}
	}
	return items
}
```

- [ ] **Step 5: Run tests**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && go test ./cmd/inspectah/internal/renderer/ -v`
Expected: All PASS.

- [ ] **Step 6: Commit**

```bash
git add cmd/inspectah/internal/renderer/triage.go cmd/inspectah/internal/renderer/triage_test.go
git commit -m "triage: group system items, display-only network/fstab, risky mounts

Sysctl/kmod/firewall grouped by subsystem. Network connections and
fstab entries marked display-only. Risky mounts (/, /boot, /var,
/sysroot, /usr, /etc, unstable device paths) get individual cards.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 8: JS — buildOutputAccordion function

**Files:**
- Modify: `cmd/inspectah/internal/renderer/static/report.html`

- [ ] **Step 1: Add App state for grouped selection**

In the `var App = {` block, add:

```javascript
groupPriorState: {},  // groupKey -> {itemKey: priorInclude, ...}
```

- [ ] **Step 2: Write buildOutputAccordion function**

Add after the existing `buildDecidedCard` function:

```javascript
function buildOutputAccordion(groupName, items, options) {
  options = options || {};
  var div = document.createElement('div');
  div.className = 'accordion-card';
  div.setAttribute('data-group', groupName);

  // Compute group state
  var allCount = items.length;
  var includedCount = 0;
  for (var i = 0; i < items.length; i++) {
    if (getSnapshotInclude(items[i].key)) includedCount++;
  }
  var groupState = includedCount === allCount ? 'all' : includedCount === 0 ? 'none' : 'partial';
  var isExpanded = App.tierExpanded['group-' + groupName] || false;

  // Header
  var header = document.createElement('div');
  header.className = 'accordion-header';
  header.setAttribute('role', 'button');
  header.setAttribute('aria-expanded', isExpanded ? 'true' : 'false');
  header.setAttribute('tabindex', '0');

  var chevron = document.createElement('span');
  chevron.className = 'accordion-chevron';
  chevron.textContent = isExpanded ? '▼' : '▶';
  header.appendChild(chevron);

  var info = document.createElement('div');
  info.className = 'accordion-info';

  var nameEl = document.createElement('span');
  nameEl.className = 'accordion-name';
  nameEl.textContent = options.displayName || groupName.replace(/^repo:|^kind:|^sub:/, '');
  info.appendChild(nameEl);

  if (options.badge) {
    var badge = document.createElement('span');
    badge.className = 'accordion-badge';
    badge.textContent = options.badge;
    info.appendChild(badge);
  }

  var subtitle = document.createElement('span');
  subtitle.className = 'accordion-subtitle';
  if (groupState === 'partial') {
    subtitle.textContent = includedCount + ' of ' + allCount + ' included';
  } else if (groupState === 'none') {
    subtitle.textContent = allCount + ' items, excluded';
  } else {
    subtitle.textContent = allCount + ' items';
  }
  info.appendChild(subtitle);
  header.appendChild(info);

  // Toggle switch (unless alwaysIncluded)
  if (!options.alwaysIncluded) {
    var toggle = document.createElement('div');
    toggle.className = 'accordion-toggle';
    toggle.setAttribute('role', 'switch');
    toggle.setAttribute('aria-checked', groupState !== 'none' ? 'true' : 'false');
    toggle.setAttribute('tabindex', '0');
    toggle.setAttribute('aria-label', (options.displayName || groupName) + ' toggle');
    var knob = document.createElement('div');
    knob.className = 'accordion-toggle-knob';
    toggle.appendChild(knob);

    if (groupState === 'none') {
      toggle.classList.add('toggle-off');
      div.classList.add('accordion-disabled');
    }

    toggle.onclick = function(e) {
      e.stopPropagation();
      if (App.mode === 'static') return;
      toggleAccordionGroup(groupName, items, groupState !== 'none');
    };
    toggle.onkeydown = function(e) {
      if (e.key === 'Enter' || e.key === ' ') {
        e.preventDefault();
        e.stopPropagation();
        if (App.mode === 'static') return;
        toggleAccordionGroup(groupName, items, groupState !== 'none');
      }
    };
    header.appendChild(toggle);
  } else {
    var label = document.createElement('span');
    label.className = 'accordion-always-label';
    label.textContent = 'always included';
    header.appendChild(label);
  }

  // Header click expands/collapses
  header.onclick = function() {
    var expanded = App.tierExpanded['group-' + groupName];
    App.tierExpanded['group-' + groupName] = !expanded;
    // Re-render parent section
    var sectionId = items[0] ? items[0].section : '';
    if (sectionId) renderTriageSection(sectionId);
  };
  header.onkeydown = function(e) {
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      header.click();
    }
  };

  div.appendChild(header);

  // Expanded content
  if (isExpanded) {
    var table = document.createElement('table');
    table.className = 'accordion-table';

    for (var j = 0; j < items.length; j++) {
      var item = items[j];
      var tr = document.createElement('tr');
      var tdCheck = document.createElement('td');
      var cb = document.createElement('input');
      cb.type = 'checkbox';
      cb.checked = getSnapshotInclude(item.key);
      cb.disabled = groupState === 'none' || App.mode === 'static';
      cb.setAttribute('aria-label', item.name);
      (function(itemKey, sectionId) {
        cb.onchange = function() {
          updateSnapshotInclude(itemKey, this.checked);
          if (this.checked) {
            incrementChangeCounter();
          } else {
            incrementChangeCounter();
          }
          App.decisions[itemKey] = true;
          reopenReviewedSection(sectionId);
          scheduleAutosave();
          renderTriageSection(sectionId);
          updateBadge(sectionId);
        };
      })(item.key, item.section);
      tdCheck.appendChild(cb);
      tr.appendChild(tdCheck);

      var tdName = document.createElement('td');
      tdName.textContent = item.name;
      tr.appendChild(tdName);

      var tdMeta = document.createElement('td');
      tdMeta.className = 'accordion-meta';
      tdMeta.textContent = item.meta || '';
      tr.appendChild(tdMeta);

      table.appendChild(tr);
    }
    div.appendChild(table);
  }

  // Screen reader label
  var srLabel = (options.displayName || groupName.replace(/^repo:|^kind:|^sub:/, ''));
  if (options.badge) srLabel += ', ' + options.badge + ' repository';
  srLabel += ', ' + (groupState === 'partial' ? includedCount + ' of ' + allCount + ' included' : allCount + ' items');
  if (groupState === 'none') srLabel += ', excluded';
  srLabel += isExpanded ? ', expanded' : ', collapsed';
  div.setAttribute('aria-label', srLabel);

  return div;
}

function toggleAccordionGroup(groupName, items, isCurrentlyOn) {
  if (isCurrentlyOn) {
    // Save prior state before turning off
    var prior = {};
    for (var i = 0; i < items.length; i++) {
      prior[items[i].key] = getSnapshotInclude(items[i].key);
      updateSnapshotInclude(items[i].key, false);
    }
    App.groupPriorState[groupName] = prior;
  } else {
    // Restore prior state or default to all-on
    var saved = App.groupPriorState[groupName];
    for (var j = 0; j < items.length; j++) {
      var val = saved ? (saved[items[j].key] !== undefined ? saved[items[j].key] : true) : true;
      updateSnapshotInclude(items[j].key, val);
    }
  }

  incrementChangeCounter();
  var sectionId = items[0] ? items[0].section : '';
  if (sectionId) {
    reopenReviewedSection(sectionId);
    renderTriageSection(sectionId);
    updateBadge(sectionId);
  }
  scheduleAutosave();
}

function reopenReviewedSection(sectionId) {
  if (App.reviewStates[sectionId] === 'reviewed') {
    App.reviewStates[sectionId] = 'in-progress';
    updateSidebarDot(sectionId);
    updateProgressBar();
  }
}
```

- [ ] **Step 3: Add CSS for accordion cards**

Add in the `<style>` section:

```css
.accordion-card {
  border: 1px solid rgba(255,255,255,0.1);
  border-radius: 8px;
  margin-bottom: 6px;
  overflow: hidden;
}
.accordion-card.accordion-disabled {
  opacity: 0.4;
}
.accordion-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 12px 14px;
  cursor: pointer;
  background: rgba(255,255,255,0.03);
}
.accordion-chevron {
  font-size: 11px;
  margin-right: 10px;
  transition: transform 0.15s;
}
.accordion-info {
  flex: 1;
}
.accordion-name {
  font-weight: 600;
  font-size: 14px;
}
.accordion-badge {
  background: rgba(210,153,34,0.15);
  color: #d29922;
  padding: 2px 8px;
  border-radius: 10px;
  font-size: 11px;
  font-weight: 500;
  margin-left: 8px;
}
.accordion-subtitle {
  display: block;
  font-size: 12px;
  opacity: 0.5;
  margin-top: 2px;
}
.accordion-always-label {
  font-size: 11px;
  opacity: 0.35;
  padding: 4px 10px;
  border: 1px solid rgba(255,255,255,0.1);
  border-radius: 10px;
}
.accordion-toggle {
  width: 40px;
  height: 22px;
  border-radius: 11px;
  background: #3fb950;
  position: relative;
  cursor: pointer;
  transition: background 0.15s;
  flex-shrink: 0;
}
.accordion-toggle.toggle-off {
  background: rgba(255,255,255,0.15);
}
.accordion-toggle-knob {
  width: 18px;
  height: 18px;
  border-radius: 50%;
  background: #fff;
  position: absolute;
  top: 2px;
  right: 2px;
  transition: all 0.15s;
}
.accordion-toggle.toggle-off .accordion-toggle-knob {
  right: auto;
  left: 2px;
}
.accordion-table {
  width: 100%;
  border-collapse: collapse;
  border-top: 1px solid rgba(255,255,255,0.06);
  padding: 8px 14px;
  font-size: 13px;
}
.accordion-table td {
  padding: 6px 8px;
  border-top: 1px solid rgba(255,255,255,0.04);
}
.accordion-meta {
  opacity: 0.6;
}
```

- [ ] **Step 4: Manually test in browser**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && go test ./cmd/inspectah/internal/renderer/ -v`
Expected: All PASS (JS changes don't break Go tests).

- [ ] **Step 5: Commit**

```bash
git add cmd/inspectah/internal/renderer/static/report.html
git commit -m "report: add output-affecting accordion component

buildOutputAccordion with three-state model (all/partial/none),
toggle switch, per-item checkboxes, restore on toggle-on, disabled
rows while off. CSS for accordion cards.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 9: JS — buildDisplayAccordion and buildNotificationCard

**Files:**
- Modify: `cmd/inspectah/internal/renderer/static/report.html`

- [ ] **Step 1: Write buildDisplayAccordion function**

```javascript
function buildDisplayAccordion(groupName, items) {
  var div = document.createElement('div');
  div.className = 'accordion-card display-accordion';
  div.setAttribute('data-group', groupName);
  var isExpanded = App.tierExpanded['group-' + groupName] || false;

  var header = document.createElement('div');
  header.className = 'accordion-header';
  header.setAttribute('role', 'button');
  header.setAttribute('aria-expanded', isExpanded ? 'true' : 'false');
  header.setAttribute('tabindex', '0');

  var chevron = document.createElement('span');
  chevron.className = 'accordion-chevron';
  chevron.textContent = isExpanded ? '▼' : '▶';
  header.appendChild(chevron);

  var info = document.createElement('div');
  info.className = 'accordion-info';
  var nameEl = document.createElement('span');
  nameEl.className = 'accordion-name';
  nameEl.textContent = groupName.replace(/^sub:/, '');
  info.appendChild(nameEl);
  var subtitle = document.createElement('span');
  subtitle.className = 'accordion-subtitle';
  subtitle.textContent = items.length + ' items — informational';
  info.appendChild(subtitle);
  header.appendChild(info);

  header.onclick = function() {
    App.tierExpanded['group-' + groupName] = !isExpanded;
    var sectionId = items[0] ? items[0].section : '';
    if (sectionId) renderTriageSection(sectionId);
  };
  header.onkeydown = function(e) {
    if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); header.click(); }
  };
  div.appendChild(header);

  if (isExpanded) {
    var table = document.createElement('table');
    table.className = 'accordion-table';
    for (var i = 0; i < items.length; i++) {
      var tr = document.createElement('tr');
      var tdName = document.createElement('td');
      tdName.textContent = items[i].name;
      tr.appendChild(tdName);
      var tdMeta = document.createElement('td');
      tdMeta.className = 'accordion-meta';
      tdMeta.textContent = items[i].meta || '';
      tr.appendChild(tdMeta);
      table.appendChild(tr);
    }
    div.appendChild(table);
  }

  div.setAttribute('aria-label', groupName.replace(/^sub:/, '') + ', ' + items.length + ' items, informational, ' + (isExpanded ? 'expanded' : 'collapsed'));
  return div;
}
```

- [ ] **Step 2: Write buildNotificationCard function**

```javascript
function buildNotificationCard(item) {
  var wrapper = document.createElement('div');
  wrapper.className = 'notification-card-wrapper';
  wrapper.setAttribute('data-key', item.key);

  // Full (expanded) card
  var full = document.createElement('div');
  full.className = 'triage-card tier-3 notification-full';
  if (item.acknowledged) full.style.display = 'none';

  var warning = document.createElement('div');
  warning.className = 'card-warning';
  warning.textContent = item.reason;
  full.appendChild(warning);

  var nameEl = document.createElement('div');
  nameEl.className = 'card-name';
  nameEl.textContent = item.name;
  full.appendChild(nameEl);

  var meta = document.createElement('div');
  meta.className = 'card-meta';
  meta.textContent = item.meta || '';
  full.appendChild(meta);

  var desc = document.createElement('div');
  desc.style.cssText = 'font-size:13px;opacity:0.7;margin-bottom:10px;';
  desc.textContent = 'Provide a reproducible build-time source and add the installation to your Containerfile.';
  full.appendChild(desc);

  var ackBtn = document.createElement('button');
  ackBtn.className = 'btn-primary';
  ackBtn.textContent = 'Acknowledge';
  ackBtn.style.cssText = 'padding:8px 24px;border-radius:6px;border:none;background:#4493f8;color:#fff;font-size:14px;font-weight:500;cursor:pointer;';
  ackBtn.onclick = function() {
    acknowledgeNotification(item.key, item.section);
  };
  full.appendChild(ackBtn);
  wrapper.appendChild(full);

  // Collapsed card
  var collapsed = document.createElement('div');
  collapsed.className = 'notification-collapsed';
  if (!item.acknowledged) collapsed.style.display = 'none';

  var row = document.createElement('div');
  row.style.cssText = 'display:flex;align-items:center;justify-content:space-between;';
  var left = document.createElement('div');
  left.style.cssText = 'display:flex;align-items:center;gap:10px;';
  var cName = document.createElement('span');
  cName.style.cssText = 'font-weight:600;font-size:14px;';
  cName.textContent = item.name;
  left.appendChild(cName);
  var cWarn = document.createElement('span');
  cWarn.style.cssText = 'font-size:12px;color:#f85149;';
  cWarn.textContent = 'no repo — manual install required';
  if (item.card_type === 'notification' && item.section === 'containers') {
    cWarn.textContent = 'unclear provenance — manual follow-up required';
  }
  left.appendChild(cWarn);
  row.appendChild(left);

  var undoBtn = document.createElement('button');
  undoBtn.style.cssText = 'padding:3px 10px;border-radius:4px;border:1px solid rgba(255,255,255,0.15);background:transparent;color:inherit;font-size:12px;cursor:pointer;opacity:0.6;';
  undoBtn.textContent = 'undo';
  undoBtn.onclick = function() {
    unacknowledgeNotification(item.key, item.section);
  };
  row.appendChild(undoBtn);
  collapsed.appendChild(row);
  wrapper.appendChild(collapsed);

  wrapper.setAttribute('aria-label', item.name + ': ' + (item.acknowledged ? 'acknowledged' : 'manual follow-up required'));
  return wrapper;
}

function acknowledgeNotification(key, sectionId) {
  if (App.mode === 'static') return;
  setSnapshotAcknowledged(key, true);
  App.decisions[key] = true;
  incrementChangeCounter();
  reopenReviewedSection(sectionId);
  renderTriageSection(sectionId);
  updateBadge(sectionId);
  focusNextUndecided(sectionId);
  scheduleAutosave();
}

function unacknowledgeNotification(key, sectionId) {
  if (App.mode === 'static') return;
  setSnapshotAcknowledged(key, false);
  delete App.decisions[key];
  decrementChangeCounter();
  reopenReviewedSection(sectionId);
  renderTriageSection(sectionId);
  updateBadge(sectionId);
  scheduleAutosave();
}
```

- [ ] **Step 3: Add snapshot acknowledged read/write helpers**

```javascript
function getSnapshotAcknowledged(key) {
  // Parse key to find the right item in the snapshot and read acknowledged
  // Uses the same path-walking logic as getSnapshotInclude
  var item = findSnapshotItem(key);
  return item ? (item.acknowledged || false) : false;
}

function setSnapshotAcknowledged(key, value) {
  var item = findSnapshotItem(key);
  if (item) item.acknowledged = value;
}
```

Note: `findSnapshotItem` needs to be extracted from the existing `getSnapshotInclude`/`updateSnapshotInclude` pattern. Look at how those functions navigate the snapshot to find the matching item by key — the same logic applies.

- [ ] **Step 4: Add CSS for notification cards**

```css
.notification-card-wrapper {
  margin-bottom: 8px;
}
.notification-collapsed {
  border: 1px solid rgba(255,255,255,0.08);
  border-radius: 8px;
  padding: 10px 14px;
  background: rgba(255,255,255,0.02);
}
```

- [ ] **Step 5: Commit**

```bash
git add cmd/inspectah/internal/renderer/static/report.html
git commit -m "report: add display-only accordion and notification card components

buildDisplayAccordion for informational surfaces (no toggle/checkboxes).
buildNotificationCard with Acknowledge/undo and acknowledged persistence.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 10: JS — Rewrite renderTriageSection with grouped rendering

**Files:**
- Modify: `cmd/inspectah/internal/renderer/static/report.html`

- [ ] **Step 1: Update renderTriageSection**

Replace the inner tier-rendering loop in `renderTriageSection` to group items by `Group` field:

```javascript
function renderTriageSection(sectionId) {
  var container = document.getElementById('section-' + sectionId);
  if (!container) return;
  container.innerHTML = '';

  var heading = document.createElement('h2');
  heading.className = 'section-heading';
  heading.id = 'heading-' + sectionId;
  heading.setAttribute('tabindex', '-1');
  var label = sectionId;
  for (var s = 0; s < MIGRATION_SECTIONS.length; s++) {
    if (MIGRATION_SECTIONS[s].id === sectionId) {
      label = MIGRATION_SECTIONS[s].label;
      break;
    }
  }
  heading.textContent = label;
  container.appendChild(heading);

  var allItems = getManifestItemsForSection(sectionId);
  if (allItems.length === 0) return;

  var groups = groupByTier(allItems);
  var tiers = [3, 2, 1];

  for (var ti = 0; ti < tiers.length; ti++) {
    var tier = tiers[ti];
    var tierItems = groups[tier] || [];
    if (tierItems.length === 0) continue;

    var tierDiv = document.createElement('div');
    tierDiv.className = 'tier-group';

    var tierLabel = tier === 3 ? 'Flagged' : tier === 2 ? 'Needs decision' : 'Auto-included';
    var tierHeader = document.createElement('div');
    tierHeader.className = 'tier-group-header';
    tierHeader.setAttribute('role', 'button');
    var expandKey = sectionId + '-' + tier;
    var isExpanded = App.tierExpanded[expandKey];
    if (isExpanded === undefined) isExpanded = (tier !== 1);
    tierHeader.setAttribute('aria-expanded', isExpanded ? 'true' : 'false');

    var indicator = document.createElement('span');
    indicator.className = 'tier-indicator tier-' + tier;
    tierHeader.appendChild(indicator);
    var headerText = document.createElement('span');
    headerText.textContent = tierLabel + ' (' + tierItems.length + ')';
    tierHeader.appendChild(headerText);

    tierHeader.onclick = (function(key, hdr) {
      return function() {
        var exp = hdr.getAttribute('aria-expanded') === 'true';
        App.tierExpanded[key] = !exp;
        renderTriageSection(sectionId);
      };
    })(expandKey, tierHeader);

    tierDiv.appendChild(tierHeader);

    if (isExpanded) {
      var itemsDiv = document.createElement('div');
      itemsDiv.className = 'tier-group-items';

      // Separate grouped vs ungrouped items
      var groupedMap = {};
      var ungrouped = [];
      for (var i = 0; i < tierItems.length; i++) {
        var item = tierItems[i];
        if (item.group) {
          if (!groupedMap[item.group]) groupedMap[item.group] = [];
          groupedMap[item.group].push(item);
        } else {
          ungrouped.push(item);
        }
      }

      // Render ungrouped items first (individual cards)
      for (var u = 0; u < ungrouped.length; u++) {
        var card;
        if (ungrouped[u].card_type === 'notification') {
          card = buildNotificationCard(ungrouped[u]);
        } else if (isItemDecided(ungrouped[u])) {
          var inc = getSnapshotInclude(ungrouped[u].key);
          card = buildDecidedCard(ungrouped[u], inc);
        } else {
          card = buildTriageCard(ungrouped[u]);
        }
        itemsDiv.appendChild(card);
      }

      // Render grouped items as accordions
      var groupNames = Object.keys(groupedMap).sort();
      for (var g = 0; g < groupNames.length; g++) {
        var gName = groupNames[g];
        var gItems = groupedMap[gName];
        var isDisplayOnlyGroup = gItems[0] && gItems[0].display_only;

        if (isDisplayOnlyGroup) {
          itemsDiv.appendChild(buildDisplayAccordion(gName, gItems));
        } else {
          var opts = {};
          if (gName.indexOf('repo:baseos') === 0) {
            opts.alwaysIncluded = true;
            opts.displayName = 'BaseOS';
          }
          if (gName.indexOf('repo:') === 0) {
            var repoName = gName.substring(5);
            opts.displayName = opts.displayName || repoName;
            if (isThirdPartyRepoJS(repoName)) {
              opts.badge = 'third-party';
            }
          }
          itemsDiv.appendChild(buildOutputAccordion(gName, gItems, opts));
        }
      }

      tierDiv.appendChild(itemsDiv);
    }

    container.appendChild(tierDiv);
  }

  // Store inventory for rebuild comparison
  var keys = [];
  for (var k = 0; k < allItems.length; k++) {
    keys.push(allItems[k].key + ':' + allItems[k].tier);
  }
  keys.sort();
  var newInv = keys.join(',');
  var oldInv = App.prevInventories[sectionId];
  if (oldInv && oldInv !== newInv && App.reviewStates[sectionId] === 'reviewed') {
    App.reviewStates[sectionId] = 'in-progress';
    updateSidebarDot(sectionId);
    updateProgressBar();
  }
  App.prevInventories[sectionId] = newInv;
}

function isThirdPartyRepoJS(repoName) {
  var standard = ['baseos', 'appstream', 'rhel', 'fedora', 'crb', 'codeready'];
  var lower = repoName.toLowerCase();
  for (var i = 0; i < standard.length; i++) {
    if (lower.indexOf(standard[i]) !== -1) return false;
  }
  return true;
}
```

- [ ] **Step 2: Update isDisplayOnly to use manifest data**

Replace the prefix-based `isDisplayOnly` function:

```javascript
function isDisplayOnly(key) {
  for (var i = 0; i < App.triageManifest.length; i++) {
    if (App.triageManifest[i].key === key) {
      return App.triageManifest[i].display_only || false;
    }
  }
  return false;
}
```

- [ ] **Step 3: Update buildTriageCard to use correct language for display-only items**

In the existing `buildTriageCard` function, check `item.display_only` and use "Acknowledge" / "Skip" instead of "Include in image" / "Leave out":

```javascript
// Inside buildTriageCard, replace the button creation section:
if (item.display_only) {
  var ackBtn = document.createElement('button');
  ackBtn.className = 'btn-primary';
  ackBtn.textContent = 'Acknowledge';
  ackBtn.style.cssText = 'padding:6px 16px;border-radius:6px;border:none;background:#4493f8;color:#fff;font-size:13px;cursor:pointer;';
  ackBtn.onclick = function() { acknowledgeNotification(item.key, item.section); };
  actions.appendChild(ackBtn);

  var skipBtn = document.createElement('button');
  skipBtn.className = 'btn-outline';
  skipBtn.textContent = 'Skip';
  skipBtn.onclick = function() { makeDecision(item.key, item.section, false); };
  actions.appendChild(skipBtn);
} else {
  // existing Include/Leave out buttons
}
```

- [ ] **Step 4: Update static mode**

In `enableStaticMode`, keep accordions collapsed instead of forcing expansion:

```javascript
function enableStaticMode() {
  App.mode = 'static';
  // Static mode: accordions stay collapsed, no interactive controls
  // The renderTriageSection function checks App.mode === 'static'
  // and disables toggles, checkboxes, and acknowledge buttons
}
```

Update `disableAllDecisionButtons` to also disable accordion toggles and acknowledge buttons:

```javascript
function disableAllDecisionButtons() {
  var buttons = document.querySelectorAll('.triage-card button, .accordion-toggle, .notification-full button, .accordion-table input');
  for (var i = 0; i < buttons.length; i++) {
    buttons[i].disabled = true;
    buttons[i].setAttribute('aria-disabled', 'true');
  }
}
```

- [ ] **Step 5: Run Go tests to verify no regressions**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && go test ./cmd/inspectah/internal/renderer/ -v`
Expected: All PASS.

- [ ] **Step 6: Commit**

```bash
git add cmd/inspectah/internal/renderer/static/report.html
git commit -m "report: rewrite renderTriageSection with grouped rendering

Items with Group field render as accordion members. Ungrouped items
render as individual cards. Display-only accordions have no toggle.
BaseOS gets always-included label. Third-party repos get badge.
Static mode keeps accordions collapsed.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 11: Golden-file tests for new card types

**Files:**
- Create: `cmd/inspectah/internal/renderer/testdata/golden-output-accordion.html`
- Create: `cmd/inspectah/internal/renderer/testdata/golden-notification-card.html`
- Modify: `cmd/inspectah/internal/renderer/html_test.go`

- [ ] **Step 1: Write test that generates and compares accordion fragment**

Add a test in `html_test.go` that creates a snapshot with grouped packages, renders the HTML report, and compares a semantic fragment against a golden file. Follow the existing `TestHTMLReportGoldenTierSection` pattern.

```go
func TestHTMLReportGoldenGroupedPackages(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		BaselinePackageNames: strSlicePtr([]string{"bash"}),
		PackagesAdded: []schema.PackageEntry{
			{Name: "bash", Arch: "x86_64", Include: true, SourceRepo: "baseos", Version: "5.2", Release: "1.el9"},
			{Name: "vim", Arch: "x86_64", Include: true, SourceRepo: "appstream", Version: "9.1", Release: "1.el9"},
			{Name: "htop", Arch: "x86_64", Include: true, SourceRepo: "epel", Version: "3.3", Release: "1.el9"},
			{Name: "custom", Arch: "x86_64", Include: true, State: "local_install", Version: "1.0", Release: "1"},
		},
	}

	items := ClassifySnapshot(snap, nil)

	// Verify grouping is correct
	var grouped, ungrouped int
	for _, item := range items {
		if item.Group != "" {
			grouped++
		} else {
			ungrouped++
		}
	}
	assert.Equal(t, 3, grouped, "3 repo-backed packages should be grouped")
	assert.Equal(t, 1, ungrouped, "1 local-install package should be ungrouped")

	// Verify notification card type
	custom := findItem(items, "pkg-custom-x86_64")
	require.NotNil(t, custom)
	assert.Equal(t, "notification", custom.CardType)
}
```

- [ ] **Step 2: Run test**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && go test ./cmd/inspectah/internal/renderer/ -run "TestHTMLReportGoldenGrouped" -v`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add cmd/inspectah/internal/renderer/html_test.go
git commit -m "test: add golden test for grouped package classification

Verifies repo-based grouping and notification card type assignment
for the single-machine triage redesign.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 12: Classifier integration test — fleet vs single-machine

**Files:**
- Modify: `cmd/inspectah/internal/renderer/triage_test.go`

- [ ] **Step 1: Write integration test**

```go
func TestClassifySnapshot_FleetVsSingleMachine(t *testing.T) {
	makeSnap := func() *schema.InspectionSnapshot {
		snap := schema.NewSnapshot()
		snap.Rpm = &schema.RpmSection{
			PackagesAdded: []schema.PackageEntry{
				{Name: "vim", Arch: "x86_64", Include: true, SourceRepo: "appstream"},
			},
		}
		snap.Network = &schema.NetworkSection{
			Connections: []schema.NMConnection{
				{Name: "eth0", Type: "ethernet"},
			},
		}
		return snap
	}

	t.Run("single-machine populates groups", func(t *testing.T) {
		snap := makeSnap()
		items := ClassifySnapshot(snap, nil)
		vim := findItem(items, "pkg-vim-x86_64")
		require.NotNil(t, vim)
		assert.Equal(t, "repo:appstream", vim.Group)
		eth0 := findItem(items, "conn-eth0")
		require.NotNil(t, eth0)
		assert.True(t, eth0.DisplayOnly)
	})

	t.Run("fleet does not populate groups", func(t *testing.T) {
		snap := makeSnap()
		snap.Meta["fleet"] = map[string]interface{}{
			"source_hosts": []interface{}{"h1", "h2"},
			"total_hosts":  float64(2),
		}
		items := ClassifySnapshot(snap, nil)
		vim := findItem(items, "pkg-vim-x86_64")
		require.NotNil(t, vim)
		assert.Equal(t, "", vim.Group)
		eth0 := findItem(items, "conn-eth0")
		require.NotNil(t, eth0)
		assert.False(t, eth0.DisplayOnly)
	})
}
```

- [ ] **Step 2: Run test**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && go test ./cmd/inspectah/internal/renderer/ -run "TestClassifySnapshot_FleetVsSingle" -v`
Expected: PASS

- [ ] **Step 3: Run full test suite**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && go test ./... 2>&1 | tail -20`
Expected: All packages PASS.

- [ ] **Step 4: Commit**

```bash
git add cmd/inspectah/internal/renderer/triage_test.go
git commit -m "test: fleet vs single-machine classification integration test

Verifies that Group and DisplayOnly fields are populated only for
single-machine snapshots (no Meta[fleet]).

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 13: Browser smoke test — manual verification

**Files:** None (manual testing)

- [ ] **Step 1: Build and run refine against a real snapshot**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
go build -o inspectah ./cmd/inspectah
./inspectah refine input-20260323-133834/
```

Open the URL in a browser.

- [ ] **Step 2: Verify accordion rendering in Packages section**

Expected:
- Tier 3 flagged items (if any) appear as individual cards
- Tier 2 items grouped by repo in accordion cards with toggle switches
- BaseOS accordion has "always included" label, no toggle
- Third-party repos have yellow "third-party" badge
- Module streams (if any) appear as individual decision cards
- Tier 1 collapsed as before

- [ ] **Step 3: Test accordion interactions**

- Click accordion header → expands/collapses with chevron rotation
- Click toggle switch → card dims, all items excluded
- Click toggle back on → items restored
- Expand and uncheck individual package → summary updates to "N of M"
- Toggle off after unchecking → toggle on → prior unchecked items stay unchecked (within session)

- [ ] **Step 4: Test notification cards (if no-repo packages present)**

If no no-repo packages in test data, this is tested via unit tests only.

- [ ] **Step 5: Test other sections**

- Config: unchanged configs grouped, modified configs as individual cards
- Runtime: services grouped by state, cron/timers grouped
- System: sysctl/kmod/firewall grouped, network display-only, fstab display-only
- Identity: all individual cards
- Secrets: unchanged

- [ ] **Step 6: Test static mode**

Open `report.html` as `file://` directly. Expected: accordions collapsed, no toggles, no checkboxes, no acknowledge buttons, static banner.

- [ ] **Step 7: Test dark/light theme**

Toggle theme. Expected: all card types readable in both themes.

- [ ] **Step 8: Commit any fixes found during manual testing**

Fix issues discovered during manual testing in focused commits.

---

### Task 14: Final — full test suite and cleanup

**Files:** All modified files

- [ ] **Step 1: Run all Go tests**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && go test ./... -v 2>&1 | tail -30
```

Expected: All packages PASS.

- [ ] **Step 2: Count total tests**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && go test ./... -v 2>&1 | grep -c "^--- PASS\|^--- FAIL"
```

Expected: Test count > 550 (baseline) + new tests.

- [ ] **Step 3: Verify no linting issues**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && go vet ./...
```

Expected: No issues.

- [ ] **Step 4: Final commit if needed**

If any cleanup was needed, commit with appropriate message.
