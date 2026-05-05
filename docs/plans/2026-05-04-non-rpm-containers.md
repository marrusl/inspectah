# Non-RPM + Containers Design Implementation Plan

> **Revision 4** (2026-05-04): Addresses two must-fix issues from round-3 review.
>
> **Changes from revision 3:**
> - **Task 8 / Task 14 atomicity:** Former Task 8 (classifier routes non-RPM items to `section="nonrpm"`) and former Task 14 (SPA section plumbing for nonrpm) are merged into a single indivisible Task 8. The classifier change and SPA section plumbing land in one commit — no window where items route to a section the SPA cannot render. Former Tasks 15-20 renumbered to 14-19.
> - **Task 17 (former Task 18) live-seam fixes:**
>   - **Route dispatch:** `ServeHTTP` switch now includes `case path == "/api/quadlet-draft":` alongside the `h.mux.HandleFunc` registration. Tests updated to exercise the full `ServeHTTP` dispatch path (they already use `httptest.NewServer(handler)`, which calls `ServeHTTP`).
>   - **Card path:** Running containers are `DisplayOnly` in the live classifier and render through `buildInfoReadOnlyCard`. The draft button is now attached inside `buildInfoReadOnlyCard` (for `container-` prefixed keys in refine mode), not `buildToggleCard`.
>   - **Stale snapshot refresh:** The draft endpoint now returns a full rebuild response (same shape as `POST /api/render`: `html`, `snapshot`, `containerfile`, `triage_manifest`, `render_id`, `revision`) by calling `h.reRenderFn` after saving the draft. The SPA calls `applyRebuildResponse()` directly on the draft endpoint's response — no separate `POST /api/render` call, no stale `App.snapshot` race.
>
> **Changes from revision 2:**
> - **B1 (Non-RPM section plumbing):** Task 8 now atomically includes both the classifier extraction AND all SPA section plumbing (MIGRATION_SECTIONS entry, renderAllSections/updateAllBadges/navigateTo countBadge extensions, section heading, footer suppression, badge initialization). No sequencing dependency — one commit covers both sides.
> - **B2 (Flatpak retry + SPA caveats):** Task 12 oneshot service now includes bounded systemd retry (`Restart=on-failure`, `RestartSec=30s`, `StartLimitBurst=3`, `StartLimitIntervalSec=300s`). Tests assert retry directives are present in generated service. Task 15 (formerly Task 16) flatpak card annotation expanded to include three operator-facing caveats: network access required at first boot, remote configuration is best-effort, custom/enterprise remotes may require manual setup.
> - **B3 (Quadlet draft HostIp + deferred TODOs + Task 17 live seams):** Task 13 `extractPortMappings()` now preserves non-default `HostIp` (e.g., `127.0.0.1:8080:8080`). Test added for non-default HostIp. Generated `.container` draft now includes explicit TODO comments for deferred healthcheck, dependency-ordering, and user-namespace gaps. Task 17 (formerly Task 18) test harness fixed: uses `httptest.NewServer(handler)` (not `handler.mux`), route registration uses live `h.mux.HandleFunc(...)` pattern plus `ServeHTTP` switch case, draft endpoint returns full rebuild response, SPA calls `applyRebuildResponse()` on the draft response directly. Running containers get draft button via `buildInfoReadOnlyCard` (`DisplayOnly` items), with correct card-path routing.
> - **B4 (Compose output drift + running-container null predicate + DisplayOnly tier escape + overall empty state):** Task 12 `containersSectionLines()` compose handling completely removed — no `includedCompose`, no comments, nothing. Compose is invisible to the Containerfile renderer. Task 18 (formerly Task 19) running-container empty-state predicate fixed to use `=== null` (not `!== undefined`) to correctly distinguish `null` (not queried) from `[]` (queried, none found). Task 18 compose items escape the informational tier by using `card_type: 'compose-info'` instead of `DisplayOnly: true`, rendering through a dedicated card builder in the main grouped loop. Task 10 updated to set `CardType: "compose-info"` instead of `DisplayOnly: true`. Containers section overall empty state added (`No container workloads detected.`).
> - **B5 (Verification story):** Task 19 (formerly Task 20) integration test now renders actual Containerfile output from snapshot (calls `containersSectionLines` and `nonRpmSectionLines`, not `goldenTestHelper` with embedded string). Assertions: non-RPM `migration_planned` items produce stubs, compose produces ZERO output (no lines at all), flatpak included apps produce manifest/service references. Tests for flatpak retry directives added in Task 12. Test for non-default HostIp port mapping added in Task 13.
>
> **Changes from revision 1:**
> - **Blocker 1 (Non-RPM live-branch binding):** Non-RPM section uses `tracked: false` with custom sidebar badge (not_reviewed count, not the include/exclude progress dot). Task 8 now atomically includes the non-RPM–specific sidebar badge handler alongside the classifier extraction. Task 14 (formerly Task 15) rewrites the review-status control from hidden-radio inputs to visible ARIA `role="radio"` buttons matching the version-changes filter pattern. renderTriageSection dispatches to `buildReviewStatusCard` for `sectionId === 'nonrpm'`, bypassing the toggle-card path entirely. Task 11 gates on `review_status == "migration_planned"` only — the `Include` field is not consulted for non-RPM Containerfile output. Added empty-state messages for both "no items detected" and "scanning not performed."
> - **Blocker 2 (Flatpak path not independently landable):** Task 9 now includes the SPA `getSnapshotInclude` / `updateSnapshotInclude` flatpak handlers (previously in Task 15), so flatpak triage items have a working read/write path from the moment they appear. Task 12 oneshot service uses pure shell (no jq), implements bounded `flatpak remote-add --if-not-exists` before installs, and includes a comment for remotes that cannot be fully reconstructed.
> - **Blocker 3 (Quadlet draft contract drift):** Task 13 maps restart policy from actual `podman inspect` data when available, falls back to `# TODO: Review restart policy` comment when absent — does not invent `Restart=on-failure`. Task 17 (formerly Task 18) sets `Include: false` on generated drafts, adds duplicate-draft suppression (no-op + disabled "Draft generated" button), missing-image error handling, and full post-click UX states (scroll-to-new-entry, focus management).
> - **Blocker 4 (Containers hierarchy / Compose contract):** Task 10 compose items set `DisplayOnly: true` and produce zero Containerfile output. Task 18 (formerly Task 19) rewrites sort/group logic to work with the live `renderTriageSection` path — custom subsection ordering within the existing tier/group rendering, with subsection header elements inserted before each group's cards. Running containers (ungrouped, no `sub:` prefix) sort after flatpaks by default.
> - **Blocker 5 (Verification story overclaims):** All tasks now distinguish Go-testable behavior (HTML structure, JSON content, API responses, triage classification) from JS-dependent behavior (keyboard navigation, focus management, visual hierarchy, post-action UX). Each SPA task includes a "Browser verification needed" section listing exactly what requires Playwright or manual testing. The integration test (Task 19, formerly Task 20) verifies structural correctness only and does not claim to prove JS interaction behavior.
> - **Additional reviewer notes:** Empty-state messages added (Task 8 for non-RPM, Task 18 for containers/running-containers). Running-container empty state distinguishes `--query-podman` not used vs. no containers found.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Restructure containers and non-RPM software into properly separated triage sections with richer inspection data, new card interactions, and data-driven Containerfile output.

**Architecture:** Phase 1 adds schema fields and bumps the version. Phases 2-3 improve inspection and triage classification independently. Phase 4 enhances Containerfile rendering. Phase 5 updates the SPA for the new section structure and interaction patterns. Each phase depends only on the one before it.

**Tech Stack:** Go 1.22+, vanilla JS (PatternFly 6 CSS), `encoding/json`, `os/exec` for inspector commands, `net/http` for refine server. No new dependencies.

---

## Phase 1: Schema

### Task 1: Add new fields to NonRpmItem

**Files:**
- Modify: `cmd/inspectah/internal/schema/types.go:579-599`
- Test: `cmd/inspectah/internal/schema/types_test.go`

- [ ] **Step 1: Write the failing test**

```go
// In types_test.go — add at end of file
func TestNonRpmItem_ReviewStatusJSON(t *testing.T) {
	item := NonRpmItem{
		Path:         "usr/local/bin/agent",
		Name:         "agent",
		Method:       "standalone binary",
		Confidence:   "high",
		ReviewStatus: "migration_planned",
		Notes:        "Will COPY binary directly",
	}

	data, err := json.Marshal(item)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}

	var decoded NonRpmItem
	if err := json.Unmarshal(data, &decoded); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}

	if decoded.ReviewStatus != "migration_planned" {
		t.Errorf("ReviewStatus = %q, want %q", decoded.ReviewStatus, "migration_planned")
	}
	if decoded.Notes != "Will COPY binary directly" {
		t.Errorf("Notes = %q, want %q", decoded.Notes, "Will COPY binary directly")
	}
}

func TestNonRpmItem_ReviewStatusOmitEmpty(t *testing.T) {
	item := NonRpmItem{
		Path:   "usr/local/bin/tool",
		Name:   "tool",
		Method: "standalone binary",
	}

	data, err := json.Marshal(item)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}

	s := string(data)
	if strings.Contains(s, "review_status") {
		t.Error("review_status should be omitted when empty")
	}
	if strings.Contains(s, `"notes"`) {
		t.Error("notes should be omitted when empty")
	}
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/schema/... -run TestNonRpmItem_ReviewStatus -v`
Expected: FAIL — `ReviewStatus` and `Notes` fields don't exist on `NonRpmItem`

- [ ] **Step 3: Add ReviewStatus and Notes fields to NonRpmItem**

In `cmd/inspectah/internal/schema/types.go`, add two fields after the `Fleet` field (line 598):

```go
// NonRpmItem is a single item found by the Non-RPM Software inspector.
type NonRpmItem struct {
	Path               string                  `json:"path"`
	Name               string                  `json:"name"`
	Method             string                  `json:"method"`
	Confidence         string                  `json:"confidence"`
	Include            bool                    `json:"include"`
	Acknowledged       bool                    `json:"acknowledged,omitempty"`
	Lang               string                  `json:"lang"`
	Static             bool                    `json:"static"`
	Version            string                  `json:"version"`
	SharedLibs         []string                `json:"shared_libs"`
	SystemSitePackages bool                    `json:"system_site_packages"`
	Packages           []PipPackage            `json:"packages"`
	HasCExtensions     bool                    `json:"has_c_extensions"`
	GitRemote          string                  `json:"git_remote"`
	GitCommit          string                  `json:"git_commit"`
	GitBranch          string                  `json:"git_branch"`
	Files              *map[string]interface{} `json:"files"`
	Content            string                  `json:"content"`
	Fleet              *FleetPrevalence        `json:"fleet"`
	ReviewStatus       string                  `json:"review_status,omitempty"`
	Notes              string                  `json:"notes,omitempty"`
}
```

- [ ] **Step 4: Add `strings` import to types_test.go if not present**

Check imports in `types_test.go`. If `"strings"` is not already imported, add it.

- [ ] **Step 5: Run test to verify it passes**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/schema/... -run TestNonRpmItem_ReviewStatus -v`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add cmd/inspectah/internal/schema/types.go cmd/inspectah/internal/schema/types_test.go
git commit -m "feat(schema): add ReviewStatus and Notes fields to NonRpmItem

Supports the non-RPM review workflow where operators mark items as
not_reviewed, reviewed, or migration_planned with freeform notes.

Assisted-by: Claude Code"
```

---

### Task 2: Add new fields to QuadletUnit

**Files:**
- Modify: `cmd/inspectah/internal/schema/types.go:508-518`
- Test: `cmd/inspectah/internal/schema/types_test.go`

- [ ] **Step 1: Write the failing test**

```go
// In types_test.go
func TestQuadletUnit_PortsVolumesGenerated(t *testing.T) {
	unit := QuadletUnit{
		Path:    "etc/containers/systemd/webapp.container",
		Name:    "webapp.container",
		Content: "[Container]\nImage=foo\n",
		Image:   "foo",
		Ports:   []string{"8080:8080", "443:443"},
		Volumes: []string{"data.volume:/data"},
		Generated: true,
	}

	data, err := json.Marshal(unit)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}

	var decoded QuadletUnit
	if err := json.Unmarshal(data, &decoded); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}

	if len(decoded.Ports) != 2 {
		t.Errorf("Ports len = %d, want 2", len(decoded.Ports))
	}
	if len(decoded.Volumes) != 1 {
		t.Errorf("Volumes len = %d, want 1", len(decoded.Volumes))
	}
	if !decoded.Generated {
		t.Error("Generated should be true")
	}
}

func TestQuadletUnit_PortsVolumesOmitEmpty(t *testing.T) {
	unit := QuadletUnit{
		Name: "simple.container",
	}

	data, err := json.Marshal(unit)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}

	s := string(data)
	if strings.Contains(s, `"ports"`) {
		t.Error("ports should be omitted when nil")
	}
	if strings.Contains(s, `"volumes"`) {
		t.Error("volumes should be omitted when nil")
	}
	if strings.Contains(s, `"generated"`) {
		t.Error("generated should be omitted when false")
	}
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/schema/... -run TestQuadletUnit_PortsVolumes -v`
Expected: FAIL — fields don't exist

- [ ] **Step 3: Add Ports, Volumes, Generated fields to QuadletUnit**

```go
// QuadletUnit is a Podman Quadlet unit file.
type QuadletUnit struct {
	Path      string           `json:"path"`
	Name      string           `json:"name"`
	Content   string           `json:"content"`
	Image     string           `json:"image"`
	Include   bool             `json:"include"`
	Tie       bool             `json:"tie"`
	TieWinner bool             `json:"tie_winner"`
	Fleet     *FleetPrevalence `json:"fleet"`
	Ports     []string         `json:"ports,omitempty"`
	Volumes   []string         `json:"volumes,omitempty"`
	Generated bool             `json:"generated,omitempty"`
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/schema/... -run TestQuadletUnit_PortsVolumes -v`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add cmd/inspectah/internal/schema/types.go cmd/inspectah/internal/schema/types_test.go
git commit -m "feat(schema): add Ports, Volumes, Generated fields to QuadletUnit

Ports and Volumes are extracted from PublishPort= and Volume=
directives by the inspector. Generated marks units created by
the quadlet-draft feature rather than detected on the source system.

Assisted-by: Claude Code"
```

---

### Task 3: Add new fields to FlatpakApp

**Files:**
- Modify: `cmd/inspectah/internal/schema/types.go:552-558`
- Test: `cmd/inspectah/internal/schema/types_test.go`

- [ ] **Step 1: Write the failing test**

```go
// In types_test.go
func TestFlatpakApp_RemoteFields(t *testing.T) {
	app := FlatpakApp{
		AppID:     "org.mozilla.firefox",
		Origin:    "flathub",
		Branch:    "stable",
		Include:   true,
		Remote:    "flathub",
		RemoteURL: "https://dl.flathub.org/repo/",
	}

	data, err := json.Marshal(app)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}

	var decoded FlatpakApp
	if err := json.Unmarshal(data, &decoded); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}

	if decoded.Remote != "flathub" {
		t.Errorf("Remote = %q, want %q", decoded.Remote, "flathub")
	}
	if decoded.RemoteURL != "https://dl.flathub.org/repo/" {
		t.Errorf("RemoteURL = %q, want %q", decoded.RemoteURL, "https://dl.flathub.org/repo/")
	}
}

func TestFlatpakApp_RemoteFieldsOmitEmpty(t *testing.T) {
	app := FlatpakApp{
		AppID:  "org.gnome.Calculator",
		Origin: "fedora",
		Branch: "stable",
	}

	data, err := json.Marshal(app)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}

	s := string(data)
	if strings.Contains(s, `"remote"`) {
		t.Error("remote should be omitted when empty")
	}
	if strings.Contains(s, `"remote_url"`) {
		t.Error("remote_url should be omitted when empty")
	}
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/schema/... -run TestFlatpakApp_Remote -v`
Expected: FAIL — fields don't exist

- [ ] **Step 3: Add Remote and RemoteURL fields to FlatpakApp**

```go
// FlatpakApp is a Flatpak application detected on an ostree system.
type FlatpakApp struct {
	AppID     string `json:"app_id"`
	Origin    string `json:"origin"`
	Branch    string `json:"branch"`
	Include   bool   `json:"include"`
	Remote    string `json:"remote,omitempty"`
	RemoteURL string `json:"remote_url,omitempty"`
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/schema/... -run TestFlatpakApp_Remote -v`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add cmd/inspectah/internal/schema/types.go cmd/inspectah/internal/schema/types_test.go
git commit -m "feat(schema): add Remote and RemoteURL fields to FlatpakApp

Supports flatpak remote configuration capture via
'flatpak remotes --columns=name,url'.

Assisted-by: Claude Code"
```

---

### Task 4: Bump SchemaVersion to 13

**Files:**
- Modify: `cmd/inspectah/internal/schema/types.go:12`
- Modify: `cmd/inspectah/internal/schema/snapshot.go` (LoadSnapshot migration)
- Test: `cmd/inspectah/internal/schema/snapshot_test.go`

- [ ] **Step 1: Write the failing test for v12→v13 migration**

```go
// In snapshot_test.go
func TestLoadSnapshot_V12AcceptedAndMigrated(t *testing.T) {
	snap := NewSnapshot()
	snap.SchemaVersion = 12

	dir := t.TempDir()
	path := filepath.Join(dir, "snapshot.json")
	data, _ := json.Marshal(snap)
	os.WriteFile(path, data, 0644)

	loaded, err := LoadSnapshot(path)
	if err != nil {
		t.Fatalf("LoadSnapshot should accept v12: %v", err)
	}
	if loaded.SchemaVersion != SchemaVersion {
		t.Errorf("SchemaVersion = %d, want %d", loaded.SchemaVersion, SchemaVersion)
	}
}

func TestLoadSnapshot_V11Rejected(t *testing.T) {
	snap := NewSnapshot()
	snap.SchemaVersion = 11

	dir := t.TempDir()
	path := filepath.Join(dir, "snapshot.json")
	data, _ := json.Marshal(snap)
	os.WriteFile(path, data, 0644)

	_, err := LoadSnapshot(path)
	if err == nil {
		t.Fatal("LoadSnapshot should reject v11 when SchemaVersion is 13")
	}
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/schema/... -run "TestLoadSnapshot_V12AcceptedAndMigrated|TestLoadSnapshot_V11Rejected" -v`
Expected: FAIL — v12 is currently SchemaVersion (not SchemaVersion-1), v11 is currently accepted

- [ ] **Step 3: Bump SchemaVersion and update LoadSnapshot migration**

In `types.go` line 12, change:
```go
const SchemaVersion = 13
```

In `snapshot.go`, in the `LoadSnapshot` function, update the migration block. The existing v11→v12 migration for module streams should be kept, and a new v12→v13 no-op migration added:

```go
func LoadSnapshot(path string) (*InspectionSnapshot, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("failed to read snapshot: %w", err)
	}

	var snap InspectionSnapshot
	if err := json.Unmarshal(data, &snap); err != nil {
		return nil, fmt.Errorf("failed to parse snapshot JSON: %w", err)
	}

	if snap.SchemaVersion != SchemaVersion && snap.SchemaVersion != SchemaVersion-1 {
		return nil, fmt.Errorf("schema version mismatch: file has %d, expected %d or %d",
			snap.SchemaVersion, SchemaVersion-1, SchemaVersion)
	}

	// v12→v13 migration: new fields (QuadletUnit.Ports/Volumes/Generated,
	// FlatpakApp.Remote/RemoteURL, NonRpmItem.ReviewStatus/Notes) have
	// zero-value defaults that are correct for existing snapshots.
	if snap.SchemaVersion < SchemaVersion {
		snap.SchemaVersion = SchemaVersion
	}

	return &snap, nil
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/schema/... -run "TestLoadSnapshot_V12AcceptedAndMigrated|TestLoadSnapshot_V11Rejected" -v`
Expected: PASS

- [ ] **Step 5: Run all schema tests to verify no regressions**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/schema/... -v`
Expected: PASS (all tests)

- [ ] **Step 6: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add cmd/inspectah/internal/schema/types.go cmd/inspectah/internal/schema/snapshot.go cmd/inspectah/internal/schema/snapshot_test.go
git commit -m "feat(schema): bump SchemaVersion to 13

v12→v13 adds optional fields with correct zero-value defaults:
QuadletUnit.Ports/Volumes/Generated, FlatpakApp.Remote/RemoteURL,
NonRpmItem.ReviewStatus/Notes. No data migration needed.

Assisted-by: Claude Code"
```

---

## Phase 2: Inspector Fixes

### Task 5: Pip false-positive RPM ownership check

**Files:**
- Modify: `cmd/inspectah/internal/inspector/nonrpm.go:775-870`
- Test: `cmd/inspectah/internal/inspector/nonrpm_test.go`

- [ ] **Step 1: Write the failing test**

```go
// In nonrpm_test.go
func TestScanPip_SkipsRpmOwnedDistInfo(t *testing.T) {
	exec := NewFakeExecutor(map[string]ExecResult{
		// rpm -qf returns the owning package — this dist-info is RPM-managed
		"rpm -qf /usr/lib/python3.12/site-packages/dnf-4.18.0.dist-info": {
			ExitCode: 0,
			Stdout:   "python3-dnf-4.18.0-1.fc40.noarch",
		},
		// rpm -qf fails — this dist-info is pip-installed
		"rpm -qf /usr/lib/python3.12/site-packages/requests-2.31.0.dist-info": {
			ExitCode: 1,
			Stderr:   "file /usr/lib/python3.12/site-packages/requests-2.31.0.dist-info is not owned by any package",
		},
	}).WithDirs(map[string][]string{
		"/usr/lib":                          {"python3.12"},
		"/usr/lib/python3.12":               {"site-packages"},
		"/usr/lib/python3.12/site-packages": {"dnf-4.18.0.dist-info", "requests-2.31.0.dist-info"},
	}).WithFiles(map[string]string{
		"/usr/lib/python3.12/site-packages/dnf-4.18.0.dist-info/RECORD":      "",
		"/usr/lib/python3.12/site-packages/requests-2.31.0.dist-info/RECORD": "",
	})

	section := &schema.NonRpmSoftwareSection{}
	scanPip(exec, section, false)

	// Should only find requests, not dnf
	if len(section.Items) != 1 {
		t.Fatalf("got %d items, want 1 (only non-RPM pip packages)", len(section.Items))
	}
	if section.Items[0].Name != "requests" {
		t.Errorf("item name = %q, want %q", section.Items[0].Name, "requests")
	}
}

func TestScanPip_SkipsRpmCheckOnOstree(t *testing.T) {
	// On ostree systems, scanPip scans /usr/local/ paths only.
	// RPM check should not be invoked because ostree paths are
	// outside RPM's domain. This test verifies the scanner still
	// finds items in /usr/local/ without rpm -qf calls.
	exec := NewFakeExecutor(nil).
		WithDirs(map[string][]string{
			"/usr/local/lib":                          {"python3.12"},
			"/usr/local/lib/python3.12":               {"site-packages"},
			"/usr/local/lib/python3.12/site-packages": {"custom_lib-1.0.0.dist-info"},
		}).
		WithFiles(map[string]string{
			"/usr/local/lib/python3.12/site-packages/custom_lib-1.0.0.dist-info/RECORD": "",
		})

	section := &schema.NonRpmSoftwareSection{}
	scanPip(exec, section, true)

	if len(section.Items) != 1 {
		t.Fatalf("got %d items, want 1", len(section.Items))
	}
	if section.Items[0].Name != "custom_lib" {
		t.Errorf("item name = %q, want %q", section.Items[0].Name, "custom_lib")
	}
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/inspector/... -run "TestScanPip_SkipsRpm" -v`
Expected: FAIL — dnf will appear in items (no RPM filtering exists)

- [ ] **Step 3: Add RPM ownership check in scanPip**

In `nonrpm.go`, inside the `scanPip` function, after the `parseDistInfoName` call and before appending to `section.Items`, add the RPM ownership check. Find the block that starts with the dist-info directory iteration (around line 830-840 in current code) and add the check right before the `section.Items = append(...)` call:

```go
				// On package-mode systems, check if this dist-info dir
				// is owned by an RPM. If so, skip it — it's not a pip
				// finding, it's a system package.
				if !isOstree {
					distInfoFullPath := filepath.Join(spDir, sp.Name())
					rpmResult := exec.Run("rpm", "-qf", distInfoFullPath)
					if rpmResult.ExitCode == 0 {
						continue
					}
				}
```

Place this block immediately after the `hasCExt` detection loop and before the `relPath` assignment. The exact insertion point in the dist-info iteration loop is after:
```go
				hasCExt := false
				recordPath := filepath.Join(spDir, sp.Name(), "RECORD")
				// ... hasCExt detection ...
```

And before:
```go
				relPath := strings.TrimPrefix(
					filepath.Join(spDir, sp.Name()), "/",
				)
				section.Items = append(section.Items, schema.NonRpmItem{
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/inspector/... -run "TestScanPip_SkipsRpm" -v`
Expected: PASS

- [ ] **Step 5: Run all inspector tests**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/inspector/... -v`
Expected: PASS (all tests)

- [ ] **Step 6: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add cmd/inspectah/internal/inspector/nonrpm.go cmd/inspectah/internal/inspector/nonrpm_test.go
git commit -m "fix(inspector): filter RPM-owned pip dist-info directories

On package-mode systems, cross-reference each .dist-info directory
against 'rpm -qf' before reporting it as pip-installed software.
Eliminates 13+ false positives on typical Fedora systems (dnf,
setools, distro, etc.). Skipped on ostree systems where scanPip
only searches /usr/local/.

Assisted-by: Claude Code"
```

---

### Task 6: Flatpak --system flag and remote capture

**Files:**
- Modify: `cmd/inspectah/internal/inspector/container.go:472-508`
- Modify: `cmd/inspectah/internal/inspector/testdata/container/flatpak_list.txt`
- Create: `cmd/inspectah/internal/inspector/testdata/container/flatpak_remotes.txt`
- Test: `cmd/inspectah/internal/inspector/container_test.go`

- [ ] **Step 1: Create the flatpak_remotes.txt fixture**

```
flathub	https://dl.flathub.org/repo/
fedora	oci+https://registry.fedoraproject.org
```

Write this to `cmd/inspectah/internal/inspector/testdata/container/flatpak_remotes.txt`.

- [ ] **Step 2: Write the failing test**

```go
// In container_test.go
func TestDetectFlatpakApps_SystemFlagAndRemotes(t *testing.T) {
	flatpakOutput := loadContainerFixture(t, "flatpak_list.txt")
	remotesOutput := loadContainerFixture(t, "flatpak_remotes.txt")

	exec := NewFakeExecutor(map[string]ExecResult{
		"which flatpak": {ExitCode: 0, Stdout: "/usr/bin/flatpak"},
		"flatpak list --app --system --columns=application,origin,branch": {
			ExitCode: 0,
			Stdout:   flatpakOutput,
		},
		"flatpak remotes --system --columns=name,url": {
			ExitCode: 0,
			Stdout:   remotesOutput,
		},
	})

	apps := detectFlatpakApps(exec)

	if len(apps) != 3 {
		t.Fatalf("got %d apps, want 3", len(apps))
	}

	// Firefox should have flathub remote URL
	if apps[0].Remote != "flathub" {
		t.Errorf("apps[0].Remote = %q, want %q", apps[0].Remote, "flathub")
	}
	if apps[0].RemoteURL != "https://dl.flathub.org/repo/" {
		t.Errorf("apps[0].RemoteURL = %q, want %q", apps[0].RemoteURL, "https://dl.flathub.org/repo/")
	}

	// Calculator should have fedora remote URL
	if apps[1].Remote != "fedora" {
		t.Errorf("apps[1].Remote = %q, want %q", apps[1].Remote, "fedora")
	}
	if apps[1].RemoteURL != "oci+https://registry.fedoraproject.org" {
		t.Errorf("apps[1].RemoteURL = %q, want %q", apps[1].RemoteURL, "oci+https://registry.fedoraproject.org")
	}
}

func TestDetectFlatpakApps_RemotesFailGracefully(t *testing.T) {
	flatpakOutput := loadContainerFixture(t, "flatpak_list.txt")

	exec := NewFakeExecutor(map[string]ExecResult{
		"which flatpak": {ExitCode: 0, Stdout: "/usr/bin/flatpak"},
		"flatpak list --app --system --columns=application,origin,branch": {
			ExitCode: 0,
			Stdout:   flatpakOutput,
		},
		"flatpak remotes --system --columns=name,url": {
			ExitCode: 1,
			Stderr:   "error fetching remotes",
		},
	})

	apps := detectFlatpakApps(exec)

	if len(apps) != 3 {
		t.Fatalf("got %d apps, want 3 even when remotes fails", len(apps))
	}

	// Remote fields should be empty but apps still detected
	if apps[0].Remote != "" {
		t.Errorf("apps[0].Remote = %q, want empty when remotes fails", apps[0].Remote)
	}
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/inspector/... -run "TestDetectFlatpakApps_System|TestDetectFlatpakApps_RemotesFail" -v`
Expected: FAIL — current code uses `flatpak list --app --columns=...` (no `--system`)

- [ ] **Step 4: Update detectFlatpakApps to use --system and capture remotes**

Replace the `detectFlatpakApps` function in `container.go`:

```go
// detectFlatpakApps lists installed system-level Flatpak applications
// and resolves remote URLs for each origin.
func detectFlatpakApps(exec Executor) []schema.FlatpakApp {
	// Check if flatpak is installed.
	which := exec.Run("which", "flatpak")
	if which.ExitCode != 0 {
		return nil
	}

	result := exec.Run("flatpak", "list", "--app", "--system", "--columns=application,origin,branch")
	if result.ExitCode != 0 {
		return nil
	}

	// Build remote name → URL map (best-effort).
	remoteURLs := make(map[string]string)
	remotesResult := exec.Run("flatpak", "remotes", "--system", "--columns=name,url")
	if remotesResult.ExitCode == 0 {
		for _, line := range strings.Split(remotesResult.Stdout, "\n") {
			line = strings.TrimSpace(line)
			if line == "" {
				continue
			}
			parts := strings.SplitN(line, "\t", 2)
			if len(parts) == 2 {
				remoteURLs[strings.TrimSpace(parts[0])] = strings.TrimSpace(parts[1])
			}
		}
	}

	var apps []schema.FlatpakApp
	for _, line := range strings.Split(result.Stdout, "\n") {
		line = strings.TrimSpace(line)
		if line == "" {
			continue
		}

		parts := strings.Split(line, "\t")
		if len(parts) < 3 {
			continue
		}

		origin := strings.TrimSpace(parts[1])
		apps = append(apps, schema.FlatpakApp{
			AppID:     strings.TrimSpace(parts[0]),
			Origin:    origin,
			Branch:    strings.TrimSpace(parts[2]),
			Remote:    origin,
			RemoteURL: remoteURLs[origin],
		})
	}
	return apps
}
```

- [ ] **Step 5: Update existing TestDetectFlatpakApps to use --system flag**

The existing `TestDetectFlatpakApps` test uses the old command string `"flatpak list --app --columns=application,origin,branch"`. Update the fake executor key to match the new command:

```go
func TestDetectFlatpakApps(t *testing.T) {
	flatpakOutput := loadContainerFixture(t, "flatpak_list.txt")

	exec := NewFakeExecutor(map[string]ExecResult{
		"which flatpak": {ExitCode: 0, Stdout: "/usr/bin/flatpak"},
		"flatpak list --app --system --columns=application,origin,branch": {
			ExitCode: 0,
			Stdout:   flatpakOutput,
		},
	})

	apps := detectFlatpakApps(exec)

	if len(apps) != 3 {
		t.Fatalf("got %d apps, want 3", len(apps))
	}

	want := []schema.FlatpakApp{
		{AppID: "org.mozilla.firefox", Origin: "flathub", Branch: "stable", Remote: "flathub"},
		{AppID: "org.gnome.Calculator", Origin: "fedora", Branch: "stable", Remote: "fedora"},
		{AppID: "com.visualstudio.code", Origin: "flathub", Branch: "stable", Remote: "flathub"},
	}

	for i, app := range apps {
		if app.AppID != want[i].AppID {
			t.Errorf("app[%d].AppID = %q, want %q", i, app.AppID, want[i].AppID)
		}
		if app.Origin != want[i].Origin {
			t.Errorf("app[%d].Origin = %q, want %q", i, app.Origin, want[i].Origin)
		}
		if app.Branch != want[i].Branch {
			t.Errorf("app[%d].Branch = %q, want %q", i, app.Branch, want[i].Branch)
		}
		if app.Remote != want[i].Remote {
			t.Errorf("app[%d].Remote = %q, want %q", i, app.Remote, want[i].Remote)
		}
	}
}
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/inspector/... -run "TestDetectFlatpak" -v`
Expected: PASS (all flatpak tests)

- [ ] **Step 7: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add cmd/inspectah/internal/inspector/container.go cmd/inspectah/internal/inspector/container_test.go cmd/inspectah/internal/inspector/testdata/container/flatpak_remotes.txt
git commit -m "feat(inspector): add --system flag to flatpak detection and capture remotes

Only system-level flatpak installations are relevant for machine
state migration. Remote URLs are captured via 'flatpak remotes
--system --columns=name,url' for manifest generation.

Assisted-by: Claude Code"
```

---

### Task 7: Quadlet PublishPort and Volume parsing

**Files:**
- Modify: `cmd/inspectah/internal/inspector/container.go:116-155`
- Test: `cmd/inspectah/internal/inspector/container_test.go`

- [ ] **Step 1: Write the failing test**

```go
// In container_test.go
func TestScanQuadletDir_ParsesPortsAndVolumes(t *testing.T) {
	exec := NewFakeExecutor(nil).
		WithDirs(map[string][]string{
			"/etc/containers/systemd": {"web.container"},
		}).
		WithFiles(map[string]string{
			"/etc/containers/systemd/web.container": `[Container]
Image=registry.example.com/web:latest
PublishPort=8080:8080
PublishPort=443:443
Volume=data.volume:/data
Volume=/host/config:/etc/app:ro

[Service]
Restart=always
`,
		})

	units := scanQuadletDir(exec, "/etc/containers/systemd")

	if len(units) != 1 {
		t.Fatalf("got %d units, want 1", len(units))
	}

	u := units[0]
	if len(u.Ports) != 2 {
		t.Fatalf("Ports len = %d, want 2", len(u.Ports))
	}
	if u.Ports[0] != "8080:8080" {
		t.Errorf("Ports[0] = %q, want %q", u.Ports[0], "8080:8080")
	}
	if u.Ports[1] != "443:443" {
		t.Errorf("Ports[1] = %q, want %q", u.Ports[1], "443:443")
	}

	if len(u.Volumes) != 2 {
		t.Fatalf("Volumes len = %d, want 2", len(u.Volumes))
	}
	if u.Volumes[0] != "data.volume:/data" {
		t.Errorf("Volumes[0] = %q, want %q", u.Volumes[0], "data.volume:/data")
	}
	if u.Volumes[1] != "/host/config:/etc/app:ro" {
		t.Errorf("Volumes[1] = %q, want %q", u.Volumes[1], "/host/config:/etc/app:ro")
	}
}

func TestScanQuadletDir_NoPortsOrVolumes(t *testing.T) {
	exec := NewFakeExecutor(nil).
		WithDirs(map[string][]string{
			"/etc/containers/systemd": {"simple.container"},
		}).
		WithFiles(map[string]string{
			"/etc/containers/systemd/simple.container": `[Container]
Image=registry.example.com/simple:latest
`,
		})

	units := scanQuadletDir(exec, "/etc/containers/systemd")

	if len(units) != 1 {
		t.Fatalf("got %d units, want 1", len(units))
	}

	if len(units[0].Ports) != 0 {
		t.Errorf("Ports should be empty, got %v", units[0].Ports)
	}
	if len(units[0].Volumes) != 0 {
		t.Errorf("Volumes should be empty, got %v", units[0].Volumes)
	}
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/inspector/... -run "TestScanQuadletDir_ParsesPortsAndVolumes|TestScanQuadletDir_NoPortsOrVolumes" -v`
Expected: FAIL — Ports and Volumes are nil/empty because parsing doesn't exist

- [ ] **Step 3: Add extractQuadletPortsAndVolumes function and wire into scanQuadletDir**

Add a new function after `extractQuadletImage`:

```go
// extractQuadletPortsAndVolumes parses PublishPort= and Volume= directives
// from a .container quadlet file.
func extractQuadletPortsAndVolumes(content string) (ports, volumes []string) {
	for _, line := range strings.Split(content, "\n") {
		trimmed := strings.TrimSpace(line)
		lower := strings.ToLower(trimmed)
		if strings.HasPrefix(lower, "publishport") && strings.Contains(trimmed, "=") {
			val := strings.TrimSpace(trimmed[strings.Index(trimmed, "=")+1:])
			if val != "" {
				ports = append(ports, val)
			}
		} else if strings.HasPrefix(lower, "volume") && !strings.HasPrefix(lower, "volumedriver") && strings.Contains(trimmed, "=") {
			val := strings.TrimSpace(trimmed[strings.Index(trimmed, "=")+1:])
			if val != "" {
				volumes = append(volumes, val)
			}
		}
	}
	return
}
```

Then update `scanQuadletDir` to call it. In the loop where units are built, after the `imageRef` extraction, add:

```go
		var ports, volumes []string
		if ext == ".container" {
			imageRef = extractQuadletImage(content)
			ports, volumes = extractQuadletPortsAndVolumes(content)
		}

		// Store path relative to host root.
		relPath := strings.TrimPrefix(path, exec.HostRoot())
		relPath = strings.TrimPrefix(relPath, "/")

		units = append(units, schema.QuadletUnit{
			Path:    relPath,
			Name:    entry.Name(),
			Content: content,
			Image:   imageRef,
			Ports:   ports,
			Volumes: volumes,
		})
```

Remove the existing standalone `if ext == ".container" { imageRef = extractQuadletImage(content) }` block since it's now merged into the combined extraction above.

- [ ] **Step 4: Run test to verify it passes**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/inspector/... -run "TestScanQuadletDir_ParsesPortsAndVolumes|TestScanQuadletDir_NoPortsOrVolumes" -v`
Expected: PASS

- [ ] **Step 5: Run all existing quadlet tests to verify no regressions**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/inspector/... -run "TestScanQuadlet" -v`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add cmd/inspectah/internal/inspector/container.go cmd/inspectah/internal/inspector/container_test.go
git commit -m "feat(inspector): extract PublishPort and Volume from quadlet .container files

Parses PublishPort= and Volume= directives into structured fields
for richer triage display. Non-.container quadlet units (volumes,
networks) are unaffected.

Assisted-by: Claude Code"
```

---

## Phase 3: Triage Classifier

### Task 8: Extract non-RPM items into new Section="nonrpm"

**Files:**
- Modify: `cmd/inspectah/internal/renderer/triage.go:62-75` (classifyAll)
- Modify: `cmd/inspectah/internal/renderer/triage.go:589-660` (classifyContainerItems)
- Test: `cmd/inspectah/internal/renderer/triage_test.go`

- [ ] **Step 1: Write the failing test**

```go
// In triage_test.go
func TestClassifyNonRpmItems_NewSection(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.NonRpmSoftware = &schema.NonRpmSoftwareSection{
		Items: []schema.NonRpmItem{
			{
				Path:       "opt/agent/bin/agent",
				Name:       "agent",
				Method:     "standalone binary",
				Confidence: "high",
				Lang:       "go",
				Static:     true,
			},
			{
				Path:       "usr/local/lib/python3.12/site-packages/requests-2.31.0.dist-info",
				Name:       "requests",
				Method:     "pip dist-info",
				Confidence: "high",
				Version:    "2.31.0",
			},
		},
	}

	items := classifyNonRpmItems(snap, make(map[string]bool), false)

	for _, item := range items {
		if item.Section != "nonrpm" {
			t.Errorf("item %q has Section=%q, want %q", item.Key, item.Section, "nonrpm")
		}
	}

	if len(items) != 2 {
		t.Fatalf("got %d items, want 2", len(items))
	}
}

func TestClassifyContainerItems_NoLongerIncludesNonRpm(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Containers = &schema.ContainerSection{
		QuadletUnits: []schema.QuadletUnit{
			{Name: "webapp.container", Image: "webapp:latest", Include: true},
		},
	}
	snap.NonRpmSoftware = &schema.NonRpmSoftwareSection{
		Items: []schema.NonRpmItem{
			{Path: "opt/agent/bin/agent", Name: "agent", Method: "standalone binary", Confidence: "high"},
		},
	}

	items := classifyContainerItems(snap, make(map[string]bool), false)

	for _, item := range items {
		if item.Section == "nonrpm" {
			t.Error("classifyContainerItems should not produce nonrpm-section items")
		}
		if strings.HasPrefix(item.Key, "nonrpm-") {
			t.Errorf("classifyContainerItems should not produce nonrpm keys, got %q", item.Key)
		}
	}
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/renderer/... -run "TestClassifyNonRpmItems_NewSection|TestClassifyContainerItems_NoLongerIncludesNonRpm" -v`
Expected: FAIL — `classifyNonRpmItems` doesn't exist, non-RPM items are still in `classifyContainerItems`

- [ ] **Step 3: Create classifyNonRpmItems and update classifyAll**

Add a new function in `triage.go` (after `classifyContainerItems`):

```go
// classifyNonRpmItems classifies non-RPM software items into the
// dedicated "nonrpm" section. Previously these were bundled into
// the "containers" section.
func classifyNonRpmItems(snap *schema.InspectionSnapshot, secrets map[string]bool, isFleet bool) []TriageItem {
	var items []TriageItem
	if snap.NonRpmSoftware == nil {
		return items
	}

	for _, nri := range snap.NonRpmSoftware.Items {
		if secrets[nri.Path] {
			continue
		}
		name := nri.Path
		if name == "" {
			name = nri.Name
		}
		// Non-RPM items use review-status cards, not toggle or notification
		// cards. CardType stays empty; the SPA dispatches to
		// buildReviewStatusCard based on sectionId === 'nonrpm'.
		// DefaultInclude is not set — non-RPM items do not participate
		// in the include/exclude system; output is gated on review_status.
		item := TriageItem{
			Section: "nonrpm",
			Key:     "nonrpm-" + name,
			Tier:    3,
			Reason:  "Non-RPM software — requires manual review for image-mode migration.",
			Name:    name,
			Meta:    nri.Method,
		}
		items = append(items, item)
	}
	return items
}
```

Then remove the non-RPM block from `classifyContainerItems`. In that function, delete the entire block that starts with `if snap.NonRpmSoftware != nil {` and its contents (the `for _, nri := range snap.NonRpmSoftware.Items` loop).

Finally, update `classifyAll` to call the new function. Add after `classifyContainerItems`:

```go
func classifyAll(snap *schema.InspectionSnapshot, isFleet bool) []TriageItem {
	secretPaths := buildSecretPathSet(snap)

	var items []TriageItem
	items = append(items, classifyPackages(snap, secretPaths, isFleet)...)
	items = append(items, classifyConfigFiles(snap, secretPaths, isFleet)...)
	items = append(items, classifyRuntime(snap, secretPaths, isFleet)...)
	items = append(items, classifyContainerItems(snap, secretPaths, isFleet)...)
	items = append(items, classifyNonRpmItems(snap, secretPaths, isFleet)...)
	items = append(items, classifyIdentity(snap, secretPaths, isFleet)...)
	items = append(items, classifySystemItems(snap, secretPaths, isFleet)...)
	items = append(items, classifySecretItems(snap, secretPaths)...)
	items = append(items, classifyVersionChanges(snap, isFleet)...)
	return items
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/renderer/... -run "TestClassifyNonRpmItems_NewSection|TestClassifyContainerItems_NoLongerIncludesNonRpm" -v`
Expected: PASS

- [ ] **Step 5: Run all triage tests to check for regressions**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/renderer/... -v`
Expected: PASS (some existing tests that check non-RPM items in the "containers" section may need updating — fix them in the next step)

- [ ] **Step 6: Fix any broken tests that expect non-RPM items in Section="containers"**

The existing `TestClassifyContainerItems_SingleMachine_Grouping` test expects a non-RPM item with key `"nonrpm-/opt/agent/bin/agent"` in the output of `classifyContainerItems`. Update it to test via `classifyNonRpmItems` instead or remove the non-RPM assertion from that test:

In the test, change the assertion for the agent item from looking in `classifyContainerItems` output to looking in `classifyNonRpmItems` output. Split the test if needed. The quadlet and running container assertions stay on `classifyContainerItems`.

- [ ] **Step 7: Run all tests again**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/renderer/... -v`
Expected: PASS (all tests)

- [ ] **Step 8: Add nonrpm to MIGRATION_SECTIONS (SPA section plumbing)**

The classifier (Steps 3-7) now routes non-RPM items to `section="nonrpm"`. The SPA must be able to render that section before any items route there. This step and the following steps add all SPA plumbing atomically in the same commit as the classifier change.

In `report.html`, find the MIGRATION_SECTIONS array and add the nonrpm entry after containers:

```javascript
var MIGRATION_SECTIONS = [
  {id: 'overview',   label: 'Overview',       tracked: false},
  {id: 'packages',   label: 'Packages',       tracked: true},
  {id: 'config',     label: 'Configuration',  tracked: true},
  {id: 'runtime',    label: 'Runtime',         tracked: true},
  {id: 'containers', label: 'Containers',      tracked: true},
  {id: 'nonrpm',    label: 'Non-RPM Software', tracked: false, countBadge: true},
  {id: 'identity',   label: 'Identity',        tracked: true},
  {id: 'system',     label: 'System & Security', tracked: true},
  {id: 'secrets',    label: 'Secrets',         tracked: true},
  {id: 'version-changes', label: 'Version Changes', tracked: false, infoBadge: true},
  {id: 'editor',    label: 'Edit Files',      tracked: false}
];
```

`tracked: false` — non-RPM does NOT participate in the progress bar or sidebar completion dots. It is a planning worksheet, not a decision checklist. `countBadge: true` tells the sidebar renderer to show a neutral count badge (not_reviewed count) instead of a review dot.

- [ ] **Step 9: Add countBadge rendering to renderSidebar**

In `renderSidebar`, find the block `if (section.tracked) {` that creates the review dot. After the closing `}` for that block, add:

```javascript
    // Non-RPM countBadge: shows unreviewed item count as neutral indicator
    if (section.countBadge) {
      var badge = document.createElement('span');
      badge.className = 'nav-badge';
      badge.id = 'nonrpm-badge';
      badge.style.cssText = 'font-size:11px;padding:1px 6px;border-radius:8px;background:var(--pf-t--global--background--color--secondary-default);margin-left:auto;';
      a.appendChild(badge);
    }
```

- [ ] **Step 10: Add empty-state rendering for nonrpm in renderTriageSection**

In `renderTriageSection`, find the block that checks whether the section has any items to render (after the manifest items are filtered for the section). Add a nonrpm-specific empty-state handler:

```javascript
    // Non-RPM empty state
    if (sectionId === 'nonrpm') {
      var snap = App.snapshot;
      if (!snap.non_rpm_software || !snap.non_rpm_software.items || snap.non_rpm_software.items.length === 0) {
        var emptyMsg = document.createElement('div');
        emptyMsg.style.cssText = 'padding:24px;text-align:center;opacity:0.6;font-size:14px;';
        // Distinguish "no items" from "scanning not performed"
        if (snap.non_rpm_software) {
          emptyMsg.textContent = 'No non-RPM software detected.';
        } else {
          emptyMsg.textContent = 'Non-RPM scanning was not performed. Re-run inspectah to detect non-package software.';
        }
        container.appendChild(emptyMsg);
        return;
      }
    }
```

Insert this at the top of the section rendering path, before the tier/group rendering logic.

- [ ] **Step 11: Wire renderAllSections() to render countBadge sections**

The live `renderAllSections()` only calls `renderTriageSection()` for `section.tracked` entries. Non-RPM is `tracked: false, countBadge: true`, so it would be skipped — items would strand off-nav. Fix: extend the filter to include `countBadge` sections.

In `renderAllSections()`, find the block:
```javascript
  MIGRATION_SECTIONS.forEach(function(section) {
    if (!section.tracked) return;
    renderTriageSection(section.id);
  });
```

Change to:
```javascript
  MIGRATION_SECTIONS.forEach(function(section) {
    if (!section.tracked && !section.countBadge) return;
    renderTriageSection(section.id);
  });
```

- [ ] **Step 12: Wire updateAllBadges() to initialize the nonrpm sidebar badge**

The live `updateAllBadges()` only calls `updateBadge()` for `section.tracked` entries. Non-RPM uses a separate badge handler (`updateNonRpmSidebarBadge()`). Wire it in.

In `updateAllBadges()`, find:
```javascript
function updateAllBadges() {
  MIGRATION_SECTIONS.forEach(function(s) {
    if (s.tracked) updateBadge(s.id);
  });
}
```

Change to:
```javascript
function updateAllBadges() {
  MIGRATION_SECTIONS.forEach(function(s) {
    if (s.tracked) updateBadge(s.id);
    if (s.countBadge) updateNonRpmSidebarBadge();
  });
}
```

- [ ] **Step 13: Wire navigateTo() to lazy-render uninitialized countBadge sections**

The live `navigateTo()` does not lazy-render uninitialized sections. If a user clicks the Non-RPM sidebar entry before `renderAllSections()` runs (e.g., deep link), the section container is empty. Add a guard.

In `navigateTo()`, after the section visibility toggle, add:
```javascript
  // Lazy-render countBadge sections that may not have been initialized
  var sectionDiv = document.getElementById('section-' + id);
  if (sectionDiv && sectionDiv.children.length === 0) {
    var sectionMeta = null;
    for (var ms = 0; ms < MIGRATION_SECTIONS.length; ms++) {
      if (MIGRATION_SECTIONS[ms].id === id) {
        sectionMeta = MIGRATION_SECTIONS[ms];
        break;
      }
    }
    if (sectionMeta && (sectionMeta.tracked || sectionMeta.countBadge)) {
      renderTriageSection(id);
    }
  }
```

- [ ] **Step 14: Use approved heading format for nonrpm section**

The approved heading is `Non-RPM Software (X of Y reviewed)`. In `renderTriageSection`, after the heading is created and the label is set, add a nonrpm-specific heading override:

```javascript
  // Non-RPM section heading: "Non-RPM Software (X of Y reviewed)"
  if (sectionId === 'nonrpm') {
    var nrSnap = App.snapshot;
    var totalNr = 0;
    var reviewedNr = 0;
    if (nrSnap && nrSnap.non_rpm_software && nrSnap.non_rpm_software.items) {
      totalNr = nrSnap.non_rpm_software.items.length;
      for (var nri = 0; nri < nrSnap.non_rpm_software.items.length; nri++) {
        var nrStatus = nrSnap.non_rpm_software.items[nri].review_status;
        if (nrStatus === 'reviewed' || nrStatus === 'migration_planned') {
          reviewedNr++;
        }
      }
    }
    heading.textContent = label + ' (' + reviewedNr + ' of ' + totalNr + ' reviewed)';
  }
```

Insert this after `heading.textContent = label;`, replacing the label for the nonrpm section only.

- [ ] **Step 15: Suppress section footer for nonrpm section**

The live section footer uses generic checklist semantics (`X / Y decided` + `Mark section reviewed`). Non-RPM is a planning worksheet, not a decision checklist — the footer should be suppressed.

In `renderTriageSection`, find the footer rendering block:
```javascript
  // Section footer
  var footer = document.createElement('div');
  footer.className = 'section-footer';
```

Wrap the entire footer block (through the `container.appendChild(footer)` line) in a condition:
```javascript
  // Section footer — suppress for nonrpm (planning worksheet, not decision checklist)
  if (sectionId !== 'nonrpm') {
    var footer = document.createElement('div');
    footer.className = 'section-footer';
    // ... existing footer code ...
    container.appendChild(footer);
  }
```

- [ ] **Step 16: Call updateNonRpmSidebarBadge on initial render and snapshot load**

The live code only calls `updateNonRpmSidebarBadge()` on status change (Task 14). The sidebar count would start empty even when Non-RPM items exist. Fix: call it at the end of `renderAllSections()` and in `applyRebuildResponse()`.

In `renderAllSections()`, after the call to `updateAllBadges()` (or at the end of the function if `updateAllBadges` is not called there), add:
```javascript
  updateNonRpmSidebarBadge();
```

In `applyRebuildResponse()`, after the `renderAllSections()` call, add:
```javascript
  updateNonRpmSidebarBadge();
```

In `enableRefineMode()`, in the `xhr.onload` callback after `renderAllSections()`, add:
```javascript
  updateNonRpmSidebarBadge();
```

- [ ] **Step 17: Write the HTML test for nonrpm section**

```go
// In html_test.go
func TestRenderHTML_NonRpmSection(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.NonRpmSoftware = &schema.NonRpmSoftwareSection{
		Items: []schema.NonRpmItem{
			{Path: "opt/agent/bin/agent", Name: "agent", Method: "standalone binary", Confidence: "high"},
		},
	}

	containerfile := "FROM rhel-bootc:9.4\n"
	html := goldenTestHelper(t, snap, containerfile)

	if !strings.Contains(html, "Non-RPM Software") {
		t.Error("HTML should contain Non-RPM Software section label")
	}
	if !strings.Contains(html, `id: 'nonrpm'`) || !strings.Contains(html, `label: 'Non-RPM Software'`) {
		t.Error("MIGRATION_SECTIONS should include nonrpm entry")
	}
	// Non-RPM is not tracked (no progress dot)
	if !strings.Contains(html, `tracked: false, countBadge: true`) {
		t.Error("nonrpm entry should have tracked: false and countBadge: true")
	}
}

func TestRenderHTML_NonRpmEmptyState(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.NonRpmSoftware = &schema.NonRpmSoftwareSection{
		Items: []schema.NonRpmItem{}, // empty after filtering
	}

	containerfile := "FROM rhel-bootc:9.4\n"
	html := goldenTestHelper(t, snap, containerfile)

	if !strings.Contains(html, "No non-RPM software detected") {
		t.Error("HTML should contain empty state message for no items")
	}
}
```

- [ ] **Step 18: Run tests to verify**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/renderer/... -run "TestClassifyNonRpmItems_NewSection|TestClassifyContainerItems_NoLongerIncludesNonRpm|TestRenderHTML_NonRpm" -v`
Expected: PASS

- [ ] **Step 19: Run all triage and renderer tests for regressions**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/renderer/... -v`
Expected: PASS

- [ ] **Step 20: Commit (atomic: classifier + SPA plumbing)**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add cmd/inspectah/internal/renderer/triage.go cmd/inspectah/internal/renderer/triage_test.go cmd/inspectah/internal/renderer/static/report.html cmd/inspectah/internal/renderer/html_test.go
git commit -m "feat(triage+spa): extract non-RPM section with full SPA plumbing

Non-RPM software items classify into Section='nonrpm' instead of
being bundled with container items. SPA section plumbing lands
atomically: MIGRATION_SECTIONS entry (tracked: false, countBadge:
true), renderAllSections/updateAllBadges/navigateTo countBadge
extensions, approved heading format, footer suppression, sidebar
badge initialization, and empty-state messages.

Assisted-by: Claude Code"
```

---

### Task 9: Add flatpak classifier + SPA include handlers

**Files:**
- Modify: `cmd/inspectah/internal/renderer/triage.go:589-630` (classifyContainerItems)
- Modify: `cmd/inspectah/internal/renderer/triage.go` (NormalizeIncludeDefaults)
- Modify: `cmd/inspectah/internal/renderer/static/report.html` (getSnapshotInclude, updateSnapshotInclude)
- Test: `cmd/inspectah/internal/renderer/triage_test.go`

> **Blocker 2 fix:** The SPA flatpak include read/write handlers land in this task
> alongside the classifier, so flatpak triage items have a working toggle path from
> the moment they appear. Without this, toggling a flatpak item in the SPA would
> silently do nothing.

- [ ] **Step 1: Write the failing test**

```go
// In triage_test.go
func TestClassifyContainerItems_FlatpakApps(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Containers = &schema.ContainerSection{
		FlatpakApps: []schema.FlatpakApp{
			{AppID: "org.mozilla.firefox", Origin: "flathub", Branch: "stable", Include: true},
			{AppID: "org.gnome.Calculator", Origin: "fedora", Branch: "stable", Include: true},
		},
	}

	items := classifyContainerItems(snap, make(map[string]bool), false)

	var flatpakItems []TriageItem
	for _, item := range items {
		if strings.HasPrefix(item.Key, "flatpak-") {
			flatpakItems = append(flatpakItems, item)
		}
	}

	if len(flatpakItems) != 2 {
		t.Fatalf("got %d flatpak items, want 2", len(flatpakItems))
	}

	ff := flatpakItems[0]
	if ff.Section != "containers" {
		t.Errorf("Section = %q, want %q", ff.Section, "containers")
	}
	if ff.Group != "sub:flatpak" {
		t.Errorf("Group = %q, want %q", ff.Group, "sub:flatpak")
	}
	if ff.Tier != 2 {
		t.Errorf("Tier = %d, want 2", ff.Tier)
	}
	if ff.Key != "flatpak-org.mozilla.firefox" {
		t.Errorf("Key = %q, want %q", ff.Key, "flatpak-org.mozilla.firefox")
	}
}

func TestClassifyContainerItems_FlatpakApps_Fleet(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Containers = &schema.ContainerSection{
		FlatpakApps: []schema.FlatpakApp{
			{AppID: "org.mozilla.firefox", Origin: "flathub", Branch: "stable", Include: true},
		},
	}

	items := classifyContainerItems(snap, make(map[string]bool), true)

	var flatpakItems []TriageItem
	for _, item := range items {
		if strings.HasPrefix(item.Key, "flatpak-") {
			flatpakItems = append(flatpakItems, item)
		}
	}

	if len(flatpakItems) != 1 {
		t.Fatalf("got %d flatpak items, want 1", len(flatpakItems))
	}

	// Fleet mode: no sub-group
	if flatpakItems[0].Group != "" {
		t.Errorf("Group should be empty in fleet mode, got %q", flatpakItems[0].Group)
	}
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/renderer/... -run "TestClassifyContainerItems_FlatpakApps" -v`
Expected: FAIL — no flatpak classification exists

- [ ] **Step 3: Add flatpak classification to classifyContainerItems**

In `classifyContainerItems`, after the running containers loop and before the function returns, add:

```go
		// Flatpak apps
		for _, app := range snap.Containers.FlatpakApps {
			group := ""
			if !isFleet {
				group = "sub:flatpak"
			}
			items = append(items, TriageItem{
				Section:        "containers",
				Key:            "flatpak-" + app.AppID,
				Tier:           2,
				Reason:         "Flatpak application — installed on first boot, not baked into image.",
				Name:           app.AppID,
				Meta:           app.Origin + "/" + app.Branch,
				Group:          group,
				DefaultInclude: app.Include,
			})
		}
```

- [ ] **Step 4: Add flatpak to NormalizeIncludeDefaults**

In the `NormalizeIncludeDefaults` function, after the `// Quadlet units` block that sets `snap.Containers.QuadletUnits[i].Include = true`, add:

```go
	// Flatpak apps
	if snap.Containers != nil {
		for i := range snap.Containers.FlatpakApps {
			snap.Containers.FlatpakApps[i].Include = true
		}
	}
```

Note: the existing `// Quadlet units` block already has `if snap.Containers != nil {` — merge the flatpak loop into the same `if` block:

```go
	// Quadlet units and flatpak apps
	if snap.Containers != nil {
		for i := range snap.Containers.QuadletUnits {
			snap.Containers.QuadletUnits[i].Include = true
		}
		for i := range snap.Containers.FlatpakApps {
			snap.Containers.FlatpakApps[i].Include = true
		}
	}
```

- [ ] **Step 5: Add flatpak handler to getSnapshotInclude in report.html**

In `getSnapshotInclude`, find the `container-` handler block (around line 4100). After the closing `}` for the container handler, add a `flatpak-` handler:

```javascript
  } else if (key.indexOf('flatpak-') === 0) {
    var fpId = key.substring(8);
    if (snap.containers && snap.containers.flatpak_apps) {
      for (var fi = 0; fi < snap.containers.flatpak_apps.length; fi++) {
        if (snap.containers.flatpak_apps[fi].app_id === fpId) {
          return snap.containers.flatpak_apps[fi].include !== false;
        }
      }
    }
```

- [ ] **Step 6: Add flatpak handler to updateSnapshotInclude in report.html**

In `updateSnapshotInclude`, find the `container-` handler block. After the closing `}` for the container handler, add:

```javascript
  } else if (key.indexOf('flatpak-') === 0) {
    var fpId = key.substring(8);
    if (snap.containers && snap.containers.flatpak_apps) {
      for (var fi = 0; fi < snap.containers.flatpak_apps.length; fi++) {
        if (snap.containers.flatpak_apps[fi].app_id === fpId) {
          snap.containers.flatpak_apps[fi].include = include;
          return;
        }
      }
    }
```

- [ ] **Step 7: Run test to verify it passes**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/renderer/... -run "TestClassifyContainerItems_FlatpakApps" -v`
Expected: PASS

- [ ] **Step 8: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add cmd/inspectah/internal/renderer/triage.go cmd/inspectah/internal/renderer/triage_test.go cmd/inspectah/internal/renderer/static/report.html
git commit -m "feat(triage): add flatpak app classification with SPA include handlers

Flatpak apps are now first-class triage items with Section=containers,
Group=sub:flatpak, Tier 2 with toggle switches. Include defaults
set in NormalizeIncludeDefaults. SPA getSnapshotInclude and
updateSnapshotInclude handle flatpak- keys so toggles work
immediately when flatpak items appear.

Assisted-by: Claude Code"
```

---

### Task 10: Add compose file classifier

**Files:**
- Modify: `cmd/inspectah/internal/renderer/triage.go` (classifyContainerItems)
- Test: `cmd/inspectah/internal/renderer/triage_test.go`

- [ ] **Step 1: Write the failing test**

```go
// In triage_test.go
func TestClassifyContainerItems_ComposeFiles(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Containers = &schema.ContainerSection{
		ComposeFiles: []schema.ComposeFile{
			{
				Path: "opt/myapp/docker-compose.yml",
				Images: []schema.ComposeService{
					{Service: "web", Image: "nginx:latest"},
					{Service: "db", Image: "postgres:15"},
				},
				Include: true,
			},
		},
	}

	items := classifyContainerItems(snap, make(map[string]bool), false)

	var composeItems []TriageItem
	for _, item := range items {
		if strings.HasPrefix(item.Key, "compose-") {
			composeItems = append(composeItems, item)
		}
	}

	if len(composeItems) != 1 {
		t.Fatalf("got %d compose items, want 1", len(composeItems))
	}

	ci := composeItems[0]
	if ci.Section != "containers" {
		t.Errorf("Section = %q, want %q", ci.Section, "containers")
	}
	if ci.Group != "sub:compose" {
		t.Errorf("Group = %q, want %q", ci.Group, "sub:compose")
	}
	if ci.CardType != "compose-info" {
		t.Errorf("CardType = %q, want %q", ci.CardType, "compose-info")
	}
	if ci.DisplayOnly {
		t.Error("compose items should NOT be DisplayOnly — they use card_type routing instead to escape the informational tier")
	}
	if ci.Tier != 2 {
		t.Errorf("Tier = %d, want 2", ci.Tier)
	}
	if ci.Meta != "2 services: web, db" {
		t.Errorf("Meta = %q, want %q", ci.Meta, "2 services: web, db")
	}
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/renderer/... -run "TestClassifyContainerItems_ComposeFiles" -v`
Expected: FAIL — no compose classification exists

- [ ] **Step 3: Add compose file classification to classifyContainerItems**

In `classifyContainerItems`, after the flatpak apps loop, add:

```go
		// Compose files — use CardType "compose-info" instead of DisplayOnly.
		// DisplayOnly: true would route items into the informational tier
		// (rendered after the main grouped/ungrouped loop), stranding them
		// outside the four-subsection hierarchy. CardType: "compose-info"
		// keeps items in the main loop where the container subsection
		// ordering (Task 18) can place them as the fourth peer subsection.
		for _, cf := range snap.Containers.ComposeFiles {
			group := ""
			if !isFleet {
				group = "sub:compose"
			}

			// Build service inventory meta string
			var serviceNames []string
			for _, svc := range cf.Images {
				serviceNames = append(serviceNames, svc.Service)
			}
			meta := fmt.Sprintf("%d services: %s", len(serviceNames), strings.Join(serviceNames, ", "))

			items = append(items, TriageItem{
				Section:        "containers",
				Key:            "compose-" + cf.Path,
				Tier:           2,
				Reason:         "Compose file — cannot be safely auto-migrated. Review services and consider converting to Quadlet units.",
				Name:           cf.Path,
				Meta:           meta,
				Group:          group,
				CardType:       "compose-info",
				DefaultInclude: cf.Include,
			})
		}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/renderer/... -run "TestClassifyContainerItems_ComposeFiles" -v`
Expected: PASS

- [ ] **Step 5: Run all triage tests**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/renderer/... -v`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add cmd/inspectah/internal/renderer/triage.go cmd/inspectah/internal/renderer/triage_test.go
git commit -m "feat(triage): add compose file classification as informational items

Compose files get display-only triage items in the containers
section with a service inventory summary. Group=sub:compose
for visual grouping on single-machine snapshots.

Assisted-by: Claude Code"
```

---

## Phase 4: Renderer

### Task 11: Non-RPM Containerfile stubs — data-driven output

**Files:**
- Modify: `cmd/inspectah/internal/renderer/containerfile.go:719-795` (nonRpmSectionLines)
- Test: `cmd/inspectah/internal/renderer/containerfile_test.go`

- [ ] **Step 1: Write the failing test**

```go
// In containerfile_test.go
func TestNonRpmSectionLines_MigrationPlannedStubs(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.NonRpmSoftware = &schema.NonRpmSoftwareSection{
		Items: []schema.NonRpmItem{
			{
				// Non-RPM items do NOT use the Include field for output gating.
				// Output is controlled solely by review_status.
				Path:         "opt/deploy.sh",
				Name:         "deploy.sh",
				Method:       "standalone binary",
				ReviewStatus: "migration_planned",
				Notes:        "Ship as-is",
				Lang:         "shell",
				Static:       false,
			},
			{
				Path:         "usr/local/bin/myapp",
				Name:         "myapp",
				Method:       "go binary",
				ReviewStatus: "migration_planned",
				Lang:         "go",
				Static:       true,
			},
			{
				Path:         "usr/local/bin/cbridge",
				Name:         "cbridge",
				Method:       "standalone binary",
				ReviewStatus: "migration_planned",
				Lang:         "c",
				Static:       false,
				SharedLibs:   []string{"libc.so.6", "libssl.so"},
			},
			{
				Path:         "opt/app/venv",
				Name:         "custom_lib",
				Method:       "pip dist-info",
				ReviewStatus: "migration_planned",
				Version:      "1.0.0",
				HasCExtensions: false,
			},
			{
				Path:         "opt/app2/venv",
				Name:         "numpy",
				Method:       "pip dist-info",
				ReviewStatus: "migration_planned",
				Version:      "1.24.0",
				HasCExtensions: true,
			},
			{
				Path:         "opt/agent/bin/agent",
				Name:         "agent",
				Method:       "standalone binary",
				ReviewStatus: "reviewed",
			},
			{
				Path:         "opt/other",
				Name:         "other",
				Method:       "standalone binary",
				ReviewStatus: "not_reviewed",
			},
		},
	}

	lines := nonRpmSectionLines(snap, nil, false)
	content := strings.Join(lines, "\n")

	// Shell script should get a COPY stub
	if !strings.Contains(content, "# COPY opt/deploy.sh /usr/local/bin/") {
		t.Error("shell script should produce COPY stub")
	}

	// Static Go binary should get a COPY stub
	if !strings.Contains(content, "# COPY usr/local/bin/myapp /usr/local/bin/") {
		t.Error("static Go binary should produce COPY stub")
	}

	// Dynamic C binary should get comment-only with shared libs
	if !strings.Contains(content, "libc.so.6") {
		t.Error("dynamic binary should list shared libs")
	}
	if strings.Contains(content, "COPY usr/local/bin/cbridge") {
		t.Error("dynamic binary should NOT produce COPY stub")
	}

	// Pure pip should get requirements stub
	if !strings.Contains(content, "pip install") {
		t.Error("pure pip should produce pip install stub")
	}

	// C-extension pip should get comment-only
	if !strings.Contains(content, "numpy") && !strings.Contains(content, "native extensions") {
		t.Error("c-extension pip should produce warning comment")
	}

	// "reviewed" and "not_reviewed" items should NOT produce output
	if strings.Contains(content, "agent") {
		t.Error("reviewed items should not appear in output")
	}
	if strings.Contains(content, "other") {
		t.Error("not_reviewed items should not appear in output")
	}
}

func TestNonRpmSectionLines_NoMigrationPlannedItems(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.NonRpmSoftware = &schema.NonRpmSoftwareSection{
		Items: []schema.NonRpmItem{
			{
				Path:         "opt/tool",
				Name:         "tool",
				Method:       "standalone binary",
				ReviewStatus: "reviewed",
			},
		},
	}

	lines := nonRpmSectionLines(snap, nil, false)
	if len(lines) != 0 {
		t.Errorf("should produce no output for reviewed-only items, got %d lines", len(lines))
	}
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/renderer/... -run "TestNonRpmSectionLines_MigrationPlanned" -v`
Expected: FAIL — current function doesn't check ReviewStatus

- [ ] **Step 3: Rewrite nonRpmSectionLines for data-driven stubs**

Replace the `nonRpmSectionLines` function:

```go
func nonRpmSectionLines(snap *schema.InspectionSnapshot, purePip []schema.NonRpmItem, needsMultistage bool) []string {
	var lines []string
	if snap.NonRpmSoftware == nil || len(snap.NonRpmSoftware.Items) == 0 {
		return lines
	}

	// Collect items marked for migration. The gate is review_status
	// alone — non-RPM items do not use the Include toggle system.
	// Items with "reviewed" or "not_reviewed" status produce no output.
	var migrationItems []schema.NonRpmItem
	for _, item := range snap.NonRpmSoftware.Items {
		if item.ReviewStatus == "migration_planned" {
			migrationItems = append(migrationItems, item)
		}
	}
	if len(migrationItems) == 0 {
		return lines
	}

	lines = append(lines, "# === Non-RPM Software (migration planned) ===")

	for _, item := range migrationItems {
		note := ""
		if item.Notes != "" {
			note = " — " + item.Notes
		}

		switch {
		case item.Method == "pip dist-info" && item.HasCExtensions:
			// C-extension pip: comment-only warning
			lines = append(lines, fmt.Sprintf("# %s==%s — pip package with native extensions, rebuild required%s", item.Name, item.Version, note))

		case item.Method == "pip dist-info":
			// Pure pip: requirements stub
			lines = append(lines, fmt.Sprintf("# %s==%s — pip package%s", item.Name, item.Version, note))
			lines = append(lines, fmt.Sprintf("# RUN pip install %s==%s", item.Name, item.Version))

		case (item.Lang == "go" || item.Method == "go binary") && item.Static:
			// Static Go binary: COPY stub
			dest := filepath.Base(item.Path)
			lines = append(lines, fmt.Sprintf("# COPY %s /usr/local/bin/%s", item.Path, dest)+note)

		case item.Lang == "shell" || strings.HasSuffix(item.Path, ".sh"):
			// Shell script: COPY stub
			dest := filepath.Base(item.Path)
			lines = append(lines, fmt.Sprintf("# COPY %s /usr/local/bin/%s", item.Path, dest)+note)

		case len(item.SharedLibs) > 0:
			// Dynamic binary with shared libs: comment-only warning
			lines = append(lines, fmt.Sprintf("# %s — dynamic binary, shared libs: %s%s",
				item.Path, strings.Join(item.SharedLibs, ", "), note))
			lines = append(lines, fmt.Sprintf("# Dependency analysis required before COPY"))

		case item.Static:
			// Other static binary: COPY stub
			dest := filepath.Base(item.Path)
			lines = append(lines, fmt.Sprintf("# COPY %s /usr/local/bin/%s", item.Path, dest)+note)

		default:
			// Unknown/other: comment with source path
			lines = append(lines, fmt.Sprintf("# %s (%s) — review required for migration%s", item.Path, item.Method, note))
		}
	}

	lines = append(lines, "")
	return lines
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/renderer/... -run "TestNonRpmSectionLines_MigrationPlanned" -v`
Expected: PASS

- [ ] **Step 5: Run all containerfile tests**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/renderer/... -run "TestNonRpm|TestRenderContainerfile" -v`
Expected: PASS (check if any existing tests break due to the new ReviewStatus filter — they may need `ReviewStatus: "migration_planned"` added to test data)

- [ ] **Step 6: Fix any broken tests**

Existing tests that create NonRpmItem with `Include: true` but no `ReviewStatus` will now produce no output. Add `ReviewStatus: "migration_planned"` to any test items that expect Containerfile output.

- [ ] **Step 7: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add cmd/inspectah/internal/renderer/containerfile.go cmd/inspectah/internal/renderer/containerfile_test.go
git commit -m "feat(renderer): data-driven non-RPM Containerfile stubs

Only items with ReviewStatus='migration_planned' produce output.
Static binaries and shell scripts get COPY stubs. Dynamic binaries
get comment-only warnings with shared_libs list. Pure pip packages
get 'pip install' stubs. C-extension pip gets rebuild warnings.

Assisted-by: Claude Code"
```

---

### Task 12: Flatpak output — JSON manifest and oneshot service reference

**Files:**
- Modify: `cmd/inspectah/internal/renderer/containerfile.go`
- Modify: `cmd/inspectah/internal/renderer/configtree.go`
- Test: `cmd/inspectah/internal/renderer/containerfile_test.go`

- [ ] **Step 1: Write the failing test**

```go
// In containerfile_test.go
func TestContainersSectionLines_FlatpakOutput(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Containers = &schema.ContainerSection{
		FlatpakApps: []schema.FlatpakApp{
			{AppID: "org.mozilla.firefox", Origin: "flathub", Branch: "stable", Include: true, Remote: "flathub", RemoteURL: "https://dl.flathub.org/repo/"},
			{AppID: "org.gnome.Calculator", Origin: "fedora", Branch: "stable", Include: false},
		},
	}

	lines := containersSectionLines(snap)
	content := strings.Join(lines, "\n")

	if !strings.Contains(content, "flatpak/flatpak-install.json") {
		t.Error("should reference flatpak manifest file")
	}
	if !strings.Contains(content, "flatpak-provision.service") {
		t.Error("should reference oneshot service")
	}
	// Excluded app should not appear
	if strings.Contains(content, "Calculator") {
		t.Error("excluded flatpak should not appear in output")
	}
}

func TestWriteConfigTree_FlatpakManifest(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Containers = &schema.ContainerSection{
		FlatpakApps: []schema.FlatpakApp{
			{AppID: "org.mozilla.firefox", Origin: "flathub", Branch: "stable", Include: true, Remote: "flathub", RemoteURL: "https://dl.flathub.org/repo/"},
		},
	}

	outDir := t.TempDir()
	WriteConfigTree(snap, outDir)

	// Check manifest file exists
	manifestPath := filepath.Join(outDir, "flatpak", "flatpak-install.json")
	data, err := os.ReadFile(manifestPath)
	if err != nil {
		t.Fatalf("flatpak manifest not written: %v", err)
	}

	var manifest []map[string]string
	if err := json.Unmarshal(data, &manifest); err != nil {
		t.Fatalf("invalid manifest JSON: %v", err)
	}

	if len(manifest) != 1 {
		t.Fatalf("manifest has %d entries, want 1", len(manifest))
	}
	if manifest[0]["app_id"] != "org.mozilla.firefox" {
		t.Errorf("app_id = %q, want %q", manifest[0]["app_id"], "org.mozilla.firefox")
	}

	// Check oneshot service file exists
	servicePath := filepath.Join(outDir, "flatpak", "flatpak-provision.service")
	svcData, err := os.ReadFile(servicePath)
	if err != nil {
		t.Fatalf("flatpak service not written: %v", err)
	}
	svcContent := string(svcData)
	if !strings.Contains(svcContent, "ConditionPathExists") {
		t.Error("service should use sentinel-based ConditionPathExists")
	}
	// No jq dependency — service uses direct flatpak commands
	if strings.Contains(svcContent, "jq") {
		t.Error("service must not depend on jq — use direct flatpak commands")
	}
	// Bounded remote-add
	if !strings.Contains(svcContent, "remote-add --if-not-exists") {
		t.Error("service should configure remotes via flatpak remote-add --if-not-exists")
	}
	// Retry logic: the first-boot path is network-sensitive
	if !strings.Contains(svcContent, "Restart=on-failure") {
		t.Error("service should have bounded retry via Restart=on-failure")
	}
	if !strings.Contains(svcContent, "RestartSec=30s") {
		t.Error("service should have RestartSec=30s for retry delay")
	}
	if !strings.Contains(svcContent, "StartLimitBurst=3") {
		t.Error("service should have StartLimitBurst=3 for bounded retry")
	}
	if !strings.Contains(svcContent, "StartLimitIntervalSec=300s") {
		t.Error("service should have StartLimitIntervalSec=300s for retry window")
	}
}

func TestWriteConfigTree_FlatpakUnreconstructableRemote(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Containers = &schema.ContainerSection{
		FlatpakApps: []schema.FlatpakApp{
			{AppID: "com.example.app", Origin: "custom", Branch: "stable",
				Include: true, Remote: "custom", RemoteURL: ""},
		},
	}

	outDir := t.TempDir()
	WriteConfigTree(snap, outDir)

	servicePath := filepath.Join(outDir, "flatpak", "flatpak-provision.service")
	svcData, err := os.ReadFile(servicePath)
	if err != nil {
		t.Fatalf("flatpak service not written: %v", err)
	}
	if !strings.Contains(string(svcData), "could not be fully reconstructed") {
		t.Error("service should warn about unreconstructable remotes")
	}
	if !strings.Contains(string(svcData), "flatpak remote-modify --help") {
		t.Error("service should reference remote-modify help for unreconstructable remotes")
	}
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/renderer/... -run "TestContainersSectionLines_FlatpakOutput|TestWriteConfigTree_FlatpakManifest" -v`
Expected: FAIL — no flatpak output exists

- [ ] **Step 3: Add flatpak output to containersSectionLines**

In `containersSectionLines`, after the compose file block, add:

```go
	// Flatpak apps
	var includedFlatpaks []schema.FlatpakApp
	for _, app := range snap.Containers.FlatpakApps {
		if app.Include {
			includedFlatpaks = append(includedFlatpaks, app)
		}
	}

	if len(includedFlatpaks) > 0 {
		if len(lines) == 0 {
			lines = append(lines, "# === Container Workloads ===")
		}
		lines = append(lines, "# Flatpak applications — installed on first boot via oneshot service")
		lines = append(lines, "COPY flatpak/ /usr/share/inspectah/flatpak/")
		lines = append(lines, "COPY flatpak/flatpak-provision.service /etc/systemd/system/flatpak-provision.service")
		lines = append(lines, "RUN systemctl enable flatpak-provision.service")
	}
```

Also update the early-return condition to include flatpaks:

```go
	if len(includedQuadlets) == 0 && len(includedCompose) == 0 && len(includedFlatpaks) == 0 {
		return lines
	}
```

This requires moving the flatpak collection before the early return. Restructure:

```go
func containersSectionLines(snap *schema.InspectionSnapshot) []string {
	var lines []string
	if snap.Containers == nil {
		return lines
	}

	var includedQuadlets []schema.QuadletUnit
	for _, u := range snap.Containers.QuadletUnits {
		if u.Include {
			includedQuadlets = append(includedQuadlets, u)
		}
	}

	// Compose files are inventory-only — the approved design narrowed
	// compose to inventory/informational display in the SPA. No
	// Containerfile output at all. Do not filter, collect, or emit
	// any compose data here.

	var includedFlatpaks []schema.FlatpakApp
	for _, app := range snap.Containers.FlatpakApps {
		if app.Include {
			includedFlatpaks = append(includedFlatpaks, app)
		}
	}

	if len(includedQuadlets) == 0 && len(includedFlatpaks) == 0 {
		return lines
	}

	lines = append(lines, "# === Container Workloads ===")
	if len(includedQuadlets) > 0 {
		lines = append(lines, "COPY quadlet/ /etc/containers/systemd/")
	}
	if len(includedFlatpaks) > 0 {
		lines = append(lines, "# Flatpak applications — installed on first boot via oneshot service")
		lines = append(lines, "COPY flatpak/ /usr/share/inspectah/flatpak/")
		lines = append(lines, "COPY flatpak/flatpak-provision.service /etc/systemd/system/flatpak-provision.service")
		lines = append(lines, "RUN systemctl enable flatpak-provision.service")
	}
	lines = append(lines, "")
	return lines
}
```

- [ ] **Step 4: Add flatpak manifest and service writing to WriteConfigTree**

In `configtree.go`, after the quadlet unit writing block (around line 184), add:

```go
	// Flatpak manifest and oneshot service
	if snap.Containers != nil {
		var includedFlatpaks []schema.FlatpakApp
		for _, app := range snap.Containers.FlatpakApps {
			if app.Include {
				includedFlatpaks = append(includedFlatpaks, app)
			}
		}
		if len(includedFlatpaks) > 0 {
			flatpakDir := filepath.Join(outputDir, "flatpak")
			os.MkdirAll(flatpakDir, 0755)

			// Write JSON manifest
			type flatpakEntry struct {
				AppID     string `json:"app_id"`
				Remote    string `json:"remote"`
				Branch    string `json:"branch"`
				RemoteURL string `json:"remote_url,omitempty"`
			}
			var manifest []flatpakEntry
			for _, app := range includedFlatpaks {
				manifest = append(manifest, flatpakEntry{
					AppID:     app.AppID,
					Remote:    app.Remote,
					Branch:    app.Branch,
					RemoteURL: app.RemoteURL,
				})
			}
			manifestJSON, _ := json.MarshalIndent(manifest, "", "  ")
			os.WriteFile(filepath.Join(flatpakDir, "flatpak-install.json"), manifestJSON, 0644)

			// Build the oneshot service with bounded remote-add and
			// pure-shell manifest parsing (no jq dependency).
			//
			// The service first configures remotes via flatpak remote-add
			// --if-not-exists, then installs each app. Remotes missing a
			// URL (custom trust material, authenticator plugins, etc.)
			// cannot be fully reconstructed — the service includes a
			// comment listing them.
			var remoteAddLines []string
			var unreconstructable []string
			seenRemotes := make(map[string]bool)
			for _, app := range includedFlatpaks {
				if seenRemotes[app.Remote] {
					continue
				}
				seenRemotes[app.Remote] = true
				if app.RemoteURL != "" {
					remoteAddLines = append(remoteAddLines, fmt.Sprintf(
						"flatpak remote-add --if-not-exists --system %s %s",
						app.Remote, app.RemoteURL))
				} else if app.Remote != "" {
					unreconstructable = append(unreconstructable, app.Remote)
				}
			}

			var installLines []string
			for _, app := range includedFlatpaks {
				installLines = append(installLines, fmt.Sprintf(
					"flatpak install -y --system %s %s//%s",
					app.Remote, app.AppID, app.Branch))
			}

			var serviceBuilder strings.Builder
			serviceBuilder.WriteString(`[Unit]
Description=Provision Flatpak applications from inspectah manifest
After=network-online.target
Wants=network-online.target
ConditionPathExists=!/var/lib/.flatpak-provisioned
StartLimitBurst=3
StartLimitIntervalSec=300s

`)
			if len(unreconstructable) > 0 {
				serviceBuilder.WriteString(fmt.Sprintf(
					"# WARNING: The following remotes could not be fully reconstructed\n"+
					"# (custom trust material, authenticator plugins, or filter config).\n"+
					"# These remotes may require additional configuration.\n"+
					"# See 'flatpak remote-modify --help'.\n"+
					"# Remotes: %s\n\n", strings.Join(unreconstructable, ", ")))
			}
			serviceBuilder.WriteString("[Service]\nType=oneshot\n")
			// Bounded retry: first-boot path is network-sensitive.
			// Retry up to 3 times with 30s delay within a 5-minute window.
			serviceBuilder.WriteString("Restart=on-failure\nRestartSec=30s\n")
			for _, line := range remoteAddLines {
				serviceBuilder.WriteString(fmt.Sprintf("ExecStartPre=/usr/bin/%s\n", line))
			}
			for _, line := range installLines {
				serviceBuilder.WriteString(fmt.Sprintf("ExecStart=/usr/bin/%s\n", line))
			}
			serviceBuilder.WriteString("ExecStartPost=/usr/bin/touch /var/lib/.flatpak-provisioned\n")
			serviceBuilder.WriteString("RemainAfterExit=yes\n\n")
			serviceBuilder.WriteString("[Install]\nWantedBy=multi-user.target\n")

			os.WriteFile(filepath.Join(flatpakDir, "flatpak-provision.service"),
				[]byte(serviceBuilder.String()), 0644)
		}
	}
```

Add `"encoding/json"` to the imports of `configtree.go` if not already present.

- [ ] **Step 5: Run test to verify it passes**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/renderer/... -run "TestContainersSectionLines_FlatpakOutput|TestWriteConfigTree_FlatpakManifest" -v`
Expected: PASS

- [ ] **Step 6: Run all renderer tests**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/renderer/... -v`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add cmd/inspectah/internal/renderer/containerfile.go cmd/inspectah/internal/renderer/containerfile_test.go cmd/inspectah/internal/renderer/configtree.go
git commit -m "feat(renderer): flatpak manifest and oneshot service output

Included flatpak apps produce a JSON manifest and a systemd oneshot
service that provisions apps on first boot using a sentinel file.
Follows the uBlue/Fedora Atomic pattern.

Assisted-by: Claude Code"
```

---

### Task 13: Quadlet draft generation from running containers

**Files:**
- Create: `cmd/inspectah/internal/renderer/quadletdraft.go`
- Create: `cmd/inspectah/internal/renderer/quadletdraft_test.go`
- Modify: `cmd/inspectah/internal/schema/types.go` (add RestartPolicy to RunningContainer)
- Modify: `cmd/inspectah/internal/inspector/container.go` (capture restart policy from podman inspect)

> **Blocker 3 prerequisite:** `RunningContainer` does not currently have a
> `RestartPolicy` field. This task adds `RestartPolicy string
> \`json:"restart_policy,omitempty"\`` to `RunningContainer` in `types.go`
> and populates it from `podman inspect` data in the inspector.
> This is a minor schema addition (no version bump needed — it's additive
> and omitempty).

- [ ] **Step 1: Write the failing test**

```go
// quadletdraft_test.go
package renderer

import (
	"strings"
	"testing"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
)

func TestGenerateQuadletDraft(t *testing.T) {
	container := schema.RunningContainer{
		Name:          "webapp",
		Image:         "registry.example.com/webapp:latest",
		RestartPolicy: "always",
		Ports: map[string]interface{}{
			"8080/tcp": []interface{}{
				map[string]interface{}{"HostIp": "0.0.0.0", "HostPort": "8080"},
			},
		},
		Mounts: []schema.ContainerMount{
			{Source: "/data/webapp", Destination: "/app/data", Mode: "rw"},
		},
		Env: []string{"APP_ENV=production", "PATH=/usr/bin:/bin"},
	}

	draft := GenerateQuadletDraft(container)

	if !strings.Contains(draft, "[Container]") {
		t.Error("draft should contain [Container] section")
	}
	if !strings.Contains(draft, "Image=registry.example.com/webapp:latest") {
		t.Error("draft should contain Image directive")
	}
	if !strings.Contains(draft, "PublishPort=8080:8080") {
		t.Error("draft should contain port mapping")
	}
	if !strings.Contains(draft, "Volume=/data/webapp:/app/data:rw") {
		t.Error("draft should contain volume mount")
	}
	if !strings.Contains(draft, "Environment=APP_ENV=production") {
		t.Error("draft should contain non-PATH env vars")
	}
	if strings.Contains(draft, "Environment=PATH=") {
		t.Error("draft should skip PATH env var")
	}
	if !strings.Contains(draft, "[Install]") {
		t.Error("draft should contain [Install] section")
	}
	// Restart mapped from actual data, in [Service] section
	if !strings.Contains(draft, "Restart=always") {
		t.Error("draft should map restart policy from captured data")
	}
	if strings.Contains(draft, "[Container]") && strings.Contains(draft, "Restart=") {
		// Verify Restart is NOT in [Container] section
		containerSection := draft[:strings.Index(draft, "[Service]")]
		if strings.Contains(containerSection, "Restart=") {
			t.Error("Restart belongs in [Service], not [Container]")
		}
	}
}

func TestGenerateQuadletDraft_NoRestartPolicy(t *testing.T) {
	container := schema.RunningContainer{
		Name:  "noreset",
		Image: "test:latest",
		// No RestartPolicy set
	}

	draft := GenerateQuadletDraft(container)

	if !strings.Contains(draft, "# TODO: Review restart policy") {
		t.Error("draft should include TODO comment when no restart policy captured")
	}
	if strings.Contains(draft, "Restart=on-failure") {
		t.Error("draft must not invent Restart=on-failure when no policy captured")
	}
}

func TestGenerateQuadletDraft_MinimalContainer(t *testing.T) {
	container := schema.RunningContainer{
		Name:  "simple",
		Image: "alpine:latest",
	}

	draft := GenerateQuadletDraft(container)

	if !strings.Contains(draft, "Image=alpine:latest") {
		t.Error("draft should contain Image")
	}
	if !strings.Contains(draft, "ContainerName=simple") {
		t.Error("draft should contain ContainerName")
	}
}

func TestGenerateQuadletDraft_NonDefaultHostIp(t *testing.T) {
	container := schema.RunningContainer{
		Name:  "localonly",
		Image: "app:latest",
		Ports: map[string]interface{}{
			"8080/tcp": []interface{}{
				map[string]interface{}{"HostIp": "127.0.0.1", "HostPort": "8080"},
			},
			"9090/tcp": []interface{}{
				map[string]interface{}{"HostIp": "0.0.0.0", "HostPort": "9090"},
			},
		},
	}

	draft := GenerateQuadletDraft(container)

	// Non-default HostIp must be preserved to avoid broadening bind scope
	if !strings.Contains(draft, "PublishPort=127.0.0.1:8080:8080") {
		t.Error("draft should preserve non-default HostIp (127.0.0.1:8080:8080)")
	}
	// Default HostIp (0.0.0.0) should be omitted from the mapping
	if strings.Contains(draft, "0.0.0.0:9090") {
		t.Error("draft should omit default HostIp 0.0.0.0 from port mapping")
	}
	if !strings.Contains(draft, "PublishPort=9090:9090") {
		t.Error("draft should emit 9090:9090 without HostIp prefix for default bind")
	}
}

func TestGenerateQuadletDraft_DeferredTodoComments(t *testing.T) {
	container := schema.RunningContainer{
		Name:  "full",
		Image: "app:latest",
	}

	draft := GenerateQuadletDraft(container)

	// All deferred gaps must have explicit TODO comments
	if !strings.Contains(draft, "TODO: Review healthcheck") {
		t.Error("draft should include TODO for deferred healthcheck")
	}
	if !strings.Contains(draft, "TODO: Review dependency ordering") {
		t.Error("draft should include TODO for deferred dependency ordering")
	}
	if !strings.Contains(draft, "TODO: Review user namespace") {
		t.Error("draft should include TODO for deferred user namespace")
	}
}

func TestGenerateQuadletDraft_NetworksToString(t *testing.T) {
	container := schema.RunningContainer{
		Name:     "nettest",
		Image:    "test:latest",
		Networks: map[string]interface{}{"mynet": map[string]interface{}{}},
	}

	draft := GenerateQuadletDraft(container)

	if !strings.Contains(draft, "Network=mynet") {
		t.Error("draft should contain Network directive")
	}
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/renderer/... -run "TestGenerateQuadletDraft" -v`
Expected: FAIL — `GenerateQuadletDraft` doesn't exist

- [ ] **Step 3: Implement GenerateQuadletDraft**

Create `cmd/inspectah/internal/renderer/quadletdraft.go`:

```go
package renderer

import (
	"fmt"
	"sort"
	"strings"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
)

// GenerateQuadletDraft produces a .container quadlet unit file from
// running container data. The output is a draft — operators must
// review before using.
func GenerateQuadletDraft(c schema.RunningContainer) string {
	var lines []string

	lines = append(lines, "# Generated by inspectah — review before using")
	lines = append(lines, "[Container]")
	lines = append(lines, fmt.Sprintf("ContainerName=%s", c.Name))
	lines = append(lines, fmt.Sprintf("Image=%s", c.Image))

	// Ports
	for _, portSpec := range extractPortMappings(c.Ports) {
		lines = append(lines, fmt.Sprintf("PublishPort=%s", portSpec))
	}

	// Volumes
	for _, mount := range c.Mounts {
		mode := mount.Mode
		if mode == "" {
			mode = "rw"
		}
		lines = append(lines, fmt.Sprintf("Volume=%s:%s:%s", mount.Source, mount.Destination, mode))
	}

	// Environment (skip common runtime vars)
	skipEnv := map[string]bool{
		"PATH": true, "HOME": true, "HOSTNAME": true, "TERM": true,
	}
	for _, env := range c.Env {
		parts := strings.SplitN(env, "=", 2)
		if len(parts) == 2 && !skipEnv[parts[0]] {
			lines = append(lines, fmt.Sprintf("Environment=%s", env))
		}
	}

	// Networks
	for _, net := range sortedNetworkNames(c.Networks) {
		lines = append(lines, fmt.Sprintf("Network=%s", net))
	}

	// Service section — restart policy belongs in [Service], not [Container].
	// Map from actual podman inspect data when available; otherwise omit
	// with an explicit TODO comment. Never invent restart semantics.
	lines = append(lines, "")
	lines = append(lines, "[Service]")
	if c.RestartPolicy != "" && c.RestartPolicy != "no" {
		// Map podman restart policies to systemd equivalents
		switch c.RestartPolicy {
		case "always":
			lines = append(lines, "Restart=always")
		case "on-failure":
			lines = append(lines, "Restart=on-failure")
		case "unless-stopped":
			lines = append(lines, "Restart=always")
			lines = append(lines, "# Note: podman 'unless-stopped' mapped to systemd 'always'")
		default:
			lines = append(lines, fmt.Sprintf("# Restart policy '%s' — review for systemd equivalent", c.RestartPolicy))
		}
	} else {
		lines = append(lines, "# TODO: Review restart policy — no restart policy captured from container")
	}
	lines = append(lines, "# TODO: Review resource limits")
	lines = append(lines, "# TODO: Review healthcheck — deferred from initial draft generation")
	lines = append(lines, "# TODO: Review dependency ordering — deferred from initial draft generation")
	lines = append(lines, "# TODO: Review user namespace mapping — deferred from initial draft generation")

	// Install section
	lines = append(lines, "")
	lines = append(lines, "[Install]")
	lines = append(lines, "WantedBy=default.target")

	return strings.Join(lines, "\n") + "\n"
}

// extractPortMappings converts podman inspect port data to host:container
// format strings.
func extractPortMappings(ports map[string]interface{}) []string {
	if len(ports) == 0 {
		return nil
	}

	var result []string
	// Sort keys for deterministic output
	keys := make([]string, 0, len(ports))
	for k := range ports {
		keys = append(keys, k)
	}
	sort.Strings(keys)

	for _, containerPort := range keys {
		bindings, ok := ports[containerPort].([]interface{})
		if !ok {
			continue
		}
		// Extract the port number (strip /tcp, /udp suffix)
		cPort := strings.Split(containerPort, "/")[0]
		proto := ""
		if parts := strings.Split(containerPort, "/"); len(parts) > 1 && parts[1] != "tcp" {
			proto = "/" + parts[1]
		}

		for _, b := range bindings {
			binding, ok := b.(map[string]interface{})
			if !ok {
				continue
			}
			hostPort, _ := binding["HostPort"].(string)
			if hostPort == "" {
				continue
			}
			hostIp, _ := binding["HostIp"].(string)
			// Preserve non-default HostIp to avoid silently broadening
			// bind scope (e.g., 127.0.0.1:8080:8080 → 0.0.0.0:8080:8080)
			if hostIp != "" && hostIp != "0.0.0.0" && hostIp != "::" {
				result = append(result, fmt.Sprintf("%s:%s:%s%s", hostIp, hostPort, cPort, proto))
			} else {
				result = append(result, fmt.Sprintf("%s:%s%s", hostPort, cPort, proto))
			}
		}
	}
	return result
}

// sortedNetworkNames returns sorted network names from the networks map.
func sortedNetworkNames(networks map[string]interface{}) []string {
	if len(networks) == 0 {
		return nil
	}
	names := make([]string, 0, len(networks))
	for name := range networks {
		names = append(names, name)
	}
	sort.Strings(names)
	return names
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/renderer/... -run "TestGenerateQuadletDraft" -v`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add cmd/inspectah/internal/renderer/quadletdraft.go cmd/inspectah/internal/renderer/quadletdraft_test.go
git commit -m "feat(renderer): quadlet draft generation from running containers

Maps running container data (image, ports, volumes, env, networks)
to a .container quadlet unit file. Restart policy placed in [Service]
section. Healthcheck, dependency ordering, and user namespace
mapping deferred from v1.

Assisted-by: Claude Code"
```

---

## Phase 5: SPA

### Task 14: Non-RPM review status cards with visible ARIA radio-group

**Files:**
- Modify: `cmd/inspectah/internal/renderer/static/report.html`

> **Blocker 1 fix:** Uses visible ARIA `role="radio"` buttons (same pattern as the
> version-changes filter) instead of hidden `<input type="radio">` elements. This
> matches the SPA's existing accessibility pattern and avoids hidden-radio CSS hacks.
>
> **Blocker 5 note — Go tests vs. browser verification:**
> Go tests verify: review-status card HTML structure is present in rendered output,
> `buildReviewStatusCard` function exists in JS, CSS classes are present.
> Go tests CANNOT verify: radio-group keyboard navigation (Left/Right arrow keys),
> focus management (Tab enters group on active segment), visual fill state,
> `aria-checked` toggle behavior, notes blur-save behavior.
> **Browser verification needed:** keyboard navigation within radio group,
> focus management, status change saves via PUT /api/snapshot, notes blur-save,
> screen reader announcement via aria-live.

- [ ] **Step 1: Add CSS for review-status cards**

In the `<style>` section of `report.html`, add after the existing card styles:

```css
/* Non-RPM review status cards */
.review-status-card {
  padding: 12px 16px;
  border-radius: 8px;
  margin-bottom: 6px;
  background: var(--pf-t--global--background--color--secondary-default);
  border-left: 3px solid var(--pf-t--global--border--color--default);
}
.review-status-card[data-status="reviewed"] {
  border-left-color: var(--pf-t--global--color--status--info--default);
}
.review-status-card[data-status="migration_planned"] {
  border-left-color: #3fb950;
}
.review-status-row {
  display: flex; align-items: center; gap: 12px;
}
.review-status-content {
  flex: 1; min-width: 0;
  cursor: pointer;
}
.review-status-name {
  font-weight: 600; font-size: 14px;
}
.review-status-meta {
  font-size: 12px; opacity: 0.7; margin-top: 2px;
}
/* Visible ARIA radio-group — matches version-changes filter pattern */
.review-status-control [role="radiogroup"] {
  display: flex;
  gap: 0;
  border: 1px solid var(--pf-t--global--border--color--default, #444);
  border-radius: 6px;
  overflow: hidden;
}
.review-status-control [role="radio"] {
  padding: 0.35rem 0.75rem;
  font-size: 0.8rem;
  cursor: pointer;
  border: none;
  border-right: 1px solid var(--pf-t--global--border--color--default, #444);
  background: var(--pf-t--global--background--color--secondary-default);
  color: var(--pf-t--global--text--color--primary);
  user-select: none;
  outline: none;
  white-space: nowrap;
}
.review-status-control [role="radio"]:last-child {
  border-right: none;
}
.review-status-control [role="radio"][aria-checked="true"] {
  background: var(--pf-t--global--color--status--info--default, #2b9af3);
  color: #fff;
  font-weight: 600;
}
.review-status-control [role="radio"][data-value="migration_planned"][aria-checked="true"] {
  background: #3fb950;
}
.review-status-control [role="radio"]:focus-visible {
  box-shadow: 0 0 0 2px var(--pf-t--global--color--status--info--default, #2b9af3);
  z-index: 1;
  position: relative;
}
.review-notes-field {
  width: 100%; margin-top: 8px; padding: 6px 8px;
  border: 1px solid var(--pf-t--global--border--color--default);
  border-radius: 4px; font-size: 13px; resize: vertical;
  min-height: 32px; background: transparent; color: inherit;
}
.review-status-detail {
  display: none; padding: 8px 0 4px 0;
}
```

- [ ] **Step 2: Add buildReviewStatusCard function**

In the JavaScript section, add after `buildInfoReadOnlyCard`. Uses visible `role="radio"` buttons with the same keyboard model as the version-changes filter (Left/Right arrow keys move selection, Tab enters/exits the group):

```javascript
// ── Non-RPM Review Status Card ──
function buildReviewStatusCard(item, sectionId) {
  var snap = App.snapshot;
  var nrItem = findNonRpmItem(snap, item.key);
  var currentStatus = (nrItem && nrItem.review_status) || 'not_reviewed';
  var currentNotes = (nrItem && nrItem.notes) || '';

  var card = document.createElement('div');
  card.className = 'review-status-card';
  card.setAttribute('data-key', item.key);
  card.setAttribute('data-status', currentStatus);

  var row = document.createElement('div');
  row.className = 'review-status-row';

  // Visible ARIA radio-group (same pattern as version-changes filter)
  var controlWrap = document.createElement('div');
  controlWrap.className = 'review-status-control';
  var seg = document.createElement('div');
  seg.setAttribute('role', 'radiogroup');
  seg.setAttribute('aria-label', 'Review status for ' + (item.name || item.key));

  // Live region for screen reader announcements
  var liveRegion = document.createElement('span');
  liveRegion.setAttribute('role', 'status');
  liveRegion.setAttribute('aria-live', 'polite');
  liveRegion.className = 'sr-only';
  controlWrap.appendChild(liveRegion);

  var statuses = [
    {value: 'not_reviewed', label: 'Not reviewed'},
    {value: 'reviewed', label: 'Reviewed'},
    {value: 'migration_planned', label: 'Migration planned'}
  ];

  var radioButtons = [];
  statuses.forEach(function(s, idx) {
    var btn = document.createElement('button');
    btn.type = 'button';
    btn.setAttribute('role', 'radio');
    btn.setAttribute('aria-checked', s.value === currentStatus ? 'true' : 'false');
    btn.setAttribute('data-value', s.value);
    btn.setAttribute('tabindex', s.value === currentStatus ? '0' : '-1');
    btn.textContent = s.label;

    btn.addEventListener('click', function() {
      // Deselect all, select this one
      radioButtons.forEach(function(rb) {
        rb.setAttribute('aria-checked', 'false');
        rb.setAttribute('tabindex', '-1');
      });
      btn.setAttribute('aria-checked', 'true');
      btn.setAttribute('tabindex', '0');
      btn.focus();

      if (nrItem) {
        nrItem.review_status = s.value;
        card.setAttribute('data-status', s.value);
        liveRegion.textContent = s.label;
        scheduleAutosave();
        updateNonRpmSidebarBadge();
      }
    });

    // Keyboard: Left/Right arrows move selection (radio-group model)
    btn.addEventListener('keydown', function(e) {
      var dir = 0;
      if (e.key === 'ArrowRight' || e.key === 'ArrowDown') dir = 1;
      else if (e.key === 'ArrowLeft' || e.key === 'ArrowUp') dir = -1;
      if (dir !== 0) {
        e.preventDefault();
        var nextIdx = (idx + dir + statuses.length) % statuses.length;
        radioButtons[nextIdx].click();
      }
    });

    radioButtons.push(btn);
    seg.appendChild(btn);
  });

  controlWrap.appendChild(seg);
  row.appendChild(controlWrap);

  // Content
  var content = document.createElement('div');
  content.className = 'review-status-content';
  content.style.cursor = 'pointer';

  var nameEl = document.createElement('div');
  nameEl.className = 'review-status-name';
  nameEl.textContent = item.name || item.key;
  content.appendChild(nameEl);

  if (item.meta) {
    var metaEl = document.createElement('div');
    metaEl.className = 'review-status-meta';
    metaEl.textContent = item.meta;
    content.appendChild(metaEl);
  }

  row.appendChild(content);

  // Expand chevron
  var chevron = document.createElement('button');
  chevron.type = 'button';
  chevron.className = 'tier-chevron';
  chevron.innerHTML = '&#9656;';
  chevron.setAttribute('aria-expanded', 'false');
  chevron.setAttribute('aria-label', 'Expand details for ' + (item.name || item.key));
  chevron.setAttribute('tabindex', '0');
  row.appendChild(chevron);

  card.appendChild(row);

  // Detail pane
  var detail = document.createElement('div');
  detail.className = 'review-status-detail';

  if (item.reason) {
    var reasonEl = document.createElement('div');
    reasonEl.style.cssText = 'font-size:13px;opacity:0.7;margin-bottom:8px;';
    reasonEl.textContent = item.reason;
    detail.appendChild(reasonEl);
  }

  // Notes textarea
  var notes = document.createElement('textarea');
  notes.className = 'review-notes-field';
  notes.placeholder = 'Migration notes...';
  notes.value = currentNotes;
  notes.setAttribute('aria-label', 'Notes for ' + (item.name || item.key));
  notes.addEventListener('blur', function() {
    if (nrItem && nrItem.notes !== notes.value) {
      nrItem.notes = notes.value;
      scheduleAutosave();
    }
  });
  detail.appendChild(notes);

  card.appendChild(detail);

  // Expand/collapse
  var toggleExpand = function() {
    var isExpanded = chevron.getAttribute('aria-expanded') === 'true';
    chevron.setAttribute('aria-expanded', isExpanded ? 'false' : 'true');
    detail.style.display = isExpanded ? 'none' : '';
    chevron.classList.toggle('expanded', !isExpanded);
  };
  chevron.onclick = function(e) { e.stopPropagation(); toggleExpand(); };
  chevron.addEventListener('keydown', function(e) {
    if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); toggleExpand(); }
  });
  content.onclick = function() { toggleExpand(); };

  return card;
}

function findNonRpmItem(snap, key) {
  if (!snap || !snap.non_rpm_software || !snap.non_rpm_software.items) return null;
  var nrName = key.substring(7); // strip 'nonrpm-'
  for (var i = 0; i < snap.non_rpm_software.items.length; i++) {
    var item = snap.non_rpm_software.items[i];
    var itemName = item.path || item.name;
    if (itemName === nrName) return item;
  }
  return null;
}

function updateNonRpmSidebarBadge() {
  var snap = App.snapshot;
  if (!snap || !snap.non_rpm_software || !snap.non_rpm_software.items) return;
  var unreviewed = 0;
  for (var i = 0; i < snap.non_rpm_software.items.length; i++) {
    var item = snap.non_rpm_software.items[i];
    if (!item.review_status || item.review_status === 'not_reviewed') {
      unreviewed++;
    }
  }
  // Update the sidebar badge for nonrpm section
  var navItem = document.querySelector('[data-section="nonrpm"] .nav-badge');
  if (navItem) {
    if (unreviewed > 0) {
      navItem.textContent = unreviewed;
      navItem.style.display = '';
    } else {
      navItem.style.display = 'none';
    }
  }
}
```

- [ ] **Step 3: Wire buildReviewStatusCard into renderTriageSection**

In the `renderTriageSection` function, in the card-building switch logic (around the `buildToggleCard` / `buildNotificationCard` / `buildInfoReadOnlyCard` selection), add a check for the nonrpm section:

Find the block in `renderTriageSection` that selects which card to build (around line 5350-5365). Before the `else { card = buildToggleCard(uItem, sectionId); }` fallback, add:

```javascript
          } else if (sectionId === 'nonrpm') {
            card = buildReviewStatusCard(uItem, sectionId);
```

So the full selection becomes:
```javascript
          if (uIsSecret) {
            // ...secret card...
          } else if (uItem.display_only) {
            card = buildInfoReadOnlyCard(uItem);
          } else if (uItem.card_type === 'notification') {
            card = buildNotificationCard(uItem);
          } else if (sectionId === 'nonrpm') {
            card = buildReviewStatusCard(uItem, sectionId);
          } else {
            card = buildToggleCard(uItem, sectionId);
          }
```

- [ ] **Step 4: Run HTML tests**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/renderer/... -run "TestRenderHTML" -v`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add cmd/inspectah/internal/renderer/static/report.html
git commit -m "feat(spa): non-RPM review status cards with segmented radio-group

Cards use three-state segmented control (not_reviewed, reviewed,
migration_planned) with freeform notes field. Status changes save
via PUT /api/snapshot. Notes save on blur. Sidebar shows unreviewed
count.

Assisted-by: Claude Code"
```

---

### Task 15: Flatpak cards with persistent annotation

**Files:**
- Modify: `cmd/inspectah/internal/renderer/static/report.html`

> **Blocker 2 note:** The flatpak `getSnapshotInclude` / `updateSnapshotInclude`
> handlers were moved to Task 9 so they land with the classifier. This task only
> adds the visual annotation and can be landed independently.
>
> **Blocker 5 note — Browser verification needed:** annotation visibility across
> themes, annotation does not clip on narrow viewports.

- [ ] **Step 1: Add flatpak annotation to toggle cards**

In the `buildToggleCard` function, after creating the `content` div and name/meta elements, add a check for flatpak items. Find where `item.meta` is rendered (the meta element creation) and add after it:

```javascript
  // Flatpak persistent annotation — always visible, not a tooltip.
  // Includes operator-facing caveats from the approved UX where the
  // operator makes the include decision.
  if (item.key.indexOf('flatpak-') === 0) {
    var annotation = document.createElement('div');
    annotation.style.cssText = 'font-size:11px;color:#d29922;margin-top:4px;font-style:italic;';
    annotation.textContent = 'Installed on first boot (not baked into image)';
    content.appendChild(annotation);

    var caveats = document.createElement('ul');
    caveats.style.cssText = 'font-size:11px;opacity:0.7;margin:4px 0 0 16px;padding:0;list-style:disc;';
    var caveatItems = [
      'Network access required at first boot',
      'Remote configuration is best-effort',
      'Custom/enterprise remotes may require manual setup'
    ];
    for (var ci = 0; ci < caveatItems.length; ci++) {
      var li = document.createElement('li');
      li.style.cssText = 'margin:1px 0;';
      li.textContent = caveatItems[ci];
      caveats.appendChild(li);
    }
    content.appendChild(caveats);
  }
```

This goes inside `buildToggleCard`, after the `nameEl` and `badge` are appended to `content`, and after the `meta` element. Find the meta element creation:

```javascript
  var meta = document.createElement('span');
  meta.className = 'toggle-card-meta';
  // ... meta setup ...
  content.appendChild(meta);
```

Add the annotation block right after that `content.appendChild(meta)`.

- [ ] **Step 2: Run HTML tests**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/renderer/... -run "TestRenderHTML" -v`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add cmd/inspectah/internal/renderer/static/report.html
git commit -m "feat(spa): flatpak toggle cards with persistent annotation

Flatpak items show 'Installed on first boot (not baked into image)'
annotation. Include/exclude toggles connected to snapshot flatpak_apps.

Assisted-by: Claude Code"
```

---

### Task 16: Compose informational cards (display-only)

**Files:**
- Modify: `cmd/inspectah/internal/renderer/static/report.html`

> **Blocker 4 fix:** Compose items use `CardType: "compose-info"` (not `DisplayOnly: true`)
> so they stay in the main grouped rendering loop and appear as the fourth subsection
> in the containers hierarchy. A dedicated `buildComposeInfoCard` renders them as muted
> cards with a service inventory. Compose items never participate in the
> include/exclude system.
>
> **Blocker 5 note — Browser verification needed:** muted card visual styling,
> service inventory layout on narrow viewports.

- [ ] **Step 1: Add compose-info card builder**

Compose items use `card_type: "compose-info"` from the triage classifier (Task 10).
They render through a dedicated `buildComposeInfoCard` function. This is NOT
`buildInfoReadOnlyCard` — compose items need to stay in the main grouped loop
(not the informational tier) so they render as the fourth subsection.

Add after `buildInfoReadOnlyCard`:

```javascript
// ── Compose Informational Card ──
// Renders compose files as muted cards with service inventory.
// Uses card_type='compose-info' to stay in the main rendering loop
// (not the informational tier) so subsection ordering works.
function buildComposeInfoCard(item) {
  var card = document.createElement('div');
  card.className = 'triage-card tier-' + item.tier + ' info-readonly compose-info';
  card.setAttribute('data-key', item.key);
  card.style.opacity = '0.85';

  // Name
  var nameEl = document.createElement('div');
  nameEl.className = 'card-name';
  nameEl.textContent = item.name || item.key;
  card.appendChild(nameEl);

  // Meta
  if (item.meta) {
    var metaEl = document.createElement('div');
    metaEl.className = 'card-meta';
    metaEl.textContent = item.meta;
    card.appendChild(metaEl);
  }

  // Compose service inventory
  var snap = App.snapshot;
  var composePath = item.key.substring(8);
  if (snap && snap.containers && snap.containers.compose_files) {
    for (var ci = 0; ci < snap.containers.compose_files.length; ci++) {
      var cf = snap.containers.compose_files[ci];
      if (cf.path === composePath && cf.images) {
        var svcList = document.createElement('div');
        svcList.style.cssText = 'margin-top:6px;font-size:12px;opacity:0.8;';
        for (var si = 0; si < cf.images.length; si++) {
          var svc = cf.images[si];
          var svcLine = document.createElement('div');
          svcLine.style.cssText = 'padding:2px 0;';
          svcLine.textContent = svc.service + ' → ' + svc.image;
          svcList.appendChild(svcLine);
        }
        card.appendChild(svcList);
        break;
      }
    }
  }

  // Reason / guidance text
  if (item.reason) {
    var reasonEl = document.createElement('div');
    reasonEl.className = 'card-reason';
    reasonEl.textContent = item.reason;
    card.appendChild(reasonEl);
  }

  // No action buttons — read-only
  return card;
}
```

- [ ] **Step 1b: Wire buildComposeInfoCard into the card-selection logic**

In `renderTriageSection`, in the ungrouped card-building switch, add a check for `compose-info` card_type. Find the block with `buildInfoReadOnlyCard` / `buildNotificationCard` / `buildToggleCard` and add before the `display_only` check:

```javascript
          } else if (uItem.card_type === 'compose-info') {
            card = buildComposeInfoCard(uItem);
```

Also add the same routing in the grouped items path where cards are built for grouped items (inside accordion rendering), so compose items within `sub:compose` groups get the correct card type:

```javascript
          if (gItem.card_type === 'compose-info') {
            card = buildComposeInfoCard(gItem);
          } else if (...existing logic...) {
```

- [ ] **Step 2: Run HTML tests**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/renderer/... -run "TestRenderHTML" -v`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add cmd/inspectah/internal/renderer/static/report.html
git commit -m "feat(spa): compose file informational cards with service inventory

Display-only compose cards show per-service image mappings from
the snapshot data. Muted card styling with no action affordances.

Assisted-by: Claude Code"
```

---

### Task 17: Generate Quadlet Draft button for running containers

**Files:**
- Modify: `cmd/inspectah/internal/renderer/static/report.html`
- Modify: `cmd/inspectah/internal/refine/server.go`
- Test: `cmd/inspectah/internal/refine/server_test.go`

> **Blocker 3 fixes:**
> - Generated drafts stored with `Include: false` (operator must review and toggle on)
> - Duplicate-draft suppression: endpoint returns 409 if draft already exists; SPA
>   checks before sending request and shows disabled "Draft generated" button
> - Missing-image error: endpoint returns 422 if container has no image field; SPA
>   shows inline error "Cannot generate draft — container image unknown"
> - Post-click UX: card updates to "Draft generated — see Quadlet Units above" with
>   scroll-to-new-entry and focus moves to the new quadlet entry
>
> **Revision 4 live-seam fixes:**
> - **Route dispatch:** The live `ServeHTTP` uses a manual switch on `r.URL.Path`,
>   not mux-based dispatch. Both `h.mux.HandleFunc` AND the `ServeHTTP` switch must
>   register `/api/quadlet-draft`. Tests already use `httptest.NewServer(handler)`
>   which calls `ServeHTTP`, so they exercise the real dispatch path.
> - **Card path:** Running containers ARE `DisplayOnly` in the live classifier
>   (`triage.go` sets `DisplayOnly: true` for running containers). They render through
>   `buildInfoReadOnlyCard`, NOT `buildToggleCard`. The draft button attaches inside
>   `buildInfoReadOnlyCard` for `container-` prefixed keys in refine mode.
> - **Stale snapshot refresh:** The draft endpoint returns a full rebuild response
>   (same shape as `POST /api/render`) by calling `h.reRenderFn` after saving the
>   snapshot. The SPA calls `applyRebuildResponse()` directly on the draft endpoint's
>   response — no separate render call, no stale `App.snapshot` race.
>
> **Blocker 5 note — Go tests verify:** API endpoint returns correct status codes
> (200, 409, 422), response contains full rebuild payload (html, snapshot,
> containerfile, triage_manifest, render_id, revision), snapshot contains generated
> unit with Include=false and Generated=true, draft content maps actual restart policy.
> **Browser verification needed:** post-click button state transition, scroll-to
> behavior, focus management to new entry, disabled button styling.

- [ ] **Step 1: Add quadlet draft API endpoint**

In `server.go`, add the route in TWO places:

**a) Mux registration** — in `newRefineHandler` where the other `h.mux.HandleFunc` calls are registered:

```go
	h.mux.HandleFunc("/api/quadlet-draft", h.handleQuadletDraft)
```

**b) ServeHTTP switch** — the live server dispatches requests through a manual `switch` on `r.URL.Path` in `ServeHTTP`, not through mux-based routing. Adding to the mux alone is incomplete. Find the `switch` block in `ServeHTTP` and add a case:

```go
	case path == "/api/quadlet-draft":
		h.handleQuadletDraft(w, r)
```

Add this case before the `default:` case, alongside the other `/api/*` routes.

Then add the handler. Note: the handler returns a **full rebuild response** (same shape as `POST /api/render`) to avoid the stale-snapshot race. After saving the draft to the snapshot, it calls `h.reRenderFn` to produce the rebuild payload. This means the SPA can call `applyRebuildResponse()` directly on the draft endpoint's response.

```go
func (h *refineHandler) handleQuadletDraft(w http.ResponseWriter, r *http.Request) {
	if r.Method != "POST" {
		h.sendError(w, 405, "method not allowed")
		return
	}

	body, err := io.ReadAll(r.Body)
	if err != nil {
		h.sendError(w, 400, "failed to read request body")
		return
	}
	defer r.Body.Close()

	var req struct {
		ContainerName string `json:"container_name"`
	}
	if err := json.Unmarshal(body, &req); err != nil || req.ContainerName == "" {
		h.sendError(w, 400, "container_name required")
		return
	}

	// Read current snapshot
	snapPath := filepath.Join(h.outputDir, "inspection-snapshot.json")
	snap, err := schema.LoadSnapshot(snapPath)
	if err != nil {
		h.sendError(w, 500, "failed to load snapshot")
		return
	}

	if snap.Containers == nil {
		h.sendError(w, 404, "no containers in snapshot")
		return
	}

	// Find the running container
	var target *schema.RunningContainer
	for i := range snap.Containers.RunningContainers {
		if snap.Containers.RunningContainers[i].Name == req.ContainerName {
			target = &snap.Containers.RunningContainers[i]
			break
		}
	}
	if target == nil {
		h.sendError(w, 404, "container not found: "+req.ContainerName)
		return
	}

	// Missing-image guard: cannot generate draft without an image
	if target.Image == "" {
		h.sendError(w, 422, "Cannot generate draft — container image unknown")
		return
	}

	// Duplicate-draft suppression: no-op if draft already exists
	unitName := req.ContainerName + ".container"
	for _, u := range snap.Containers.QuadletUnits {
		if u.Name == unitName && u.Generated {
			h.sendError(w, 409, "draft already exists for "+req.ContainerName)
			return
		}
	}

	// Generate the draft
	draft := renderer.GenerateQuadletDraft(*target)

	// Add to snapshot with Include: false — operator must review and toggle on
	snap.Containers.QuadletUnits = append(snap.Containers.QuadletUnits, schema.QuadletUnit{
		Name:      unitName,
		Path:      "etc/containers/systemd/" + unitName,
		Content:   draft,
		Image:     target.Image,
		Include:   false,
		Generated: true,
	})

	// Extract ports and volumes from the draft for structured fields
	ports, volumes := inspector.ExtractQuadletPortsAndVolumes(draft)
	snap.Containers.QuadletUnits[len(snap.Containers.QuadletUnits)-1].Ports = ports
	snap.Containers.QuadletUnits[len(snap.Containers.QuadletUnits)-1].Volumes = volumes

	// Save snapshot
	schema.SaveSnapshot(snap, snapPath)

	// Return a full rebuild response (same shape as POST /api/render).
	// This avoids the stale-snapshot race: the SPA calls
	// applyRebuildResponse() directly on this response — no separate
	// POST /api/render call needed, no window where App.snapshot is stale.
	if h.reRenderFn == nil {
		// Fallback: if no re-render function (shouldn't happen in refine mode),
		// return minimal response with just the draft info
		h.mu.Lock()
		h.revision++
		rev := h.revision
		h.mu.Unlock()
		h.sendJSON(w, 200, map[string]interface{}{
			"draft":     draft,
			"unit_name": unitName,
			"revision":  rev,
		})
		return
	}

	// Re-serialize the updated snapshot for the re-render pipeline
	snapData, err := json.Marshal(snap)
	if err != nil {
		h.sendError(w, 500, "failed to serialize updated snapshot")
		return
	}

	// Call the re-render function with the updated snapshot.
	// Pass nil for origData — the original snapshot hasn't changed.
	result, err := h.reRenderFn(snapData, nil, h.outputDir)
	if err != nil {
		h.sendError(w, 500, "re-render failed: "+err.Error())
		return
	}

	h.mu.Lock()
	h.revision++
	h.renderID = generateRenderID()
	rev := h.revision
	rid := h.renderID
	h.mu.Unlock()

	h.sendJSON(w, 200, map[string]interface{}{
		"html":            result.HTML,
		"snapshot":        result.Snapshot,
		"containerfile":   result.Containerfile,
		"triage_manifest": result.TriageManifest,
		"render_id":       rid,
		"revision":        rev,
	})
}
```

Note: This requires importing the `renderer` and `inspector` packages. However, `refine` already imports `renderer`. For `inspector`, we need to either move `extractQuadletPortsAndVolumes` to a shared location or make it exported. The simplest path is to export it from `inspector`:

In `container.go`, rename `extractQuadletPortsAndVolumes` to `ExtractQuadletPortsAndVolumes` (capitalize):

```go
func ExtractQuadletPortsAndVolumes(content string) (ports, volumes []string) {
```

And update the call site in `scanQuadletDir` to use `ExtractQuadletPortsAndVolumes`.

- [ ] **Step 2: Write server test**

Tests use `httptest.NewServer(handler)`, which calls `ServeHTTP` — this exercises the real dispatch path including the `switch` routing. The success test now validates the full rebuild response shape.

```go
// In server_test.go
func TestHandleQuadletDraft(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Containers = &schema.ContainerSection{
		RunningContainers: []schema.RunningContainer{
			{
				Name:  "webapp",
				Image: "registry.example.com/webapp:latest",
				Ports: map[string]interface{}{
					"8080/tcp": []interface{}{
						map[string]interface{}{"HostIp": "0.0.0.0", "HostPort": "8080"},
					},
				},
			},
		},
	}

	outDir := t.TempDir()
	schema.SaveSnapshot(snap, filepath.Join(outDir, "inspection-snapshot.json"))

	// newRefineHandler returns http.Handler (not *refineHandler).
	// Use httptest.NewServer(handler) directly — the handler's
	// ServeHTTP dispatches via the manual switch, exercising
	// the real route path including the /api/quadlet-draft case.
	handler := newRefineHandler(outDir, nil)
	srv := httptest.NewServer(handler)
	defer srv.Close()

	body := `{"container_name":"webapp"}`
	resp, err := http.Post(srv.URL+"/api/quadlet-draft", "application/json", strings.NewReader(body))
	if err != nil {
		t.Fatalf("POST /api/quadlet-draft: %v", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != 200 {
		t.Fatalf("status = %d, want 200", resp.StatusCode)
	}

	var result map[string]interface{}
	json.NewDecoder(resp.Body).Decode(&result)

	// When reRenderFn is nil (no refine pipeline in test), response
	// falls back to minimal format with draft + unit_name + revision.
	// When reRenderFn is provided, response is full rebuild format
	// (html, snapshot, containerfile, triage_manifest, render_id, revision).
	// Either way, verify the draft was generated correctly.
	if draft, ok := result["draft"].(string); ok {
		if !strings.Contains(draft, "Image=registry.example.com/webapp:latest") {
			t.Error("draft should contain the Image directive")
		}
	} else if _, hasHTML := result["html"]; hasHTML {
		// Full rebuild response — snapshot field contains the updated snapshot
		// with the generated draft
	}

	// Verify it was added to the snapshot with Include: false
	reloaded, _ := schema.LoadSnapshot(filepath.Join(outDir, "inspection-snapshot.json"))
	found := false
	for _, u := range reloaded.Containers.QuadletUnits {
		if u.Name == "webapp.container" && u.Generated {
			found = true
			if u.Include {
				t.Error("generated quadlet unit should have Include=false (operator must review)")
			}
			break
		}
	}
	if !found {
		t.Error("generated quadlet unit should be saved in snapshot")
	}
}

func TestHandleQuadletDraft_DuplicateSuppression(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Containers = &schema.ContainerSection{
		RunningContainers: []schema.RunningContainer{
			{Name: "webapp", Image: "webapp:latest"},
		},
		QuadletUnits: []schema.QuadletUnit{
			{Name: "webapp.container", Generated: true, Content: "existing"},
		},
	}

	outDir := t.TempDir()
	schema.SaveSnapshot(snap, filepath.Join(outDir, "inspection-snapshot.json"))

	handler := newRefineHandler(outDir, nil)
	srv := httptest.NewServer(handler)
	defer srv.Close()

	body := `{"container_name":"webapp"}`
	resp, err := http.Post(srv.URL+"/api/quadlet-draft", "application/json", strings.NewReader(body))
	if err != nil {
		t.Fatalf("POST /api/quadlet-draft: %v", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != 409 {
		t.Errorf("status = %d, want 409 for duplicate draft", resp.StatusCode)
	}
}

func TestHandleQuadletDraft_MissingImage(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Containers = &schema.ContainerSection{
		RunningContainers: []schema.RunningContainer{
			{Name: "noimage"},  // No Image field
		},
	}

	outDir := t.TempDir()
	schema.SaveSnapshot(snap, filepath.Join(outDir, "inspection-snapshot.json"))

	handler := newRefineHandler(outDir, nil)
	srv := httptest.NewServer(handler)
	defer srv.Close()

	body := `{"container_name":"noimage"}`
	resp, err := http.Post(srv.URL+"/api/quadlet-draft", "application/json", strings.NewReader(body))
	if err != nil {
		t.Fatalf("POST /api/quadlet-draft: %v", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != 422 {
		t.Errorf("status = %d, want 422 for missing image", resp.StatusCode)
	}
}
```

- [ ] **Step 3: Run test**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/refine/... -run "TestHandleQuadletDraft" -v`
Expected: PASS (after implementation)

- [ ] **Step 4: Add Generate Quadlet Draft button to SPA**

In `report.html`, add the draft button inside `buildInfoReadOnlyCard`.

**Card path analysis (revision 4 correction):** Running containers are `DisplayOnly: true` in the live classifier (`triage.go`). In the live `renderTriageSection`, the card-type dispatch checks `display_only` before the toggle path:

```javascript
} else if (uItem.display_only) {
  // Display-only: no toggle, informational only
  card = buildInfoReadOnlyCard(uItem);
}
```

Running containers route through `buildInfoReadOnlyCard`, NOT `buildToggleCard`. The draft button must be added inside `buildInfoReadOnlyCard`.

In `buildInfoReadOnlyCard`, after the reason element (and before the `return card;`), add the draft button for running-container items:

```javascript
  // Generate Quadlet Draft button for running containers (refine mode only)
  if (item.key.indexOf('container-') === 0 && App.mode === 'refine') {
    var containerName = item.key.substring(10);

    // Check if draft already exists (duplicate suppression)
    var draftExists = false;
    var snap = App.snapshot;
    if (snap && snap.containers && snap.containers.quadlet_units) {
      for (var qi = 0; qi < snap.containers.quadlet_units.length; qi++) {
        if (snap.containers.quadlet_units[qi].name === containerName + '.container' &&
            snap.containers.quadlet_units[qi].generated) {
          draftExists = true;
          break;
        }
      }
    }

    // Check if container has no image (missing-image guard)
    var hasImage = true;
    if (snap && snap.containers && snap.containers.running_containers) {
      for (var ri = 0; ri < snap.containers.running_containers.length; ri++) {
        if (snap.containers.running_containers[ri].name === containerName) {
          if (!snap.containers.running_containers[ri].image) {
            hasImage = false;
          }
          break;
        }
      }
    }

    var draftBtn = document.createElement('button');
    draftBtn.type = 'button';
    draftBtn.style.cssText = 'margin-top:8px;padding:6px 16px;border-radius:6px;border:1px solid var(--pf-t--global--border--color--default);background:transparent;color:inherit;font-size:13px;cursor:pointer;opacity:0.8;';
    draftBtn.setAttribute('tabindex', '0');

    if (draftExists) {
      // Draft already generated — show disabled state
      draftBtn.textContent = 'Draft generated';
      draftBtn.disabled = true;
      draftBtn.style.opacity = '0.5';
      draftBtn.style.cursor = 'default';
    } else if (!hasImage) {
      // Missing image — show inline error
      draftBtn.textContent = 'Cannot generate draft — container image unknown';
      draftBtn.disabled = true;
      draftBtn.style.opacity = '0.5';
      draftBtn.style.cursor = 'default';
      draftBtn.style.color = 'var(--pf-t--global--color--status--danger--default, #c9190b)';
    } else {
      draftBtn.textContent = 'Generate Quadlet Draft';
      draftBtn.setAttribute('aria-label', 'Generate quadlet draft for ' + (item.name || item.key));
      draftBtn.onclick = function(e) {
        e.stopPropagation();
        draftBtn.disabled = true;
        draftBtn.textContent = 'Generating...';

        var xhr = new XMLHttpRequest();
        xhr.open('POST', '/api/quadlet-draft', true);
        xhr.setRequestHeader('Content-Type', 'application/json');
        xhr.onload = function() {
          if (xhr.status === 200) {
            // Draft endpoint returns full rebuild response —
            // apply it directly, no separate render call needed.
            var data = JSON.parse(xhr.responseText);
            applyRebuildResponse(data);
            updateNonRpmSidebarBadge();

            // Show post-click message
            draftBtn.textContent = 'Draft generated — see Quadlet Units above';
            draftBtn.style.opacity = '0.5';
            draftBtn.style.cursor = 'default';

            // Scroll to the new quadlet entry and focus it
            requestAnimationFrame(function() {
              var newEntry = document.querySelector('[data-key="quadlet-' + containerName + '.container"]');
              if (newEntry) {
                newEntry.scrollIntoView({behavior: 'smooth', block: 'center'});
                var focusTarget = newEntry.querySelector('[tabindex="0"]');
                if (focusTarget) focusTarget.focus();
              }
            });
          } else if (xhr.status === 409) {
            // Duplicate — already generated
            draftBtn.textContent = 'Draft generated';
            draftBtn.style.opacity = '0.5';
          } else if (xhr.status === 422) {
            // Missing image
            draftBtn.textContent = 'Cannot generate draft — container image unknown';
            draftBtn.style.color = 'var(--pf-t--global--color--status--danger--default, #c9190b)';
          } else {
            draftBtn.textContent = 'Failed — retry';
            draftBtn.disabled = false;
          }
        };
        xhr.onerror = function() {
          draftBtn.textContent = 'Failed — retry';
          draftBtn.disabled = false;
        };
        xhr.send(JSON.stringify({container_name: containerName}));
      };
    }
    card.appendChild(draftBtn);
  }
```

Note: No `loadSnapshotAndRerender` helper is needed. The draft endpoint returns the full rebuild response directly, so the SPA calls `applyRebuildResponse()` inline in the `xhr.onload` handler. This eliminates the stale-snapshot race — `App.snapshot` is updated from the server's response (which includes the newly generated draft) before any re-render occurs.

- [ ] **Step 5: Run all tests**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/refine/... -v && go test ./internal/renderer/... -v`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add cmd/inspectah/internal/refine/server.go cmd/inspectah/internal/refine/server_test.go cmd/inspectah/internal/inspector/container.go cmd/inspectah/internal/renderer/static/report.html
git commit -m "feat(refine): generate quadlet draft from running containers

POST /api/quadlet-draft accepts a container_name, generates a
.container quadlet unit from the running container's inspect data,
and saves it to the snapshot as a Generated unit. SPA shows a
'Generate Quadlet Draft' button on running container cards.

Assisted-by: Claude Code"
```

---

### Task 18: Containers section visual hierarchy — subsection headers + empty states

**Files:**
- Modify: `cmd/inspectah/internal/renderer/static/report.html`
- Test: `cmd/inspectah/internal/renderer/html_test.go`

> **Blocker 4 fix:** The sort/group logic works with the live `renderTriageSection`
> path. The existing renderer groups items by their `group` field within each tier.
> Container subsections use `sub:quadlet`, `sub:flatpak`, `sub:compose` as group
> values. Running containers are ungrouped (no `sub:` prefix, individual cards).
>
> The approved visual hierarchy is:
> Quadlets (sub:quadlet) → Flatpaks (sub:flatpak) → Running containers (ungrouped)
> → Compose (sub:compose)
>
> The custom sort inserts before the existing `groupNames` iteration. Ungrouped items
> render after all `sub:` groups because the existing code renders ungrouped items
> first, then grouped items. To produce the approved hierarchy, we reverse this order
> for the containers section only: render grouped items in subsection order FIRST,
> then ungrouped items (running containers) AFTER the flatpak group.
>
> Subsection header elements (`<div class="container-subsection-header">`) are
> inserted before each group's cards by the rendering loop itself, keyed on the
> `sub:` group name.
>
> **Compose items and the informational tier (revision 3 fix):**
> Compose items previously used `DisplayOnly: true`, which routes them into the
> informational tier (rendered AFTER the main grouped/ungrouped loop). This strands
> them outside the four-subsection hierarchy. In revision 3, compose items use
> `CardType: "compose-info"` instead, which keeps them in the main loop where
> subsection ordering places them as the fourth peer. The card-selection logic
> routes `card_type === 'compose-info'` to `buildComposeInfoCard()`.
>
> **Running-container empty-state predicate (revision 3 fix):**
> Go serializes nil `RunningContainers` as JSON `null`. In JS, `null !== undefined`
> is `true`, so the revision 2 predicate `running_containers !== undefined` mislabels
> "not queried" as "no containers found." Revision 3 uses `=== null` for "not queried"
> (`--query-podman` not used) and checks for empty array for "queried, none found."
>
> **Empty-state messages:**
> - Running containers: "No running containers detected. Run inspectah with
>   --query-podman to inspect running workloads." (when `running_containers === null`)
>   vs. "No running containers found." (when `running_containers` is `[]`)
> - Containers section overall: "No container workloads detected." when all
>   subsections are empty (quadlets + flatpaks + running + compose all empty)
>
> **Blocker 5 note — Go tests verify:** subsection header CSS class and label text
> are present in rendered HTML, custom sort produces correct group ordering.
> **Browser verification needed:** visual hierarchy rendering order, subsection
> header styling, spacing between subsections.

- [ ] **Step 1: Add subsection header styling**

In the `<style>` section, add:

```css
/* Container subsection headers */
.container-subsection-header {
  font-size: 13px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.5px;
  opacity: 0.6;
  margin: 16px 0 8px 0;
  padding-bottom: 4px;
  border-bottom: 1px solid var(--pf-t--global--border--color--default);
}
.container-subsection-header:first-child {
  margin-top: 0;
}
```

- [ ] **Step 2: Enhance grouped item rendering for container subsections**

The existing `renderTriageSection` renders items in this order within each tier:
1. Ungrouped items (individual cards) — rendered first
2. Grouped items (accordion groups) — rendered second, sorted alphabetically by group name

For the containers section, we need the approved hierarchy:
**Quadlet Units → Flatpak Apps → Running Containers → Compose Files**

This means: grouped `sub:` items render in a specific order, and ungrouped items (running containers) render between `sub:flatpak` and `sub:compose`.

In `renderTriageSection`, find the section where `groupNames` are iterated (inside the `isExpanded` block where `groupedMap` is built). Replace the simple `groupNames.sort()` with a containers-specific ordering:

```javascript
      // Default alphabetical sort for non-container sections
      var groupNames = Object.keys(groupedMap).sort();

      // Container section: approved visual hierarchy
      if (sectionId === 'containers') {
        var subsectionOrder = {'sub:quadlet': 0, 'sub:flatpak': 1, 'sub:compose': 3};
        // sub:compose is 3, leaving slot 2 for ungrouped running containers
        groupNames.sort(function(a, b) {
          var oa = subsectionOrder[a] !== undefined ? subsectionOrder[a] : 2;
          var ob = subsectionOrder[b] !== undefined ? subsectionOrder[b] : 2;
          return oa - ob;
        });
      }
```

Then modify the rendering loop for the containers section. Find the loop `for (var g = 0; g < groupNames.length; g++)` and within the containers section, insert subsection headers before each group:

```javascript
        // Subsection headers for containers section
        if (sectionId === 'containers') {
          var subsectionLabels = {
            'sub:quadlet': 'Quadlet Units',
            'sub:flatpak': 'Flatpak Apps',
            'sub:compose': 'Compose Files'
          };
          var headerLabel = subsectionLabels[groupNames[g]];
          if (headerLabel) {
            var subHeader = document.createElement('div');
            subHeader.className = 'container-subsection-header';
            subHeader.textContent = headerLabel;
            itemsDiv.appendChild(subHeader);
          }
        }
```

Insert this inside the `for` loop, before the accordion/group rendering for each group.

For ungrouped items (running containers), modify the ungrouped rendering in the containers section to insert a "Running Containers" header before them and render them AFTER the `sub:flatpak` group. In the containers section, move the ungrouped items rendering to after the `sub:flatpak` group in the iteration:

```javascript
      // Containers section: render ungrouped (running containers) between
      // flatpak and compose groups
      if (sectionId === 'containers' && ungrouped.length > 0) {
        var rcHeader = document.createElement('div');
        rcHeader.className = 'container-subsection-header';
        rcHeader.textContent = 'Running Containers';
        itemsDiv.appendChild(rcHeader);

        for (var u = 0; u < ungrouped.length; u++) {
          var uItem = ungrouped[u];
          // ... same card-building logic as existing ungrouped path ...
        }
      }
```

Move the existing ungrouped rendering to AFTER the `sub:flatpak` group for the containers section. For non-containers sections, ungrouped items render first (existing behavior).

- [ ] **Step 3: Add running-container empty-state**

In the containers section rendering, after the ungrouped items block, add an empty-state check for running containers:

```javascript
      // Running container empty state
      // PREDICATE FIX: Go serializes nil RunningContainers as JSON null.
      // In JS, null !== undefined is true, so using !== undefined
      // mislabels "not queried" as "no containers found."
      // Use === null for "not queried" (--query-podman not used),
      // and check for empty array for "queried, none found."
      if (sectionId === 'containers') {
        var snap = App.snapshot;
        var hasRunningContainerItems = ungrouped.length > 0;
        if (!hasRunningContainerItems) {
          var rcEmpty = document.createElement('div');
          rcEmpty.className = 'container-subsection-header';
          rcEmpty.textContent = 'Running Containers';
          itemsDiv.appendChild(rcEmpty);

          var emptyMsg = document.createElement('div');
          emptyMsg.style.cssText = 'padding:12px 16px;font-size:13px;opacity:0.6;';
          if (snap && snap.containers && snap.containers.running_containers === null) {
            // null = --query-podman was not used (Go nil → JSON null)
            emptyMsg.textContent = 'No running containers detected. Run inspectah with --query-podman to inspect running workloads.';
          } else {
            // [] or populated = --query-podman was used but no containers found
            emptyMsg.textContent = 'No running containers found.';
          }
          itemsDiv.appendChild(emptyMsg);
        }
      }
```

- [ ] **Step 3b: Add containers section overall empty state**

When all four subsections are empty (no quadlets, no flatpaks, no running containers, no compose files), show `No container workloads detected.` instead of the subsection headers with individual empty messages.

In `renderTriageSection`, for the containers section, add an overall empty-state check BEFORE the subsection rendering. This goes after the `allItems` are filtered for the section:

```javascript
    // Containers section overall empty state
    if (sectionId === 'containers' && allItems.length === 0) {
      var emptyMsg = document.createElement('div');
      emptyMsg.style.cssText = 'padding:24px;text-align:center;opacity:0.6;font-size:14px;';
      emptyMsg.textContent = 'No container workloads detected.';
      container.appendChild(emptyMsg);
      return;
    }
```

This replaces the generic "No items found in this section." message for the containers section specifically.

- [ ] **Step 4: Write the test**

```go
// In html_test.go
func TestRenderHTML_ContainerSubsectionHeaders(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Containers = &schema.ContainerSection{
		QuadletUnits: []schema.QuadletUnit{
			{Name: "web.container", Image: "web:latest", Include: true},
		},
		FlatpakApps: []schema.FlatpakApp{
			{AppID: "org.example.app", Origin: "flathub", Branch: "stable", Include: true},
		},
		ComposeFiles: []schema.ComposeFile{
			{Path: "opt/dc.yml", Images: []schema.ComposeService{{Service: "svc", Image: "img"}}, Include: true},
		},
	}

	containerfile := "FROM rhel-bootc:9.4\n"
	html := goldenTestHelper(t, snap, containerfile)

	if !strings.Contains(html, "container-subsection-header") {
		t.Error("HTML should contain subsection header CSS class")
	}
	// Verify subsection labels exist in the HTML (they are JS-generated,
	// so we verify the strings that the JS will render are present in the
	// template code)
	if !strings.Contains(html, "Quadlet Units") {
		t.Error("HTML should contain 'Quadlet Units' subsection label")
	}
	if !strings.Contains(html, "Flatpak Apps") {
		t.Error("HTML should contain 'Flatpak Apps' subsection label")
	}
	if !strings.Contains(html, "Compose Files") {
		t.Error("HTML should contain 'Compose Files' subsection label")
	}
}

func TestRenderHTML_RunningContainerEmptyState(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Containers = &schema.ContainerSection{
		// running_containers is nil — --query-podman was not used
	}

	containerfile := "FROM rhel-bootc:9.4\n"
	html := goldenTestHelper(t, snap, containerfile)

	if !strings.Contains(html, "query-podman") {
		t.Error("HTML should contain --query-podman guidance when running container data is absent")
	}
}
```

- [ ] **Step 5: Run HTML tests**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/renderer/... -run "TestRenderHTML" -v`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add cmd/inspectah/internal/renderer/static/report.html cmd/inspectah/internal/renderer/html_test.go
git commit -m "feat(spa): container subsection hierarchy with headers and empty states

Subsections render in approved order: Quadlet Units, Flatpak Apps,
Running Containers, Compose Files. Each subsection gets a header
element. Running container empty state distinguishes '--query-podman
not used' from 'no containers found.'

Assisted-by: Claude Code"
```

---

### Task 19: Final integration test

**Files:**
- Test: `cmd/inspectah/internal/renderer/html_test.go`

> **Blocker 5 fix (revision 3):** This test renders actual Containerfile output from
> a snapshot by calling the real renderer functions (`containersSectionLines`,
> `nonRpmSectionLines`), not `goldenTestHelper` with an embedded string.
> `goldenTestHelper` embeds a caller-supplied Containerfile string — it cannot prove
> non-RPM stub generation or compose-zero-output from real data because the string
> is provided, not rendered.
>
> **What this test proves (Go-testable):**
> - Non-RPM items with `review_status="migration_planned"` produce Containerfile stubs (rendered from snapshot)
> - Compose items produce ZERO Containerfile output (no lines at all — not just no COPY, also no comments)
> - Flatpak included apps produce manifest/service references in Containerfile output (rendered from snapshot)
> - Non-RPM items with other statuses produce no Containerfile output
> - Containers and Non-RPM sections appear in rendered HTML (via `goldenTestHelper` for HTML structure)
> - Review-status card builder function is present in JS
> - Subsection label strings are present in JS
>
> **What this test does NOT prove (needs Playwright/manual):**
> - Subsection visual ordering (quadlet → flatpak → running → compose)
> - Radio-group keyboard navigation (Left/Right arrow keys)
> - Focus management after status change, draft generation, or expand/collapse
> - Post-click button state transitions
> - Scroll-to-new-entry after draft generation
> - Empty-state conditional rendering (JS-evaluated)
> - Sidebar badge count updates

- [ ] **Step 1: Write integration test**

```go
// In html_test.go
func TestRenderHTML_FullContainersAndNonRpm(t *testing.T) {
	// This test has two parts:
	// 1. HTML structure verification via goldenTestHelper (section presence, JS functions)
	// 2. Containerfile output verification via real renderer calls (not embedded strings)
	//
	// Part 1 is necessary to verify SPA structure. Part 2 is what revision 3 adds —
	// it calls the real renderer functions to prove output correctness from data.

	snap := schema.NewSnapshot()
	snap.Containers = &schema.ContainerSection{
		QuadletUnits: []schema.QuadletUnit{
			{Name: "webapp.container", Image: "webapp:latest", Include: true,
				Ports: []string{"8080:8080"}, Volumes: []string{"data:/data"}},
		},
		FlatpakApps: []schema.FlatpakApp{
			{AppID: "org.mozilla.firefox", Origin: "flathub", Branch: "stable",
				Include: true, Remote: "flathub", RemoteURL: "https://dl.flathub.org/repo/"},
		},
		RunningContainers: []schema.RunningContainer{
			{Name: "orphan", Image: "orphan:latest"},
		},
		ComposeFiles: []schema.ComposeFile{
			{Path: "opt/app/docker-compose.yml", Images: []schema.ComposeService{
				{Service: "web", Image: "nginx"},
			}, Include: true},
		},
	}
	snap.NonRpmSoftware = &schema.NonRpmSoftwareSection{
		Items: []schema.NonRpmItem{
			{Path: "opt/tool", Name: "tool", Method: "standalone binary",
				Confidence: "high", ReviewStatus: "migration_planned",
				Static: true, Lang: "go"},
			{Path: "opt/reviewed", Name: "reviewed", Method: "standalone binary",
				Confidence: "high", ReviewStatus: "reviewed"},
			{Path: "opt/unreviewed", Name: "unreviewed", Method: "standalone binary",
				Confidence: "high", ReviewStatus: "not_reviewed"},
		},
	}

	// --- Part 1: HTML structure checks via goldenTestHelper ---
	containerfile := "FROM rhel-bootc:9.4\n"
	html := goldenTestHelper(t, snap, containerfile)

	if !strings.Contains(html, "Containers") {
		t.Error("should contain Containers section")
	}
	if !strings.Contains(html, "Non-RPM Software") {
		t.Error("should contain Non-RPM Software section")
	}
	if !strings.Contains(html, "first boot") {
		t.Error("should contain flatpak first-boot annotation")
	}
	if !strings.Contains(html, "buildReviewStatusCard") {
		t.Error("should contain buildReviewStatusCard function in JS")
	}
	if !strings.Contains(html, "buildComposeInfoCard") {
		t.Error("should contain buildComposeInfoCard function in JS")
	}
	if !strings.Contains(html, "Quadlet Units") {
		t.Error("should contain 'Quadlet Units' subsection label")
	}
	if !strings.Contains(html, "Running Containers") {
		t.Error("should contain 'Running Containers' subsection label")
	}
	if !strings.Contains(html, "No container workloads detected") {
		t.Error("should contain containers overall empty-state text")
	}

	// --- Part 2: Real Containerfile output from snapshot ---
	// This calls the actual renderer functions — NOT goldenTestHelper
	// with an embedded string. Proves output correctness from data.

	// Non-RPM: migration_planned items produce stubs
	nrLines := nonRpmSectionLines(snap, nil, false)
	nrContent := strings.Join(nrLines, "\n")
	if !strings.Contains(nrContent, "COPY opt/tool") {
		t.Error("non-RPM migration_planned item should produce COPY stub")
	}
	// Non-RPM: reviewed and not_reviewed items produce NO output
	if strings.Contains(nrContent, "reviewed") {
		t.Error("non-RPM 'reviewed' item should not appear in Containerfile output")
	}
	if strings.Contains(nrContent, "unreviewed") {
		t.Error("non-RPM 'not_reviewed' item should not appear in Containerfile output")
	}

	// Compose: produces ZERO output (no lines at all)
	cLines := containersSectionLines(snap)
	cContent := strings.Join(cLines, "\n")
	if strings.Contains(cContent, "compose") || strings.Contains(cContent, "docker-compose") {
		t.Error("compose should produce ZERO Containerfile output — no lines, no comments, nothing")
	}

	// Flatpak: included apps produce manifest/service references
	if !strings.Contains(cContent, "flatpak") {
		t.Error("included flatpak apps should produce Containerfile output")
	}
	if !strings.Contains(cContent, "flatpak-provision.service") {
		t.Error("included flatpak apps should reference the oneshot service")
	}
}
```

- [ ] **Step 2: Write Containerfile output verification tests (standalone)**

```go
func TestContainerfile_ComposeProducesZeroOutput(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Containers = &schema.ContainerSection{
		ComposeFiles: []schema.ComposeFile{
			{Path: "opt/app/docker-compose.yml", Images: []schema.ComposeService{
				{Service: "web", Image: "nginx"},
			}, Include: true},
		},
	}

	lines := containersSectionLines(snap)

	// Compose produces ZERO output — not just no COPY, also no comments.
	// The entire lines slice should be empty when compose is the only
	// container workload.
	if len(lines) != 0 {
		t.Errorf("compose should produce zero Containerfile lines, got %d: %v",
			len(lines), lines)
	}
}

func TestContainerfile_FlatpakRetryDirectives(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Containers = &schema.ContainerSection{
		FlatpakApps: []schema.FlatpakApp{
			{AppID: "org.example.app", Origin: "custom", Branch: "stable",
				Include: true, Remote: "custom", RemoteURL: "https://example.com/repo"},
		},
	}

	outDir := t.TempDir()
	WriteConfigTree(snap, outDir)

	servicePath := filepath.Join(outDir, "flatpak", "flatpak-provision.service")
	svcData, err := os.ReadFile(servicePath)
	if err != nil {
		t.Fatalf("flatpak service not written: %v", err)
	}
	svcContent := string(svcData)

	// Verify retry directives
	if !strings.Contains(svcContent, "Restart=on-failure") {
		t.Error("service must have bounded retry via Restart=on-failure")
	}
	if !strings.Contains(svcContent, "StartLimitBurst=3") {
		t.Error("service must have StartLimitBurst=3")
	}
}

func TestQuadletDraft_NonDefaultHostIpPortMapping(t *testing.T) {
	container := schema.RunningContainer{
		Name:  "bound",
		Image: "app:latest",
		Ports: map[string]interface{}{
			"8080/tcp": []interface{}{
				map[string]interface{}{"HostIp": "127.0.0.1", "HostPort": "8080"},
			},
		},
	}

	draft := GenerateQuadletDraft(container)

	if !strings.Contains(draft, "PublishPort=127.0.0.1:8080:8080") {
		t.Error("draft must preserve non-default HostIp in port mapping")
	}
}
```

- [ ] **Step 3: Run the integration test**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/renderer/... -run "TestRenderHTML_FullContainersAndNonRpm|TestContainerfile_ComposeProducesNoOutput" -v`
Expected: PASS

- [ ] **Step 4: Run ALL tests across the codebase**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./... -v`
Expected: PASS (all tests across all packages)

- [ ] **Step 5: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add cmd/inspectah/internal/renderer/html_test.go cmd/inspectah/internal/renderer/containerfile_test.go
git commit -m "test(renderer): integration test for containers and non-RPM sections

Verifies HTML structure: section presence, review-status card builder,
subsection labels, flatpak annotation. Verifies compose items produce
zero Containerfile output. Does NOT claim to verify client-side JS
behavior — see Browser verification sections in plan for what needs
Playwright/manual testing.

Assisted-by: Claude Code"
```

---

## Spec Coverage Verification

| Spec NEW WORK Marker | Plan Task |
|---|---|
| NonRpmItem ReviewStatus + Notes fields | Task 1 |
| QuadletUnit Ports, Volumes, Generated | Task 2 |
| FlatpakApp Remote, RemoteURL | Task 3 |
| SchemaVersion bump to 13 | Task 4 |
| Pip false-positive RPM ownership filter | Task 5 |
| Flatpak --system flag | Task 6 |
| Flatpak remote capture | Task 6 |
| Quadlet PublishPort/Volume parsing | Task 7 |
| Non-RPM section extraction (out of containers) | Task 8 |
| Flatpak classifier + SPA include handlers | Task 9 |
| Compose classifier (informational items) | Task 10 |
| Non-RPM Containerfile stubs (review_status gated) | Task 11 |
| Flatpak manifest + shell oneshot (no jq) | Task 12 |
| Quadlet draft (data-mapped restart policy) | Task 13 |
| SPA: nonrpm section + sidebar badge + empty states | Task 8 (merged) |
| SPA: review-status cards + visible ARIA radio | Task 14 |
| SPA: flatpak annotation (visual only) | Task 15 |
| SPA: compose informational cards (display-only) | Task 16 |
| SPA: quadlet draft button (Include:false, guards) | Task 17 |
| SPA: containers hierarchy + headers + empty states | Task 18 |

**Blocker coverage matrix (revision 4):**

| Blocker | Round 2 Finding | Tasks Fixed | What Changed |
|---------|----------------|-------------|-------------|
| B1: Non-RPM section plumbing | renderAllSections/updateAllBadges/navigateTo skip countBadge sections; heading/footer wrong format; badge not initialized | 8, 14 | Task 8 now atomic (classifier + SPA plumbing in one commit); Task 14 (formerly 15) adds review-status cards |
| B2: Flatpak retry + caveats | No retry logic in oneshot; SPA card missing operator caveats | 12, 15 | Task 12 adds systemd retry directives + test; Task 15 (formerly 16) adds 3 operator caveats |
| B3: Quadlet draft HostIp + TODOs + live seams | extractPortMappings ignores HostIp; missing deferred TODOs; Task 17 test/route/refresh wrong | 13, 17 | Task 13 preserves HostIp + adds deferred TODOs + tests; Task 17 (formerly 18) fixes: ServeHTTP switch case, buildInfoReadOnlyCard card path, full rebuild response (no stale snapshot) |
| B4: Compose output drift + null predicate + tier escape + empty state | containersSectionLines emits compose; null vs undefined; DisplayOnly strands compose; no overall empty state | 10, 12, 16, 18 | Task 10 uses CardType; Task 12 removes compose; Task 16 (formerly 17) uses dedicated builder; Task 18 (formerly 19) fixes predicate + adds overall empty state |
| B5: Verification story | goldenTestHelper embeds string; no retry/HostIp tests | 12, 13, 19 | Task 19 (formerly 20) uses real renderer calls; Tasks 12/13 add specific tests |

**Explicitly deferred (per spec):**
- Non-RPM payload export (non-rpm/ tree in tarball)
- Compose v2 features (per-service ports/volumes, expand-to-YAML)
- Quadlet draft: healthcheck, dependency ordering, user namespace mapping (now with explicit TODO comments in generated draft)
- Node.js .so scanning in node_modules/

**Post-implementation: browser verification checklist**

The following behaviors require Playwright or manual browser testing (Go tests
verify HTML structure only, not client-side JS execution):

1. Non-RPM radio-group: Left/Right arrow key navigation, Tab enter/exit, focus stays on active segment after status change
2. Non-RPM notes: blur-save via PUT /api/snapshot, collapsed preview of first line
3. Non-RPM sidebar badge: count updates on status change, badge populated on initial render and snapshot load
4. Non-RPM section heading: updates reviewed count dynamically (approved format: `Non-RPM Software (X of Y reviewed)`)
5. Non-RPM section footer: suppressed (planning worksheet, not decision checklist)
6. Non-RPM empty states: correct message shown based on snapshot data
7. Flatpak annotation: visible across both themes, not clipped on narrow viewport; operator caveats (network, best-effort, manual setup) visible
8. Compose cards: muted styling via buildComposeInfoCard, service inventory visible, no interactive affordances; renders in fourth subsection position (not informational tier)
9. Generate Quadlet Draft: post-click button transition via POST /api/quadlet-draft full rebuild response + applyRebuildResponse(), scroll-to-new-entry, focus management to new quadlet, disabled state on duplicate, inline error on missing image
10. Container subsections: visual ordering matches approved hierarchy, subsection headers render with correct spacing
11. Running container empty state: distinguishes `null` (--query-podman not used) from `[]` (no containers found)
12. Containers overall empty state: shows `No container workloads detected.` when all subsections are empty
13. Screen reader: aria-live announcements on status change, aria-checked toggle
14. Non-default HostIp: port mappings with 127.0.0.1 bind show correctly in quadlet draft preview
