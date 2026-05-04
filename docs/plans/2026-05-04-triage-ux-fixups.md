# Triage UX Fixups Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix single-machine triage defaults, labels, and structural UX so inspectah acts as a recommendation engine with overrides rather than a decision questionnaire.

**Architecture:** Snapshot normalization before sidecar creation sets default inclusion state. Server-side sync functions in `nativeReRender()` bridge SPA toggle state to renderer-driving structures. JS renders version changes, informational wrappers, three-state secret cards, and an accessible preview pane splitter.

**Tech Stack:** Go (classifier, normalization, sync, reconciliation), vanilla JS (report.html SPA), CSS (themes, affordances)

**Design Spec:** `docs/specs/proposed/2026-05-04-triage-ux-fixups-design.md` (approved, revision 5)

**Go module root:** `cmd/inspectah/` (all `go test` commands run from `/Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah`)

---

## Branch-Truth Notes

**IMPORTANT:** The Go code snippets in this plan are pseudocode illustrating intent. Before implementing, verify the actual types in `cmd/inspectah/internal/schema/types.go`. Key differences from pseudocode:

| Pseudocode | Live `go-port` branch |
|---|---|
| `snap.ConfigFiles` | `snap.Config.Files` |
| `schema.ConfigFile` | `schema.ConfigFileEntry` |
| `snap.Firewall.Zones` | `snap.Network.FirewallZones` |
| `schema.ScheduledTasksSection` | `schema.ScheduledTaskSection` |
| `Include *bool` (pointer, nil-checks) | `Include bool` (plain bool) on `ServiceStateChange`, `CronJob`, `GeneratedTimerUnit`, `QuadletUnit`, `FirewallZone`, `SysctlOverride`, `ConfigFileEntry` |

All `boolPtr()` / nil-check patterns in the pseudocode should be replaced with direct `bool` assignment and comparison against the real struct fields.

**Go test commands** always run from the module root:
```bash
cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/renderer/ -run TestName -count=1 -v
```

**Golden update command:**
```bash
cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && UPDATE_GOLDEN=1 go test ./internal/renderer/ -run 'TestGolden|TestHTMLReportGolden' -count=1
```

---

## File Map

### Go files (modify)
| File | Responsibility |
|------|---------------|
| `cmd/inspectah/internal/renderer/triage.go` | `NormalizeIncludeDefaults` (exported), classifier changes, `classifyVersionChanges` section reassignment |
| `cmd/inspectah/internal/renderer/triage_test.go` | Normalization unit tests, classifier tests |
| `cmd/inspectah/internal/refine/server.go` | Wire `NormalizeIncludeDefaults` alongside existing `NormalizeLeafDefaults` call, BEFORE sidecar creation |
| `cmd/inspectah/internal/refine/server_test.go` | Pre-sidecar normalization proof (working snapshot + sidecar + classifier all agree) |
| `cmd/inspectah/internal/cli/refine.go` | Wire `SyncServiceDecisions`, `SyncCronDecisions`, `ReconcileSecretOverrides` into `nativeReRender()` |
| `cmd/inspectah/internal/cli/refine_test.go` | Artifact roundtrip tests |
| `cmd/inspectah/internal/renderer/render.go` | Pass `reconciledRedactions` through the render pipeline |
| `cmd/inspectah/internal/renderer/containerfile.go` | Accept reconciled redactions in `secretsCommentLines` |
| `cmd/inspectah/internal/renderer/containerfile_test.go` | Service/cron exclusion in rendered output |
| `cmd/inspectah/internal/renderer/configtree.go` | Accept reconciled redactions in `WriteRedactedDir` |
| `cmd/inspectah/internal/renderer/secrets.go` | Accept reconciled redactions in `RenderSecretsReview` |
| `cmd/inspectah/internal/renderer/audit.go` | Use reconciled redaction counts |
| `cmd/inspectah/internal/renderer/readme.go` | Use reconciled redaction counts |
| `cmd/inspectah/internal/renderer/html_test.go` | Golden HTML updates for tier labels, version-changes section |

### Go files (create)
| File | Responsibility |
|------|---------------|
| `cmd/inspectah/internal/renderer/sync.go` | `SyncServiceDecisions`, `SyncCronDecisions` |
| `cmd/inspectah/internal/renderer/sync_test.go` | Sync unit tests |
| `cmd/inspectah/internal/renderer/reconcile.go` | `ReconcileSecretOverrides` |
| `cmd/inspectah/internal/renderer/reconcile_test.go` | Reconciliation unit tests |

### JS/CSS (modify)
| File | Responsibility |
|------|---------------|
| `cmd/inspectah/internal/renderer/static/report.html` | All JS/CSS changes: tier labels, version changes section, info wrapper, secret cards, preview pane, theme fixes |

---

## Tier 1: Default Inclusion Sweep

### Task 1: NormalizeIncludeDefaults function + pre-sidecar wiring

**Files:**
- Modify: `cmd/inspectah/internal/renderer/triage.go` — the exported `NormalizeIncludeDefaults` helper
- Modify: `cmd/inspectah/internal/renderer/triage_test.go` — unit tests
- Modify: `cmd/inspectah/internal/refine/server.go` — wire alongside existing `NormalizeLeafDefaults` in `RunRefine`, BEFORE sidecar creation
- Modify: `cmd/inspectah/internal/refine/server_test.go` — pre-sidecar proof (modeled on existing `TestRunRefine_LeafNormalization`)

- [ ] **Step 1: Write failing tests for NormalizeIncludeDefaults**

```go
func TestNormalizeIncludeDefaults_SingleMachine(t *testing.T) {
	snap := &schema.InspectionSnapshot{
		ConfigFiles: []schema.ConfigFile{
			{Path: "/etc/foo.conf", Include: nil},
			{Path: "/etc/bar.conf", Include: boolPtr(false)},
		},
		Services: &schema.ServiceSection{
			StateChanges: []schema.ServiceStateChange{
				{Unit: "httpd.service", Action: "enable", Include: nil},
				{Unit: "sshd.service", Action: "disable", Include: boolPtr(false)},
			},
			EnabledUnits:  []string{"httpd.service"},
			DisabledUnits: []string{},
		},
		Firewall: &schema.FirewallSection{
			Zones: []schema.FirewallZone{
				{Name: "public", Include: nil},
			},
		},
	}

	NormalizeIncludeDefaults(snap, false)

	// All items should be Include=true after normalization
	for _, cf := range snap.ConfigFiles {
		if cf.Include == nil || !*cf.Include {
			t.Errorf("ConfigFile %s: expected Include=true, got %v", cf.Path, cf.Include)
		}
	}
	for _, sc := range snap.Services.StateChanges {
		if sc.Include == nil || !*sc.Include {
			t.Errorf("Service %s: expected Include=true, got %v", sc.Unit, sc.Include)
		}
	}
	for _, z := range snap.Firewall.Zones {
		if z.Include == nil || !*z.Include {
			t.Errorf("Zone %s: expected Include=true, got %v", z.Name, z.Include)
		}
	}
}

func TestNormalizeIncludeDefaults_Fleet_Untouched(t *testing.T) {
	snap := &schema.InspectionSnapshot{
		ConfigFiles: []schema.ConfigFile{
			{Path: "/etc/foo.conf", Include: nil},
		},
	}

	NormalizeIncludeDefaults(snap, true)

	if snap.ConfigFiles[0].Include != nil {
		t.Error("Fleet snapshot should not be modified by normalization")
	}
}

func TestNormalizeIncludeDefaults_Idempotent(t *testing.T) {
	snap := &schema.InspectionSnapshot{
		ConfigFiles: []schema.ConfigFile{
			{Path: "/etc/foo.conf", Include: boolPtr(true)},
		},
	}

	NormalizeIncludeDefaults(snap, false)

	if !*snap.ConfigFiles[0].Include {
		t.Error("Idempotent normalization should preserve Include=true")
	}
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/renderer/ -run TestNormalizeIncludeDefaults -v`
Expected: FAIL — `NormalizeIncludeDefaults` undefined

- [ ] **Step 3: Implement NormalizeIncludeDefaults**

Add to `triage.go`:

```go
// NormalizeIncludeDefaults sets Include=true for all tier-2 surfaces
// in single-machine mode. Must run BEFORE sidecar creation so both
// the working snapshot and the immutable sidecar agree on defaults.
func NormalizeIncludeDefaults(snap *schema.InspectionSnapshot, isFleet bool) {
	if isFleet {
		return
	}

	// Packages — already handled by normalizeLeafDefaults

	// Config files
	for i := range snap.ConfigFiles {
		if snap.ConfigFiles[i].Include == nil || !*snap.ConfigFiles[i].Include {
			snap.ConfigFiles[i].Include = boolPtr(true)
		}
	}

	// Services
	if snap.Services != nil {
		for i := range snap.Services.StateChanges {
			sc := &snap.Services.StateChanges[i]
			if sc.Include == nil || !*sc.Include {
				// Check incompatible list before defaulting to included
				if isIncompatibleService(sc.Unit) {
					sc.Include = boolPtr(false)
					// Remove from EnabledUnits if present
					snap.Services.EnabledUnits = removeFromSlice(snap.Services.EnabledUnits, sc.Unit)
				} else {
					sc.Include = boolPtr(true)
				}
			}
		}
	}

	// Cron jobs
	if snap.ScheduledTasks != nil {
		for i := range snap.ScheduledTasks.CronJobs {
			if snap.ScheduledTasks.CronJobs[i].Include == nil || !*snap.ScheduledTasks.CronJobs[i].Include {
				snap.ScheduledTasks.CronJobs[i].Include = boolPtr(true)
			}
		}
		// Systemd timers
		for i := range snap.ScheduledTasks.SystemdTimers {
			if snap.ScheduledTasks.SystemdTimers[i].Include == nil || !*snap.ScheduledTasks.SystemdTimers[i].Include {
				snap.ScheduledTasks.SystemdTimers[i].Include = boolPtr(true)
			}
		}
	}

	// Containers — quadlet units
	if snap.Containers != nil {
		for i := range snap.Containers.QuadletUnits {
			if snap.Containers.QuadletUnits[i].Include == nil || !*snap.Containers.QuadletUnits[i].Include {
				snap.Containers.QuadletUnits[i].Include = boolPtr(true)
			}
		}
	}

	// Firewall zones
	if snap.Firewall != nil {
		for i := range snap.Firewall.Zones {
			if snap.Firewall.Zones[i].Include == nil || !*snap.Firewall.Zones[i].Include {
				snap.Firewall.Zones[i].Include = boolPtr(true)
			}
		}
	}

	// Sysctl overrides
	if snap.KernelBoot != nil {
		for i := range snap.KernelBoot.SysctlOverrides {
			if snap.KernelBoot.SysctlOverrides[i].Include == nil || !*snap.KernelBoot.SysctlOverrides[i].Include {
				snap.KernelBoot.SysctlOverrides[i].Include = boolPtr(true)
			}
		}
	}
}

// incompatibleServices is the extensible list of services that cannot
// work in image mode.
var incompatibleServices = map[string]bool{
	"dnf-makecache.service": true,
	"dnf-makecache.timer":   true,
	"packagekit.service":    true,
}

func isIncompatibleService(unit string) bool {
	return incompatibleServices[unit]
}

func removeFromSlice(s []string, val string) []string {
	result := make([]string, 0, len(s))
	for _, v := range s {
		if v != val {
			result = append(result, v)
		}
	}
	return result
}
```

- [ ] **Step 4: Add incompatible service test**

```go
func TestNormalizeIncludeDefaults_IncompatibleServices(t *testing.T) {
	snap := &schema.InspectionSnapshot{
		Services: &schema.ServiceSection{
			StateChanges: []schema.ServiceStateChange{
				{Unit: "dnf-makecache.service", Action: "enable", Include: nil},
				{Unit: "httpd.service", Action: "enable", Include: nil},
			},
			EnabledUnits: []string{"dnf-makecache.service", "httpd.service"},
		},
	}

	NormalizeIncludeDefaults(snap, false)

	// dnf-makecache should be excluded
	if *snap.Services.StateChanges[0].Include != false {
		t.Error("Incompatible service should be excluded")
	}
	// httpd should be included
	if !*snap.Services.StateChanges[1].Include {
		t.Error("Normal service should be included")
	}
	// dnf-makecache removed from EnabledUnits
	for _, u := range snap.Services.EnabledUnits {
		if u == "dnf-makecache.service" {
			t.Error("Incompatible service should be removed from EnabledUnits")
		}
	}
}
```

- [ ] **Step 5: Run all normalization tests**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/renderer/ -run TestNormalizeIncludeDefaults -v`
Expected: All PASS

- [ ] **Step 6: Wire NormalizeIncludeDefaults into refine/server.go**

In `RunRefine()` in `cmd/inspectah/internal/refine/server.go`, find where `renderer.NormalizeLeafDefaults(snap)` is called. Add `renderer.NormalizeIncludeDefaults(snap, isFleet)` immediately after it, BEFORE `original-inspection-snapshot.json` (the sidecar) is written. This is the critical seam: the sidecar must reflect normalized defaults so `ClassifySnapshot(current, original)` produces correct `DefaultInclude` values.

- [ ] **Step 7: Write pre-sidecar proof test**

Add to `cmd/inspectah/internal/refine/server_test.go`, modeled on the existing `TestRunRefine_LeafNormalization`:

```go
func TestRunRefine_IncludeDefaultsNormalization(t *testing.T) {
	// Load a single-machine snapshot where some surfaces have Include=false
	// Run through RunRefine initialization
	// Verify: working snapshot has Include=true for all tier-2 surfaces
	// Verify: sidecar (original-inspection-snapshot.json) agrees
	// Verify: ClassifySnapshot(current, original) produces correct DefaultInclude
	// Verify: incompatible services have Include=false in both snapshots
}
```

- [ ] **Step 8: Run pre-sidecar tests**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/refine/ -run TestRunRefine_IncludeDefaults -count=1 -v`
Expected: All PASS

- [ ] **Step 9: Commit**

```bash
git add cmd/inspectah/internal/renderer/triage.go cmd/inspectah/internal/renderer/triage_test.go cmd/inspectah/internal/refine/server.go cmd/inspectah/internal/refine/server_test.go
git commit -m "feat: add NormalizeIncludeDefaults with pre-sidecar proof

Wired in RunRefine alongside NormalizeLeafDefaults, before sidecar
creation. Working snapshot + sidecar + classifier all agree on defaults.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 2: SyncServiceDecisions and SyncCronDecisions

**Files:**
- Create: `cmd/inspectah/internal/renderer/sync.go`
- Create: `cmd/inspectah/internal/renderer/sync_test.go`

- [ ] **Step 1: Write failing tests for SyncServiceDecisions**

```go
func TestSyncServiceDecisions_ExcludeRemovesFromLists(t *testing.T) {
	snap := &schema.InspectionSnapshot{
		Services: &schema.ServiceSection{
			StateChanges: []schema.ServiceStateChange{
				{Unit: "httpd.service", Action: "enable", Include: boolPtr(false)},
			},
			EnabledUnits:  []string{"httpd.service", "sshd.service"},
			DisabledUnits: []string{},
		},
	}

	SyncServiceDecisions(snap)

	for _, u := range snap.Services.EnabledUnits {
		if u == "httpd.service" {
			t.Error("Excluded service should be removed from EnabledUnits")
		}
	}
}

func TestSyncServiceDecisions_EnableAddsToEnabledUnits(t *testing.T) {
	snap := &schema.InspectionSnapshot{
		Services: &schema.ServiceSection{
			StateChanges: []schema.ServiceStateChange{
				{Unit: "httpd.service", Action: "enable", Include: boolPtr(true)},
			},
			EnabledUnits:  []string{},
			DisabledUnits: []string{},
		},
	}

	SyncServiceDecisions(snap)

	found := false
	for _, u := range snap.Services.EnabledUnits {
		if u == "httpd.service" {
			found = true
		}
	}
	if !found {
		t.Error("Included enable-action service should be in EnabledUnits")
	}
}

func TestSyncServiceDecisions_DisableAddsToDisabledUnits(t *testing.T) {
	snap := &schema.InspectionSnapshot{
		Services: &schema.ServiceSection{
			StateChanges: []schema.ServiceStateChange{
				{Unit: "cups.service", Action: "disable", Include: boolPtr(true)},
			},
			EnabledUnits:  []string{},
			DisabledUnits: []string{},
		},
	}

	SyncServiceDecisions(snap)

	found := false
	for _, u := range snap.Services.DisabledUnits {
		if u == "cups.service" {
			found = true
		}
	}
	if !found {
		t.Error("Included disable-action service should be in DisabledUnits")
	}
}

func TestSyncServiceDecisions_MaskCollapsesToDisabled(t *testing.T) {
	snap := &schema.InspectionSnapshot{
		Services: &schema.ServiceSection{
			StateChanges: []schema.ServiceStateChange{
				{Unit: "NetworkManager-wait-online.service", Action: "mask", Include: boolPtr(true)},
			},
			EnabledUnits:  []string{"NetworkManager-wait-online.service"},
			DisabledUnits: []string{},
		},
	}

	SyncServiceDecisions(snap)

	for _, u := range snap.Services.EnabledUnits {
		if u == "NetworkManager-wait-online.service" {
			t.Error("Masked service should be removed from EnabledUnits")
		}
	}
	found := false
	for _, u := range snap.Services.DisabledUnits {
		if u == "NetworkManager-wait-online.service" {
			found = true
		}
	}
	if !found {
		t.Error("Masked service should be collapsed into DisabledUnits")
	}
}

func TestSyncServiceDecisions_UnchangedNoOp(t *testing.T) {
	snap := &schema.InspectionSnapshot{
		Services: &schema.ServiceSection{
			StateChanges: []schema.ServiceStateChange{
				{Unit: "crond.service", Action: "unchanged", Include: boolPtr(true)},
			},
			EnabledUnits:  []string{"crond.service"},
			DisabledUnits: []string{},
		},
	}

	SyncServiceDecisions(snap)

	if len(snap.Services.EnabledUnits) != 1 || snap.Services.EnabledUnits[0] != "crond.service" {
		t.Error("Unchanged service should not modify EnabledUnits")
	}
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/renderer/ -run TestSyncService -v`
Expected: FAIL — `SyncServiceDecisions` undefined

- [ ] **Step 3: Implement SyncServiceDecisions**

Create `sync.go`:

```go
package renderer

import "github.com/marrusl/inspectah/cmd/inspectah/internal/schema"

// SyncServiceDecisions rebuilds EnabledUnits and DisabledUnits from
// StateChanges to ensure the Containerfile renderer sees the correct
// service enable/disable state after user toggles.
func SyncServiceDecisions(snap *schema.InspectionSnapshot) {
	if snap.Services == nil {
		return
	}

	for _, sc := range snap.Services.StateChanges {
		if sc.Include != nil && !*sc.Include {
			snap.Services.EnabledUnits = removeFromSlice(snap.Services.EnabledUnits, sc.Unit)
			snap.Services.DisabledUnits = removeFromSlice(snap.Services.DisabledUnits, sc.Unit)
			continue
		}

		switch sc.Action {
		case "enable":
			if !containsString(snap.Services.EnabledUnits, sc.Unit) {
				snap.Services.EnabledUnits = append(snap.Services.EnabledUnits, sc.Unit)
			}
		case "disable":
			if !containsString(snap.Services.DisabledUnits, sc.Unit) {
				snap.Services.DisabledUnits = append(snap.Services.DisabledUnits, sc.Unit)
			}
		case "mask":
			snap.Services.EnabledUnits = removeFromSlice(snap.Services.EnabledUnits, sc.Unit)
			if !containsString(snap.Services.DisabledUnits, sc.Unit) {
				snap.Services.DisabledUnits = append(snap.Services.DisabledUnits, sc.Unit)
			}
		case "unchanged":
			// no-op
		}
	}
}

func containsString(s []string, val string) bool {
	for _, v := range s {
		if v == val {
			return true
		}
	}
	return false
}
```

- [ ] **Step 4: Run service sync tests**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/renderer/ -run TestSyncService -v`
Expected: All PASS

- [ ] **Step 5: Write failing tests for SyncCronDecisions**

```go
func TestSyncCronDecisions_PropagatesIncludeToTimerUnits(t *testing.T) {
	snap := &schema.InspectionSnapshot{
		ScheduledTasks: &schema.ScheduledTasksSection{
			CronJobs: []schema.CronJob{
				{Path: "/etc/cron.d/backup", Include: boolPtr(false)},
			},
			GeneratedTimerUnits: []schema.GeneratedTimerUnit{
				{Name: "inspectah-cron-backup-0", SourcePath: "/etc/cron.d/backup", Include: true},
				{Name: "inspectah-cron-backup-1", SourcePath: "/etc/cron.d/backup", Include: true},
			},
		},
	}

	SyncCronDecisions(snap)

	for _, u := range snap.ScheduledTasks.GeneratedTimerUnits {
		if u.Include {
			t.Errorf("Timer %s should inherit Include=false from cron file", u.Name)
		}
	}
}

func TestSyncCronDecisions_MultipleFilesIndependent(t *testing.T) {
	snap := &schema.InspectionSnapshot{
		ScheduledTasks: &schema.ScheduledTasksSection{
			CronJobs: []schema.CronJob{
				{Path: "/etc/cron.d/backup", Include: boolPtr(false)},
				{Path: "/etc/cron.d/cleanup", Include: boolPtr(true)},
			},
			GeneratedTimerUnits: []schema.GeneratedTimerUnit{
				{Name: "inspectah-cron-backup-0", SourcePath: "/etc/cron.d/backup", Include: true},
				{Name: "inspectah-cron-cleanup-0", SourcePath: "/etc/cron.d/cleanup", Include: true},
			},
		},
	}

	SyncCronDecisions(snap)

	if snap.ScheduledTasks.GeneratedTimerUnits[0].Include {
		t.Error("backup timer should be excluded")
	}
	if !snap.ScheduledTasks.GeneratedTimerUnits[1].Include {
		t.Error("cleanup timer should remain included")
	}
}
```

- [ ] **Step 6: Implement SyncCronDecisions**

Add to `sync.go`:

```go
// SyncCronDecisions propagates CronJob include state to all
// GeneratedTimerUnits sharing the same SourcePath.
func SyncCronDecisions(snap *schema.InspectionSnapshot) {
	if snap.ScheduledTasks == nil {
		return
	}

	cronInclude := make(map[string]bool)
	for _, cj := range snap.ScheduledTasks.CronJobs {
		if cj.Include != nil {
			cronInclude[cj.Path] = *cj.Include
		}
	}

	for i := range snap.ScheduledTasks.GeneratedTimerUnits {
		u := &snap.ScheduledTasks.GeneratedTimerUnits[i]
		if inc, ok := cronInclude[u.SourcePath]; ok {
			u.Include = inc
		}
	}
}
```

- [ ] **Step 7: Run all sync tests**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/renderer/ -run "TestSync" -v`
Expected: All PASS

- [ ] **Step 8: Commit**

```bash
git add cmd/inspectah/internal/renderer/sync.go cmd/inspectah/internal/renderer/sync_test.go
git commit -m "feat: add SyncServiceDecisions and SyncCronDecisions

Bridge SPA toggle state to renderer-driving structures (EnabledUnits,
DisabledUnits, GeneratedTimerUnits) on every re-render.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 3: ReconcileSecretOverrides

**Files:**
- Create: `cmd/inspectah/internal/renderer/reconcile.go`
- Create: `cmd/inspectah/internal/renderer/reconcile_test.go`

- [ ] **Step 1: Write failing tests**

Add a test helper if not already present:
```go
func mustMarshal(v interface{}) json.RawMessage {
	b, err := json.Marshal(v)
	if err != nil { panic(err) }
	return b
}
```

```go
func TestReconcileSecretOverrides_ExcludedToIncluded(t *testing.T) {
	snap := &schema.InspectionSnapshot{
		ConfigFiles: []schema.ConfigFile{
			{Path: "/etc/myapp/config.conf", Include: boolPtr(true)},
		},
		Redactions: []json.RawMessage{
			mustMarshal(schema.RedactionFinding{
				Path: "/etc/myapp/config.conf", Source: "file",
				Kind: "excluded", Pattern: "api_key",
			}),
		},
	}

	reconciled := ReconcileSecretOverrides(snap)

	if len(reconciled) != 1 {
		t.Fatalf("expected 1 reconciled finding, got %d", len(reconciled))
	}
	if reconciled[0].Kind != "overridden" {
		t.Errorf("expected Kind=overridden, got %s", reconciled[0].Kind)
	}
}

func TestReconcileSecretOverrides_InlineToExcluded(t *testing.T) {
	snap := &schema.InspectionSnapshot{
		ConfigFiles: []schema.ConfigFile{
			{Path: "/etc/myapp/db.conf", Include: boolPtr(false)},
		},
		Redactions: []json.RawMessage{
			mustMarshal(schema.RedactionFinding{
				Path: "/etc/myapp/db.conf", Source: "file",
				Kind: "inline", Pattern: "password",
			}),
		},
	}

	reconciled := ReconcileSecretOverrides(snap)

	if reconciled[0].Kind != "excluded" {
		t.Errorf("expected Kind=excluded, got %s", reconciled[0].Kind)
	}
}

func TestReconcileSecretOverrides_FlaggedToExcluded(t *testing.T) {
	snap := &schema.InspectionSnapshot{
		ConfigFiles: []schema.ConfigFile{
			{Path: "/etc/suspicious.conf", Include: boolPtr(false)},
		},
		Redactions: []json.RawMessage{
			mustMarshal(schema.RedactionFinding{
				Path: "/etc/suspicious.conf", Source: "file",
				Kind: "flagged", Pattern: "heuristic",
			}),
		},
	}

	reconciled := ReconcileSecretOverrides(snap)

	if reconciled[0].Kind != "excluded" {
		t.Errorf("expected Kind=excluded, got %s", reconciled[0].Kind)
	}
}

func TestReconcileSecretOverrides_PreservesOrdering(t *testing.T) {
	snap := &schema.InspectionSnapshot{
		ConfigFiles: []schema.ConfigFile{
			{Path: "/etc/a.conf", Include: boolPtr(true)},
		},
		Redactions: []json.RawMessage{
			mustMarshal(schema.RedactionFinding{
				Path: "/etc/a.conf", Source: "file", Kind: "inline", Pattern: "p1",
			}),
			mustMarshal(schema.RedactionFinding{
				Path: "/etc/a.conf", Source: "file", Kind: "flagged", Pattern: "p2",
			}),
			mustMarshal(schema.RedactionFinding{
				Path: "/etc/b.conf", Source: "file", Kind: "excluded", Pattern: "p3",
			}),
		},
	}

	reconciled := ReconcileSecretOverrides(snap)

	if len(reconciled) != len(snap.Redactions) {
		t.Fatal("Reconciled slice must preserve length for index-based SPA binding")
	}
	// Order preserved
	if reconciled[0].Pattern != "p1" || reconciled[1].Pattern != "p2" || reconciled[2].Pattern != "p3" {
		t.Error("Reconciled slice must preserve ordering")
	}
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/renderer/ -run TestReconcileSecret -v`
Expected: FAIL

- [ ] **Step 3: Implement ReconcileSecretOverrides**

Create `reconcile.go`:

```go
package renderer

import (
	"encoding/json"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
)

// ReconcileSecretOverrides produces a derived redaction view that
// reflects user override decisions. The canonical snap.Redactions
// slice is NOT modified — ordering is preserved for SPA index binding.
// The returned slice is passed to all artifact generators.
func ReconcileSecretOverrides(snap *schema.InspectionSnapshot) []schema.RedactionFinding {
	// Build config include lookup
	configInclude := make(map[string]*bool)
	for _, cf := range snap.ConfigFiles {
		configInclude[cf.Path] = cf.Include
	}

	reconciled := make([]schema.RedactionFinding, 0, len(snap.Redactions))
	for _, raw := range snap.Redactions {
		var finding schema.RedactionFinding
		if err := json.Unmarshal(raw, &finding); err != nil {
			reconciled = append(reconciled, finding)
			continue
		}

		if finding.Source == "file" {
			if inc, ok := configInclude[finding.Path]; ok && inc != nil {
				switch {
				case finding.Kind == "excluded" && *inc:
					finding.Kind = "overridden"
				case finding.Kind == "inline" && !*inc:
					finding.Kind = "excluded"
				case finding.Kind == "flagged" && !*inc:
					finding.Kind = "excluded"
				}
			}
		}

		reconciled = append(reconciled, finding)
	}

	return reconciled
}
```

- [ ] **Step 4: Run reconciliation tests**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/renderer/ -run TestReconcileSecret -v`
Expected: All PASS

- [ ] **Step 5: Add multi-redaction same-path binding test**

```go
func TestReconcileSecretOverrides_MultiRedactionSamePath(t *testing.T) {
	snap := &schema.InspectionSnapshot{
		Config: &schema.ConfigSection{
			Files: []schema.ConfigFileEntry{
				{Path: "/etc/app.conf", Include: true},
			},
		},
		Redactions: []json.RawMessage{
			mustMarshal(schema.RedactionFinding{
				Path: "/etc/app.conf", Source: "file",
				Kind: "inline", Pattern: "api_key",
			}),
			mustMarshal(schema.RedactionFinding{
				Path: "/etc/app.conf", Source: "file",
				Kind: "flagged", Pattern: "heuristic_password",
			}),
		},
	}

	reconciled := ReconcileSecretOverrides(snap)

	// Both findings preserved, in order, with distinct Kinds
	if len(reconciled) != 2 {
		t.Fatalf("expected 2 findings, got %d", len(reconciled))
	}
	if reconciled[0].Kind != "inline" {
		t.Errorf("finding[0]: expected Kind=inline, got %s", reconciled[0].Kind)
	}
	if reconciled[1].Kind != "flagged" {
		t.Errorf("finding[1]: expected Kind=flagged, got %s", reconciled[1].Kind)
	}
}
```

- [ ] **Step 6: Commit**

```bash
git add cmd/inspectah/internal/renderer/reconcile.go cmd/inspectah/internal/renderer/reconcile_test.go
git commit -m "feat: add ReconcileSecretOverrides for artifact-truth alignment

Produces derived redaction view reflecting user overrides. Canonical
snap.Redactions ordering preserved for SPA index binding. Covers all
three override directions and multi-finding-per-file binding proof.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 4: Wire normalization, sync, and reconciliation into nativeReRender

**Files:**
- Modify: `cmd/inspectah/internal/cli/refine.go`
- Modify: `cmd/inspectah/internal/cli/refine_test.go`

- [ ] **Step 1: Read current nativeReRender implementation**

Read `cmd/inspectah/internal/cli/refine.go` to find `nativeReRender()` and understand the current flow: JSON unmarshal → write snapshot → RunAll → return result.

- [ ] **Step 2: Add sync calls after unmarshal, before re-serialization**

In `nativeReRender()`, after the snapshot is deserialized from `snapData`:

```go
// Synchronize SPA toggle state to renderer-driving structures
renderer.SyncServiceDecisions(snap)
renderer.SyncCronDecisions(snap)

// Re-serialize the synchronized snapshot for export
snapData, _ = json.Marshal(snap)
```

- [ ] **Step 2b: Wire ReconcileSecretOverrides into the render pipeline**

`ReconcileSecretOverrides` produces a derived `[]schema.RedactionFinding` view. This view must be passed to ALL artifact consumers that currently read `snap.Redactions` directly:

- `secretsCommentLines()` in `containerfile.go`
- `WriteRedactedDir()` in `configtree.go`
- `RenderSecretsReview()` in `secrets.go`
- Redaction counts in `audit.go` and `readme.go`
- The render pipeline entry point in `render.go`

The canonical `snap.Redactions` stays unchanged (returned in `ReRenderResult.Snapshot` for SPA binding). Only the render path uses the reconciled view.

This may require modifying `render.go`'s `RunAll()` or equivalent entry point to accept and distribute the reconciled slice.

- [ ] **Step 3: Write artifact roundtrip test**

Add to `refine_test.go`:

```go
func TestNativeReRender_ServiceToggleAffectsContainerfile(t *testing.T) {
	snap := loadTestSnapshot(t, "testdata/single-machine.json")
	// Toggle httpd off
	for i := range snap.Services.StateChanges {
		if snap.Services.StateChanges[i].Unit == "httpd.service" {
			snap.Services.StateChanges[i].Include = boolPtr(false)
		}
	}
	snapData, _ := json.Marshal(snap)
	result, err := nativeReRender(snapData, nil, t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(result.Containerfile, "httpd.service") {
		t.Error("Excluded service should not appear in Containerfile")
	}
	// Verify returned snapshot has httpd removed from EnabledUnits
	var returned schema.InspectionSnapshot
	json.Unmarshal(result.Snapshot, &returned)
	for _, u := range returned.Services.EnabledUnits {
		if u == "httpd.service" {
			t.Error("Excluded service should not be in returned snapshot EnabledUnits")
		}
	}
}

func TestNativeReRender_CronToggleAffectsContainerfile(t *testing.T) {
	snap := loadTestSnapshot(t, "testdata/single-machine.json")
	// Toggle first cron job off
	if len(snap.ScheduledTasks.CronJobs) > 0 {
		snap.ScheduledTasks.CronJobs[0].Include = boolPtr(false)
	}
	snapData, _ := json.Marshal(snap)
	result, err := nativeReRender(snapData, nil, t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	// Verify snapshot sync: generated timer units for that cron file are excluded
	var returned schema.InspectionSnapshot
	json.Unmarshal(result.Snapshot, &returned)
	cronPath := snap.ScheduledTasks.CronJobs[0].Path
	for _, u := range returned.ScheduledTasks.GeneratedTimerUnits {
		if u.SourcePath == cronPath && u.Include {
			t.Errorf("Timer %s should have Include=false after cron toggle", u.Name)
		}
	}
	// Verify renderer truth: generated timer/service content is absent from Containerfile
	// Find the timer unit name(s) for this cron file and assert they don't appear
	for _, u := range snap.ScheduledTasks.GeneratedTimerUnits {
		if u.SourcePath == cronPath {
			if strings.Contains(result.Containerfile, u.Name) {
				t.Errorf("Excluded cron timer %s should not appear in Containerfile", u.Name)
			}
		}
	}
}

func TestNativeReRender_SecretExcludedToIncluded(t *testing.T) {
	// Use an existing test snapshot or create a fixture with an excluded secret
	// Flip the secret's backing config file to Include=true
	// Run nativeReRender with outputDir = t.TempDir()
	// Assert: comment block for that secret is absent from result.Containerfile
	// Assert: .REDACTED placeholder is NOT written to outputDir/redacted/
	// Assert: secrets-review.md notes the override (read outputDir/secrets-review.md)
	// Assert: README redaction count excludes the overridden finding
	//         (read outputDir/README.md, verify count reflects N-1 redactions)
	// Assert: audit report redaction count matches reconciled view
	//         (read outputDir/audit.md or equivalent, verify count)
	// Assert: returned snapshot still has canonical Redactions (Kind unchanged for SPA binding)
}

func TestNativeReRender_SecretFlaggedToExcluded(t *testing.T) {
	// Use a snapshot with a flagged secret
	// Set the secret's backing config file to Include=false
	// Run nativeReRender with outputDir = t.TempDir()
	// Assert: comment block IS present in result.Containerfile (promoted to excluded)
	// Assert: .REDACTED placeholder IS written to outputDir/redacted/
	// Assert: secrets-review.md lists this as excluded
	// Assert: README redaction count includes the newly-excluded finding
	// Assert: audit report redaction count matches reconciled view
	// Assert: returned snapshot still has canonical Redactions (Kind=flagged unchanged)
}
```

- [ ] **Step 4: Run roundtrip tests**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/cli/ -run TestNativeReRender -v`
Expected: All PASS

- [ ] **Step 5: Commit**

```bash
git add cmd/inspectah/internal/cli/refine.go cmd/inspectah/internal/cli/refine_test.go \
  cmd/inspectah/internal/renderer/render.go \
  cmd/inspectah/internal/renderer/containerfile.go \
  cmd/inspectah/internal/renderer/configtree.go \
  cmd/inspectah/internal/renderer/secrets.go \
  cmd/inspectah/internal/renderer/audit.go \
  cmd/inspectah/internal/renderer/readme.go
git commit -m "feat: wire sync and reconciliation into nativeReRender pipeline

Sync runs after unmarshal and before re-serialization so exported
snapshot agrees with rendered output. ReconcileSecretOverrides view
fed to all artifact consumers.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 5: fedora repo always-included + tier labels + version-changes section

**Files:**
- Modify: `cmd/inspectah/internal/renderer/triage.go` — classifier changes (always-included set, Group strings, section assignment)
- Modify: `cmd/inspectah/internal/renderer/triage_test.go`
- Modify: `cmd/inspectah/internal/renderer/static/report.html` — visible label text in `renderTriageSection()` (tier header copy "Auto-included" → "Base image", "Needs decision" → "Your repos"), always-included toggle suppression in `buildOutputAccordion()` (add `fedora` alongside `baseos` in the `options.alwaysIncluded` check)

- [ ] **Step 1: Write failing test for fedora always-included**

```go
func TestClassifyPackages_FedoraRepoAlwaysIncluded(t *testing.T) {
	// Create a snapshot with packages from "fedora" repo
	// Verify the triage item for that repo accordion has AlwaysIncluded: true
}
```

- [ ] **Step 2: Add "fedora" to the always-included repo set**

In `classifyPackages`, find the set of always-included repo IDs (containing `baseos`, `appstream`, `crb`, etc.) and add `"fedora"`.

- [ ] **Step 3: Update tier group labels**

In the classifier, change the group label for tier 1 packages from the current text to `"Base image"` and tier 2 repo packages to `"Your repos"`. Find where `Group` strings are assigned in `classifyPackages`.

- [ ] **Step 4: Write test for tier labels**

```go
func TestClassifyPackages_TierLabels(t *testing.T) {
	// Verify tier 1 items use Group prefix "Base image"
	// Verify tier 2 repo items use Group prefix "Your repos"
}
```

- [ ] **Step 5: Run classifier tests**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/renderer/ -run TestClassifyPackages -v`
Expected: All PASS

- [ ] **Step 6: Update version changes section assignment**

In `classifyVersionChanges`, change `Section: "packages"` to `Section: "version-changes"`. This moves version changes out of the Packages tab and into their own sidebar section.

- [ ] **Step 7: Commit**

```bash
git add cmd/inspectah/internal/renderer/triage.go cmd/inspectah/internal/renderer/triage_test.go
git commit -m "feat: fedora repo always-included, tier labels, version-changes section

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Tier 2: Labeling and Affordance

### Task 6: Expandable header affordance + theme fixes

**Files:**
- Modify: `cmd/inspectah/internal/renderer/static/report.html`

- [ ] **Step 1: Add chevron to tier accordion headers**

In the JS function that renders tier headers (likely `renderTriageSection` or `buildTierHeader`), add a chevron span:

```html
<span class="tier-chevron">▸</span>
```

When expanded: class `expanded` → chevron rotates to `▾`.

- [ ] **Step 2: Add CSS for tier header affordance**

```css
.tier-header {
  cursor: pointer;
}
.tier-header:hover {
  background: var(--pf-t--global--background--color--primary--hover, rgba(128,128,128,0.08));
}
.tier-chevron {
  float: right;
  transition: transform 0.15s ease;
}
.tier-header.expanded .tier-chevron {
  transform: rotate(90deg);
}
```

- [ ] **Step 3: Fix dark mode toggle visibility**

Find the theme toggle button CSS. Add explicit border and color for dark mode:

```css
[data-theme="dark"] .theme-toggle {
  border: 1px solid rgba(255,255,255,0.3);
  color: rgba(255,255,255,0.8);
}
```

Verify contrast ratio >= 3:1 against dark background.

- [ ] **Step 4: Fix light mode brightness**

Change the body/main background from `#ffffff` to a warm off-white:

```css
:root {
  --bg-main: #f8f7f5;
}
```

- [ ] **Step 5: Manual verification**

Open the report in both light and dark mode. Verify:
- Tier headers have visible chevrons and hover states
- Theme toggle is visible in dark mode
- Light mode background is off-white, not glaring

- [ ] **Step 6: Commit**

```bash
git add cmd/inspectah/internal/renderer/static/report.html
git commit -m "fix: tier header affordance, dark toggle visibility, light mode brightness

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Tier 3: Structural Changes

### Task 7: Version changes overview block

**Files:**
- Modify: `cmd/inspectah/internal/renderer/static/report.html`

- [ ] **Step 1: Add version changes block to renderOverview**

In the `renderOverview()` JS function, after the Migration Scope section and before Attention Items, compute upgrade/downgrade counts from `App.snapshot.rpm.version_changes`:

```javascript
var upgrades = 0, downgrades = 0, downgradeNames = [];
if (snap.rpm && snap.rpm.version_changes) {
  for (var i = 0; i < snap.rpm.version_changes.length; i++) {
    var vc = snap.rpm.version_changes[i];
    if (vc.direction === 'upgrade') upgrades++;
    else if (vc.direction === 'downgrade') {
      downgrades++;
      downgradeNames.push(vc.name);
    }
  }
}
```

- [ ] **Step 2: Render the block conditionally**

Only render when `upgrades + downgrades > 0`. Render stat cards (upgrade count green, downgrade count amber), downgrade names callout if any, and "View all N →" link.

- [ ] **Step 3: Wire "View all N →" navigation**

```javascript
viewAllLink.onclick = function() {
  App.activeSection = 'version-changes';
  renderActiveSection();
  var heading = document.querySelector('#section-version-changes h2');
  if (heading) { heading.tabIndex = -1; heading.focus(); }
};
```

- [ ] **Step 4: Manual verification**

Open a report with version changes. Verify the overview block shows counts and the link navigates correctly.

- [ ] **Step 5: Commit**

```bash
git add cmd/inspectah/internal/renderer/static/report.html
git commit -m "feat: version changes overview block with migration preview framing

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 8: Version changes detail section with filters

**Files:**
- Modify: `cmd/inspectah/internal/renderer/static/report.html`

- [ ] **Step 1: Add sidebar entry and section infrastructure for version-changes**

The live `report.html` uses these JS control points for sections:
- `MIGRATION_SECTIONS` array — defines the sidebar section order and IDs
- `renderSidebar()` — builds sidebar links from `MIGRATION_SECTIONS`
- `renderAllSections()` / `renderActiveSection()` — renders section content
- `navigateTo(sectionId)` — handles sidebar clicks and section switching

Add `version-changes` to `MIGRATION_SECTIONS` as a special entry (after the triage sections, with a separator). It should NOT participate in progress tracking (`updateProgressBar`, `updateSidebarDot`). Add an "info" badge in the sidebar rendering.

- [ ] **Step 2: Implement renderVersionChangesSection**

New JS function that renders:
- Header: "Package Version Changes" with count + subtitle
- Radio-group filter (All / Upgrades / Downgrades) with roving tabindex
- Table: Package, Your Host, →, Target Image, Change

```javascript
function renderVersionChangesSection() {
  var container = document.getElementById('section-version-changes');
  // ... render header, filters, table from App.snapshot.rpm.version_changes
}
```

- [ ] **Step 3: Implement radio-group keyboard model**

```javascript
// True radio-group: Left/Right both move focus AND select
filterGroup.addEventListener('keydown', function(e) {
  if (e.key === 'ArrowRight' || e.key === 'ArrowLeft') {
    e.preventDefault();
    var radios = filterGroup.querySelectorAll('[role="radio"]');
    var current = filterGroup.querySelector('[aria-checked="true"]');
    var idx = Array.prototype.indexOf.call(radios, current);
    var next = e.key === 'ArrowRight' ? (idx + 1) % radios.length : (idx - 1 + radios.length) % radios.length;
    // Move focus AND select
    radios[idx].setAttribute('aria-checked', 'false');
    radios[idx].tabIndex = -1;
    radios[next].setAttribute('aria-checked', 'true');
    radios[next].tabIndex = 0;
    radios[next].focus();
    applyFilter(radios[next].dataset.filter);
  }
});
```

- [ ] **Step 4: Implement empty states**

When filter returns zero results: single row spanning all columns with "No [downgrades/upgrades] detected."

When no version changes exist at all: omit sidebar entry and overview block entirely.

- [ ] **Step 5: Manual verification**

Test: filters work with keyboard (arrow keys select), empty states show correctly, section has no progress tracking.

- [ ] **Step 6: Commit**

```bash
git add cmd/inspectah/internal/renderer/static/report.html
git commit -m "feat: version changes detail section with radio-group filters

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 9: Informational wrapper component

**Files:**
- Modify: `cmd/inspectah/internal/renderer/static/report.html`

- [ ] **Step 1: Implement two-pass rendering in renderTriageSection**

Modify `renderTriageSection` to split items:

```javascript
var actionable = items.filter(function(item) { return !item.display_only; });
var informational = items.filter(function(item) { return item.display_only; });
```

Render actionable items first (existing rendering). Then, if `informational.length > 0`, render the collapsible wrapper.

- [ ] **Step 2: Build the wrapper component**

```javascript
function buildInformationalWrapper(items, sectionId) {
  var wrapper = document.createElement('div');
  wrapper.className = 'info-wrapper collapsed';
  wrapper.innerHTML =
    '<button class="info-wrapper-header" aria-expanded="false">' +
    '<span class="tier-chevron">▸</span> Informational — ' + items.length + ' items</button>' +
    '<div class="info-wrapper-body" hidden></div>';
  // Render each informational item inside the body
  // ...
  return wrapper;
}
```

- [ ] **Step 3: Add CSS for the wrapper**

```css
.info-wrapper {
  border-left: 3px solid var(--pf-t--global--border--color--default, #666);
  margin-top: 1rem;
  opacity: 0.85;
}
.info-wrapper-header {
  cursor: pointer;
  padding: 0.5rem 0.75rem;
  background: none;
  border: none;
  width: 100%;
  text-align: left;
  font-size: 0.85rem;
}
```

- [ ] **Step 4: Exclude wrapper items from accounting**

Extend the `isPassiveItem` check to cover `display_only` items that belong in the Informational wrapper — but NOT kernel module items. Kernel modules are `display_only` but stay visible in the main System & Security section (approved design keeps them prominent). The routing rule:

- `display_only && key starts with "kmod-"` → render in main section, no toggle, display-only treatment
- `display_only && NOT kmod-*` → render in Informational wrapper

Update `updateBadge` and `renderTriageSection` footer counting to exclude all `display_only` items from decided/undecided counts regardless of wrapper routing.

- [ ] **Step 5: Fstab simplification**

Remove Acknowledge/Skip buttons from fstab items. They render as plain read-only text inside the informational wrapper. Bootc-managed path entries retain their guidance text as static copy.

- [ ] **Step 6: Omit wrapper when empty**

If `informational.length === 0`, skip rendering the wrapper entirely.

- [ ] **Step 7: Commit**

```bash
git add cmd/inspectah/internal/renderer/static/report.html
git commit -m "feat: informational wrapper with two-pass render, fstab passivity

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 10: Three-state secret cards

**Files:**
- Modify: `cmd/inspectah/internal/renderer/static/report.html`

- [ ] **Step 1: Add redaction kind lookup for secret items**

In the secret card rendering code, join `secret-<n>` items with `App.snapshot.redactions[n]` by extracting the index from the key:

```javascript
function getRedactionForItem(item) {
  if (item.key.indexOf('secret-') !== 0) return null;
  var idx = parseInt(item.key.substring(7), 10);
  if (isNaN(idx) || !App.snapshot.redactions || idx >= App.snapshot.redactions.length) return null;
  return App.snapshot.redactions[idx];
}
```

- [ ] **Step 2: Implement three card renderers**

Split the existing secret card rendering into three paths based on `finding.kind`:

- `renderExcludedSecretCard(item, finding)` — red badge, excluded explanation, comment-block preview
- `renderInlineSecretCard(item, finding)` — amber badge, inline-redacted explanation, COPY explanation
- `renderFlaggedSecretCard(item, finding)` — yellow badge, review-required, content-may-be-untouched warning

Each card has a consequence preview as a secondary disclosure (collapsed by default).

- [ ] **Step 3: Implement symmetric decision buttons**

Each card type has two buttons with opposite actions. Wire to `updateSnapshotInclude` via `source_path`.

- [ ] **Step 4: Implement post-action collapsed states**

After a decision, the card collapses. Show: icon + path + summary text + Undo button.

- [ ] **Step 5: Implement post-action focus rule**

After any decision action, focus moves to the Undo button:

```javascript
function collapseSecretCard(cardEl, key) {
  // ... render collapsed state ...
  var undoBtn = cardEl.querySelector('.undo-btn');
  if (undoBtn) undoBtn.focus();
}
```

After undo, focus returns to the first action button.

- [ ] **Step 6: Manual verification**

Test each card type: excluded, inline, flagged. Verify correct badges, consequence text, both decision paths, undo, and focus routing.

- [ ] **Step 7: Commit**

```bash
git add cmd/inspectah/internal/renderer/static/report.html
git commit -m "feat: three-state secret cards (excluded/inline/flagged)

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 11: Accessible preview pane splitter

**Files:**
- Modify: `cmd/inspectah/internal/renderer/static/report.html`

- [ ] **Step 1: Add splitter element to the preview pane**

Replace the existing pane divider (if any) with an accessible splitter:

```html
<div role="separator"
     aria-orientation="vertical"
     aria-valuenow="400"
     aria-valuemin="200"
     aria-valuemax="960"
     aria-label="Resize Containerfile preview"
     tabindex="0"
     class="preview-splitter">
</div>
```

- [ ] **Step 2: Implement pointer drag**

```javascript
splitter.addEventListener('mousedown', function(e) {
  e.preventDefault();
  var startX = e.clientX;
  var startWidth = previewPane.offsetWidth;
  function onMove(e) {
    var newWidth = Math.min(Math.max(startWidth + (startX - e.clientX), 200), window.innerWidth * 0.6);
    previewPane.style.width = newWidth + 'px';
    splitter.setAttribute('aria-valuenow', Math.round(newWidth));
  }
  function onUp() {
    document.removeEventListener('mousemove', onMove);
    document.removeEventListener('mouseup', onUp);
    savePreviewState();
  }
  document.addEventListener('mousemove', onMove);
  document.addEventListener('mouseup', onUp);
});
```

- [ ] **Step 3: Implement keyboard resize**

```javascript
splitter.addEventListener('keydown', function(e) {
  var current = previewPane.offsetWidth;
  var step = 10;
  switch (e.key) {
    case 'ArrowLeft': current += step; break;
    case 'ArrowRight': current -= step; break;
    case 'Home': current = 200; break;
    case 'End': current = window.innerWidth * 0.6; break;
    case 'Enter': togglePreviewCollapse(); return;
    default: return;
  }
  e.preventDefault();
  current = Math.min(Math.max(current, 200), window.innerWidth * 0.6);
  previewPane.style.width = current + 'px';
  splitter.setAttribute('aria-valuenow', Math.round(current));
  savePreviewState();
});
```

- [ ] **Step 4: Implement collapse/expand toggle**

Add a toggle button in the preview header. Collapse hides the pane and moves focus to the toggle. Expand restores last width and moves focus to the splitter.

- [ ] **Step 5: Implement localStorage persistence**

```javascript
function savePreviewState() {
  try {
    localStorage.setItem('inspectah-preview-pane', JSON.stringify({
      width: previewPane.offsetWidth,
      collapsed: previewPane.classList.contains('collapsed')
    }));
  } catch (e) { /* localStorage unavailable — no-op */ }
}

function loadPreviewState() {
  try {
    var saved = JSON.parse(localStorage.getItem('inspectah-preview-pane'));
    if (saved) {
      previewPane.style.width = saved.width + 'px';
      if (saved.collapsed) togglePreviewCollapse();
    }
  } catch (e) { /* fallback to defaults */ }
}
```

- [ ] **Step 6: Add splitter CSS**

```css
.preview-splitter {
  width: 6px;
  cursor: col-resize;
  background: var(--pf-t--global--border--color--default);
  flex-shrink: 0;
}
.preview-splitter:focus {
  outline: 2px solid var(--pf-t--global--color--brand--default);
  outline-offset: -2px;
}
```

- [ ] **Step 7: Manual verification**

Test: drag resize, keyboard resize (arrow keys, Home, End), Enter to toggle collapse, localStorage persistence across reload, works without localStorage.

- [ ] **Step 8: Commit**

```bash
git add cmd/inspectah/internal/renderer/static/report.html
git commit -m "feat: accessible preview pane splitter with localStorage persistence

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Tier 4: Investigation Items

### Task 12: Kernel module filtering + modprobe.d awareness

**Files:**
- Modify: `cmd/inspectah/internal/renderer/triage.go`
- Modify: `cmd/inspectah/internal/renderer/triage_test.go`
- Modify: `cmd/inspectah/internal/renderer/static/report.html`

- [ ] **Step 1: Write failing test for module filtering**

```go
func TestClassifyKernelModules_OnlyModulesLoadD(t *testing.T) {
	// Create snapshot with NonDefaultModules and ModulesLoadD config
	// Verify only modules named in ModulesLoadD files appear in triage
	// Verify auto-loaded modules (not in ModulesLoadD) are excluded
}
```

- [ ] **Step 2: Implement filtering in classifySystemItems**

In the kernel module classification block, cross-reference modules against `snap.KernelBoot.ModulesLoadD` content. Only include modules whose name appears in a `modules-load.d` config file.

- [ ] **Step 3: Change kernel module items to display-only**

Set `DisplayOnly: true` on kernel module triage items. They are comment-only on the current branch — no output-affecting toggle. Use a distinguishable key prefix (e.g., `kmod-`) so JS routing can keep them visible in the main System & Security section rather than burying them in the Informational wrapper.

- [ ] **Step 4: Render kernel modules as display-only in main section (not wrapper)**

In the JS rendering for System & Security, kernel module display-only items render as an accordion WITHOUT a toggle switch in the MAIN section area (above the Informational wrapper). The accordion header notes: "Module load configuration is carried via the config tree — these are shown for awareness."

- [ ] **Step 5: Add modprobe.d informational accordion**

In the JS rendering for System & Security, add a display-only accordion for modprobe.d policy files inside the Informational wrapper (this one IS in the wrapper since it's pure awareness):

Header: "Module policy files — N files"
Subheader: "These modprobe configuration files will be included in the image via the config tree. Review if migrating to different hardware."
Rows: file path + directive type summary

- [ ] **Step 6: Run tests**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/renderer/ -run TestClassifyKernel -count=1 -v`
Expected: All PASS

- [ ] **Step 7: Commit**

```bash
git add cmd/inspectah/internal/renderer/triage.go cmd/inspectah/internal/renderer/triage_test.go cmd/inspectah/internal/renderer/static/report.html
git commit -m "feat: kernel module filtering (modules-load.d only) + modprobe.d awareness

Modules are display-only in main section on the current branch.
Modprobe.d files shown as informational accordion for hardware migration
awareness.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Final Verification

### Task 13: Golden HTML updates and full test suite

**Files:**
- Modify: `cmd/inspectah/internal/renderer/html_test.go`
- Modify: `cmd/inspectah/internal/renderer/testdata/golden-*.html` (as needed)

**Important:** The existing Go goldens (`golden-sidebar.html`, `golden-tier-section.html`, `golden-containerfile.txt`) validate template output and manifest fragments. They do NOT prove the runtime JS DOM contracts (informational wrapper, version-changes UI, secret-card states, preview-pane behaviors). The browser checklist is the proof for runtime JS behavior.

- [ ] **Step 1: Regenerate golden HTML test expectations**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && UPDATE_GOLDEN=1 go test ./internal/renderer/ -run 'TestGolden|TestHTMLReportGolden' -count=1
```

Verify the updated goldens reflect:
- "Base image" / "Your repos" tier labels
- Version changes in `version-changes` section (not `packages`)
- Display-only kernel module items (with `kmod-` key prefix)

- [ ] **Step 2: Add multi-redaction binding regression test (automated)**

Add a Go test in `triage_test.go` that proves the classifier/manifest seam preserves the `secret-<n> -> redactions[n]` binding for same-path multi-redaction cases:

```go
func TestClassifySecretItems_MultiRedactionSamePath(t *testing.T) {
	// Create a snapshot with 2+ RedactionFinding entries sharing the same Path
	// (e.g., one "inline" and one "flagged" for /etc/app.conf)
	// Run ClassifySnapshot
	// Assert: manifest contains two distinct secret items: secret-N and secret-(N+1)
	// Assert: secret-N maps to redactions[N] (inline) and secret-(N+1) maps to redactions[N+1] (flagged)
	// Assert: the items have distinct keys, not collapsed by path
}
```

This proves the binding survives at the manifest level, not just at the reconciliation level.

- [ ] **Step 3: Run full Go test suite (including new regression test)**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/... -count=1 -v
```

Expected: All PASS

- [ ] **Step 4: Run manual browser verification checklist**

Open the report in a browser and verify ALL 33 items from the spec's Browser Contract Tests section. Both light and dark modes. The full checklist is in the approved design spec at `docs/specs/proposed/2026-05-04-triage-ux-fixups-design.md` under "Browser Contract Tests (Manual Verification)". Key areas that Go goldens do NOT cover:

- Version-changes focus landing from "View all →" + radio-group filter keyboard behavior
- Informational wrapper default-collapsed and empty-omission behavior
- Three-state secret card badges, consequence previews, symmetric decision paths, post-action Undo focus
- Preview pane splitter keyboard/screen-reader/focus/localStorage behavior
- Kernel module display-only framing in main section (NOT wrapper)
- All ARIA state checks (`aria-expanded`, `aria-checked`, screen reader announcements)

- [ ] **Step 5: Manual browser verification of multi-redaction binding**

Open a report with a file that has multiple redaction findings. Verify each `secret-<n>` card shows the correct kind badge and consequence text for its specific finding.

- [ ] **Step 6: Commit golden updates and new regression tests**

```bash
git add -A cmd/inspectah/internal/renderer/testdata/ \
  cmd/inspectah/internal/renderer/triage_test.go \
  cmd/inspectah/internal/renderer/html_test.go
git commit -m "test: update goldens and add multi-redaction binding regression

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Summary

| Task | Tier | Description | Files |
|------|------|-------------|-------|
| 1 | 1 | NormalizeIncludeDefaults + pre-sidecar wiring | triage.go, triage_test.go, refine/server.go, refine/server_test.go |
| 2 | 1 | SyncServiceDecisions + SyncCronDecisions | sync.go, sync_test.go |
| 3 | 1 | ReconcileSecretOverrides | reconcile.go, reconcile_test.go |
| 4 | 1 | Wire into nativeReRender + artifact consumers | cli/refine.go, cli/refine_test.go, render.go, containerfile.go, configtree.go, secrets.go, audit.go, readme.go |
| 5 | 1 | fedora always-included + tier labels + version-changes section | triage.go, triage_test.go, report.html |
| 6 | 2 | Header affordance + theme fixes | report.html |
| 7 | 3 | Version changes overview block | report.html |
| 8 | 3 | Version changes detail section + filters | report.html |
| 9 | 3 | Informational wrapper + fstab (kernel modules stay in main section) | report.html |
| 10 | 3 | Three-state secret cards | report.html |
| 11 | 3 | Accessible preview pane splitter | report.html |
| 12 | 4 | Kernel module filtering + modprobe.d (display-only in main section) | triage.go, triage_test.go, report.html |
| 13 | — | Golden updates + full 33-item browser verification | html_test.go, testdata/ |
