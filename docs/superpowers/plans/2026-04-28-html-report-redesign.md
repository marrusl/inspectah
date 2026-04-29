# HTML Report Redesign Implementation Plan (Revision 7)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the 442-line stub HTML renderer with a full-featured interactive single-page application using PatternFly v6, CodeMirror 6, and a guided triage interaction model.

**Architecture:** Go renderer classifies snapshot items into triage tiers (Go-side, table-tested), stamps the snapshot JSON and a pre-computed triage manifest into an `html/template`. The SPA consumes the manifest for display — it never runs its own classification logic. PatternFly CSS and CodeMirror JS are vendored via `go:embed`. The refine server API adds revision tracking, render_id-bound tarball downloads, and PUT-based autosave with revision guards. The server owns the immutable original snapshot sidecar. The Containerfile preview shows the last server-rendered version with a change-counter badge; no client-side Containerfile generation.

**Tech Stack:** Go 1.22+, html/template, go:embed, PatternFly v6, CodeMirror 6, vanilla JavaScript

**Branch:** `go-port` (the Go-native port branch)

**Spec:** `docs/specs/proposed/2026-04-28-html-report-redesign.md`

**Prerequisites:**
- [x] ~~Update the approved spec's "continuous Containerfile updates" language to match the badge-preview contract.~~ Applied: preview contract + display-only surfaces + resting-state labels all updated in the spec.

**Review history:** Revisions 1-2 reviewed by Kit, Thorn, Slate, Fern. Revisions 3-4 reviewed by Kit, Thorn (narrowed). Revision 5 round-4. Revision 6 stale-artifact cleanup. Revision 7 addresses Kit rounds 5-6: config/ is renderer-owned, clean-before-render removes ALL outputs including config/, disappearance proof covers config/ + redacted/ in working dir and tarball.

---

## Revision 7 Changes from Rounds 5-6

| Finding | Source | Resolution |
|---------|--------|------------|
| `config/` treated as input data but is renderer-owned output from `writeConfigTree()` | Kit R6 | `cleanRendererOutputs` no longer preserves `config/`. Only snapshot + sidecar are preserved. `renderer.RunAll()` regenerates `config/` from snapshot data via `writeConfigTree()`. |
| Repeated-rerender proof incomplete on `config/` outputs | Kit R6 | Render N fixture adds a config file (`/etc/httpd/conf/httpd.conf`). Named assertions prove `config/etc/httpd/` exists after render N and is absent (file + dir) from both working directory and tarball after render N+1. |
| Ground-truth render not refine-mode-equivalent (missing OriginalSnapshotPath) | Kit R5 | Both ground-truth `renderer.RunAll()` calls pass `OriginalSnapshotPath` with copied sidecar. |
| Disappearance proof doesn't cover tarball export | Kit R5 | Tarball extracted and checked for stale `redacted/`, stale `config/`, and sidecar. |

## Revision 6 Changes

| Finding | Source | Resolution |
|---------|--------|------------|
| `syncRenderedOutput()` only adds/overwrites — never removes files the new render didn't produce | Kit | Addressed in Rev 6, further tightened in Rev 7 with pre-render cleanup + directory removal. |
| No test proves artifacts disappear across rerenders with different snapshots | Kit | Addressed in Rev 6, further tightened in Rev 7 with guaranteed-disappearing fixture. |
| Stale "continuous Containerfile preview" language in spec | Fern | Three references updated: section layout table, rebuild flow step 1, and out-of-scope diff rationale. |
| Display-only card language and aria-labels inconsistent | Fern | Spec display-only section already uses "Acknowledge / Skip". Added explicit aria-label guidance for display-only cards. |
| Future guided queue mode compatibility not documented | — | Brief note added to Descoped section. No scope change — just a "don't close the door" constraint on card component, decision state, and manifest design. |

## Revision 5 Changes from Round 4

| Finding | Source | Resolution |
|---------|--------|------------|
| Success-path swap only covers 3 files, not full renderer output | Kit R4 | `syncRenderedOutput()` replaces ALL renderer-owned files (walks entire temp dir, skips sidecar only). No hardcoded file list — covers README, audit-report, kickstart, secrets-review, redacted/, drop-ins/, quadlet/ automatically. |
| Proof sketches mock past real nativeReRender/RunAll path | Kit R4 | New `TestNativeReRender_ProducesCanonicalOutput` in `refine_test.go` exercises the REAL `nativeReRender` with `renderer.RunAll()` in the loop. Verifies full output set, response==disk, and sidecar immutability. |
| `users_groups.groups[]` overstated as "already handled" | Kit R4 | Field table corrected: groups marked "No (renderer only consumes users, not groups)". Groups added to display-only surface list. |
| Display-only contract buried in renderer notes, not in spec/UI | Kit R4 | Display-only surfaces now listed as a required spec update (prerequisite). Triage cards for display-only types use "Acknowledge / Skip" instead of "Include / Leave out". Fern confirmation needed on card language. |
| Spec prerequisite is a real gate | Thorn R4 | Spec updates section expanded with both preview and display-only changes. Fern confirmation flagged. |

## Revision 4 Changes from Round 3

| Finding | Source | Resolution |
|---------|--------|------------|
| `*bool` still needs server-side normalization on load | Kit R3 | Phase 3 adds `NormalizeSnapshot()` — called in `RunRefine` after extraction. All nil `*bool` Include fields set to `boolPtr(true)`. SPA always sees fully normalized snapshot. Test proves v11 load + GET /api/snapshot returns explicit `true` on all typed Include fields. |
| Renderer follow-through stops at schema+SPA — browser can accept decisions artifacts ignore | Kit R3 | Phase 3 adds explicit "display-only contract" for NMConnection, FstabEntry, AtJob, RunningContainer with a table test proving Containerfile output is identical regardless of their Include state. Renderer follow-through for timers and SELinux booleans is unchanged. |
| SPA state machine lacks automated proof — prose + browser checkpoints insufficient | Thorn R3 | Phase 4-5 adds Verification Mapping table that connects every SPA behavior to a specific automated proof: classification → Go table tests, autosave/rebuild → server contract tests, review-state/409 → contract tests that exercise the same invariants the SPA relies on. Remaining browser-only items are explicitly listed with rationale. |
| E2E equality test still doesn't fully prove the invariant | Thorn R3 | Phase 2 E2E test restructured as chained assertions: response == disk == tarball for each artifact (snapshot, Containerfile, report.html). Mock uses `renderer.RunAll()` in test context for realistic coverage. |
| Preview contract diverges from approved spec | Kit R3 | Spec update added as prerequisite in plan header. Must happen before implementation. |
| Failed-render safety only protects snapshot, not full working dir | Kit R3 | Phase 2 `nativeReRender` now renders into a temp copy of the working directory and swaps on success. Test proves ALL files (snapshot, Containerfile, report.html) unchanged on renderer failure. |

## Revision 3 Changes from Round 2

| Finding | Source | Resolution |
|---------|--------|------------|
| v11→v12 migration unsafe — `bool` defaults to `false` | Kit R2 | `*bool` pointer model: nil=not-yet-decided, treated as included. Helper: `isIncluded(b *bool) bool` |
| Renderers don't filter by Include on newly triaged surfaces | Kit R2 | Phase 3 adds Include filtering to `scheduledTasksSectionLines` and `selinuxSectionLines` |
| Live preview under-scoped (covers 3 of 15+ sections) | Kit R2 | Live preview removed entirely. Badge pattern: "N changes pending — rebuild to update." Spec language updated. |
| Failed render can partially mutate working directory | Kit R2 | Phase 2 adds `nativeReRender` failure-safety test; render writes to temp then renames atomically |
| No automated proof for SPA state machine | Thorn R2 | Classification extracted to Go (`triage.go`) with table-driven tests. SPA consumes Go-produced manifest — no dual implementation. |
| E2E equality test spot-checks, doesn't prove full artifact set | Thorn R2 | Strengthened: proves snapshot, containerfile, report.html, AND tarball-extracted artifact set all match |

## Key Design Decisions (Rounds 1-3)

| Decision | Chosen | Alternatives Considered | Rationale |
|----------|--------|------------------------|-----------|
| Decision persistence | Extend schema with `*bool` Include | Separate `decisions.json` | Schema is the single source of truth; sidecar sync is a tax (Kit) |
| Revert to original | Descoped from v1 | Full server endpoint + client flow | User can re-run `inspectah refine` on original tarball; convenience, not capability (Kit) |
| Containerfile preview | Server-rendered + change badge | Client-side live preview | Partial preview creates false confidence; sysadmins prefer batch-and-review (Fern, Ember) |
| Tier classification | Go-extracted, SPA consumes manifest | JS-only classification | Single implementation, table-tested in Go, no dual-implementation drift (Thorn) |
| Original snapshot | Server-owned sidecar only | Client-controlled override | Immutable sidecar trust boundary (Slate) |

## Descoped from This Implementation

- **Revert to original.** User re-runs `inspectah refine` on original tarball. Backlogged for v2.
- **Live Containerfile preview.** Preview shows last server-rendered Containerfile with change-counter badge. Backlogged for v2 if users request it.
- **Promotion of untyped collections.** `Users/Groups`, `BooleanOverrides` remain `[]map[string]interface{}`. `CustomModules` remains `[]string`. Dynamic `include` key on maps. Backlogged.
- **Fleet prevalence-driven tier defaults.** Follow-up spec.
- **Automated browser CI tests.** No headless browser in CI.
- **Print stylesheet.** Not needed for v1.
- **Guided decision queue mode.** A card-based sequential walk-through (guided queue) that presents all decisions one at a time is being considered as a future addition. The current card component design, decision state model (`*bool` Include + untyped map key), and triage manifest structure (`[]TriageItem` with section/key/tier) should remain compatible with a queue-based consumption pattern — avoid coupling the card rendering or state management to the current section-grouped layout in ways that would prevent a linear traversal.

---

## File Structure

### Files to create

| Path | Responsibility |
|------|---------------|
| `cmd/inspectah/internal/renderer/static/report.html` | SPA shell (html/template, ~2-3K lines — simpler without live preview) |
| `cmd/inspectah/internal/renderer/static/patternfly.min.css` | PatternFly v6 vendored CSS (~200KB) |
| `cmd/inspectah/internal/renderer/static/codemirror.min.js` | CodeMirror 6 vendored bundle (~150KB) |
| `cmd/inspectah/internal/renderer/embed.go` | go:embed directives for static assets |
| `cmd/inspectah/internal/renderer/triage.go` | Go tier classification engine |
| `cmd/inspectah/internal/renderer/triage_test.go` | Table-driven classification tests |
| `cmd/inspectah/internal/renderer/testdata/golden-*.html` | Golden-file fragments |

### Files to modify

| Path | Changes |
|------|---------|
| `cmd/inspectah/internal/renderer/html.go` | Rewrite: template execution, embeds triage manifest |
| `cmd/inspectah/internal/renderer/html_test.go` | Rewrite: template, XSS, landmark, golden-file tests |
| `cmd/inspectah/internal/renderer/containerfile.go` | Add Include filtering to timers, SELinux booleans |
| `cmd/inspectah/internal/refine/server.go` | Revision tracking, PUT /api/snapshot, render_id, tarball guard, sidecar |
| `cmd/inspectah/internal/refine/server_test.go` | Contract tests, E2E equality, race test, failure-safety |
| `cmd/inspectah/internal/refine/tarball.go` | Exclusion-based tarball packaging |
| `cmd/inspectah/internal/cli/refine.go` | Server-owned sidecar, manifest in render response |
| `cmd/inspectah/internal/schema/types.go` | `*bool` Include on 5 types, schema v12 |
| `cmd/inspectah/internal/schema/snapshot.go` | v11→v12 migration compat |

---

## Schema Contract

### Include field model

All `Include` fields on newly-added types use `*bool`:

```go
Include *bool `json:"include,omitempty"`
```

**Semantics:**
- `nil` (absent from JSON) → not yet decided → treated as **included** (default-include behavior)
- `true` → explicitly included
- `false` → explicitly excluded

**Helper functions** (in `triage.go`):

```go
func isIncluded(b *bool) bool {
	return b == nil || *b
}

func boolPtr(v bool) *bool {
	return &v
}
```

**Server-side normalization on load** (in `schema/snapshot.go`):

The refine server calls `NormalizeSnapshot()` immediately after loading a snapshot from disk. This converts all nil `*bool` Include fields to explicit `true`, so the SPA always sees a fully normalized snapshot. This is a one-time operation at load time — after normalization, the snapshot is in v12 form regardless of its original version. Autosaved snapshots are already normalized.

```go
func NormalizeSnapshot(snap *InspectionSnapshot) {
	t := true

	if snap.ScheduledTasks != nil {
		for i := range snap.ScheduledTasks.SystemdTimers {
			if snap.ScheduledTasks.SystemdTimers[i].Include == nil {
				snap.ScheduledTasks.SystemdTimers[i].Include = &t
			}
		}
		for i := range snap.ScheduledTasks.AtJobs {
			if snap.ScheduledTasks.AtJobs[i].Include == nil {
				snap.ScheduledTasks.AtJobs[i].Include = &t
			}
		}
	}
	if snap.Containers != nil {
		for i := range snap.Containers.RunningContainers {
			if snap.Containers.RunningContainers[i].Include == nil {
				snap.Containers.RunningContainers[i].Include = &t
			}
		}
	}
	if snap.Network != nil {
		for i := range snap.Network.Connections {
			if snap.Network.Connections[i].Include == nil {
				snap.Network.Connections[i].Include = &t
			}
		}
	}
	if snap.Storage != nil {
		for i := range snap.Storage.FstabEntries {
			if snap.Storage.FstabEntries[i].Include == nil {
				snap.Storage.FstabEntries[i].Include = &t
			}
		}
	}

	// Untyped maps: set "include": true if absent
	if snap.UsersGroups != nil {
		for _, u := range snap.UsersGroups.Users {
			if _, ok := u["include"]; !ok {
				u["include"] = true
			}
		}
		for _, g := range snap.UsersGroups.Groups {
			if _, ok := g["include"]; !ok {
				g["include"] = true
			}
		}
	}
	if snap.Selinux != nil {
		for _, b := range snap.Selinux.BooleanOverrides {
			if _, ok := b["include"]; !ok {
				b["include"] = true
			}
		}
	}

	snap.SchemaVersion = SchemaVersion
}
```

**Call site** — `RunRefine` in `server.go`, after extraction and validation:

```go
snapPath := filepath.Join(tmpDir, "inspection-snapshot.json")
if snap, err := schema.LoadSnapshot(snapPath); err == nil {
	schema.NormalizeSnapshot(snap)
	schema.SaveSnapshot(snap, snapPath)
}
```

**Normalization test** (in `schema/snapshot_test.go`):

```go
func TestNormalizeSnapshot_SetsNilIncludeToTrue(t *testing.T) {
	snap := NewSnapshot()
	snap.ScheduledTasks = &ScheduledTaskSection{
		SystemdTimers: []SystemdTimer{{Name: "backup.timer"}},
	}
	snap.Network = &NetworkSection{
		Connections: []NMConnection{{Name: "eth0"}},
	}
	snap.Storage = &StorageSection{
		FstabEntries: []FstabEntry{{MountPoint: "/data"}},
	}
	snap.UsersGroups = &UserGroupSection{
		Users: []map[string]interface{}{{"name": "appuser", "uid": float64(1001)}},
	}

	// Before normalization: nil/absent
	assert.Nil(t, snap.ScheduledTasks.SystemdTimers[0].Include)
	assert.Nil(t, snap.Network.Connections[0].Include)
	assert.Nil(t, snap.Storage.FstabEntries[0].Include)
	_, hasInclude := snap.UsersGroups.Users[0]["include"]
	assert.False(t, hasInclude)

	NormalizeSnapshot(snap)

	// After: explicit true
	assert.Equal(t, true, *snap.ScheduledTasks.SystemdTimers[0].Include)
	assert.Equal(t, true, *snap.Network.Connections[0].Include)
	assert.Equal(t, true, *snap.Storage.FstabEntries[0].Include)
	assert.Equal(t, true, snap.UsersGroups.Users[0]["include"])
}
```

The existing `bool` fields on types that already have Include (PackageEntry, ConfigFileEntry, etc.) remain `bool` — they're always present in the JSON and unaffected by normalization.

### Field name reference

Every JS and Go reference in this plan uses these verified names from `cmd/inspectah/internal/schema/types.go`:

| Spec section | Go type | JSON path | Include type | Containerfile renderer uses Include? |
|-------------|---------|-----------|-------------|--------------------------------------|
| Packages — added | `PackageEntry` | `rpm.packages_added[]` | `bool` (existing) | Yes |
| Packages — modules | `EnabledModuleStream` | `rpm.module_streams[]` | `bool` (existing) | Yes |
| Config — files | `ConfigFileEntry` | `config.files[]` | `bool` (existing) | Yes |
| Runtime — services | `ServiceStateChange` | `services.state_changes[]` | `bool` (existing) | Yes |
| Runtime — drop-ins | `SystemdDropIn` | `services.drop_ins[]` | `bool` (existing) | Yes |
| Runtime — cron jobs | `CronJob` | `scheduled_tasks.cron_jobs[]` | `bool` (existing) | Yes |
| Runtime — timers | `SystemdTimer` | `scheduled_tasks.systemd_timers[]` | **`*bool` (Phase 3 adds)** | **Phase 3 adds filtering** |
| Runtime — at jobs | `AtJob` | `scheduled_tasks.at_jobs[]` | **`*bool` (Phase 3 adds)** | No (informational) |
| Containers — quadlets | `QuadletUnit` | `containers.quadlet_units[]` | `bool` (existing) | Yes |
| Containers — running | `RunningContainer` | `containers.running_containers[]` | **`*bool` (Phase 3 adds)** | No (informational) |
| Containers — non-RPM | `NonRpmItem` | `non_rpm_software.items[]` | `bool` (existing) | Yes |
| Identity — users | `map[string]interface{}` | `users_groups.users[]` | Dynamic key | Yes (already handles) |
| Identity — groups | `map[string]interface{}` | `users_groups.groups[]` | Dynamic key | **No** (renderer only consumes users, not groups) |
| Identity — SELinux booleans | `map[string]interface{}` | `selinux.boolean_overrides[]` | **Dynamic key (Phase 3 adds filtering)** | **Phase 3 adds filtering** |
| Identity — SELinux modules | `string` | `selinux.custom_modules[]` | N/A (display-only v1) | No (FIXME comments only) |
| Identity — SELinux ports | `SelinuxPortLabel` | `selinux.port_labels[]` | `bool` (existing) | Yes |
| System — sysctl | `SysctlOverride` | `kernel_boot.sysctl_overrides[]` | `bool` (existing) | Yes |
| System — kernel modules | `KernelModule` | `kernel_boot.loaded_modules[]` / `non_default_modules[]` | `bool` (existing) | Yes |
| System — network | `NMConnection` | `network.connections[]` | **`*bool` (Phase 3 adds)** | No (directory COPY) |
| System — firewall | `FirewallZone` | `network.firewall_zones[]` | `bool` (existing) | Yes |
| System — storage | `FstabEntry` | `storage.fstab_entries[]` | **`*bool` (Phase 3 adds)** | No (informational) |
| Secrets | `RedactionFinding` | `redactions[]` | N/A (always tier 3) | N/A |

### Display-only decision surfaces

Five item types carry triage decisions (`*bool` Include or dynamic map key) but do NOT affect Containerfile or other generated artifact output. These are **informational triage surfaces** — the admin reviews and acknowledges them, but include/exclude does not change what the renderer produces.

This is a deliberate v1 scope boundary, not an oversight. It must be surfaced in three places:

1. **Spec update:** Add a "Display-only surfaces" subsection to the triage interaction model listing these types and explaining that include/exclude is informational for v1.
2. **UI card language:** Triage cards for these types use modified button labels: "Acknowledge" / "Skip" instead of "Include in image" / "Leave out" — so the admin is not led to believe the decision affects the Containerfile.
3. **Renderer test:** Prove Containerfile output is identical regardless of Include state.

| Type | Section | Card language | Reason output is unaffected |
|------|---------|--------------|---------------------------|
| `NMConnection` | System | Acknowledge / Skip | NM files included via directory-level COPY. Per-file filtering requires WriteRedactedDir changes (v2). |
| `FstabEntry` | System | Acknowledge / Skip | Renderer generates advisory comments only, not actionable Containerfile lines. |
| `AtJob` | Runtime | Acknowledge / Skip | Renderer does not process at jobs (generates FIXME comments for cron conversion only). |
| `RunningContainer` | Containers | Acknowledge / Skip | Ephemeral state. Quadlet files (which DO affect output) are the actionable items. |
| Groups (`users_groups.groups[]`) | Identity | Acknowledge / Skip | Renderer consumes users for Containerfile generation but does not generate group-specific output. |

**Test contract** (in `cmd/inspectah/internal/renderer/containerfile_test.go`):

```go
func TestContainerfile_DisplayOnlyIncludeDoesNotAffectOutput(t *testing.T) {
	makeSnap := func() *schema.InspectionSnapshot {
		snap := schema.NewSnapshot()
		tr, fa := true, false
		snap.Network = &schema.NetworkSection{
			Connections: []schema.NMConnection{
				{Name: "eth0", Include: &tr},
				{Name: "eth1", Include: &fa},
			},
		}
		snap.Storage = &schema.StorageSection{
			FstabEntries: []schema.FstabEntry{
				{MountPoint: "/data", Fstype: "xfs", Include: &tr},
				{MountPoint: "/backup", Fstype: "ext4", Include: &fa},
			},
		}
		return snap
	}

	// Render with all included
	dir1 := t.TempDir()
	snap1 := makeSnap()
	require.NoError(t, RenderContainerfile(snap1, dir1))
	cf1, _ := os.ReadFile(filepath.Join(dir1, "Containerfile"))

	// Render with some excluded
	dir2 := t.TempDir()
	snap2 := makeSnap()
	fa := false
	snap2.Network.Connections[0].Include = &fa
	snap2.Storage.FstabEntries[0].Include = &fa
	require.NoError(t, RenderContainerfile(snap2, dir2))
	cf2, _ := os.ReadFile(filepath.Join(dir2, "Containerfile"))

	assert.Equal(t, string(cf1), string(cf2),
		"display-only Include changes must not affect Containerfile output")
}
```

---

## Phase 1: Static Foundation

**Commit:** `feat(renderer): template-based HTML report with PatternFly/CodeMirror`

Unchanged from Revision 2. Vendors static assets, creates embed.go, rewrites html.go as template renderer, builds report.html SPA skeleton with all CSS, layout, section landmarks, grouped sidebar keyboard navigation (Fern-spec'd), and accessibility structure.

**One addition to html.go:** The template data struct now includes `TriageManifest`:

```go
type reportData struct {
	PatternFlyCSS  template.CSS
	CodeMirrorJS   template.JS
	SnapshotJSON   template.JS
	Containerfile  template.JS
	TriageManifest template.JS
}
```

And report.html includes:
```html
<script>
const SNAPSHOT = {{.SnapshotJSON}};
const INITIAL_CONTAINERFILE = {{.Containerfile}};
const TRIAGE_MANIFEST = {{.TriageManifest}};
</script>
```

In Phase 1, `TriageManifest` is `[]` (empty). Phase 3 populates it.

### Tasks 1.1-1.4

Same as Revision 2: vendor PatternFly CSS, vendor CodeMirror bundle, create embed.go + html.go, create report.html skeleton. See Revision 2 for full task content.

**report.html changes from Revision 2:**
- Remove revert button from toolbar (descoped)
- Add change-counter badge to Containerfile preview header: `<span id="changes-badge" style="display:none">0 changes pending</span>`
- Add separate autosave aria-live region (visual indicator + hidden live region):
  ```html
  <span class="autosave-status" id="autosave-status"></span>
  <span id="autosave-live" class="sr-only" aria-live="polite"></span>
  ```
- SPA JavaScript skeleton references `TRIAGE_MANIFEST` instead of classification functions

---

## Phase 2: Refine Server API

**Commit:** `feat(refine): revision tracking, render_id binding, sidecar, tarball hardening`

Largely unchanged from Revision 2. Tests before implementation. E2E equality lands here before Phase 5.

### Changes from Revision 2

**Task 2.2 (POST /api/render):** `ReRenderResult` struct gains `TriageManifest`:

```go
type ReRenderResult struct {
	HTML            string          `json:"html"`
	Snapshot        json.RawMessage `json:"snapshot"`
	Containerfile   string          `json:"containerfile"`
	TriageManifest  json.RawMessage `json:"triage_manifest"`
}
```

The render response includes `triage_manifest` alongside `render_id`, `revision`, etc. In Phase 2, `nativeReRender` returns an empty manifest. Phase 3 populates it with real classification.

**New Task 2.7: Failed-render full working-directory safety (Kit R3)**

The test proves that ALL files in the working directory (snapshot, Containerfile, report.html, config/) are unchanged after a failed render. Not just the snapshot write path.

```go
func TestRenderAPI_FailedRender_EntireWorkingDirUnchanged(t *testing.T) {
	dir := setupTestOutputDir(t)

	// Record full working-dir state before render
	beforeFiles := snapshotDirContents(t, dir)

	failingRender := func(snapData []byte, origData []byte, outputDir string) (ReRenderResult, error) {
		return ReRenderResult{}, fmt.Errorf("renderer exploded")
	}
	handler := newRefineHandler(dir, failingRender)

	req := httptest.NewRequest("POST", "/api/render", strings.NewReader(`{"meta":{}}`))
	req.Header.Set("Content-Type", "application/json")
	w := httptest.NewRecorder()
	handler.ServeHTTP(w, req)

	assert.Equal(t, 500, w.Code)

	// Entire working directory must be byte-identical
	afterFiles := snapshotDirContents(t, dir)
	assert.Equal(t, beforeFiles, afterFiles,
		"working directory must be completely unchanged after failed render")
}

// snapshotDirContents reads all files in dir recursively and returns
// a map of relative-path → content for comparison.
func snapshotDirContents(t *testing.T, dir string) map[string]string {
	t.Helper()
	contents := make(map[string]string)
	filepath.Walk(dir, func(path string, info os.FileInfo, err error) error {
		if err != nil || info.IsDir() {
			return nil
		}
		rel, _ := filepath.Rel(dir, path)
		data, _ := os.ReadFile(path)
		contents[rel] = string(data)
		return nil
	})
	return contents
}
```

To make this pass, `nativeReRender` must render into a TEMP COPY of the working directory and swap results on success. On failure, the temp copy is discarded and the working directory is untouched.

**Updated nativeReRender implementation:**

```go
func nativeReRender(snapData []byte, origData []byte, outputDir string) (refine.ReRenderResult, error) {
	var snap schema.InspectionSnapshot
	if err := json.Unmarshal(snapData, &snap); err != nil {
		return refine.ReRenderResult{}, fmt.Errorf("parse snapshot: %w", err)
	}

	// Write snapshot to temp file first (atomic write)
	// Render into a temp copy — if rendering fails, the working directory
	// is completely untouched (Kit R3: full working-dir protection).
	renderDir, err := os.MkdirTemp("", "inspectah-render-")
	if err != nil {
		return refine.ReRenderResult{}, fmt.Errorf("create render dir: %w", err)
	}
	defer os.RemoveAll(renderDir)

	// Copy working dir contents to temp render dir so
	// cleanRendererOutputs can selectively preserve only snapshot + sidecar.
	if err := copyDir(outputDir, renderDir); err != nil {
		return refine.ReRenderResult{}, fmt.Errorf("copy working dir: %w", err)
	}

	// Clean all renderer-owned outputs from the temp dir BEFORE rendering.
	// This ensures the render starts from a clean state — no stale files
	// from a prior render survive into the new output. Only input data
	// (sidecar, snapshot) is preserved.
	cleanRendererOutputs(renderDir)

	// Write new snapshot to render dir
	snapPath := filepath.Join(renderDir, "inspection-snapshot.json")
	if err := os.WriteFile(snapPath, snapData, 0644); err != nil {
		return refine.ReRenderResult{}, fmt.Errorf("write snapshot: %w", err)
	}

	// Use server-owned sidecar from render dir
	origSnapPath := filepath.Join(renderDir, "original-inspection-snapshot.json")
	if _, err := os.Stat(origSnapPath); err != nil {
		origSnapPath = ""
	}

	// Run all renderers in the temp dir — fresh output, no stale artifacts
	if err := renderer.RunAll(&snap, renderDir, renderer.RunAllOptions{
		RefineMode:           true,
		OriginalSnapshotPath: origSnapPath,
	}); err != nil {
		// renderDir is cleaned up by defer — outputDir untouched
		return refine.ReRenderResult{}, fmt.Errorf("render: %w", err)
	}

	// Rendering succeeded — replace ALL renderer-owned outputs in the
	// working directory with the rendered versions. This covers the full
	// output set from renderer.RunAll(): Containerfile, report.html,
	// inspection-snapshot.json, README.md, audit-report.md,
	// kickstart-suggestion.ks, secrets-review.md, merge-notes.md,
	// config/, redacted/, drop-ins/, quadlet/.
	//
	// Strategy: walk renderDir, skip the immutable sidecar, copy
	// everything else into outputDir via temp+rename per file.
	if err := syncRenderedOutput(renderDir, outputDir); err != nil {
		return refine.ReRenderResult{}, fmt.Errorf("swap rendered output: %w", err)
	}

	htmlData, _ := os.ReadFile(filepath.Join(outputDir, "report.html"))
	containerfileData, _ := os.ReadFile(filepath.Join(outputDir, "Containerfile"))

	manifest := renderer.ClassifySnapshot(&snap)
	manifestJSON, _ := json.Marshal(manifest)

	return refine.ReRenderResult{
		HTML:           string(htmlData),
		Snapshot:       json.RawMessage(snapData),
		Containerfile:  string(containerfileData),
		TriageManifest: json.RawMessage(manifestJSON),
	}, nil
}

// cleanRendererOutputs removes all renderer-owned files and
// directories from dir, preserving only the snapshot and sidecar.
// config/ IS renderer-owned — writeConfigTree() regenerates it
// from snapshot data during renderer.RunAll().
func cleanRendererOutputs(dir string) {
	preserved := map[string]bool{
		"inspection-snapshot.json":          true,
		"original-inspection-snapshot.json": true,
	}

	entries, _ := os.ReadDir(dir)
	for _, e := range entries {
		name := e.Name()
		if preserved[name] {
			continue
		}
		os.RemoveAll(filepath.Join(dir, name))
	}
}

// syncRenderedOutput replaces ALL content in dst with the rendered
// output from src. Skips the immutable sidecar. Removes stale files
// AND empty directories from dst that are absent in src.
func syncRenderedOutput(src, dst string) error {
	// Phase 1: Build inventory of files AND directories the new render produced.
	newPaths := make(map[string]bool)
	filepath.Walk(src, func(path string, info os.FileInfo, err error) error {
		if err != nil { return nil }
		rel, _ := filepath.Rel(src, path)
		if rel == "." { return nil }
		if filepath.Base(rel) == "original-inspection-snapshot.json" { return nil }
		newPaths[rel] = true
		return nil
	})

	// Phase 2: Copy all new render output into dst.
	if err := filepath.Walk(src, func(path string, info os.FileInfo, err error) error {
		if err != nil { return err }
		rel, _ := filepath.Rel(src, path)
		if rel == "." { return nil }
		if filepath.Base(rel) == "original-inspection-snapshot.json" { return nil }
		target := filepath.Join(dst, rel)
		if info.IsDir() {
			return os.MkdirAll(target, 0755)
		}
		data, err := os.ReadFile(path)
		if err != nil { return err }
		if err := os.MkdirAll(filepath.Dir(target), 0755); err != nil { return err }
		tmp := target + ".tmp"
		if err := os.WriteFile(tmp, data, info.Mode()); err != nil { return err }
		return os.Rename(tmp, target)
	}); err != nil {
		return fmt.Errorf("copy rendered output: %w", err)
	}

	// Phase 3: Remove stale FILES from dst not in the new render.
	filepath.Walk(dst, func(path string, info os.FileInfo, err error) error {
		if err != nil { return nil }
		rel, _ := filepath.Rel(dst, path)
		if rel == "." || info.IsDir() { return nil }
		if filepath.Base(rel) == "original-inspection-snapshot.json" { return nil }
		if !newPaths[rel] {
			os.Remove(path)
		}
		return nil
	})

	// Phase 4: Remove stale empty DIRECTORIES from dst (bottom-up).
	// Walk in reverse depth order so parent dirs are removed after children.
	var dirs []string
	filepath.Walk(dst, func(path string, info os.FileInfo, err error) error {
		if err != nil { return nil }
		rel, _ := filepath.Rel(dst, path)
		if rel == "." || !info.IsDir() { return nil }
		if filepath.Base(rel) == "original-inspection-snapshot.json" { return nil }
		dirs = append(dirs, path)
		return nil
	})
	// Reverse so deepest directories are processed first
	for i := len(dirs) - 1; i >= 0; i-- {
		entries, _ := os.ReadDir(dirs[i])
		if len(entries) == 0 {
			os.Remove(dirs[i])
		}
	}

	return nil
}
```

**Strengthened E2E equality test (Thorn R3 — chained three-way proof):**

The test proves the invariant: `response == working directory == tarball` for every artifact. Each assertion chain is labeled to make the proof structure explicit.

```go
func TestE2E_RenderEquality_ThreeWayProof(t *testing.T) {
	dir := setupTestOutputDir(t)

	reRenderFn := func(snapData []byte, origData []byte, outputDir string) (ReRenderResult, error) {
		os.WriteFile(filepath.Join(outputDir, "inspection-snapshot.json"), snapData, 0644)
		cf := "FROM ubi9\nRUN echo rendered"
		os.WriteFile(filepath.Join(outputDir, "Containerfile"), []byte(cf), 0644)
		os.WriteFile(filepath.Join(outputDir, "report.html"), []byte("<html>rendered</html>"), 0644)
		return ReRenderResult{
			HTML: "<html>rendered</html>", Snapshot: json.RawMessage(snapData),
			Containerfile: cf, TriageManifest: json.RawMessage("[]"),
		}, nil
	}
	handler := newRefineHandler(dir, reRenderFn)

	// ── Step 1: POST /api/render ──
	renderReq := httptest.NewRequest("POST", "/api/render",
		strings.NewReader(`{"meta":{"hostname":"e2e"}}`))
	renderReq.Header.Set("Content-Type", "application/json")
	renderW := httptest.NewRecorder()
	handler.ServeHTTP(renderW, renderReq)
	require.Equal(t, 200, renderW.Code)

	var resp struct {
		RenderID       string          `json:"render_id"`
		Snapshot       json.RawMessage `json:"snapshot"`
		Containerfile  string          `json:"containerfile"`
		HTML           string          `json:"html"`
		TriageManifest json.RawMessage `json:"triage_manifest"`
	}
	require.NoError(t, json.Unmarshal(renderW.Body.Bytes(), &resp))

	// ── Step 2: Read working directory ──
	diskSnap, err := os.ReadFile(filepath.Join(dir, "inspection-snapshot.json"))
	require.NoError(t, err)
	diskCf, err := os.ReadFile(filepath.Join(dir, "Containerfile"))
	require.NoError(t, err)
	diskHTML, err := os.ReadFile(filepath.Join(dir, "report.html"))
	require.NoError(t, err)

	// ── Step 3: Extract tarball ──
	tarReq := httptest.NewRequest("GET", "/api/tarball?render_id="+resp.RenderID, nil)
	tarW := httptest.NewRecorder()
	handler.ServeHTTP(tarW, tarReq)
	require.Equal(t, 200, tarW.Code)

	tmpFile := filepath.Join(t.TempDir(), "e2e.tar.gz")
	os.WriteFile(tmpFile, tarW.Body.Bytes(), 0644)
	extractDir := t.TempDir()
	require.NoError(t, ExtractTarball(tmpFile, extractDir))

	tarSnap, err := os.ReadFile(filepath.Join(extractDir, "inspection-snapshot.json"))
	require.NoError(t, err)
	tarCf, err := os.ReadFile(filepath.Join(extractDir, "Containerfile"))
	require.NoError(t, err)
	tarHTML, err := os.ReadFile(filepath.Join(extractDir, "report.html"))
	require.NoError(t, err)

	// ══ PROOF: three-way equality for each artifact ══

	// Snapshot: response == disk == tarball
	t.Run("snapshot_response_eq_disk", func(t *testing.T) {
		assert.JSONEq(t, string(resp.Snapshot), string(diskSnap))
	})
	t.Run("snapshot_disk_eq_tarball", func(t *testing.T) {
		assert.JSONEq(t, string(diskSnap), string(tarSnap))
	})

	// Containerfile: response == disk == tarball
	t.Run("containerfile_response_eq_disk", func(t *testing.T) {
		assert.Equal(t, resp.Containerfile, string(diskCf))
	})
	t.Run("containerfile_disk_eq_tarball", func(t *testing.T) {
		assert.Equal(t, string(diskCf), string(tarCf))
	})

	// report.html: response == disk == tarball
	t.Run("html_response_eq_disk", func(t *testing.T) {
		assert.Equal(t, resp.HTML, string(diskHTML))
	})
	t.Run("html_disk_eq_tarball", func(t *testing.T) {
		assert.Equal(t, string(diskHTML), string(tarHTML))
	})

	// Exclusion: tarball must NOT contain sidecar or excluded files
	t.Run("tarball_excludes_sidecar", func(t *testing.T) {
		assert.NoFileExists(t, filepath.Join(extractDir, "original-inspection-snapshot.json"))
	})
}
```

**New Task 2.8: Real-path nativeReRender test (Kit R4)**

The mocked `ReRenderFn` tests prove the server handler contract. This test exercises the REAL `nativeReRender` function from `cmd/inspectah/internal/cli/refine.go` to prove that the staging, rendering, and swap logic works end-to-end with `renderer.RunAll()` in the loop.

Located in `cmd/inspectah/internal/cli/refine_test.go`:

```go
func TestNativeReRender_ProducesCanonicalOutput(t *testing.T) {
	// Set up a working directory with a valid snapshot and sidecar
	workDir := t.TempDir()
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		PackagesAdded: []schema.PackageEntry{
			{Name: "httpd", Version: "2.4", Release: "1", Arch: "x86_64",
			 State: "installed", SourceRepo: "appstream", Include: true},
		},
	}
	snapData, err := json.Marshal(snap)
	require.NoError(t, err)
	require.NoError(t, os.WriteFile(filepath.Join(workDir, "inspection-snapshot.json"), snapData, 0644))
	require.NoError(t, os.WriteFile(filepath.Join(workDir, "original-inspection-snapshot.json"), snapData, 0444))

	// Call the real nativeReRender
	result, err := nativeReRender(snapData, nil, workDir)
	require.NoError(t, err)

	// Verify response contains all expected fields
	assert.NotEmpty(t, result.HTML)
	assert.NotEmpty(t, result.Containerfile)
	assert.NotEmpty(t, result.Snapshot)

	// ── Full output set verification ──
	// Render into a FRESH directory with the same snapshot to get the
	// ground-truth output set from renderer.RunAll(). Then compare
	// the working directory against it to prove the success-path swap
	// replaced ALL renderer-owned outputs, not just a subset.
	groundTruthDir := t.TempDir()
	require.NoError(t, os.WriteFile(filepath.Join(groundTruthDir, "inspection-snapshot.json"), snapData, 0644))
	// Copy sidecar to ground-truth dir so the render is refine-mode-equivalent
	sidecarSrc := filepath.Join(workDir, "original-inspection-snapshot.json")
	sidecarGT := filepath.Join(groundTruthDir, "original-inspection-snapshot.json")
	if data, err := os.ReadFile(sidecarSrc); err == nil {
		os.WriteFile(sidecarGT, data, 0444)
	}
	var groundSnap schema.InspectionSnapshot
	require.NoError(t, json.Unmarshal(snapData, &groundSnap))
	require.NoError(t, renderer.RunAll(&groundSnap, groundTruthDir, renderer.RunAllOptions{
		RefineMode:           true,
		OriginalSnapshotPath: sidecarGT,
	}))

	// Ground truth now contains every file renderer.RunAll() produces.
	groundFiles := snapshotDirContents(t, groundTruthDir)

	// The working directory must contain every file the ground truth has.
	// (It may also contain the sidecar, which ground truth lacks — that's fine.)
	workFiles := snapshotDirContents(t, workDir)
	for path, groundContent := range groundFiles {
		workContent, exists := workFiles[path]
		assert.True(t, exists, "working directory missing renderer output: %s", path)
		if exists {
			assert.Equal(t, groundContent, workContent,
				"working directory file %s differs from ground-truth render", path)
		}
	}

	// ── Response == disk for the three API-returned artifacts ──
	diskSnap, _ := os.ReadFile(filepath.Join(workDir, "inspection-snapshot.json"))
	assert.JSONEq(t, string(result.Snapshot), string(diskSnap))

	diskCf, _ := os.ReadFile(filepath.Join(workDir, "Containerfile"))
	assert.Equal(t, result.Containerfile, string(diskCf))

	diskHTML, _ := os.ReadFile(filepath.Join(workDir, "report.html"))
	assert.Equal(t, result.HTML, string(diskHTML))

	// ── Sidecar immutability ──
	sidecar, _ := os.ReadFile(filepath.Join(workDir, "original-inspection-snapshot.json"))
	assert.Equal(t, string(snapData), string(sidecar), "sidecar must be immutable")
}

func TestNativeReRender_FailurePreservesWorkingDir(t *testing.T) {
	workDir := t.TempDir()
	// Write initial state
	require.NoError(t, os.WriteFile(filepath.Join(workDir, "inspection-snapshot.json"), []byte("{}"), 0644))
	require.NoError(t, os.WriteFile(filepath.Join(workDir, "Containerfile"), []byte("FROM ubi9\n"), 0644))
	require.NoError(t, os.WriteFile(filepath.Join(workDir, "report.html"), []byte("<html>original</html>"), 0644))

	before := snapshotDirContents(t, workDir)

	// Call with invalid snapshot JSON — should fail
	_, err := nativeReRender([]byte("not valid json"), nil, workDir)
	assert.Error(t, err)

	// Working directory must be unchanged
	after := snapshotDirContents(t, workDir)
	assert.Equal(t, before, after, "working directory must be unchanged after failed render")
}

func snapshotDirContents(t *testing.T, dir string) map[string]string {
	t.Helper()
	contents := make(map[string]string)
	filepath.Walk(dir, func(path string, info os.FileInfo, err error) error {
		if err != nil || info.IsDir() { return nil }
		rel, _ := filepath.Rel(dir, path)
		data, _ := os.ReadFile(path)
		contents[rel] = string(data)
		return nil
	})
	return contents
}
```

This test:
- Calls the REAL `nativeReRender`, not a mock
- Verifies the full renderer output set is present after success (not just 3 files)
- Verifies response == disk for all asserted artifacts
- Verifies the immutable sidecar survives rendering
- Verifies a parse failure leaves the working directory byte-identical

**New Task 2.9: Stale artifact removal — two-render disappearance proof (Kit R6)**

This test exercises the TWO-RENDER disappearance case: render N produces an artifact that render N+1 (with a different snapshot) does not produce. After render N+1, the stale artifact must be gone from the working directory. The sidecar must survive both renders.

Located in `cmd/inspectah/internal/cli/refine_test.go`:

```go
func TestNativeReRender_StaleArtifactRemoval(t *testing.T) {
	workDir := t.TempDir()

	// ── Render N: snapshot with config files + redactions ──
	// This produces TWO guaranteed-disappearing renderer-owned paths:
	// 1. redacted/etc/secret.conf.REDACTED (from WriteRedactedDir)
	// 2. config/etc/httpd/conf/httpd.conf (from writeConfigTree)
	snapN := schema.NewSnapshot()
	snapN.Rpm = &schema.RpmSection{
		PackagesAdded: []schema.PackageEntry{
			{Name: "httpd", Version: "2.4", Release: "1", Arch: "x86_64",
			 State: "installed", SourceRepo: "appstream", Include: true},
		},
	}
	snapN.Config = &schema.ConfigSection{
		Files: []schema.ConfigFileEntry{
			{Path: "/etc/httpd/conf/httpd.conf", Kind: schema.ConfigFileKindRpmOwnedModified,
			 Category: schema.ConfigCategoryOther, Content: "ServerRoot /etc/httpd", Include: true},
		},
	}
	snapN.Redactions = []json.RawMessage{
		json.RawMessage(`{"path":"/etc/secret.conf","source":"file","kind":"excluded","finding_type":"api_key","original":"REDACTED"}`),
	}
	snapDataN, err := json.Marshal(snapN)
	require.NoError(t, err)
	require.NoError(t, os.WriteFile(
		filepath.Join(workDir, "inspection-snapshot.json"), snapDataN, 0644))
	require.NoError(t, os.WriteFile(
		filepath.Join(workDir, "original-inspection-snapshot.json"), snapDataN, 0444))

	// Call the real nativeReRender for render N
	_, err = nativeReRender(snapDataN, nil, workDir)
	require.NoError(t, err)

	// ── NAMED ASSERTION: renderer-owned outputs exist after render N ──
	redactedFile := filepath.Join(workDir, "redacted", "etc", "secret.conf.REDACTED")
	assert.FileExists(t, redactedFile,
		"render N must produce redacted/etc/secret.conf.REDACTED")
	redactedDir := filepath.Join(workDir, "redacted")
	assert.DirExists(t, redactedDir,
		"render N must produce redacted/ directory")
	configFile := filepath.Join(workDir, "config", "etc", "httpd", "conf", "httpd.conf")
	assert.FileExists(t, configFile,
		"render N must produce config/etc/httpd/conf/httpd.conf")

	afterRenderN := snapshotDirContents(t, workDir)
	t.Logf("Files after render N: %v", mapKeys(afterRenderN))

	// ── Render N+1: snapshot WITHOUT redactions ──
	// WriteRedactedDir produces nothing → redacted/ must disappear entirely.
	snapN1 := schema.NewSnapshot()
	snapN1.Rpm = &schema.RpmSection{
		PackagesAdded: []schema.PackageEntry{
			{Name: "nginx", Version: "1.24", Release: "1", Arch: "x86_64",
			 State: "installed", SourceRepo: "appstream", Include: true},
		},
	}
	// No redactions
	snapDataN1, err := json.Marshal(snapN1)
	require.NoError(t, err)

	// Call the real nativeReRender for render N+1
	_, err = nativeReRender(snapDataN1, nil, workDir)
	require.NoError(t, err)

	afterRenderN1 := snapshotDirContents(t, workDir)
	t.Logf("Files after render N+1: %v", mapKeys(afterRenderN1))

	// ── NAMED ASSERTION: stale outputs are gone after render N+1 ──
	// Redacted artifacts (render N had redactions, render N+1 does not)
	assert.NoFileExists(t, redactedFile,
		"redacted/etc/secret.conf.REDACTED must disappear after render N+1")
	_, err = os.Stat(redactedDir)
	assert.True(t, os.IsNotExist(err),
		"redacted/ directory must be removed after render N+1 (empty dir cleanup)")

	// Config tree artifacts (render N had httpd config, render N+1 does not)
	assert.NoFileExists(t, configFile,
		"config/etc/httpd/conf/httpd.conf must disappear after render N+1")
	_, err = os.Stat(filepath.Join(workDir, "config", "etc", "httpd"))
	assert.True(t, os.IsNotExist(err),
		"config/etc/httpd/ directory must be removed after render N+1")

	// ── FULL SET PROOF: working dir == ground truth for render N+1 ──
	groundDir := t.TempDir()
	require.NoError(t, os.WriteFile(
		filepath.Join(groundDir, "inspection-snapshot.json"), snapDataN1, 0644))
	// Copy sidecar to ground-truth dir for refine-mode-equivalent render
	sidecarGT := filepath.Join(groundDir, "original-inspection-snapshot.json")
	sidecarData, _ := os.ReadFile(filepath.Join(workDir, "original-inspection-snapshot.json"))
	os.WriteFile(sidecarGT, sidecarData, 0444)

	var groundSnap schema.InspectionSnapshot
	require.NoError(t, json.Unmarshal(snapDataN1, &groundSnap))
	require.NoError(t, renderer.RunAll(&groundSnap, groundDir, renderer.RunAllOptions{
		RefineMode:           true,
		OriginalSnapshotPath: sidecarGT,
	}))
	groundFiles := snapshotDirContents(t, groundDir)

	// Working dir must contain ONLY ground-truth files + sidecar
	for path := range afterRenderN1 {
		if path == "original-inspection-snapshot.json" {
			continue
		}
		_, inGround := groundFiles[path]
		assert.True(t, inGround,
			"stale artifact %q survived render N+1 — must be removed", path)
	}

	for path, content := range groundFiles {
		workContent, exists := afterRenderN1[path]
		assert.True(t, exists, "missing expected file: %s", path)
		if exists {
			assert.Equal(t, content, workContent,
				"file %s differs from ground-truth render", path)
		}
	}

	// ── Sidecar must survive both renders ──
	sidecar, err := os.ReadFile(
		filepath.Join(workDir, "original-inspection-snapshot.json"))
	require.NoError(t, err)
	assert.Equal(t, string(snapDataN), string(sidecar),
		"sidecar must be immutable across multiple renders")

	// ── TARBALL PROOF: stale artifacts absent from exported tarball ──
	// Package the post-render-N+1 working directory as a tarball and
	// verify stale artifacts don't leak into the export.
	tarPath := filepath.Join(t.TempDir(), "test-refined.tar.gz")
	require.NoError(t, refine.RepackTarball(workDir, tarPath))

	tarExtractDir := t.TempDir()
	require.NoError(t, refine.ExtractTarball(tarPath, tarExtractDir))

	// Named assertions: stale artifacts must not be in the tarball
	_, tarRedactedErr := os.Stat(filepath.Join(tarExtractDir, "redacted"))
	assert.True(t, os.IsNotExist(tarRedactedErr),
		"stale redacted/ directory must not appear in exported tarball")
	assert.NoFileExists(t,
		filepath.Join(tarExtractDir, "redacted", "etc", "secret.conf.REDACTED"),
		"stale redacted file must not appear in exported tarball")

	// Stale config/ artifacts must not be in the tarball
	assert.NoFileExists(t,
		filepath.Join(tarExtractDir, "config", "etc", "httpd", "conf", "httpd.conf"),
		"stale config file must not appear in exported tarball")

	// Sidecar must also be excluded from tarball (per tarball allowlist)
	assert.NoFileExists(t,
		filepath.Join(tarExtractDir, "original-inspection-snapshot.json"),
		"sidecar must be excluded from exported tarball")
}

func mapKeys(m map[string]string) []string {
	keys := make([]string, 0, len(m))
	for k := range m {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	return keys
}
```

This test:
- Exercises the REAL `nativeReRender` path (not mocks) for BOTH renders
- Render N produces a richer file set (packages + cron jobs → more output files)
- Render N+1 uses a minimal snapshot → fewer output files
- Proves stale artifacts from render N are absent after render N+1
- Proves the sidecar survives both renders unchanged
- Uses a ground-truth directory to determine exactly which files render N+1 should produce

All other Phase 2 tasks (revision tracking, GET/PUT /api/snapshot, render_id guard, tarball allowlist, Cache-Control, sidecar, integration tests) are unchanged from Revision 2.

---

## Phase 3: Schema Alignment + Triage Engine

**Commit:** `feat(schema): *bool Include fields, Go triage engine with table-driven tests`

This is the major new phase. Adds `*bool` Include to typed structs, renderer follow-through, and the Go tier classification engine.

### Task 3.1: Add `*bool` Include to typed structs

**Files:**
- Modify: `cmd/inspectah/internal/schema/types.go`

Add `Include *bool \`json:"include,omitempty"\`` to:

```go
type NMConnection struct {
	Path    string           `json:"path"`
	Name    string           `json:"name"`
	Method  string           `json:"method"`
	Type    string           `json:"type"`
	Include *bool            `json:"include,omitempty"`
	Fleet   *FleetPrevalence `json:"fleet,omitempty"`
}

type FstabEntry struct {
	Device     string           `json:"device"`
	MountPoint string           `json:"mount_point"`
	Fstype     string           `json:"fstype"`
	Options    string           `json:"options"`
	Include    *bool            `json:"include,omitempty"`
	Fleet      *FleetPrevalence `json:"fleet,omitempty"`
}

type SystemdTimer struct {
	Name           string           `json:"name"`
	OnCalendar     string           `json:"on_calendar"`
	ExecStart      string           `json:"exec_start"`
	Description    string           `json:"description"`
	Source         string           `json:"source"`
	Path           string           `json:"path"`
	TimerContent   string           `json:"timer_content"`
	ServiceContent string           `json:"service_content"`
	Include        *bool            `json:"include,omitempty"`
	Fleet          *FleetPrevalence `json:"fleet,omitempty"`
}

type AtJob struct {
	File       string           `json:"file"`
	Command    string           `json:"command"`
	User       string           `json:"user"`
	WorkingDir string           `json:"working_dir"`
	Include    *bool            `json:"include,omitempty"`
	Fleet      *FleetPrevalence `json:"fleet,omitempty"`
}

type RunningContainer struct {
	ID       string                 `json:"id"`
	Name     string                 `json:"name"`
	Image    string                 `json:"image"`
	ImageID  string                 `json:"image_id"`
	Status   string                 `json:"status"`
	Mounts   []ContainerMount       `json:"mounts"`
	Networks map[string]interface{} `json:"networks"`
	Ports    map[string]interface{} `json:"ports"`
	Env      []string               `json:"env"`
	Include  *bool                  `json:"include,omitempty"`
	Fleet    *FleetPrevalence       `json:"fleet,omitempty"`
}
```

Bump `SchemaVersion` from 11 to 12.

- [ ] **Step 1: Make the type changes and bump version**

- [ ] **Step 2: Update LoadSnapshot for v11 compat**

```go
if snap.SchemaVersion != SchemaVersion && snap.SchemaVersion != SchemaVersion-1 {
	return nil, fmt.Errorf("schema version mismatch: file has %d, expected %d or %d",
		snap.SchemaVersion, SchemaVersion-1, SchemaVersion)
}
```

- [ ] **Step 3: Run all tests, fix any that hard-code SchemaVersion**

```bash
cd cmd/inspectah && go test ./... 2>&1 | tail -10
```

### Task 3.2: Renderer follow-through — Include filtering

**Files:**
- Modify: `cmd/inspectah/internal/renderer/containerfile.go`

The Containerfile renderer already checks `Include` on most types. Two sections need updates:

- [ ] **Step 1: Add Include filtering to scheduledTasksSectionLines**

Currently filters timers by `Source == "local"` only. Add `isIncluded` check:

```go
var localTimers []schema.SystemdTimer
for _, t := range st.SystemdTimers {
	if t.Source == "local" && isIncluded(t.Include) {
		localTimers = append(localTimers, t)
	}
}
```

- [ ] **Step 2: Add Include filtering to selinuxSectionLines for BooleanOverrides**

Currently checks `non_default` but not `include`. Add dynamic-key check (same pattern as `usersSectionLines`):

```go
var nonDefault []map[string]interface{}
for _, b := range snap.Selinux.BooleanOverrides {
	// Check dynamic include key (same pattern as users)
	if inc, ok := b["include"]; ok {
		if incBool, ok := inc.(bool); ok && !incBool {
			continue
		}
	}
	if nd, ok := b["non_default"]; ok {
		if ndBool, ok := nd.(bool); ok && ndBool {
			nonDefault = append(nonDefault, b)
		}
	}
}
```

- [ ] **Step 3: Run tests to verify no regressions**

```bash
cd cmd/inspectah && go test ./internal/renderer/ -v
```

### Task 3.3: Create triage.go — Go tier classification engine

**Files:**
- Create: `cmd/inspectah/internal/renderer/triage.go`
- Create: `cmd/inspectah/internal/renderer/triage_test.go`

This is the core classification engine. Every item in the snapshot is classified into a tier (1=auto, 2=decide, 3=flagged) with a reason string. The SPA consumes the output — it never runs its own classification.

- [ ] **Step 1: Write triage.go**

```go
package renderer

import (
	"encoding/json"
	"fmt"
	"strings"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
)

// TriageItem represents a classified snapshot item for the SPA.
type TriageItem struct {
	Section  string `json:"section"`
	Key      string `json:"key"`
	Tier     int    `json:"tier"`
	Reason   string `json:"reason"`
	Name     string `json:"name"`
	Meta     string `json:"meta"`
	IsSecret bool   `json:"is_secret,omitempty"`
}

// ClassifySnapshot classifies all triageable items in the snapshot.
// Returns a manifest sorted by section, then tier (3→2→1).
func ClassifySnapshot(snap *schema.InspectionSnapshot) []TriageItem {
	secretPaths := buildSecretPathSet(snap)

	var items []TriageItem
	items = append(items, classifyPackages(snap, secretPaths)...)
	items = append(items, classifyConfigFiles(snap, secretPaths)...)
	items = append(items, classifyRuntime(snap, secretPaths)...)
	items = append(items, classifyContainerItems(snap, secretPaths)...)
	items = append(items, classifyIdentity(snap, secretPaths)...)
	items = append(items, classifySystemItems(snap, secretPaths)...)
	items = append(items, classifySecretItems(snap)...)
	return items
}

func isIncluded(b *bool) bool {
	return b == nil || *b
}

func buildSecretPathSet(snap *schema.InspectionSnapshot) map[string]bool {
	paths := make(map[string]bool)
	for _, r := range snap.Redactions {
		var finding struct {
			Path string `json:"path"`
			Name string `json:"name"`
		}
		if json.Unmarshal(r, &finding) == nil {
			if finding.Path != "" {
				paths[finding.Path] = true
			}
			if finding.Name != "" {
				paths[finding.Name] = true
			}
		}
	}
	return paths
}

func classifyPackages(snap *schema.InspectionSnapshot, secrets map[string]bool) []TriageItem {
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
		items = append(items, TriageItem{
			Section: "packages",
			Key:     fmt.Sprintf("pkg-%s-%s", pkg.Name, pkg.Arch),
			Tier:    tier,
			Reason:  reason,
			Name:    pkg.Name,
			Meta:    joinNonEmpty(" | ", pkg.Version+"-"+pkg.Release, pkg.Arch, pkg.SourceRepo),
		})
	}

	for _, ms := range snap.Rpm.ModuleStreams {
		if ms.BaselineMatch {
			continue
		}
		items = append(items, TriageItem{
			Section: "packages",
			Key:     fmt.Sprintf("ms-%s-%s", ms.ModuleName, ms.Stream),
			Tier:    2,
			Reason:  "Module stream package. Verify compatibility.",
			Name:    ms.ModuleName + ":" + ms.Stream,
			Meta:    strings.Join(ms.Profiles, ", "),
		})
	}
	return items
}

func classifyPackage(pkg schema.PackageEntry, baseline map[string]bool) (int, string) {
	state := string(pkg.State)
	repo := strings.ToLower(pkg.SourceRepo)

	if state == "local_install" || state == "no_repo" {
		return 3, "Package installed locally (no repository). Verify provenance."
	}
	if baseline[pkg.Name] {
		return 1, "Standard package matching base image."
	}
	if isThirdPartyRepo(repo) {
		return 2, fmt.Sprintf("Third-party repository (%s). Not in base image.", pkg.SourceRepo)
	}
	return 2, "Package from standard repo, not in base image."
}

func isThirdPartyRepo(repo string) bool {
	standard := []string{"baseos", "appstream", "rhel", "fedora"}
	if repo == "" {
		return false
	}
	lower := strings.ToLower(repo)
	for _, s := range standard {
		if strings.Contains(lower, s) {
			return false
		}
	}
	return true
}

func classifyConfigFiles(snap *schema.InspectionSnapshot, secrets map[string]bool) []TriageItem {
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
		items = append(items, TriageItem{
			Section: "config",
			Key:     "cfg-" + f.Path,
			Tier:    tier,
			Reason:  reason,
			Name:    f.Path,
			Meta:    joinNonEmpty(" | ", string(f.Kind), string(f.Category)),
		})
	}
	return items
}

func classifyConfigFile(f schema.ConfigFileEntry) (int, string) {
	switch f.Kind {
	case schema.ConfigFileKindRpmOwnedDefault, "baseline_match":
		return 1, "Config file matches base image content."
	case schema.ConfigFileKindRpmOwnedModified:
		return 2, "Config file modified from RPM default."
	case "systemd_dropin":
		return 2, "Systemd drop-in override file."
	default:
		return 2, "Config file not in base image."
	}
}

func isQuadletPath(path string) bool {
	exts := []string{".container", ".volume", ".network", ".kube"}
	for _, ext := range exts {
		if strings.HasSuffix(path, ext) && strings.Contains(path, "/containers/") {
			return true
		}
	}
	return false
}

func classifyRuntime(snap *schema.InspectionSnapshot, secrets map[string]bool) []TriageItem {
	var items []TriageItem
	if snap.Services != nil {
		for _, svc := range snap.Services.StateChanges {
			if secrets[svc.Unit] {
				continue
			}
			isDefault := svc.CurrentState == svc.DefaultState
			tier := 2
			reason := fmt.Sprintf("Service state changed (%s → %s).", svc.DefaultState, svc.CurrentState)
			if isDefault {
				tier = 1
				reason = "Service in default state."
			}
			meta := svc.CurrentState
			if svc.OwningPackage != nil {
				meta += " | " + *svc.OwningPackage
			}
			items = append(items, TriageItem{
				Section: "runtime", Key: "svc-" + svc.Unit,
				Tier: tier, Reason: reason, Name: svc.Unit, Meta: meta,
			})
		}
	}
	if snap.ScheduledTasks != nil {
		for _, job := range snap.ScheduledTasks.CronJobs {
			items = append(items, TriageItem{
				Section: "runtime", Key: "cron-" + job.Path,
				Tier: 2, Reason: "Scheduled cron job.",
				Name: job.Path, Meta: job.Source,
			})
		}
		for _, timer := range snap.ScheduledTasks.SystemdTimers {
			items = append(items, TriageItem{
				Section: "runtime", Key: "timer-" + timer.Name,
				Tier: 2, Reason: "Systemd timer unit.",
				Name: timer.Name, Meta: timer.OnCalendar,
			})
		}
	}
	return items
}

func classifyContainerItems(snap *schema.InspectionSnapshot, secrets map[string]bool) []TriageItem {
	var items []TriageItem
	if snap.Containers != nil {
		quadletNames := make(map[string]bool)
		for _, q := range snap.Containers.QuadletUnits {
			quadletNames[q.Name] = true
			items = append(items, TriageItem{
				Section: "containers", Key: "quadlet-" + q.Name,
				Tier: 2, Reason: "Quadlet file with container unit.",
				Name: q.Name, Meta: q.Image,
			})
		}
		for _, c := range snap.Containers.RunningContainers {
			tier, reason := 2, "Running container with quadlet backing."
			if !quadletNames[c.Name] {
				tier, reason = 3, "Running container without quadlet. May not survive reboot."
			}
			items = append(items, TriageItem{
				Section: "containers", Key: "container-" + c.Name,
				Tier: tier, Reason: reason, Name: c.Name, Meta: c.Image,
			})
		}
	}
	if snap.NonRpmSoftware != nil {
		for _, item := range snap.NonRpmSoftware.Items {
			if secrets[item.Path] {
				continue
			}
			name := item.Path
			if name == "" {
				name = item.Name
			}
			items = append(items, TriageItem{
				Section: "containers", Key: "nonrpm-" + name,
				Tier: 3, Reason: "Non-RPM binary with unclear provenance.",
				Name: name, Meta: item.Type,
			})
		}
	}
	return items
}

func classifyIdentity(snap *schema.InspectionSnapshot, secrets map[string]bool) []TriageItem {
	var items []TriageItem
	if snap.UsersGroups != nil {
		for _, u := range snap.UsersGroups.Users {
			name, _ := u["name"].(string)
			uid, _ := u["uid"].(float64)
			isSystem := uid < 1000
			tier, reason := 2, "User-created account (UID >= 1000)."
			if isSystem {
				tier, reason = 1, "System user (UID < 1000), matches base."
			}
			items = append(items, TriageItem{
				Section: "identity", Key: "user-" + name,
				Tier: tier, Reason: reason, Name: name,
				Meta: fmt.Sprintf("UID %.0f", uid),
			})
		}
		for _, g := range snap.UsersGroups.Groups {
			name, _ := g["name"].(string)
			gid, _ := g["gid"].(float64)
			isSystem := gid < 1000
			tier, reason := 2, "User-created group."
			if isSystem {
				tier, reason = 1, "System group (GID < 1000)."
			}
			items = append(items, TriageItem{
				Section: "identity", Key: "group-" + name,
				Tier: tier, Reason: reason, Name: name,
				Meta: fmt.Sprintf("GID %.0f", gid),
			})
		}
	}
	if snap.Selinux != nil {
		for _, b := range snap.Selinux.BooleanOverrides {
			name, _ := b["name"].(string)
			val, _ := b["current_value"].(string)
			items = append(items, TriageItem{
				Section: "identity", Key: "sebool-" + name,
				Tier: 2, Reason: "SELinux boolean changed from default.",
				Name: name, Meta: val,
			})
		}
		for _, m := range snap.Selinux.CustomModules {
			items = append(items, TriageItem{
				Section: "identity", Key: "semod-" + m,
				Tier: 3, Reason: "Custom SELinux policy module.",
				Name: m,
			})
		}
		for _, p := range snap.Selinux.PortLabels {
			items = append(items, TriageItem{
				Section: "identity", Key: fmt.Sprintf("seport-%s-%s", p.Protocol, p.Port),
				Tier: 2, Reason: "Custom SELinux port label.",
				Name: fmt.Sprintf("%s/%s → %s", p.Protocol, p.Port, p.Type),
			})
		}
	}
	return items
}

func classifySystemItems(snap *schema.InspectionSnapshot, secrets map[string]bool) []TriageItem {
	var items []TriageItem
	if snap.KernelBoot != nil {
		for _, s := range snap.KernelBoot.SysctlOverrides {
			items = append(items, TriageItem{
				Section: "system", Key: "sysctl-" + s.Key,
				Tier: 2, Reason: "Custom sysctl parameter.",
				Name: s.Key, Meta: s.Value,
			})
		}
		for _, m := range snap.KernelBoot.NonDefaultModules {
			tier, reason := 2, "Kernel module loaded."
			if m.Source != "" && m.Source != "standard" {
				tier, reason = 3, "Kernel module from non-standard source."
			}
			items = append(items, TriageItem{
				Section: "system", Key: "kmod-" + m.Name,
				Tier: tier, Reason: reason, Name: m.Name, Meta: m.Source,
			})
		}
	}
	if snap.Network != nil {
		for _, conn := range snap.Network.Connections {
			items = append(items, TriageItem{
				Section: "system", Key: "conn-" + conn.Name,
				Tier: 2, Reason: "Network connection configuration.",
				Name: conn.Name, Meta: conn.Type,
			})
		}
		for _, zone := range snap.Network.FirewallZones {
			items = append(items, TriageItem{
				Section: "system", Key: "fw-" + zone.Name,
				Tier: 2, Reason: "Custom firewall zone.",
				Name: zone.Name,
			})
		}
	}
	if snap.Storage != nil {
		for _, entry := range snap.Storage.FstabEntries {
			items = append(items, TriageItem{
				Section: "system", Key: "fstab-" + entry.MountPoint,
				Tier: 2, Reason: "Non-default mount point.",
				Name: entry.MountPoint, Meta: entry.Fstype,
			})
		}
	}
	return items
}

func classifySecretItems(snap *schema.InspectionSnapshot) []TriageItem {
	var items []TriageItem
	for i, r := range snap.Redactions {
		var finding struct {
			Path        string `json:"path"`
			Name        string `json:"name"`
			FindingType string `json:"finding_type"`
			Type        string `json:"type"`
		}
		json.Unmarshal(r, &finding)
		name := finding.Path
		if name == "" {
			name = finding.Name
		}
		if name == "" {
			name = fmt.Sprintf("Redaction %d", i+1)
		}
		ftype := finding.FindingType
		if ftype == "" {
			ftype = finding.Type
		}
		items = append(items, TriageItem{
			Section:  "secrets",
			Key:      fmt.Sprintf("secret-%d", i),
			Tier:     3,
			Reason:   "Secret or credential detected: " + ftype,
			Name:     name,
			Meta:     ftype,
			IsSecret: true,
		})
	}
	return items
}

func joinNonEmpty(sep string, parts ...string) string {
	var filtered []string
	for _, p := range parts {
		if p != "" {
			filtered = append(filtered, p)
		}
	}
	return strings.Join(filtered, sep)
}
```

- [ ] **Step 2: Write table-driven triage tests**

Create `cmd/inspectah/internal/renderer/triage_test.go`:

```go
package renderer

import (
	"testing"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
	"github.com/stretchr/testify/assert"
)

func TestClassifyPackage_BaseImage(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		PackagesAdded:        []schema.PackageEntry{{Name: "coreutils", Arch: "x86_64", State: "installed", SourceRepo: "baseos", Include: true}},
		BaselinePackageNames: &[]string{"coreutils"},
	}
	items := ClassifySnapshot(snap)
	assert.Len(t, items, 1)
	assert.Equal(t, 1, items[0].Tier)
	assert.Equal(t, "packages", items[0].Section)
}

func TestClassifyPackage_ThirdParty(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		PackagesAdded: []schema.PackageEntry{{Name: "epel-pkg", Arch: "x86_64", SourceRepo: "epel", Include: true}},
	}
	items := ClassifySnapshot(snap)
	assert.Len(t, items, 1)
	assert.Equal(t, 2, items[0].Tier)
	assert.Contains(t, items[0].Reason, "Third-party")
}

func TestClassifyPackage_LocalInstall(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		PackagesAdded: []schema.PackageEntry{{Name: "mystery", Arch: "x86_64", State: "local_install", Include: true}},
	}
	items := ClassifySnapshot(snap)
	assert.Len(t, items, 1)
	assert.Equal(t, 3, items[0].Tier)
}

func TestClassifyPrecedence_HighestTierWins(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Rpm = &schema.RpmSection{
		PackagesAdded: []schema.PackageEntry{
			{Name: "local-and-base", Arch: "x86_64", State: "local_install", SourceRepo: "baseos", Include: true},
		},
		BaselinePackageNames: &[]string{"local-and-base"},
	}
	items := ClassifySnapshot(snap)
	assert.Len(t, items, 1)
	assert.Equal(t, 3, items[0].Tier, "local_install (tier 3) must win over baseline (tier 1)")
}

func TestClassifySecretPrecedence(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Config = &schema.ConfigSection{
		Files: []schema.ConfigFileEntry{
			{Path: "/etc/secret.conf", Kind: "non_rpm", Include: true},
		},
	}
	snap.Redactions = []json.RawMessage{
		json.RawMessage(`{"path":"/etc/secret.conf","finding_type":"api_key"}`),
	}
	items := ClassifySnapshot(snap)

	// Secret-flagged item should appear in secrets section only
	for _, item := range items {
		if item.Name == "/etc/secret.conf" {
			assert.Equal(t, "secrets", item.Section, "secret-flagged item must appear in secrets, not config")
			assert.Equal(t, 3, item.Tier)
		}
	}
	// Should NOT appear in config section
	for _, item := range items {
		if item.Section == "config" && item.Name == "/etc/secret.conf" {
			t.Error("secret-flagged item must not appear in config section")
		}
	}
}

func TestClassifyConfig_RpmDefault(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Config = &schema.ConfigSection{
		Files: []schema.ConfigFileEntry{
			{Path: "/etc/default.conf", Kind: schema.ConfigFileKindRpmOwnedDefault, Include: true},
		},
	}
	items := ClassifySnapshot(snap)
	assert.Len(t, items, 1)
	assert.Equal(t, 1, items[0].Tier)
}

func TestClassifyConfig_Modified(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Config = &schema.ConfigSection{
		Files: []schema.ConfigFileEntry{
			{Path: "/etc/modified.conf", Kind: schema.ConfigFileKindRpmOwnedModified, Include: true},
		},
	}
	items := ClassifySnapshot(snap)
	assert.Len(t, items, 1)
	assert.Equal(t, 2, items[0].Tier)
}

func TestClassifyConfig_QuadletExcluded(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Config = &schema.ConfigSection{
		Files: []schema.ConfigFileEntry{
			{Path: "/etc/containers/systemd/app.container", Kind: "non_rpm", Include: true},
		},
	}
	items := ClassifySnapshot(snap)
	for _, item := range items {
		if item.Section == "config" {
			t.Error("quadlet files must not appear in config section")
		}
	}
}

func TestClassifyIdentity_SystemUser(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.UsersGroups = &schema.UserGroupSection{
		Users: []map[string]interface{}{
			{"name": "root", "uid": float64(0)},
		},
	}
	items := ClassifySnapshot(snap)
	assert.Len(t, items, 1)
	assert.Equal(t, 1, items[0].Tier)
}

func TestClassifyIdentity_UserCreated(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.UsersGroups = &schema.UserGroupSection{
		Users: []map[string]interface{}{
			{"name": "appuser", "uid": float64(1001)},
		},
	}
	items := ClassifySnapshot(snap)
	assert.Len(t, items, 1)
	assert.Equal(t, 2, items[0].Tier)
}

func TestClassifyContainer_WithoutQuadlet(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Containers = &schema.ContainerSection{
		RunningContainers: []schema.RunningContainer{
			{Name: "orphan", Image: "nginx"},
		},
	}
	items := ClassifySnapshot(snap)
	assert.Len(t, items, 1)
	assert.Equal(t, 3, items[0].Tier)
	assert.Contains(t, items[0].Reason, "without quadlet")
}

func TestClassifySnapshot_EmptySnapshot(t *testing.T) {
	snap := schema.NewSnapshot()
	items := ClassifySnapshot(snap)
	assert.Empty(t, items)
}

func TestIsIncluded(t *testing.T) {
	tr := true
	fa := false
	assert.True(t, isIncluded(nil), "nil should be included (default)")
	assert.True(t, isIncluded(&tr), "true should be included")
	assert.False(t, isIncluded(&fa), "false should be excluded")
}
```

- [ ] **Step 3: Run triage tests**

```bash
cd cmd/inspectah && go test ./internal/renderer/ -run TestClassify -v
cd cmd/inspectah && go test ./internal/renderer/ -run TestIsIncluded -v
```

Expected: all PASS.

### Task 3.4: Wire triage manifest into html.go

**Files:**
- Modify: `cmd/inspectah/internal/renderer/html.go`

Update `RenderHTMLReport` to call `ClassifySnapshot` and embed the manifest:

```go
func RenderHTMLReport(snap *schema.InspectionSnapshot, outputDir string, opts HTMLReportOptions) error {
	tmpl, err := template.New("report").Parse(reportTemplate)
	if err != nil {
		return fmt.Errorf("parse report template: %w", err)
	}

	snapJSON, err := json.Marshal(snap)
	if err != nil {
		return fmt.Errorf("marshal snapshot: %w", err)
	}

	manifest := ClassifySnapshot(snap)
	manifestJSON, err := json.Marshal(manifest)
	if err != nil {
		return fmt.Errorf("marshal triage manifest: %w", err)
	}

	cfData, _ := os.ReadFile(filepath.Join(outputDir, "Containerfile"))
	cfJSON, _ := json.Marshal(string(cfData))

	data := reportData{
		PatternFlyCSS:  template.CSS(patternFlyCSS),
		CodeMirrorJS:   template.JS(codeMirrorJS),
		SnapshotJSON:   template.JS(escapeScriptClose(string(snapJSON))),
		Containerfile:  template.JS(escapeScriptClose(string(cfJSON))),
		TriageManifest: template.JS(escapeScriptClose(string(manifestJSON))),
	}

	outPath := filepath.Join(outputDir, "report.html")
	f, err := os.Create(outPath)
	if err != nil {
		return fmt.Errorf("create report.html: %w", err)
	}
	defer f.Close()

	return tmpl.Execute(f, data)
}
```

- [ ] **Step 4: Run all tests**

```bash
cd cmd/inspectah && go test ./... 2>&1 | tail -10
```

- [ ] **Step 5: Wire NormalizeSnapshot into RunRefine**

In `server.go`, add the normalization call after extraction and validation (code shown in the Schema Contract section above). This ensures the SPA always sees a fully normalized snapshot with explicit `true` on all Include fields. Test this by verifying that a v11-format snapshot served via GET /api/snapshot has all typed Include fields present and set to `true`.

- [ ] **Step 6: Wire display-only contract test into containerfile_test.go**

Add the `TestContainerfile_DisplayOnlyIncludeDoesNotAffectOutput` test shown in the Display-only Contract section above. This proves that NMConnection, FstabEntry, AtJob, and RunningContainer Include state does not affect Containerfile output.

- [ ] **Step 7: Run all tests**

```bash
cd cmd/inspectah && go test ./... 2>&1 | tail -10
```

- [ ] **Step 8: Commit Phase 3**

```bash
git add cmd/inspectah/internal/schema/ cmd/inspectah/internal/renderer/triage.go \
  cmd/inspectah/internal/renderer/triage_test.go cmd/inspectah/internal/renderer/html.go \
  cmd/inspectah/internal/renderer/containerfile.go cmd/inspectah/internal/renderer/containerfile_test.go \
  cmd/inspectah/internal/refine/server.go
git commit -m "feat(schema): *bool Include fields, normalization, Go triage engine

Schema v12: adds *bool Include to NMConnection, FstabEntry,
SystemdTimer, AtJob, RunningContainer. NormalizeSnapshot() sets
nil Include to explicit true on load — SPA always sees normalized
state. v11 snapshots are migrated at extraction time.

Go triage engine (triage.go): classifies all snapshot items into
tiers with secret-precedence and highest-severity-wins rules.
Table-driven tests cover all signal types and precedence cases.
SPA consumes the Go-produced manifest — no client-side classification.

Renderer follow-through: scheduledTasksSectionLines and
selinuxSectionLines now filter by Include on timers and booleans.
Display-only contract tested for NMConnection, FstabEntry, AtJob,
RunningContainer — Include state does not affect Containerfile."
```

---

## Phase 4: SPA Rendering + Decisions

**Commit:** `feat(renderer): SPA triage display with badge preview and review states`

The SPA consumes the Go-produced `TRIAGE_MANIFEST` for display. No client-side classification. Decisions modify `App.snapshot` include flags and track a change counter for the Containerfile preview badge.

### Key changes from Revision 2

**Manifest-driven rendering:** The SPA reads `TRIAGE_MANIFEST` (an array of `{section, key, tier, reason, name, meta, is_secret}` objects). It groups items by section and tier, renders triage cards, and tracks decisions. It never computes tiers itself.

**Badge preview instead of live Containerfile:** The preview panel shows `INITIAL_CONTAINERFILE` (last server-rendered version). A change counter updates on every decision: "N changes pending — rebuild to update preview." The Containerfile text only refreshes after a server rebuild in Phase 5.

**Review state zero-item inventory tracking (Fern fix):** `renderTriageSection()` stores the item-key inventory for ALL sections, including zero-item sections that auto-complete. This ensures rebuild comparison catches the case where a previously-empty section gains items.

### Tasks 4.1-4.6

Tasks cover:
- **4.1:** Mode detection + boot sequence (reads manifest from `TRIAGE_MANIFEST` or from rebuild response)
- **4.2:** SPA router (navigateTo, grouped sidebar keyboard, overlay trap — unchanged from Rev 2)
- **4.3:** Overview section renderer
- **4.4:** Manifest-driven triage card component + decision handling. `makeDecision()` updates `App.snapshot` include flags, increments change counter, re-renders section. `updateSnapshotInclude()` covers ALL item types using the `key` prefix to dispatch: `pkg-`, `ms-`, `cfg-`, `svc-`, `dropin-`, `cron-`, `timer-`, `atjob-`, `quadlet-`, `container-`, `nonrpm-`, `user-`, `group-`, `sebool-`, `seport-`, `sysctl-`, `kmod-`, `conn-`, `fw-`, `fstab-`. For `*bool` types, sets `item.include = true/false`. For `map[string]interface{}` types, sets `item["include"] = true/false`.
- **4.5:** All 7 section renderers (shared `renderTriageSection()` function), review state machine, theme toggle
- **4.6:** Containerfile preview badge (change counter, copy button)

**Change counter implementation:**

```javascript
let changeCount = 0;

function incrementChangeCounter() {
  changeCount++;
  const badge = document.getElementById('changes-badge');
  badge.style.display = '';
  badge.textContent = changeCount + ' change' + (changeCount !== 1 ? 's' : '') +
    ' pending — rebuild to update preview';
}

function resetChangeCounter() {
  changeCount = 0;
  document.getElementById('changes-badge').style.display = 'none';
}
```

Called by `makeDecision()` and `undoDecision()`. Reset by `triggerRebuild()` after canonical state is applied.

- [ ] **Browser checkpoint after Phase 4:** Open report.html in static mode. Verify all sections render from the manifest, tier cards display correctly, decisions collapse cards, badge counter increments, theme toggle works.

- [ ] **Commit Phase 4**

---

## Phase 5: Editor + Autosave + Rebuild

**Commit:** `feat(renderer): editor, autosave, rebuild with tarball export`

Unchanged from Revision 2 except:

- Rebuild response now includes `triage_manifest`. After applying canonical state, the SPA replaces its manifest with the new one and re-renders all sections.
- Change counter resets after successful rebuild.
- `nativeReRender` now calls `ClassifySnapshot` and includes manifest in response (shown in Phase 2 implementation above).

### Tasks 5.1-5.3

- **5.1:** Editor section covering config files + drop-ins + quadlets with keyboard-accessible file browser (unchanged from Rev 2)
- **5.2:** Autosave manager with correct aria-live behavior (unchanged from Rev 2)
- **5.3:** Rebuild + download flow. After rebuild, applies new `triage_manifest` from response, re-renders sections from fresh manifest, resets change counter. Review-state inventory comparison uses manifest keys. Focus: success → status region, failure → error message.

**Rebuild manifest refresh:**

```javascript
function applyRebuildResponse(data) {
  App.snapshot = data.snapshot;
  App.containerfile = data.containerfile;
  App.renderId = data.render_id;
  App.revision = data.revision;

  // Replace manifest with server-produced classification
  App.triageManifest = data.triage_manifest;

  // Check inventory changes for review state
  MIGRATION_SECTIONS.forEach(sec => {
    const newItems = App.triageManifest.filter(i => i.section === sec);
    const newInv = newItems.map(i => i.key).sort().join(',');
    if (App.prevInventories[sec] !== undefined && App.prevInventories[sec] !== newInv) {
      if (App.reviewStates[sec] === 'reviewed') {
        App.reviewStates[sec] = 'in-progress';
      }
    }
    App.prevInventories[sec] = newInv;
  });

  renderAllSections();
  renderContainerfilePreview();
  resetChangeCounter();
}
```

- [ ] **Browser checkpoint after Phase 5:** Test autosave persists, rebuild produces correct tarball, manifest refreshes, review states reopen on inventory change.

- [ ] **Commit Phase 5**

---

## Verification Mapping (Phases 4-5)

Every SPA behavior from the spec is mapped to a specific automated or manual proof. This table resolves Thorn R3's concern that Phases 4-5 rely too much on prose.

### Automated proofs (Go tests — run before implementation depends on them)

| SPA behavior | Proof | Test location |
|-------------|-------|---------------|
| Tier classification — all signals | Table-driven Go tests for every signal type | `triage_test.go`: TestClassifyPackage_*, TestClassifyConfig_*, TestClassifyContainer_*, etc. |
| Tier precedence — highest severity wins | Go test: local_install (tier 3) wins over baseline (tier 1) | `triage_test.go`: TestClassifyPrecedence_HighestTierWins |
| Secret precedence — secret-flagged items go to Secrets only | Go test: config file with redaction appears in secrets, not config | `triage_test.go`: TestClassifySecretPrecedence |
| Quadlet exclusion from Config | Go test: quadlet path does not appear in config section | `triage_test.go`: TestClassifyConfig_QuadletExcluded |
| `isIncluded` nil semantics | Go test: nil=true, true=true, false=false | `triage_test.go`: TestIsIncluded |
| v11 snapshot normalization | Go test: nil Include fields → explicit true after NormalizeSnapshot | `snapshot_test.go`: TestNormalizeSnapshot_SetsNilIncludeToTrue |
| Display-only Include contract | Go test: NMConnection/FstabEntry Include changes do not affect Containerfile | `containerfile_test.go`: TestContainerfile_DisplayOnlyIncludeDoesNotAffectOutput |
| Autosave continuation after reconnect | Server contract test: GET revision → PUT revision+1 → 200 | `server_test.go`: TestIntegration_ReconnectContinuation |
| Autosave continuation after rebuild | Server contract test: POST render → PUT revision+1 → 200, stale PUT → 409 | `server_test.go`: TestIntegration_RenderThenAutosave |
| Late PUT during render → 409 | Server contract test: PUT with pre-render revision during active render | `server_test.go`: TestRenderAPI_LatePutDuringRender_Gets409 |
| Silent 409 on stale revision | Server contract test: stale PUT → 409 response | `server_test.go`: TestSnapshotAPI_PutStaleRevision |
| Render response == disk == tarball | Three-way equality proof for snapshot, Containerfile, report.html | `server_test.go`: TestE2E_RenderEquality_ThreeWayProof |
| Failed render → working dir unchanged | Full directory snapshot comparison before/after failed render | `server_test.go`: TestRenderAPI_FailedRender_EntireWorkingDirUnchanged |
| Real nativeReRender produces canonical output | Exercises real nativeReRender with renderer.RunAll(), verifies full output set + response==disk | `refine_test.go`: TestNativeReRender_ProducesCanonicalOutput |
| Real nativeReRender failure preserves working dir | Exercises real nativeReRender failure path, verifies byte-identical directory | `refine_test.go`: TestNativeReRender_FailurePreservesWorkingDir |
| Server ignores client-supplied original | Server test: original field in POST /api/render is not forwarded | `server_test.go`: TestRenderAPI_AcceptsOnlySnapshot_NoOriginal |
| Cache-Control on all endpoints | Integration test covers /, health, snapshot GET/PUT, render, tarball | `server_test.go`: TestAllEndpoints_CacheControlNoStore |

### Browser-only verification (manual smoke tests — no automated path)

| SPA behavior | Why browser-only | Manual verification step |
|-------------|-----------------|------------------------|
| Review-state transitions (unreviewed → in-progress → reviewed) | Pure client-side JS state machine; no server round-trip to test | Phase 6 smoke test: make decision → dot yellow; mark reviewed → dot green; undo → dot yellow |
| Review-state reopen on rebuild inventory change | Requires DOM + server interaction in sequence | Phase 6 smoke test: mark reviewed → edit in editor → rebuild → dot reverts to yellow if inventory changed |
| Zero-item section auto-complete | Client-side DOM rendering with no server state | Phase 6 smoke test: section with no items shows "reviewed" dot automatically |
| Narrow-view overlay focus trap + Escape | DOM focus management | Phase 6 smoke test: hamburger → sidebar opens → Tab cycles within → Escape closes → focus returns |
| Grouped sidebar keyboard navigation | DOM keyboard event handling | Phase 6 smoke test: ArrowDown within group, Tab between groups |
| Theme toggle persistence | localStorage + CSS class toggle | Phase 6 smoke test: toggle → reload → persisted |
| Editor file browser keyboard nav | DOM roving tabindex | Phase 6 smoke test: ArrowDown between files, Enter opens |
| Rebuild focus landing | DOM focus management | Phase 6 smoke test: success → focus on status region; failure → focus on error |

**Rationale for browser-only items:** These are all client-side DOM/focus/CSS behaviors with no server contract to test against. Adding a headless browser test harness is explicitly out of scope for v1. The manual smoke test matrix in Phase 6 covers all of these with specific verification steps.

---

## Phase 6: Polish + Golden-File Tests

**Commit:** `feat(renderer): accessibility audit, responsive layout, golden-file tests`

Unchanged from Revision 2.

### Tasks 6.1-6.4

- **6.1:** Accessibility audit — verify all ARIA, reduced motion, focus management
- **6.2:** Narrow viewport responsive layout
- **6.3:** Golden-file tests — 3 fragments: sidebar, tier section (render with fixture snapshot containing items in all 3 tiers), Containerfile. Generate with `UPDATE_GOLDEN=1`.
- **6.4:** Final browser smoke test pass (full test matrix from spec)

- [ ] **Commit Phase 6**

---

## Summary

| Phase | Commit | Focus |
|-------|--------|-------|
| 1 | Static Foundation | Vendor assets, embed, template renderer with manifest slot, SPA skeleton |
| 2 | Server API | Revision, PUT, render_id, tarball, sidecar, E2E equality, race test, failure-safety |
| 3 | Schema + Triage | `*bool` Include (5 types), renderer follow-through, Go triage engine with table tests |
| 4 | SPA Rendering | Manifest-driven triage display, badge preview, decisions, review states, theme |
| 5 | Editor + Autosave | Editor (all file types), autosave, rebuild with manifest refresh |
| 6 | Polish + Tests | A11y audit, responsive, golden-file fragments, browser smoke tests |

**6 commits. Single classification implementation in Go. `*bool` migration-safe. Server-rendered preview with badge. Tests before risk. Accessibility inline.**

## Spec Updates Required (Prerequisites)

Both updates must be applied to the spec before implementation begins. Fern confirmation is needed on the preview contract change (UX-visible).

### 1. Preview contract — continuous → badge

> ~~Every include/exclude decision immediately updates the Containerfile preview panel.~~
> Every include/exclude decision increments the change counter in the Containerfile preview header ("N changes pending — rebuild to update preview"). The preview panel shows the last server-rendered Containerfile. Click "Download tarball" to rebuild and refresh the preview with canonical output.

### 2. Display-only decision surfaces

Add a new subsection under "Guided Triage Interaction Model":

> **Display-only surfaces (v1).** The following item types carry triage decisions that persist in the snapshot but do not affect the Containerfile or other generated artifacts in v1. Triage cards for these types use "Acknowledge / Skip" button language instead of "Include in image / Leave out" to reflect this:
>
> - Network connections (NMConnection) — included via directory-level COPY
> - Fstab entries — informational advisory comments only
> - At jobs — informational, not processed by renderer
> - Running containers — ephemeral state; quadlet files are the actionable items
> - Groups — renderer generates user accounts only, not group-specific output
>
> These surfaces are scoped for full renderer integration in a follow-up.

---

## Appendix: Recovered Revision 2 Task Details (Phases 4-6)

> **History:** The plan was iteratively revised across 7 review rounds. Revisions 1-5 existed only in the planning session's conversation context and were never committed to git. The detailed task breakdowns below were recovered from session transcript `edcde213`. Tasks superseded by later revisions are annotated.

### Phase 4: SPA Rendering + Decision Flow

> **Revision 7 changes:** Task 4.2 (JS classification) was eliminated — classification moved to Go (`triage.go`) in Phase 3. The SPA consumes `TRIAGE_MANIFEST` from the Go-produced manifest. Task 4.5 (live Containerfile preview) was replaced by the badge preview pattern — no client-side `generatePreviewContainerfile()`. The current Phase 4 section (above) describes the manifest-driven approach and badge preview. These recovered tasks should be read alongside the revision 7 Phase 4 notes.

#### Task 4.1: Mode detection, boot sequence, and SPA router

Replace the placeholder functions in report.html with full implementations:

- `detectMode()` — file:// short-circuit, /api/health check with re_render gate
- `enableStaticMode()` — show banner, disable controls with `aria-disabled`, add callouts with `aria-describedby`
- `enableRefineMode()` — fetch GET /api/snapshot, set revision, render all sections, start autosave
- `navigateTo(section)` — SPA routing with `aria-current="page"`, focus moves to section heading
- `updateProgressBar()`, `updateSidebarDot()`, `updateBadge()`, `updateAllBadges()`

#### ~~Task 4.2: Tier classification engine~~ (SUPERSEDED — now in Phase 3 Go triage engine)

~~Implement `classifySection()` and per-section classifiers using correct field names from the schema contract table.~~

> **Revision 3+ change:** Classification is now single-sourced in Go (`triage.go`). The SPA reads `TRIAGE_MANIFEST` — it never computes tiers itself. This task is fully handled by Phase 3, Task 3.3.

#### Task 4.3: Triage card component + decision handling

- `buildTriageCard()` — tier-specific card with correct button language (Secrets inverted: "Exclude from image" primary, "Keep in image (acknowledged)" secondary). Display-only types use "Acknowledge / Skip" (revision 5+).
- `buildDecidedCard()` — collapsed single-line with tier-specific labels. Display-only resting labels: "Acknowledged / Skipped" (revision 6+).
- `makeDecision()` — updates `App.snapshot` include flags, calls `updateSnapshotInclude()` for ALL item types, increments change counter, re-renders section, triggers autosave
- `undoDecision()` — removes decision, reverts review state if reviewed, decrements change counter
- `updateSnapshotInclude()` — handles ALL types using the `key` prefix to dispatch: `pkg-`, `ms-`, `cfg-`, `svc-`, `dropin-`, `cron-`, `timer-`, `atjob-`, `quadlet-`, `container-`, `nonrpm-`, `user-`, `group-`, `sebool-`, `seport-`, `sysctl-`, `kmod-`, `conn-`, `fw-`, `fstab-`. For `*bool` types, sets `item.include = true/false`. For `map[string]interface{}` types (users, groups, boolean_overrides), sets `item["include"] = true/false`.

#### Task 4.4: Section renderers

Shared `renderTriageSection(sectionName)` function used by all 7 migration-area renderers. Includes:
- Tier group rendering (3→2→1 order)
- Tier-1 collapsed by default with override visibility
- Section footer with stats and "Mark section reviewed" button
- Auto-complete for zero-item sections (stores empty inventory string for rebuild comparison — Fern fix)

#### ~~Task 4.5: Client-side Containerfile preview generation~~ (SUPERSEDED — badge preview)

> **Revision 3+ change:** Live Containerfile preview was replaced by the badge preview pattern. The Containerfile preview panel shows the last server-rendered version. A change counter badge ("N changes pending — rebuild to update preview") updates on every decision. The Containerfile text only refreshes after a server rebuild in Phase 5. No `generatePreviewContainerfile()` function.

<details>
<summary>Original revision 2 code (for reference only — do not implement)</summary>

```javascript
function generatePreviewContainerfile() {
  const snap = App.snapshot;
  const lines = [];
  const baseImage = (snap.rpm && snap.rpm.base_image) || 'ubi9';
  lines.push('FROM ' + baseImage);
  lines.push('');

  // Packages section
  const includedPkgs = (snap.rpm ? snap.rpm.packages_added || [] : [])
    .filter(p => p.include !== false)
    .map(p => p.name);
  if (includedPkgs.length > 0) {
    lines.push('# Packages');
    for (let i = 0; i < includedPkgs.length; i += 5) {
      const chunk = includedPkgs.slice(i, i + 5);
      const cont = i + 5 < includedPkgs.length ? ' \\' : '';
      if (i === 0) {
        lines.push('RUN dnf install -y ' + chunk.join(' ') + cont);
      } else {
        lines.push('    ' + chunk.join(' ') + cont);
      }
    }
    lines.push('');
  }

  // Config files
  const includedCfg = (snap.config ? snap.config.files || [] : [])
    .filter(f => f.include !== false);
  if (includedCfg.length > 0) {
    lines.push('# Configuration');
    includedCfg.forEach(f => {
      lines.push('COPY ' + f.path + ' ' + f.path);
    });
    lines.push('');
  }

  // Services
  const enabledSvcs = (snap.services ? snap.services.state_changes || [] : [])
    .filter(s => s.include !== false && s.action === 'enable');
  if (enabledSvcs.length > 0) {
    lines.push('# Services');
    lines.push('RUN systemctl enable ' + enabledSvcs.map(s => s.unit).join(' '));
    lines.push('');
  }

  return lines.join('\n');
}
```

</details>

#### Task 4.6: Review state machine + theme toggle

- Review state transitions: all 6 events from the spec table
- Zero-item sections auto-complete AND store empty inventory (Fern fix)
- `toggleTheme()` — toggles `pf-v6-theme-dark` class, persists to localStorage
- Theme restoration on page load

#### Task 4.7: Overview section renderer

Stats table from snapshot data, warnings list.

- [ ] **Browser checkpoint: verify static mode renders, refine mode enables, decisions update badge counter**

### Phase 5: Editor + Autosave + Rebuild

#### Task 5.1: Editor section

CodeMirror integration covering ALL approved file families (Kit finding):
- `App.snapshot.config.files` — config files
- `App.snapshot.services.drop_ins` — systemd drop-in files
- `App.snapshot.containers.quadlet_units` — quadlet files

File browser uses roving tabindex with ArrowUp/Down for keyboard navigation (Fern finding):

```javascript
function renderEditorFileBrowser(files, container) {
  container.innerHTML = '';
  files.forEach((f, i) => {
    const item = document.createElement('div');
    item.className = 'file-browser-item';
    item.setAttribute('role', 'option');
    item.setAttribute('tabindex', i === 0 ? '0' : '-1');
    item.textContent = f.path;
    item.onclick = () => openFileInEditor(f);
    item.onkeydown = (e) => {
      const items = Array.from(container.querySelectorAll('.file-browser-item'));
      const idx = items.indexOf(item);
      if (e.key === 'ArrowDown' && idx < items.length - 1) {
        e.preventDefault(); items[idx + 1].focus(); items[idx + 1].setAttribute('tabindex', '0'); item.setAttribute('tabindex', '-1');
      } else if (e.key === 'ArrowUp' && idx > 0) {
        e.preventDefault(); items[idx - 1].focus(); items[idx - 1].setAttribute('tabindex', '0'); item.setAttribute('tabindex', '-1');
      } else if (e.key === 'Enter') {
        e.preventDefault(); item.click();
      }
    };
    container.appendChild(item);
  });
}
```

#### Task 5.2: Autosave manager

- Debounced PUT /api/snapshot (500ms)
- Revision counter management
- Autosave status indicator — **only announce failures and recovery** (Fern finding):
  - "Saving..." shown visually but NOT announced via aria-live
  - "Saved 3s ago" shown visually but NOT announced
  - "Save failed — retrying" announced via aria-live (it IS a failure)
  - After recovery from failure: "Saved" announced once
- Silent 409 discard — no UI change, no announcement

```javascript
function setAutosaveStatus(status, isRecovery) {
  const el = document.getElementById('autosave-status');
  const liveRegion = document.getElementById('autosave-live');

  switch (status) {
    case 'saving':
      el.textContent = 'Saving...';
      break;
    case 'saved':
      el.textContent = 'Saved ' + new Date().toLocaleTimeString();
      if (isRecovery) {
        liveRegion.textContent = 'Saved';
      }
      break;
    case 'failed':
      el.textContent = 'Save failed — retrying';
      liveRegion.textContent = 'Save failed';
      break;
  }
}
```

#### Task 5.3: Rebuild + download flow

- Cancel pending autosave before POST /api/render
- Apply canonical response: replace `App.snapshot` and `App.containerfile`
- **Revision 7 addition:** Replace `App.triageManifest` with `data.triage_manifest` from rebuild response, re-render all sections from fresh manifest, reset change counter
- Inventory-aware review state check (sections with changed items reopen to in-progress)
- Success focus → rebuild status region (Fern finding), not download button
- Failure focus → error message on button (Fern finding)
- Trigger `GET /api/tarball?render_id=X` download

```javascript
// Success: focus status region, not button
document.getElementById('rebuild-status').focus();
// Failure: focus stays on button which now shows error
btn.focus();
```

- [ ] **Browser checkpoint: editor edits, autosave persists, rebuild produces correct tarball, manifest refreshes**

### Phase 6: Polish + Golden-File Tests

#### Task 6.1: Accessibility audit

Verify all ARIA attributes are correct. Add any missing:
- `aria-disabled="true"` + `aria-describedby` on all static-mode controls
- `role="option"` on editor file browser items
- Reduced-motion: all animations disabled
- Focus-next-undecided after card decision

#### Task 6.2: Narrow viewport responsive layout

- Sidebar overlay fully works (< 1200px)
- Preview panel hidden; Containerfile destination becomes the only access
- Hamburger menu with correct focus management

#### Task 6.3: Golden-file tests (3 fragments)

Add to `html_test.go`:

1. **Sidebar fragment** — extract `<nav class="sidebar">` through `</nav>`, normalize whitespace, compare to `testdata/golden-sidebar.html`
2. **Tier section fragment** — render with a snapshot containing items in all 3 tiers, extract the Packages section, compare to `testdata/golden-tier-section.html`
3. **Containerfile fragment** — render with a known snapshot, extract embedded `INITIAL_CONTAINERFILE`, compare to `testdata/golden-containerfile.txt`

Generate goldens with `UPDATE_GOLDEN=1 go test ./internal/renderer/ -run TestGolden`.

#### Task 6.4: Final browser smoke test

| Scenario | Verify |
|----------|--------|
| Static mode (file://) | Renders, banner shows, controls disabled, no console errors |
| Refine mode (http://) | Renders, controls enabled, no banner |
| Sidebar navigation | All destinations, grouped keyboard, focus moves |
| Tier rendering | Red → Yellow → Green ordering |
| Include/exclude + undo | Decision → badge increments → undo → badge decrements |
| Editor change + rebuild | Edit config → rebuild → tarball has edit → manifest refreshes |
| Mark reviewed then mutate | Review → undo decision → status reverts |
| Rebuild inventory change | Review → edit → rebuild → section reopens if inventory changed |
| Successful rebuild | Spinner → Done → tarball downloads. Focus on status region. |
| Failed rebuild | Error on button → prior state preserved. Focus on error. |
| Session resume | Download → stop → refine downloaded tarball → decisions intact |
| Theme toggle | Dark → Light → Dark. All readable. Persists across reload. |
| Narrow viewport | Hamburger → overlay → Escape closes → focus returns |
