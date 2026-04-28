# Go-Native Port Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port inspectah from a two-component architecture (Go CLI wrapper + Python container) to a single Go binary, matching all existing behavior validated by golden-file tests.

**Architecture:** Module-by-module port on a `go-port` feature branch following command-slice cutover — each CLI command stays container-backed until its full Go pipeline is ready. Schema first, then inspectors, baseline, renderers, and finally the interactive tools (refine, fleet, architect). Golden-file testing validates parity at every step.

**Tech Stack:** Go 1.21+, pongo2 (Jinja2-compatible template engine), cobra (CLI — already in use), go:embed (static asset embedding)

**Spec:** `docs/specs/proposed/2026-04-28-go-native-port-design.md`

**Guiding principle:** Reimplement, don't translate. Match *behavior* (golden files), not *structure*. Write idiomatic Go.

---

## File Structure

New packages created under `cmd/inspectah/internal/`:

```
cmd/inspectah/internal/
├── schema/
│   ├── types.go              # All Go structs (enums, sub-models, sections)
│   ├── snapshot.go            # InspectionSnapshot root + JSON load/save helpers
│   ├── types_test.go          # Enum serialization tests
│   └── snapshot_test.go       # JSON round-trip tests
├── inspector/
│   ├── executor.go            # Command execution abstraction
│   ├── executor_test.go
│   ├── orchestrator.go        # Run all inspectors in sequence
│   ├── orchestrator_test.go
│   ├── rpm.go                 # RPM inspector
│   ├── rpm_test.go
│   ├── config.go              # Config inspector
│   ├── config_test.go
│   ├── services.go            # Services inspector
│   ├── services_test.go
│   ├── network.go
│   ├── network_test.go
│   ├── storage.go
│   ├── storage_test.go
│   ├── scheduled.go           # Scheduled tasks inspector
│   ├── scheduled_test.go
│   ├── container.go           # Container workloads inspector
│   ├── container_test.go
│   ├── nonrpm.go              # Non-RPM software inspector
│   ├── nonrpm_test.go
│   ├── kernelboot.go          # Kernel/boot inspector
│   ├── kernelboot_test.go
│   ├── selinux.go
│   ├── selinux_test.go
│   ├── users.go               # Users/groups inspector
│   ├── users_test.go
│   └── testdata/              # Per-inspector test fixtures
│       ├── rpm/
│       ├── config/
│       ├── services/
│       └── ...                # One subdir per inspector
├── pipeline/
│   ├── systemtype.go          # System-type detection
│   ├── systemtype_test.go
│   ├── preflight.go           # RPM availability preflight
│   ├── preflight_test.go
│   ├── redact.go              # Pattern-based redaction engine
│   ├── redact_test.go
│   ├── heuristic.go           # Heuristic secret detection
│   ├── heuristic_test.go
│   ├── subscription.go        # RHEL subscription cert bundling
│   ├── subscription_test.go
│   ├── packaging.go           # Tarball creation + naming
│   ├── packaging_test.go
│   ├── github.go              # GitHub push flow
│   ├── github_test.go
│   ├── validate.go            # --validate (podman build check)
│   ├── run.go                 # Pipeline orchestrator (main entry)
│   └── run_test.go
├── baseline/
│   ├── mapping.go             # OS → base image mapping tables
│   ├── mapping_test.go
│   ├── resolver.go            # BaselineResolver (podman nsenter)
│   └── resolver_test.go
├── renderer/
│   ├── render.go              # RunAll entry point
│   ├── render_test.go
│   ├── containerfile/
│   │   ├── core.go            # Main Containerfile renderer
│   │   ├── core_test.go
│   │   ├── packages.go        # Package install/remove sections
│   │   ├── services.go        # Service enablement sections
│   │   ├── config.go          # Config file COPY sections
│   │   ├── network.go         # Network config sections
│   │   ├── storage.go         # Storage/fstab sections
│   │   ├── scheduled.go       # Timer generation sections
│   │   ├── containers.go      # Container workload sections
│   │   ├── nonrpm.go          # Non-RPM software sections
│   │   ├── kernelboot.go      # Kernel/boot config sections
│   │   ├── selinux.go         # SELinux sections
│   │   ├── users.go           # User/group creation sections
│   │   └── configtree.go      # Config tree writer (redacted files to config/)
│   ├── html.go                # HTML report renderer
│   ├── audit.go               # Audit report renderer
│   ├── readme.go              # README renderer
│   ├── kickstart.go           # Kickstart renderer
│   ├── secrets.go             # Secrets review renderer
│   ├── mergenotes.go          # Merge notes renderer
│   └── templates/             # pongo2 templates (embedded via go:embed)
│       ├── containerfile/
│       ├── report/            # HTML report partials
│       ├── architect/         # Architect UI HTML
│       ├── kickstart.j2
│       ├── secrets_review.j2
│       ├── audit_report.j2
│       ├── readme.j2
│       └── merge_notes.j2
├── refine/
│   ├── server.go              # HTTP refine server + re-render loop
│   ├── server_test.go
│   ├── tarball.go             # Tarball extract/repack helpers
│   └── tarball_test.go
├── fleet/
│   ├── loader.go              # Snapshot loader + validation
│   ├── loader_test.go
│   ├── merge.go               # Multi-host merge engine
│   └── merge_test.go
├── architect/
│   ├── analyzer.go            # Layer topology analyzer
│   ├── analyzer_test.go
│   ├── loader.go              # Snapshot loader + schema version gate
│   ├── loader_test.go
│   ├── export.go              # Topology → Containerfile export
│   ├── export_test.go
│   ├── server.go              # HTTP server with embedded UI
│   ├── server_test.go
│   └── static/                # Embedded web assets (go:embed)
│       └── architect.html     # Single-page app (rendered from template at build time)
├── cli/                       # EXISTING — modified during command unlocks
├── container/                 # EXISTING — removed at final cutover
├── build/                     # EXISTING — already Go-native, untouched
├── version/                   # EXISTING — DefaultImageRef pinned during port
├── platform/                  # EXISTING — untouched
├── paths/                     # EXISTING — untouched
└── errors/                    # EXISTING — untouched
```

**Testdata convention:** Each inspector's test fixtures live in `cmd/inspectah/internal/inspector/testdata/<inspector-name>/`. Golden files for integration tests live in `cmd/inspectah/internal/testdata/golden/`.

---

## Phase 0: Pre-port Groundwork

### Task 1: Create go-port branch and pin container image

**Files:**
- Modify: `cmd/inspectah/internal/version/check.go`

- [ ] **Step 1: Create the go-port feature branch**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git checkout -b go-port
```

- [ ] **Step 2: Find and read the DefaultImageRef function**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
grep -n 'DefaultImageRef\|defaultImage\|ImageRef' cmd/inspectah/internal/version/check.go
```

- [ ] **Step 3: Pin DefaultImageRef to specific 0.6.x tag**

In `cmd/inspectah/internal/version/check.go`, change the default image reference from `:latest` to a pinned 0.6.x tag. Find the constant or function that returns the default image and change it:

```go
// Before:
// const defaultImage = "ghcr.io/marrusl/inspectah:latest"
// After:
const defaultImage = "ghcr.io/marrusl/inspectah:0.6.0"
```

The exact constant name may differ — read the file and update the value that ends in `:latest`.

- [ ] **Step 4: Run existing tests**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah
go test ./... -v
```

Expected: all existing tests pass (container delegation behavior unchanged).

- [ ] **Step 5: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add cmd/inspectah/internal/version/check.go
git commit -m "chore: pin container image to 0.6.0 for go-port branch

During the port, unported commands still delegate to the container.
Pin to a specific tag for reproducible parity testing."
```

---

### Task 2: Update CI for go-port branch

**Files:**
- Modify: `.github/workflows/go-cli.yml`
- Modify: `.github/workflows/build-image.yml`
- Modify: `.github/workflows/package-release.yml`

- [ ] **Step 1: Update go-cli.yml to run on go-port branch**

In `.github/workflows/go-cli.yml`, add `go-port` to the branches list for both push and pull_request triggers:

```yaml
on:
  push:
    branches: [main, go-port]
    paths:
      - 'cmd/inspectah/**'
      - '.github/workflows/go-cli.yml'
  pull_request:
    branches: [main, go-port]
    paths:
      - 'cmd/inspectah/**'
      - '.github/workflows/go-cli.yml'
```

- [ ] **Step 2: Update build-image.yml to exclude go-port from tag builds**

In `.github/workflows/build-image.yml`, change the tag filter from `v*` to a pattern that excludes prerelease suffixes. Replace:

```yaml
tags: ["v*"]
```

With:

```yaml
tags:
  - 'v[0-9]+.[0-9]+.[0-9]+'
```

This ensures `v0.7.0-rc1` tags on go-port do NOT trigger container image builds.

- [ ] **Step 3: Add prerelease guard to package-release.yml**

In `.github/workflows/package-release.yml`, add a prerelease guard to every job. Add this condition to each job:

```yaml
jobs:
  build-srpm:
    if: ${{ !github.event.release.prerelease }}
    runs-on: ubuntu-latest
    # ...

  build-rpm:
    if: ${{ !github.event.release.prerelease }}
    needs: build-srpm
    # ...

  update-homebrew:
    if: ${{ !github.event.release.prerelease }}
    runs-on: ubuntu-latest
    # ...
```

GitHub's `release.published` event fires for prereleases too, so the trigger alone is insufficient. This guard prevents RC tags from building and publishing artifacts.

- [ ] **Step 4: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add .github/workflows/go-cli.yml .github/workflows/build-image.yml .github/workflows/package-release.yml
git commit -m "ci: configure workflows for go-port branch

- go-cli.yml: run on go-port branch
- build-image.yml: exclude prerelease tags from container builds
- package-release.yml: add prerelease guard to all jobs"
```

---

### Task 3: Triage existing specs

**Files:**
- Move: various files in `docs/specs/proposed/` → `docs/specs/implemented/`

- [ ] **Step 1: List specs in proposed/**

```bash
ls docs/specs/proposed/
```

- [ ] **Step 2: Move superseded/implemented specs**

Any specs in `proposed/` that are already implemented should be moved to `implemented/`. The go-native-port-design.md stays in proposed (it's the spec for THIS work).

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
# Move any implemented specs (check each one)
# git mv docs/specs/proposed/<implemented-spec>.md docs/specs/implemented/
```

- [ ] **Step 3: Commit if any specs were moved**

```bash
git add docs/specs/
git commit -m "docs: triage specs, move implemented to implemented/"
```

---

### Task 4: Add pongo2 dependency

**Files:**
- Modify: `cmd/inspectah/go.mod`
- Modify: `cmd/inspectah/go.sum`
- Modify: `cmd/inspectah/vendor/` (vendored dependency)

- [ ] **Step 1: Add pongo2 to go.mod**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah
go get github.com/flosch/pongo2/v6
```

- [ ] **Step 2: Vendor the dependency**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah
go mod vendor
```

- [ ] **Step 3: Verify build still works**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah
go build -o /dev/null .
```

- [ ] **Step 4: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add cmd/inspectah/go.mod cmd/inspectah/go.sum cmd/inspectah/vendor/
git commit -m "deps: add pongo2 v6 for Jinja2-compatible template rendering"
```

---

## Phase 1: Schema

Port all Pydantic models from `src/inspectah/schema.py` (726 lines) to Go structs. Everything downstream depends on these types. JSON field names must match exactly — the golden files validate this.

### Task 5: Schema enums and metadata types

**Files:**
- Create: `cmd/inspectah/internal/schema/types.go`
- Test: `cmd/inspectah/internal/schema/types_test.go`

- [ ] **Step 1: Write the failing test**

Create `cmd/inspectah/internal/schema/types_test.go`:

```go
package schema

import (
	"encoding/json"
	"testing"
)

func TestSystemTypeJSON(t *testing.T) {
	tests := []struct {
		st   SystemType
		want string
	}{
		{SystemTypePackageMode, `"package-mode"`},
		{SystemTypeRpmOstree, `"rpm-ostree"`},
		{SystemTypeBootc, `"bootc"`},
	}
	for _, tt := range tests {
		b, err := json.Marshal(tt.st)
		if err != nil {
			t.Fatalf("Marshal(%v): %v", tt.st, err)
		}
		if string(b) != tt.want {
			t.Errorf("Marshal(%v) = %s, want %s", tt.st, b, tt.want)
		}

		var got SystemType
		if err := json.Unmarshal(b, &got); err != nil {
			t.Fatalf("Unmarshal(%s): %v", b, err)
		}
		if got != tt.st {
			t.Errorf("Unmarshal(%s) = %v, want %v", b, got, tt.st)
		}
	}
}

func TestOsReleaseJSON(t *testing.T) {
	osr := OsRelease{
		Name:      "Red Hat Enterprise Linux",
		VersionID: "9.4",
		Version:   "9.4 (Plow)",
		ID:        "rhel",
		IDLike:    "fedora",
		PrettyName: "Red Hat Enterprise Linux 9.4 (Plow)",
		VariantID: "server",
	}
	b, err := json.Marshal(osr)
	if err != nil {
		t.Fatalf("Marshal: %v", err)
	}
	var got OsRelease
	if err := json.Unmarshal(b, &got); err != nil {
		t.Fatalf("Unmarshal: %v", err)
	}
	if got != osr {
		t.Errorf("round-trip mismatch:\n got: %+v\nwant: %+v", got, osr)
	}
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah
go test ./internal/schema/ -v -run 'TestSystemType|TestOsRelease'
```

Expected: FAIL — package schema does not exist yet.

- [ ] **Step 3: Write the types**

Create `cmd/inspectah/internal/schema/types.go`:

```go
package schema

import (
	"encoding/json"
	"fmt"
)

const SchemaVersion = 11

// --- System Type ---

type SystemType string

const (
	SystemTypePackageMode SystemType = "package-mode"
	SystemTypeRpmOstree   SystemType = "rpm-ostree"
	SystemTypeBootc       SystemType = "bootc"
)

func (s SystemType) MarshalJSON() ([]byte, error) {
	return json.Marshal(string(s))
}

func (s *SystemType) UnmarshalJSON(data []byte) error {
	var str string
	if err := json.Unmarshal(data, &str); err != nil {
		return err
	}
	switch SystemType(str) {
	case SystemTypePackageMode, SystemTypeRpmOstree, SystemTypeBootc:
		*s = SystemType(str)
		return nil
	default:
		return fmt.Errorf("unknown SystemType: %q", str)
	}
}

// --- Metadata ---

type OsRelease struct {
	Name      string `json:"name"`
	VersionID string `json:"version_id"`
	Version   string `json:"version"`
	ID        string `json:"id"`
	IDLike    string `json:"id_like"`
	PrettyName string `json:"pretty_name"`
	VariantID string `json:"variant_id"`
}

// --- Fleet metadata ---

type FleetPrevalence struct {
	Count int      `json:"count"`
	Total int      `json:"total"`
	Hosts []string `json:"hosts"`
}

type FleetMeta struct {
	SourceHosts   []string `json:"source_hosts"`
	TotalHosts    int      `json:"total_hosts"`
	MinPrevalence int      `json:"min_prevalence"`
}

// --- RPM enums ---

type PackageState string

const (
	PackageStateAdded        PackageState = "added"
	PackageStateBaseImageOnly PackageState = "base_image_only"
	PackageStateModified     PackageState = "modified"
)

type VersionChangeDirection string

const (
	VersionChangeUpgrade   VersionChangeDirection = "upgrade"
	VersionChangeDowngrade VersionChangeDirection = "downgrade"
)

// --- Config enums ---

type ConfigFileKind string

const (
	ConfigFileKindRpmOwnedModified ConfigFileKind = "rpm_owned_modified"
	ConfigFileKindUnowned          ConfigFileKind = "unowned"
	ConfigFileKindOrphaned         ConfigFileKind = "orphaned"
)

type ConfigCategory string

const (
	ConfigCategoryTmpfiles      ConfigCategory = "tmpfiles"
	ConfigCategoryEnvironment   ConfigCategory = "environment"
	ConfigCategoryAudit         ConfigCategory = "audit"
	ConfigCategoryLibraryPath   ConfigCategory = "library_path"
	ConfigCategoryJournal       ConfigCategory = "journal"
	ConfigCategoryLogrotate     ConfigCategory = "logrotate"
	ConfigCategoryAutomount     ConfigCategory = "automount"
	ConfigCategorySysctl        ConfigCategory = "sysctl"
	ConfigCategoryCryptoPolicy  ConfigCategory = "crypto_policy"
	ConfigCategoryIdentity      ConfigCategory = "identity"
	ConfigCategoryLimits        ConfigCategory = "limits"
	ConfigCategoryOther         ConfigCategory = "other"
)
```

- [ ] **Step 4: Run tests**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah
go test ./internal/schema/ -v -run 'TestSystemType|TestOsRelease'
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add cmd/inspectah/internal/schema/
git commit -m "feat(schema): add enums, metadata types, and OsRelease

Port SystemType, PackageState, VersionChangeDirection, ConfigFileKind,
ConfigCategory enums plus OsRelease, FleetPrevalence, FleetMeta structs
from Python Pydantic models."
```

---

### Task 6: Schema section types

**Files:**
- Modify: `cmd/inspectah/internal/schema/types.go`
- Modify: `cmd/inspectah/internal/schema/types_test.go`

Port all section sub-models and section root types. Reference: `src/inspectah/schema.py` lines 62-641.

- [ ] **Step 1: Write a round-trip test for RpmSection**

Append to `cmd/inspectah/internal/schema/types_test.go`:

```go
func TestRpmSectionJSON(t *testing.T) {
	section := RpmSection{
		PackagesAdded: []PackageEntry{
			{Name: "httpd", Epoch: "0", Version: "2.4.57", Release: "5.el9", Arch: "x86_64", State: PackageStateAdded, Include: true},
		},
		BaseImage:    strPtr("quay.io/centos-bootc/centos-bootc:stream9"),
		NoBaseline:   false,
	}
	b, err := json.Marshal(section)
	if err != nil {
		t.Fatalf("Marshal: %v", err)
	}
	var got RpmSection
	if err := json.Unmarshal(b, &got); err != nil {
		t.Fatalf("Unmarshal: %v", err)
	}
	if len(got.PackagesAdded) != 1 || got.PackagesAdded[0].Name != "httpd" {
		t.Errorf("round-trip failed for packages_added")
	}
	if got.BaseImage == nil || *got.BaseImage != "quay.io/centos-bootc/centos-bootc:stream9" {
		t.Errorf("round-trip failed for base_image")
	}
}

func strPtr(s string) *string { return &s }
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah
go test ./internal/schema/ -v -run TestRpmSection
```

Expected: FAIL — RpmSection not defined.

- [ ] **Step 3: Add all section types to types.go**

Append all remaining structs to `cmd/inspectah/internal/schema/types.go`. These map 1:1 from the Python Pydantic models in `src/inspectah/schema.py`. JSON tag names must match the Python field names exactly (snake_case). Nullable fields use pointer types. List fields use slice types with `omitempty` NOT set (empty lists serialize as `[]`, not omitted — matching Pydantic's behavior).

```go
// --- RPM sub-models ---

type VersionChange struct {
	Name        string                 `json:"name"`
	Arch        string                 `json:"arch"`
	HostVersion string                 `json:"host_version"`
	BaseVersion string                 `json:"base_version"`
	HostEpoch   string                 `json:"host_epoch"`
	BaseEpoch   string                 `json:"base_epoch"`
	Direction   VersionChangeDirection `json:"direction"`
}

type PackageEntry struct {
	Name       string            `json:"name"`
	Epoch      string            `json:"epoch"`
	Version    string            `json:"version"`
	Release    string            `json:"release"`
	Arch       string            `json:"arch"`
	State      PackageState      `json:"state"`
	Include    bool              `json:"include"`
	SourceRepo string            `json:"source_repo"`
	Fleet      *FleetPrevalence  `json:"fleet"`
}

type EnabledModuleStream struct {
	ModuleName    string           `json:"module_name"`
	Stream        string           `json:"stream"`
	Profiles      []string         `json:"profiles"`
	Include       bool             `json:"include"`
	BaselineMatch bool             `json:"baseline_match"`
	Fleet         *FleetPrevalence `json:"fleet"`
}

type VersionLockEntry struct {
	RawPattern string           `json:"raw_pattern"`
	Name       string           `json:"name"`
	Epoch      int              `json:"epoch"`
	Version    string           `json:"version"`
	Release    string           `json:"release"`
	Arch       string           `json:"arch"`
	Include    bool             `json:"include"`
	Fleet      *FleetPrevalence `json:"fleet"`
}

type RpmVaEntry struct {
	Path    string  `json:"path"`
	Flags   string  `json:"flags"`
	Package *string `json:"package"`
}

type UnverifiablePackage struct {
	Name   string `json:"name"`
	Reason string `json:"reason"`
}

type RepoStatus struct {
	RepoID           string   `json:"repo_id"`
	RepoName         string   `json:"repo_name"`
	Error            string   `json:"error"`
	AffectedPackages []string `json:"affected_packages"`
}

type PreflightResult struct {
	Status        string                `json:"status"`
	StatusReason  *string               `json:"status_reason"`
	Available     []string              `json:"available"`
	Unavailable   []string              `json:"unavailable"`
	Unverifiable  []UnverifiablePackage `json:"unverifiable"`
	DirectInstall []string              `json:"direct_install"`
	RepoUnreachable []RepoStatus        `json:"repo_unreachable"`
	BaseImage     string                `json:"base_image"`
	ReposQueried  []string              `json:"repos_queried"`
	Timestamp     string                `json:"timestamp"`
}

type OstreePackageOverride struct {
	Name     string `json:"name"`
	FromNevra string `json:"from_nevra"`
	ToNevra  string `json:"to_nevra"`
}

type RepoFile struct {
	Path          string           `json:"path"`
	Content       string           `json:"content"`
	IsDefaultRepo bool             `json:"is_default_repo"`
	Include       bool             `json:"include"`
	Fleet         *FleetPrevalence `json:"fleet"`
}

type RpmSection struct {
	PackagesAdded        []PackageEntry          `json:"packages_added"`
	BaseImageOnly        []PackageEntry          `json:"base_image_only"`
	RpmVa                []RpmVaEntry            `json:"rpm_va"`
	RepoFiles            []RepoFile              `json:"repo_files"`
	GpgKeys              []RepoFile              `json:"gpg_keys"`
	DnfHistoryRemoved    []string                `json:"dnf_history_removed"`
	VersionChanges       []VersionChange         `json:"version_changes"`
	LeafPackages         *[]string               `json:"leaf_packages"`
	AutoPackages         *[]string               `json:"auto_packages"`
	LeafDepTree          map[string][]string     `json:"leaf_dep_tree"`
	ModuleStreams         []EnabledModuleStream   `json:"module_streams"`
	VersionLocks         []VersionLockEntry      `json:"version_locks"`
	ModuleStreamConflicts []string               `json:"module_stream_conflicts"`
	BaselineModuleStreams map[string]string       `json:"baseline_module_streams"`
	VersionlockCommandOutput *string             `json:"versionlock_command_output"`
	MultiarchPackages    []string                `json:"multiarch_packages"`
	DuplicatePackages    []string                `json:"duplicate_packages"`
	RepoProvidingPackages []string               `json:"repo_providing_packages"`
	OstreeOverrides      []OstreePackageOverride  `json:"ostree_overrides"`
	OstreeRemovals       []string                `json:"ostree_removals"`
	BaseImage            *string                 `json:"base_image"`
	BaselinePackageNames *[]string               `json:"baseline_package_names"`
	NoBaseline           bool                    `json:"no_baseline"`
}

// --- Config section ---

type ConfigFileEntry struct {
	Path          string           `json:"path"`
	Kind          ConfigFileKind   `json:"kind"`
	Category      ConfigCategory   `json:"category"`
	Content       string           `json:"content"`
	RpmVaFlags    *string          `json:"rpm_va_flags"`
	Package       *string          `json:"package"`
	DiffAgainstRpm *string         `json:"diff_against_rpm"`
	Include       bool             `json:"include"`
	Tie           bool             `json:"tie"`
	TieWinner     bool             `json:"tie_winner"`
	Fleet         *FleetPrevalence `json:"fleet"`
}

type ConfigSection struct {
	Files []ConfigFileEntry `json:"files"`
}

// --- Service section ---

type ServiceStateChange struct {
	Unit          string           `json:"unit"`
	CurrentState  string           `json:"current_state"`
	DefaultState  string           `json:"default_state"`
	Action        string           `json:"action"`
	Include       bool             `json:"include"`
	OwningPackage *string          `json:"owning_package"`
	Fleet         *FleetPrevalence `json:"fleet"`
}

type SystemdDropIn struct {
	Unit      string           `json:"unit"`
	Path      string           `json:"path"`
	Content   string           `json:"content"`
	Include   bool             `json:"include"`
	Tie       bool             `json:"tie"`
	TieWinner bool             `json:"tie_winner"`
	Fleet     *FleetPrevalence `json:"fleet"`
}

type ServiceSection struct {
	StateChanges  []ServiceStateChange `json:"state_changes"`
	EnabledUnits  []string             `json:"enabled_units"`
	DisabledUnits []string             `json:"disabled_units"`
	DropIns       []SystemdDropIn      `json:"drop_ins"`
}

// --- Network section ---

type NMConnection struct {
	Path   string `json:"path"`
	Name   string `json:"name"`
	Method string `json:"method"`
	Type   string `json:"type"`
}

type FirewallZone struct {
	Path      string           `json:"path"`
	Name      string           `json:"name"`
	Content   string           `json:"content"`
	Services  []string         `json:"services"`
	Ports     []string         `json:"ports"`
	RichRules []string         `json:"rich_rules"`
	Include   bool             `json:"include"`
	Fleet     *FleetPrevalence `json:"fleet"`
}

type FirewallDirectRule struct {
	IPV      string `json:"ipv"`
	Table    string `json:"table"`
	Chain    string `json:"chain"`
	Priority string `json:"priority"`
	Args     string `json:"args"`
	Include  bool   `json:"include"`
}

type StaticRouteFile struct {
	Path string `json:"path"`
	Name string `json:"name"`
}

type ProxyEntry struct {
	Source string `json:"source"`
	Line   string `json:"line"`
}

type NetworkSection struct {
	Connections        []NMConnection       `json:"connections"`
	FirewallZones      []FirewallZone       `json:"firewall_zones"`
	FirewallDirectRules []FirewallDirectRule `json:"firewall_direct_rules"`
	StaticRoutes       []StaticRouteFile    `json:"static_routes"`
	IPRoutes           []string             `json:"ip_routes"`
	IPRules            []string             `json:"ip_rules"`
	ResolvProvenance   string               `json:"resolv_provenance"`
	HostsAdditions     []string             `json:"hosts_additions"`
	Proxy              []ProxyEntry         `json:"proxy"`
}

// --- Storage section ---

type FstabEntry struct {
	Device     string `json:"device"`
	MountPoint string `json:"mount_point"`
	Fstype     string `json:"fstype"`
	Options    string `json:"options"`
}

type CredentialRef struct {
	MountPoint     string `json:"mount_point"`
	CredentialPath string `json:"credential_path"`
	Source         string `json:"source"`
}

type MountPoint struct {
	Target  string `json:"target"`
	Source  string `json:"source"`
	Fstype  string `json:"fstype"`
	Options string `json:"options"`
}

type LvmVolume struct {
	LvName string `json:"lv_name"`
	VgName string `json:"vg_name"`
	LvSize string `json:"lv_size"`
}

type VarDirectory struct {
	Path           string `json:"path"`
	SizeEstimate   string `json:"size_estimate"`
	Recommendation string `json:"recommendation"`
}

type StorageSection struct {
	FstabEntries   []FstabEntry    `json:"fstab_entries"`
	MountPoints    []MountPoint    `json:"mount_points"`
	LvmInfo        []LvmVolume     `json:"lvm_info"`
	VarDirectories []VarDirectory  `json:"var_directories"`
	CredentialRefs []CredentialRef  `json:"credential_refs"`
}

// --- Scheduled task section ---

type CronJob struct {
	Path     string           `json:"path"`
	Source   string           `json:"source"`
	RpmOwned bool             `json:"rpm_owned"`
	Include  bool             `json:"include"`
	Fleet    *FleetPrevalence `json:"fleet"`
}

type SystemdTimer struct {
	Name           string `json:"name"`
	OnCalendar     string `json:"on_calendar"`
	ExecStart      string `json:"exec_start"`
	Description    string `json:"description"`
	Source         string `json:"source"`
	Path           string `json:"path"`
	TimerContent   string `json:"timer_content"`
	ServiceContent string `json:"service_content"`
}

type AtJob struct {
	File       string `json:"file"`
	Command    string `json:"command"`
	User       string `json:"user"`
	WorkingDir string `json:"working_dir"`
}

type GeneratedTimerUnit struct {
	Name           string           `json:"name"`
	TimerContent   string           `json:"timer_content"`
	ServiceContent string           `json:"service_content"`
	CronExpr       string           `json:"cron_expr"`
	SourcePath     string           `json:"source_path"`
	Command        string           `json:"command"`
	Include        bool             `json:"include"`
	Fleet          *FleetPrevalence `json:"fleet"`
}

type ScheduledTaskSection struct {
	CronJobs            []CronJob            `json:"cron_jobs"`
	SystemdTimers       []SystemdTimer       `json:"systemd_timers"`
	AtJobs              []AtJob              `json:"at_jobs"`
	GeneratedTimerUnits []GeneratedTimerUnit `json:"generated_timer_units"`
}

// --- Container section ---

type ContainerMount struct {
	Type        string `json:"type"`
	Source      string `json:"source"`
	Destination string `json:"destination"`
	Mode        string `json:"mode"`
	RW          bool   `json:"rw"`
}

type QuadletUnit struct {
	Path      string           `json:"path"`
	Name      string           `json:"name"`
	Content   string           `json:"content"`
	Image     string           `json:"image"`
	Include   bool             `json:"include"`
	Tie       bool             `json:"tie"`
	TieWinner bool             `json:"tie_winner"`
	Fleet     *FleetPrevalence `json:"fleet"`
}

type ComposeService struct {
	Service string `json:"service"`
	Image   string `json:"image"`
}

type ComposeFile struct {
	Path      string           `json:"path"`
	Images    []ComposeService `json:"images"`
	Include   bool             `json:"include"`
	Tie       bool             `json:"tie"`
	TieWinner bool             `json:"tie_winner"`
	Fleet     *FleetPrevalence `json:"fleet"`
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
}

type FlatpakApp struct {
	AppID   string `json:"app_id"`
	Origin  string `json:"origin"`
	Branch  string `json:"branch"`
	Include bool   `json:"include"`
}

type ContainerSection struct {
	QuadletUnits      []QuadletUnit      `json:"quadlet_units"`
	ComposeFiles      []ComposeFile      `json:"compose_files"`
	RunningContainers []RunningContainer `json:"running_containers"`
	FlatpakApps       []FlatpakApp       `json:"flatpak_apps"`
}

// --- Non-RPM software section ---

type PipPackage struct {
	Name    string `json:"name"`
	Version string `json:"version"`
}

type NonRpmItem struct {
	Path               string           `json:"path"`
	Name               string           `json:"name"`
	Method             string           `json:"method"`
	Confidence         string           `json:"confidence"`
	Include            bool             `json:"include"`
	Lang               string           `json:"lang"`
	Static             bool             `json:"static"`
	Version            string           `json:"version"`
	SharedLibs         []string         `json:"shared_libs"`
	SystemSitePackages bool             `json:"system_site_packages"`
	Packages           []PipPackage     `json:"packages"`
	HasCExtensions     bool             `json:"has_c_extensions"`
	GitRemote          string           `json:"git_remote"`
	GitCommit          string           `json:"git_commit"`
	GitBranch          string           `json:"git_branch"`
	Files              map[string]interface{} `json:"files"`
	Content            string           `json:"content"`
	Fleet              *FleetPrevalence `json:"fleet"`
}

type NonRpmSoftwareSection struct {
	Items    []NonRpmItem      `json:"items"`
	EnvFiles []ConfigFileEntry `json:"env_files"`
}

// --- Kernel/Boot section ---

type ConfigSnippet struct {
	Path    string `json:"path"`
	Content string `json:"content"`
}

type SysctlOverride struct {
	Key     string `json:"key"`
	Runtime string `json:"runtime"`
	Default string `json:"default"`
	Source  string `json:"source"`
	Include bool   `json:"include"`
}

type KernelModule struct {
	Name    string `json:"name"`
	Size    string `json:"size"`
	UsedBy  string `json:"used_by"`
	Include bool   `json:"include"`
}

type AlternativeEntry struct {
	Name   string `json:"name"`
	Path   string `json:"path"`
	Status string `json:"status"`
}

type KernelBootSection struct {
	Cmdline              string            `json:"cmdline"`
	GrubDefaults         string            `json:"grub_defaults"`
	SysctlOverrides      []SysctlOverride  `json:"sysctl_overrides"`
	ModulesLoadD         []ConfigSnippet   `json:"modules_load_d"`
	ModprobeD            []ConfigSnippet   `json:"modprobe_d"`
	DracutConf           []ConfigSnippet   `json:"dracut_conf"`
	LoadedModules        []KernelModule    `json:"loaded_modules"`
	NonDefaultModules    []KernelModule    `json:"non_default_modules"`
	TunedActive          string            `json:"tuned_active"`
	TunedCustomProfiles  []ConfigSnippet   `json:"tuned_custom_profiles"`
	Locale               *string           `json:"locale"`
	Timezone             *string           `json:"timezone"`
	Alternatives         []AlternativeEntry `json:"alternatives"`
}

// --- SELinux section ---

type SelinuxPortLabel struct {
	Protocol string           `json:"protocol"`
	Port     string           `json:"port"`
	Type     string           `json:"type"`
	Include  bool             `json:"include"`
	Fleet    *FleetPrevalence `json:"fleet"`
}

type SelinuxSection struct {
	Mode              string             `json:"mode"`
	CustomModules     []string           `json:"custom_modules"`
	BooleanOverrides  []map[string]interface{} `json:"boolean_overrides"`
	FcontextRules     []string           `json:"fcontext_rules"`
	AuditRules        []string           `json:"audit_rules"`
	FipsMode          bool               `json:"fips_mode"`
	PamConfigs        []string           `json:"pam_configs"`
	PortLabels        []SelinuxPortLabel `json:"port_labels"`
}

// --- Users/Groups section ---

type UserGroupSection struct {
	Users                 []map[string]interface{} `json:"users"`
	Groups                []map[string]interface{} `json:"groups"`
	SudoersRules          []string                 `json:"sudoers_rules"`
	SSHAuthorizedKeysRefs []map[string]interface{} `json:"ssh_authorized_keys_refs"`
	PasswdEntries         []string                 `json:"passwd_entries"`
	ShadowEntries         []string                 `json:"shadow_entries"`
	GroupEntries          []string                 `json:"group_entries"`
	GshadowEntries        []string                 `json:"gshadow_entries"`
	SubuidEntries         []string                 `json:"subuid_entries"`
	SubgidEntries         []string                 `json:"subgid_entries"`
}

// --- Redaction ---

type RedactionFinding struct {
	Path            string  `json:"path"`
	Source          string  `json:"source"`
	Kind            string  `json:"kind"`
	Pattern         string  `json:"pattern"`
	Remediation     string  `json:"remediation"`
	Line            *int    `json:"line"`
	Replacement     *string `json:"replacement"`
	DetectionMethod string  `json:"detection_method"`
	Confidence      *string `json:"confidence"`
}
```

- [ ] **Step 4: Run tests**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah
go test ./internal/schema/ -v
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add cmd/inspectah/internal/schema/types.go cmd/inspectah/internal/schema/types_test.go
git commit -m "feat(schema): add all section types and sub-models

Port RPM, Config, Service, Network, Storage, ScheduledTask, Container,
NonRpmSoftware, KernelBoot, SELinux, UserGroup sections plus
PreflightResult and RedactionFinding from Pydantic models."
```

---

### Task 7: InspectionSnapshot root type and JSON round-trip

**Files:**
- Create: `cmd/inspectah/internal/schema/snapshot.go`
- Create: `cmd/inspectah/internal/schema/snapshot_test.go`
- Create: `cmd/inspectah/internal/schema/testdata/minimal-snapshot.json`

- [ ] **Step 1: Create a minimal golden-file snapshot for testing**

Create `cmd/inspectah/internal/schema/testdata/minimal-snapshot.json` by running a Python snapshot and capturing JSON output, or manually construct a minimal valid snapshot:

```json
{
  "schema_version": 11,
  "meta": {"hostname": "test-host", "timestamp": "2026-01-01T00:00:00Z"},
  "os_release": {"name": "Red Hat Enterprise Linux", "version_id": "9.4", "version": "9.4 (Plow)", "id": "rhel", "id_like": "fedora", "pretty_name": "RHEL 9.4", "variant_id": ""},
  "system_type": "package-mode",
  "rpm": null,
  "config": null,
  "services": null,
  "network": null,
  "storage": null,
  "scheduled_tasks": null,
  "containers": null,
  "non_rpm_software": null,
  "kernel_boot": null,
  "selinux": null,
  "users_groups": null,
  "preflight": {"status": "skipped", "status_reason": null, "available": [], "unavailable": [], "unverifiable": [], "direct_install": [], "repo_unreachable": [], "base_image": "", "repos_queried": [], "timestamp": ""},
  "warnings": [],
  "redactions": []
}
```

- [ ] **Step 2: Write the failing test**

Create `cmd/inspectah/internal/schema/snapshot_test.go`:

```go
package schema

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
)

func TestSnapshotRoundTrip(t *testing.T) {
	data, err := os.ReadFile(filepath.Join("testdata", "minimal-snapshot.json"))
	if err != nil {
		t.Fatalf("read golden file: %v", err)
	}
	var snap InspectionSnapshot
	if err := json.Unmarshal(data, &snap); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}
	if snap.SchemaVersion != SchemaVersion {
		t.Errorf("schema_version = %d, want %d", snap.SchemaVersion, SchemaVersion)
	}
	if snap.SystemType != SystemTypePackageMode {
		t.Errorf("system_type = %q, want %q", snap.SystemType, SystemTypePackageMode)
	}
	if snap.Meta["hostname"] != "test-host" {
		t.Errorf("meta.hostname = %v, want test-host", snap.Meta["hostname"])
	}

	// Re-marshal and compare structure
	out, err := json.Marshal(snap)
	if err != nil {
		t.Fatalf("re-marshal: %v", err)
	}
	var original, roundTripped map[string]interface{}
	json.Unmarshal(data, &original)
	json.Unmarshal(out, &roundTripped)

	origBytes, _ := json.MarshalIndent(original, "", "  ")
	rtBytes, _ := json.MarshalIndent(roundTripped, "", "  ")
	if string(origBytes) != string(rtBytes) {
		t.Errorf("round-trip mismatch:\noriginal:\n%s\n\nround-tripped:\n%s", origBytes, rtBytes)
	}
}

func TestLoadSnapshot(t *testing.T) {
	snap, err := LoadSnapshot(filepath.Join("testdata", "minimal-snapshot.json"))
	if err != nil {
		t.Fatalf("LoadSnapshot: %v", err)
	}
	if snap.SchemaVersion != SchemaVersion {
		t.Errorf("schema_version = %d, want %d", snap.SchemaVersion, SchemaVersion)
	}
}

func TestLoadSnapshotVersionMismatch(t *testing.T) {
	data := []byte(`{"schema_version": 999}`)
	tmp := t.TempDir()
	path := filepath.Join(tmp, "bad.json")
	os.WriteFile(path, data, 0644)

	_, err := LoadSnapshot(path)
	if err == nil {
		t.Fatal("expected error for schema version mismatch")
	}
}

func TestSaveSnapshot(t *testing.T) {
	snap := NewSnapshot()
	snap.Meta["hostname"] = "test"
	tmp := t.TempDir()
	path := filepath.Join(tmp, "out.json")
	if err := SaveSnapshot(snap, path); err != nil {
		t.Fatalf("SaveSnapshot: %v", err)
	}
	loaded, err := LoadSnapshot(path)
	if err != nil {
		t.Fatalf("LoadSnapshot: %v", err)
	}
	if loaded.Meta["hostname"] != "test" {
		t.Errorf("hostname = %v, want test", loaded.Meta["hostname"])
	}
}
```

- [ ] **Step 3: Run test to verify it fails**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah
go test ./internal/schema/ -v -run 'TestSnapshot|TestLoad|TestSave'
```

Expected: FAIL — InspectionSnapshot, LoadSnapshot, SaveSnapshot not defined.

- [ ] **Step 4: Implement InspectionSnapshot and helpers**

Create `cmd/inspectah/internal/schema/snapshot.go`:

```go
package schema

import (
	"encoding/json"
	"fmt"
	"os"
)

type InspectionSnapshot struct {
	SchemaVersion  int                    `json:"schema_version"`
	Meta           map[string]interface{} `json:"meta"`
	OsRelease      *OsRelease             `json:"os_release"`
	SystemType     SystemType             `json:"system_type"`
	Rpm            *RpmSection            `json:"rpm"`
	Config         *ConfigSection         `json:"config"`
	Services       *ServiceSection        `json:"services"`
	Network        *NetworkSection        `json:"network"`
	Storage        *StorageSection        `json:"storage"`
	ScheduledTasks *ScheduledTaskSection  `json:"scheduled_tasks"`
	Containers     *ContainerSection      `json:"containers"`
	NonRpmSoftware *NonRpmSoftwareSection `json:"non_rpm_software"`
	KernelBoot     *KernelBootSection     `json:"kernel_boot"`
	Selinux        *SelinuxSection        `json:"selinux"`
	UsersGroups    *UserGroupSection      `json:"users_groups"`
	Preflight      PreflightResult        `json:"preflight"`
	Warnings       []map[string]interface{} `json:"warnings"`
	Redactions     []json.RawMessage      `json:"redactions"`
}

func NewSnapshot() *InspectionSnapshot {
	return &InspectionSnapshot{
		SchemaVersion: SchemaVersion,
		Meta:          make(map[string]interface{}),
		SystemType:    SystemTypePackageMode,
		Preflight:     PreflightResult{Status: "skipped"},
		Warnings:      []map[string]interface{}{},
		Redactions:    []json.RawMessage{},
	}
}

func LoadSnapshot(path string) (*InspectionSnapshot, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("read snapshot: %w", err)
	}
	var snap InspectionSnapshot
	if err := json.Unmarshal(data, &snap); err != nil {
		return nil, fmt.Errorf("parse snapshot: %w", err)
	}
	if snap.SchemaVersion != SchemaVersion {
		return nil, fmt.Errorf(
			"snapshot schema version %d does not match inspectah version %d — re-scan the host with this version of inspectah",
			snap.SchemaVersion, SchemaVersion,
		)
	}
	return &snap, nil
}

func SaveSnapshot(snap *InspectionSnapshot, path string) error {
	data, err := json.MarshalIndent(snap, "", "  ")
	if err != nil {
		return fmt.Errorf("marshal snapshot: %w", err)
	}
	return os.WriteFile(path, data, 0644)
}
```

Note: `Redactions` uses `[]json.RawMessage` to handle the union type (RedactionFinding or plain dict). Add a helper to parse individual redactions:

```go
func ParseRedaction(raw json.RawMessage) (*RedactionFinding, error) {
	var finding RedactionFinding
	if err := json.Unmarshal(raw, &finding); err != nil {
		return nil, err
	}
	if finding.Source == "" || finding.Kind == "" {
		return nil, fmt.Errorf("not a RedactionFinding")
	}
	return &finding, nil
}
```

- [ ] **Step 5: Run tests**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah
go test ./internal/schema/ -v
```

Expected: PASS. Fix any JSON field name mismatches — these indicate parity issues with the Python schema.

- [ ] **Step 6: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add cmd/inspectah/internal/schema/
git commit -m "feat(schema): add InspectionSnapshot root type and JSON helpers

Includes LoadSnapshot with schema-version enforcement,
SaveSnapshot, and NewSnapshot constructor. Golden-file round-trip
test validates JSON field name parity with Python schema."
```

---

### Task 8: RPM spec rewrite for Go-native binary

**Files:**
- Modify: `packaging/inspectah.spec`

- [ ] **Step 1: Read the current spec**

Read `packaging/inspectah.spec` to understand the current structure.

- [ ] **Step 2: Update the spec for Go-native binary**

The spec already uses `go build` with ldflags — it was written for the wrapper binary. The key changes for the Go-native port:

1. Bump `Version:` to `0.7.0`
2. Update `%description` to remove references to container lifecycle management
3. Keep `Requires: podman >= 4.4` (still needed for scan baseline + build)
4. Keep `Conflicts: python3-inspectah`
5. Keep the shell completions generation
6. Ensure the build command path and ldflags are correct

```spec
Version:        0.7.0

%description
inspectah inspects package-based RHEL, CentOS, and Fedora hosts and
produces bootc-compatible image artifacts including Containerfiles,
configuration trees, and migration reports.

Install via dnf, run inspectah scan, and the tool handles host
inspection and artifact generation.
```

- [ ] **Step 3: Verify the spec builds locally (if rpmbuild available)**

```bash
# If rpmbuild is available:
# rpmbuild --nobuild packaging/inspectah.spec
```

- [ ] **Step 4: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add packaging/inspectah.spec
git commit -m "chore(packaging): rewrite RPM spec for Go-native binary

Bump to 0.7.0, remove container lifecycle references from description.
Build pipeline and runtime deps unchanged."
```

---

## Phase 2: Inspectors

Port all 11 inspectors plus system-type detection and RPM preflight. Each inspector is a leaf node — it reads host files / runs commands and produces a schema section. Port one at a time, validate parity via golden-file testing.

### Task 9: Executor abstraction and test helpers

**Files:**
- Create: `cmd/inspectah/internal/inspector/executor.go`
- Create: `cmd/inspectah/internal/inspector/executor_test.go`

The executor wraps `os/exec` for running commands against the host. In native mode, commands run directly. For testing, a fake executor returns canned output from fixture files.

- [ ] **Step 1: Write the failing test**

Create `cmd/inspectah/internal/inspector/executor_test.go`:

```go
package inspector

import "testing"

func TestFakeExecutor(t *testing.T) {
	fake := NewFakeExecutor(map[string]ExecResult{
		"rpm -qa": {Stdout: "httpd-2.4.57-5.el9.x86_64\n", ExitCode: 0},
	})
	result := fake.Run("rpm", "-qa")
	if result.ExitCode != 0 {
		t.Fatalf("exit code = %d, want 0", result.ExitCode)
	}
	if result.Stdout != "httpd-2.4.57-5.el9.x86_64\n" {
		t.Errorf("stdout = %q, want httpd line", result.Stdout)
	}
}

func TestFakeExecutorUnknownCommand(t *testing.T) {
	fake := NewFakeExecutor(map[string]ExecResult{})
	result := fake.Run("unknown", "command")
	if result.ExitCode == 0 {
		t.Error("expected non-zero exit code for unknown command")
	}
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah
go test ./internal/inspector/ -v -run TestFakeExecutor
```

Expected: FAIL — package doesn't exist.

- [ ] **Step 3: Implement executor**

Create `cmd/inspectah/internal/inspector/executor.go`:

```go
package inspector

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
)

type ExecResult struct {
	Stdout   string
	Stderr   string
	ExitCode int
}

type Executor interface {
	Run(name string, args ...string) ExecResult
	ReadFile(path string) (string, error)
	FileExists(path string) bool
	ReadDir(path string) ([]os.DirEntry, error)
	HostRoot() string
}

type RealExecutor struct {
	hostRoot string
}

func NewRealExecutor(hostRoot string) *RealExecutor {
	return &RealExecutor{hostRoot: hostRoot}
}

func (e *RealExecutor) HostRoot() string { return e.hostRoot }

func (e *RealExecutor) Run(name string, args ...string) ExecResult {
	cmd := exec.Command(name, args...)
	var stdout, stderr strings.Builder
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr
	err := cmd.Run()
	exitCode := 0
	if err != nil {
		if exitErr, ok := err.(*exec.ExitError); ok {
			exitCode = exitErr.ExitCode()
		} else {
			exitCode = 1
		}
	}
	return ExecResult{
		Stdout:   stdout.String(),
		Stderr:   stderr.String(),
		ExitCode: exitCode,
	}
}

func (e *RealExecutor) ReadFile(path string) (string, error) {
	full := filepath.Join(e.hostRoot, path)
	data, err := os.ReadFile(full)
	if err != nil {
		return "", err
	}
	return string(data), nil
}

func (e *RealExecutor) FileExists(path string) bool {
	full := filepath.Join(e.hostRoot, path)
	_, err := os.Stat(full)
	return err == nil
}

func (e *RealExecutor) ReadDir(path string) ([]os.DirEntry, error) {
	full := filepath.Join(e.hostRoot, path)
	return os.ReadDir(full)
}

// FakeExecutor for testing — returns canned results keyed by command string.
type FakeExecutor struct {
	commands map[string]ExecResult
	files    map[string]string
	hostRoot string
}

func NewFakeExecutor(commands map[string]ExecResult) *FakeExecutor {
	return &FakeExecutor{
		commands: commands,
		files:    make(map[string]string),
		hostRoot: "/fake-host",
	}
}

func (e *FakeExecutor) WithFiles(files map[string]string) *FakeExecutor {
	e.files = files
	return e
}

func (e *FakeExecutor) HostRoot() string { return e.hostRoot }

func (e *FakeExecutor) Run(name string, args ...string) ExecResult {
	key := name + " " + strings.Join(args, " ")
	key = strings.TrimSpace(key)
	if result, ok := e.commands[key]; ok {
		return result
	}
	return ExecResult{Stderr: fmt.Sprintf("fake: command not found: %s", key), ExitCode: 127}
}

func (e *FakeExecutor) ReadFile(path string) (string, error) {
	if content, ok := e.files[path]; ok {
		return content, nil
	}
	return "", fmt.Errorf("fake: file not found: %s", path)
}

func (e *FakeExecutor) FileExists(path string) bool {
	_, ok := e.files[path]
	return ok
}

func (e *FakeExecutor) ReadDir(path string) ([]os.DirEntry, error) {
	return nil, fmt.Errorf("fake: ReadDir not implemented for %s", path)
}
```

- [ ] **Step 4: Run tests**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah
go test ./internal/inspector/ -v
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add cmd/inspectah/internal/inspector/
git commit -m "feat(inspector): add Executor abstraction and FakeExecutor for testing

Executor interface wraps os/exec for host commands and filesystem
reads. FakeExecutor returns canned results for unit testing."
```

---

### Task 10: System-type detection

**Files:**
- Create: `cmd/inspectah/internal/pipeline/systemtype.go`
- Create: `cmd/inspectah/internal/pipeline/systemtype_test.go`

Port from `src/inspectah/system_type.py`. Detects whether the host is package-mode, rpm-ostree, or bootc. Also handles base-image mapping for ostree systems.

- [ ] **Step 1: Write the failing test**

Create `cmd/inspectah/internal/pipeline/systemtype_test.go`:

```go
package pipeline

import (
	"testing"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/inspector"
	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
)

func TestDetectSystemType_PackageMode(t *testing.T) {
	exec := inspector.NewFakeExecutor(map[string]inspector.ExecResult{
		"rpm -q ostree": {Stderr: "package ostree is not installed\n", ExitCode: 1},
	})
	st, err := DetectSystemType(exec)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if st != schema.SystemTypePackageMode {
		t.Errorf("got %q, want %q", st, schema.SystemTypePackageMode)
	}
}

func TestDetectSystemType_Bootc(t *testing.T) {
	exec := inspector.NewFakeExecutor(map[string]inspector.ExecResult{
		"rpm -q ostree":    {Stdout: "ostree-2024.1-1.el9.x86_64\n", ExitCode: 0},
		"bootc status":     {Stdout: "running\n", ExitCode: 0},
	})
	st, err := DetectSystemType(exec)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if st != schema.SystemTypeBootc {
		t.Errorf("got %q, want %q", st, schema.SystemTypeBootc)
	}
}

func TestDetectSystemType_RpmOstree(t *testing.T) {
	exec := inspector.NewFakeExecutor(map[string]inspector.ExecResult{
		"rpm -q ostree":    {Stdout: "ostree-2024.1-1.el9.x86_64\n", ExitCode: 0},
		"bootc status":     {Stderr: "not found\n", ExitCode: 127},
		"rpm-ostree status": {Stdout: "State: idle\n", ExitCode: 0},
	})
	st, err := DetectSystemType(exec)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if st != schema.SystemTypeRpmOstree {
		t.Errorf("got %q, want %q", st, schema.SystemTypeRpmOstree)
	}
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah
go test ./internal/pipeline/ -v -run TestDetectSystemType
```

Expected: FAIL — package pipeline doesn't exist.

- [ ] **Step 3: Implement system-type detection**

Create `cmd/inspectah/internal/pipeline/systemtype.go`. Reference `src/inspectah/system_type.py` for the detection logic. The key decision tree is:

1. `rpm -q ostree` → if not installed → `package-mode`
2. `bootc status` → if succeeds → `bootc`
3. `rpm-ostree status` → if succeeds → `rpm-ostree`
4. ostree installed but neither bootc nor rpm-ostree → error (unknown ostree)

Also port `MapOstreeBaseImage()` which maps os-release data to the corresponding bootc base image reference. Reference `src/inspectah/system_type.py` lines 60-150 for the mapping table.

```go
package pipeline

import (
	"fmt"
	"strings"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/inspector"
	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
)

type OstreeDetectionError struct {
	Message string
}

func (e *OstreeDetectionError) Error() string { return e.Message }

func DetectSystemType(exec inspector.Executor) (schema.SystemType, error) {
	// Check if ostree is installed
	result := exec.Run("rpm", "-q", "ostree")
	if result.ExitCode != 0 {
		return schema.SystemTypePackageMode, nil
	}

	// Check for bootc
	result = exec.Run("bootc", "status")
	if result.ExitCode == 0 {
		return schema.SystemTypeBootc, nil
	}

	// Check for rpm-ostree
	result = exec.Run("rpm-ostree", "status")
	if result.ExitCode == 0 {
		return schema.SystemTypeRpmOstree, nil
	}

	return "", &OstreeDetectionError{
		Message: "ostree is installed but neither bootc nor rpm-ostree is functional. " +
			"inspectah cannot determine the system type. " +
			"This system may use an unsupported ostree deployment method.",
	}
}

func MapOstreeBaseImage(
	osRelease *schema.OsRelease,
	systemType schema.SystemType,
	targetImageOverride string,
) (string, error) {
	if targetImageOverride != "" {
		return targetImageOverride, nil
	}
	if osRelease == nil {
		return "", fmt.Errorf("os-release required for base image mapping")
	}
	// Port the mapping table from system_type.py — maps (os_id, version) → base image
	// Reference: src/inspectah/system_type.py lines 60-150
	osID := strings.ToLower(osRelease.ID)
	ver := osRelease.VersionID

	switch osID {
	case "rhel":
		major := strings.Split(ver, ".")[0]
		switch major {
		case "9":
			return "registry.redhat.io/rhel9/rhel-bootc:9-" + ver, nil
		case "10":
			return "registry.redhat.io/rhel10-beta/rhel-bootc:10-" + ver, nil
		}
	case "centos":
		if strings.Contains(ver, "stream") || strings.HasPrefix(ver, "9") {
			return "quay.io/centos-bootc/centos-bootc:stream9", nil
		}
		if strings.HasPrefix(ver, "10") {
			return "quay.io/centos-bootc/centos-bootc:stream10", nil
		}
	case "fedora":
		return "quay.io/fedora/fedora-bootc:" + ver, nil
	}

	return "", nil // Unknown — caller decides whether to error or degrade
}
```

- [ ] **Step 4: Run tests**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah
go test ./internal/pipeline/ -v
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add cmd/inspectah/internal/pipeline/
git commit -m "feat(pipeline): add system-type detection and base image mapping

Port detect_system_type() and map_ostree_base_image() from Python.
Detection tree: ostree installed? → bootc status? → rpm-ostree status?
Unknown ostree systems produce a hard error."
```

---

### Tasks 11-21: Individual Inspectors

Each inspector follows the same pattern:

1. **Read the Python source** in `src/inspectah/inspectors/<name>.py` to understand what commands it runs and what data it parses
2. **Create golden-file fixtures** by running the Python version on reference inputs and capturing the output JSON section
3. **Write tests** using FakeExecutor with canned command outputs matching the fixtures
4. **Implement** the Go inspector function
5. **Validate** that Go output matches the golden-file JSON

Each inspector function has this signature:

```go
func Run<Name>(exec Executor) (*schema.<Name>Section, []map[string]interface{}, error)
```

Returns: (section data, warnings list, error).

Below is the task for each inspector. For each one, the "Python reference" column tells you which file to study and what key commands/files it reads.

#### Task 11: RPM Inspector

**Files:**
- Create: `cmd/inspectah/internal/inspector/rpm.go`
- Create: `cmd/inspectah/internal/inspector/rpm_test.go`
- Create: `cmd/inspectah/internal/inspector/testdata/rpm/`

**Python reference:** `src/inspectah/inspectors/rpm.py`

**Key operations to port:**
- `rpm -qa --queryformat '%{EPOCH}:%{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}\n'` — installed packages
- `rpm -Va` — verify modified files
- Read repo files from `/etc/yum.repos.d/`
- Read GPG keys from `/etc/pki/rpm-gpg/`
- `dnf history list --reverse` — removed packages
- `rpm -qa --queryformat '%{NAME}\n' | sort | uniq -d` — duplicate packages
- `repoquery --installed --requires` / `--whatrequires` for leaf package detection
- `dnf module list --installed` — module streams
- `dnf versionlock list` — version locks
- For ostree systems: `rpm-ostree status --json` for overrides/removals

**Test approach:**
1. Create fixture files in `testdata/rpm/` for each command output
2. Use FakeExecutor with fixture data
3. Compare output against golden-file JSON

- [ ] **Step 1: Create test fixtures**

Create fixture files in `cmd/inspectah/internal/inspector/testdata/rpm/` containing sample output for each command listed above. Base these on real RHEL 9 output or generate representative samples.

- [ ] **Step 2: Write tests**

```go
package inspector

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
)

func loadFixture(t *testing.T, name string) string {
	t.Helper()
	data, err := os.ReadFile(filepath.Join("testdata", name))
	if err != nil {
		t.Fatalf("load fixture %s: %v", name, err)
	}
	return string(data)
}

func TestRunRpm_PackageMode(t *testing.T) {
	exec := NewFakeExecutor(map[string]ExecResult{
		"rpm -qa --queryformat %{EPOCH}:%{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}\\n": {
			Stdout:   loadFixture(t, "rpm/rpm-qa.txt"),
			ExitCode: 0,
		},
		"rpm -Va": {
			Stdout:   loadFixture(t, "rpm/rpm-va.txt"),
			ExitCode: 0,
		},
		// Add remaining command fixtures
	}).WithFiles(map[string]string{
		"/etc/yum.repos.d/redhat.repo": loadFixture(t, "rpm/redhat.repo"),
	})

	section, warnings, err := RunRpm(exec, nil)
	if err != nil {
		t.Fatalf("RunRpm: %v", err)
	}
	_ = warnings

	if len(section.PackagesAdded) == 0 {
		t.Error("expected non-empty packages_added")
	}

	// Golden-file comparison
	got, _ := json.MarshalIndent(section, "", "  ")
	goldenPath := filepath.Join("testdata", "rpm", "golden-output.json")
	if golden, err := os.ReadFile(goldenPath); err == nil {
		if string(got) != string(golden) {
			t.Errorf("output differs from golden file.\nGot:\n%s", got)
		}
	} else {
		// First run: write golden file
		os.WriteFile(goldenPath, got, 0644)
		t.Log("wrote golden file — re-run to validate")
	}
}
```

- [ ] **Step 3: Implement RunRpm**

Create `cmd/inspectah/internal/inspector/rpm.go`. This is the largest inspector (~450 lines of Python). Port the key logic:

1. Parse `rpm -qa` output into PackageEntry structs
2. Parse `rpm -Va` output into RpmVaEntry structs
3. Read repo files from `/etc/yum.repos.d/` using executor.ReadDir + ReadFile
4. Read GPG keys from `/etc/pki/rpm-gpg/`
5. Parse `dnf history` for removed packages
6. Detect duplicate packages and multiarch packages
7. For ostree: parse `rpm-ostree status --json` for overrides/removals
8. Leaf package detection via repoquery

The function signature:

```go
func RunRpm(exec Executor, baselineResolver interface{}) (*schema.RpmSection, []map[string]interface{}, error)
```

Study `src/inspectah/inspectors/rpm.py` line by line. The golden files validate behavioral parity, not structural parity — write idiomatic Go.

- [ ] **Step 4: Run tests and iterate until golden files match**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah
go test ./internal/inspector/ -v -run TestRunRpm
```

- [ ] **Step 5: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add cmd/inspectah/internal/inspector/rpm.go cmd/inspectah/internal/inspector/rpm_test.go cmd/inspectah/internal/inspector/testdata/rpm/
git commit -m "feat(inspector): port RPM inspector

Parses rpm -qa, rpm -Va, repo files, GPG keys, dnf history,
module streams, version locks, and ostree overrides.
Golden-file validated."
```

---

#### Task 12: Config Inspector

**Files:**
- Create: `cmd/inspectah/internal/inspector/config.go`
- Create: `cmd/inspectah/internal/inspector/config_test.go`
- Create: `cmd/inspectah/internal/inspector/testdata/config/`

**Python reference:** `src/inspectah/inspectors/config.py`

**Key operations to port:**
- Uses `rpm -Va` output (from RPM inspector or re-run) to find RPM-owned modified files
- Walks `/etc/` directories for unowned config files
- Categorizes files by path patterns (tmpfiles, sysctl, audit, etc.)
- Reads file content for each captured config
- Optional `--config-diffs`: runs `diff` against RPM-original for modified files

**Function signature:**
```go
func RunConfig(exec Executor, rpmVa []schema.RpmVaEntry) (*schema.ConfigSection, []map[string]interface{}, error)
```

Follow the same test pattern as Task 11: fixtures in `testdata/config/`, FakeExecutor, golden-file comparison. Commit when tests pass.

---

#### Task 13: Services Inspector

**Files:**
- Create: `cmd/inspectah/internal/inspector/services.go`
- Create: `cmd/inspectah/internal/inspector/services_test.go`
- Create: `cmd/inspectah/internal/inspector/testdata/services/`

**Python reference:** `src/inspectah/inspectors/services.py`

**Key operations:**
- `systemctl list-unit-files --type=service --state=enabled,disabled,masked` — unit states
- `systemctl list-unit-files --preset-mode=enable-only --type=service --preset` — default presets
- Compares current state to preset defaults → produces ServiceStateChange entries
- Scans `/etc/systemd/system/` for drop-in overrides

**Function signature:**
```go
func RunServices(exec Executor) (*schema.ServiceSection, []map[string]interface{}, error)
```

---

#### Task 14: Network Inspector

**Files:**
- Create: `cmd/inspectah/internal/inspector/network.go`
- Create: `cmd/inspectah/internal/inspector/network_test.go`
- Create: `cmd/inspectah/internal/inspector/testdata/network/`

**Python reference:** `src/inspectah/inspectors/network.py`

**Key operations:**
- Read NM connection files from `/etc/NetworkManager/system-connections/`
- Read firewalld zone XMLs from `/etc/firewalld/zones/` and `/etc/firewalld/direct.xml`
- `ip route show` and `ip rule show`
- Read `/etc/hosts` for additions
- Scan environment files for proxy settings
- Read `/etc/resolv.conf` and detect provenance (systemd-resolved, NM, hand-edited)
- Read static route files (`/etc/sysconfig/network-scripts/route-*`)

**Function signature:**
```go
func RunNetwork(exec Executor) (*schema.NetworkSection, []map[string]interface{}, error)
```

---

#### Task 15: Storage Inspector

**Files:**
- Create: `cmd/inspectah/internal/inspector/storage.go`
- Create: `cmd/inspectah/internal/inspector/storage_test.go`
- Create: `cmd/inspectah/internal/inspector/testdata/storage/`

**Python reference:** `src/inspectah/inspectors/storage.py`

**Key operations:**
- Read `/etc/fstab` → parse into FstabEntry structs
- `findmnt --json` → parse active mount points
- `lvs --noheadings` → LVM volume info
- Scan `/var/` for non-empty directories with size estimates
- Detect credential references in mount options

**Function signature:**
```go
func RunStorage(exec Executor) (*schema.StorageSection, []map[string]interface{}, error)
```

---

#### Task 16: Scheduled Tasks Inspector

**Files:**
- Create: `cmd/inspectah/internal/inspector/scheduled.go`
- Create: `cmd/inspectah/internal/inspector/scheduled_test.go`
- Create: `cmd/inspectah/internal/inspector/testdata/scheduled/`

**Python reference:** `src/inspectah/inspectors/scheduled_tasks.py`

**Key operations:**
- Scan `/etc/cron.d/`, `/etc/cron.daily/`, `/etc/cron.hourly/`, etc.
- Read `/var/spool/cron/` for user crontabs
- `systemctl list-timers --all` → parse timer info
- Read timer/service unit files for content
- Generate systemd timer equivalents for cron jobs
- `atq` → list at jobs

**Function signature:**
```go
func RunScheduledTasks(exec Executor) (*schema.ScheduledTaskSection, []map[string]interface{}, error)
```

---

#### Task 17: Container Workloads Inspector

**Files:**
- Create: `cmd/inspectah/internal/inspector/container.go`
- Create: `cmd/inspectah/internal/inspector/container_test.go`
- Create: `cmd/inspectah/internal/inspector/testdata/container/`

**Python reference:** `src/inspectah/inspectors/container.py`

**Key operations:**
- Scan `/etc/containers/systemd/` and `~/.config/containers/systemd/` for quadlet units
- Find docker-compose.yml / compose.yml files
- Optional `--query-podman`: `podman ps --format json` for running containers
- For ostree systems: `flatpak list --app --columns=application,origin,branch`

**Function signature:**
```go
func RunContainers(exec Executor, queryPodman bool) (*schema.ContainerSection, []map[string]interface{}, error)
```

---

#### Task 18: Non-RPM Software Inspector

**Files:**
- Create: `cmd/inspectah/internal/inspector/nonrpm.go`
- Create: `cmd/inspectah/internal/inspector/nonrpm_test.go`
- Create: `cmd/inspectah/internal/inspector/testdata/nonrpm/`

**Python reference:** `src/inspectah/inspectors/non_rpm_software.py`

**Key operations:**
- Walk `/opt/`, `/usr/local/`, `/home/`, `/srv/` for non-RPM software
- `file` and `readelf` on binaries for classification
- Detect Python venvs, pip requirements, git repos
- Detect npm/yarn lockfiles, Gemfiles
- Read `.env` files from common locations
- Prune known false-positive directories (build trees, IDE metadata)

**Function signature:**
```go
func RunNonRpmSoftware(exec Executor, deepBinaryScan bool) (*schema.NonRpmSoftwareSection, []map[string]interface{}, error)
```

---

#### Task 19: Kernel/Boot Inspector

**Files:**
- Create: `cmd/inspectah/internal/inspector/kernelboot.go`
- Create: `cmd/inspectah/internal/inspector/kernelboot_test.go`
- Create: `cmd/inspectah/internal/inspector/testdata/kernelboot/`

**Python reference:** `src/inspectah/inspectors/kernel_boot.py`

**Key operations:**
- Read `/proc/cmdline`
- Read `/etc/default/grub` for GRUB defaults
- `sysctl -a` → compare against defaults, produce SysctlOverride entries
- Read `/etc/modules-load.d/`, `/etc/modprobe.d/`, `/etc/dracut.conf.d/`
- `lsmod` → parse loaded kernel modules, compare against default set
- `tuned-adm active` → active tuned profile
- Read `/etc/tuned/` for custom profiles
- `localectl status` → locale
- `timedatectl show` → timezone
- `update-alternatives --get-installations` → alternatives

**Function signature:**
```go
func RunKernelBoot(exec Executor) (*schema.KernelBootSection, []map[string]interface{}, error)
```

---

#### Task 20: SELinux Inspector

**Files:**
- Create: `cmd/inspectah/internal/inspector/selinux.go`
- Create: `cmd/inspectah/internal/inspector/selinux_test.go`
- Create: `cmd/inspectah/internal/inspector/testdata/selinux/`

**Python reference:** `src/inspectah/inspectors/selinux.py`

**Key operations:**
- `getenforce` → SELinux mode
- `semodule -l` → custom modules (filter vendor modules)
- `semanage boolean -l -C` → boolean overrides
- `semanage fcontext -l -C` → custom fcontext rules
- `semanage port -l -C` → custom port labels
- Read `/etc/audit/rules.d/` for audit rules
- `fips-mode-setup --check` → FIPS mode
- Scan `/etc/pam.d/` for custom PAM configs

**Function signature:**
```go
func RunSelinux(exec Executor) (*schema.SelinuxSection, []map[string]interface{}, error)
```

---

#### Task 21: Users/Groups Inspector

**Files:**
- Create: `cmd/inspectah/internal/inspector/users.go`
- Create: `cmd/inspectah/internal/inspector/users_test.go`
- Create: `cmd/inspectah/internal/inspector/testdata/users/`

**Python reference:** `src/inspectah/inspectors/users_groups.py`

**Key operations:**
- Read `/etc/passwd`, `/etc/shadow`, `/etc/group`, `/etc/gshadow`
- Filter to non-system users (UID >= 1000 typically)
- Read `/etc/subuid`, `/etc/subgid`
- Read `/etc/sudoers` and `/etc/sudoers.d/` for sudoers rules
- Find SSH authorized_keys files

**Function signature:**
```go
func RunUsersGroups(exec Executor) (*schema.UserGroupSection, []map[string]interface{}, error)
```

---

### Task 22: Inspector orchestrator and RPM preflight

**Files:**
- Create: `cmd/inspectah/internal/inspector/orchestrator.go`
- Create: `cmd/inspectah/internal/inspector/orchestrator_test.go`
- Create: `cmd/inspectah/internal/pipeline/preflight.go`
- Create: `cmd/inspectah/internal/pipeline/preflight_test.go`

**Python reference:** `src/inspectah/inspectors/__init__.py` (orchestrator), `src/inspectah/rpm_preflight.py` (preflight)

- [ ] **Step 1: Port RPM preflight**

Port `src/inspectah/rpm_preflight.py`. This checks package availability on the target system using `dnf install --assumeno`. It runs against the host's package repos. Key operations:

- For each added package from the RPM inspector, check if it's available in configured repos
- Report available, unavailable, and unverifiable packages
- Parse DNF stderr for repo sync failures → report RepoStatus
- Uses nsenter to access host's package manager (in container mode — in native mode, runs directly)

```go
func RunPreflight(exec inspector.Executor, packages []string, baseImage string) (*schema.PreflightResult, error)
```

- [ ] **Step 2: Implement inspector orchestrator**

Port `src/inspectah/inspectors/__init__.py`. This runs all inspectors in sequence, handles system-type gating, and manages warnings.

```go
package inspector

import "github.com/marrusl/inspectah/cmd/inspectah/internal/schema"

type InspectOptions struct {
	HostRoot         string
	ConfigDiffs      bool
	DeepBinaryScan   bool
	QueryPodman      bool
	TargetVersion    string
	TargetImage      string
	NoBaseline       bool
	SkipPreflight    bool
	BaselinePackages string
	UserStrategy     string
}

func RunAll(exec Executor, opts InspectOptions) (*schema.InspectionSnapshot, error) {
	snap := schema.NewSnapshot()
	var warnings []map[string]interface{}

	// 1. RPM inspector
	rpm, rpmWarns, err := RunRpm(exec, nil)
	if err != nil {
		return nil, err
	}
	snap.Rpm = rpm
	warnings = append(warnings, rpmWarns...)

	// 2. Config inspector (needs rpm -Va data)
	var rpmVa []schema.RpmVaEntry
	if rpm != nil {
		rpmVa = rpm.RpmVa
	}
	config, configWarns, err := RunConfig(exec, rpmVa)
	if err != nil {
		return nil, err
	}
	snap.Config = config
	warnings = append(warnings, configWarns...)

	// 3-11. Remaining inspectors...
	// Each follows the same pattern: call RunXxx, set snap.Xxx, collect warnings

	services, svcWarns, _ := RunServices(exec)
	snap.Services = services
	warnings = append(warnings, svcWarns...)

	network, netWarns, _ := RunNetwork(exec)
	snap.Network = network
	warnings = append(warnings, netWarns...)

	storage, stoWarns, _ := RunStorage(exec)
	snap.Storage = storage
	warnings = append(warnings, stoWarns...)

	scheduled, schWarns, _ := RunScheduledTasks(exec)
	snap.ScheduledTasks = scheduled
	warnings = append(warnings, schWarns...)

	containers, conWarns, _ := RunContainers(exec, opts.QueryPodman)
	snap.Containers = containers
	warnings = append(warnings, conWarns...)

	nonrpm, nrWarns, _ := RunNonRpmSoftware(exec, opts.DeepBinaryScan)
	snap.NonRpmSoftware = nonrpm
	warnings = append(warnings, nrWarns...)

	kernelboot, kbWarns, _ := RunKernelBoot(exec)
	snap.KernelBoot = kernelboot
	warnings = append(warnings, kbWarns...)

	selinux, selWarns, _ := RunSelinux(exec)
	snap.Selinux = selinux
	warnings = append(warnings, selWarns...)

	users, usrWarns, _ := RunUsersGroups(exec)
	snap.UsersGroups = users
	warnings = append(warnings, usrWarns...)

	snap.Warnings = warnings
	return snap, nil
}
```

- [ ] **Step 3: Run tests**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah
go test ./internal/inspector/ -v -run TestRunAll
go test ./internal/pipeline/ -v -run TestRunPreflight
```

- [ ] **Step 4: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add cmd/inspectah/internal/inspector/orchestrator.go cmd/inspectah/internal/inspector/orchestrator_test.go cmd/inspectah/internal/pipeline/preflight.go cmd/inspectah/internal/pipeline/preflight_test.go
git commit -m "feat(inspector): add orchestrator and RPM preflight

Orchestrator runs all 11 inspectors in sequence. RPM preflight
checks package availability against target repos via dnf."
```

---

## Phase 3: Baseline Subtraction

### Task 23: Base image mapping

**Files:**
- Create: `cmd/inspectah/internal/baseline/mapping.go`
- Create: `cmd/inspectah/internal/baseline/mapping_test.go`

**Python reference:** `src/inspectah/baseline.py` (mapping functions) and `src/inspectah/system_type.py` (map_ostree_base_image)

Port the OS → base image mapping table and the version resolution logic. This is a pure-function module — no I/O, just data lookup. The mapping table lives in `system_type.py` lines 60-150 and `baseline.py` has the resolution entry points.

- [ ] **Step 1: Write table-driven tests covering all mapped OS variants (RHEL 9.x, CentOS Stream 9/10, Fedora)**
- [ ] **Step 2: Implement the mapping table and lookup functions**
- [ ] **Step 3: Run tests, iterate until passing**
- [ ] **Step 4: Commit**

---

### Task 24: Baseline resolver

**Files:**
- Create: `cmd/inspectah/internal/baseline/resolver.go`
- Create: `cmd/inspectah/internal/baseline/resolver_test.go`

**Python reference:** `src/inspectah/baseline.py` (BaselineResolver class)

Port the BaselineResolver which:
1. Takes a base image reference
2. Runs `podman run --rm <image> rpm -qa --queryformat '%{NAME}\n'` to get base image package list
3. Caches the result for the session
4. Provides a `Resolve()` method that returns (packageNames, resolvedImage, noBaseline)

For the Go port, this runs directly (no nsenter needed — the Go binary is on the host). Podman is required on `PATH`.

- [ ] **Step 1: Write tests with a FakeExecutor that returns canned podman output**
- [ ] **Step 2: Implement BaselineResolver struct and Resolve method**
- [ ] **Step 3: Add ServicePresets method (queries base image for systemd preset data)**
- [ ] **Step 4: Run tests, commit**

---

## Phase 4: Renderers, Redaction, and Pipeline

### Task 25: pongo2 template compatibility validation

**Files:**
- Create: `cmd/inspectah/internal/renderer/compat_test.go`

Before porting renderers, validate that existing Jinja2 templates work with pongo2. Run each `.j2` template through pongo2 with sample data and check for syntax errors.

- [ ] **Step 1: Write a test that loads each .j2 template from `src/inspectah/templates/` and compiles it with pongo2**

```go
package renderer

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/flosch/pongo2/v6"
)

func TestPongo2TemplateCompilation(t *testing.T) {
	templatesDir := filepath.Join("..", "..", "..", "..", "src", "inspectah", "templates")
	err := filepath.Walk(templatesDir, func(path string, info os.FileInfo, err error) error {
		if err != nil || info.IsDir() || filepath.Ext(path) != ".j2" {
			return err
		}
		t.Run(filepath.Base(path), func(t *testing.T) {
			data, err := os.ReadFile(path)
			if err != nil {
				t.Fatalf("read: %v", err)
			}
			_, err = pongo2.FromString(string(data))
			if err != nil {
				t.Errorf("pongo2 compilation failed: %v", err)
			}
		})
		return nil
	})
	if err != nil {
		t.Fatalf("walk: %v", err)
	}
}
```

- [ ] **Step 2: Run the test and catalogue any failures**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah
go test ./internal/renderer/ -v -run TestPongo2
```

Document which templates need syntax changes. Common issues: Jinja2 `{% else if %}` vs pongo2 `{% elif %}`, unsupported filters, macro syntax differences.

- [ ] **Step 3: Create a list of required template modifications**

Save findings as comments in the test file for reference during template migration.

- [ ] **Step 4: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add cmd/inspectah/internal/renderer/
git commit -m "test(renderer): validate pongo2 template compatibility

Catalogue Jinja2 syntax gaps that need fixing during template port."
```

---

### Task 26: Template embedding infrastructure

**Files:**
- Create: `cmd/inspectah/internal/renderer/templates/` (copy and adapt from `src/inspectah/templates/`)
- Create: `cmd/inspectah/internal/renderer/embed.go`

- [ ] **Step 1: Copy all .j2 templates to the Go template directory**

```bash
cp -r src/inspectah/templates/* cmd/inspectah/internal/renderer/templates/
```

- [ ] **Step 2: Fix pongo2 compatibility issues found in Task 25**

Apply the syntax fixes identified in the compatibility test.

- [ ] **Step 3: Create the embed.go file with go:embed directives**

```go
package renderer

import "embed"

//go:embed templates
var TemplateFS embed.FS
```

- [ ] **Step 4: Write a test that loads templates from the embedded FS**

```go
func TestEmbeddedTemplatesExist(t *testing.T) {
	entries, err := TemplateFS.ReadDir("templates")
	if err != nil {
		t.Fatalf("read embedded templates dir: %v", err)
	}
	if len(entries) == 0 {
		t.Error("no embedded templates found")
	}
}
```

- [ ] **Step 5: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add cmd/inspectah/internal/renderer/
git commit -m "feat(renderer): add pongo2 template embedding infrastructure

Migrate Jinja2 templates from src/inspectah/templates/ to
cmd/inspectah/internal/renderer/templates/ with pongo2 compatibility
fixes. Templates embedded via go:embed."
```

---

### Tasks 27-33: Individual Renderers

Each renderer follows this pattern:
1. Read the Python renderer source for logic and template usage
2. Implement the Go renderer function that builds template context and renders
3. Write golden-file tests comparing Go output against Python output for the same snapshot
4. Each renderer writes output files to the output directory

#### Task 27: Containerfile Renderer

**Files:**
- Create: `cmd/inspectah/internal/renderer/containerfile/core.go` and all sub-modules
- Create: `cmd/inspectah/internal/renderer/containerfile/configtree.go`
- Create: corresponding test files

**Python reference:** `src/inspectah/renderers/containerfile/_core.py` and all domain modules in `src/inspectah/renderers/containerfile/`

This is the most critical renderer — it produces the migration Containerfile. It has domain-specific modules for each inspector's data (packages, services, config, network, etc.). Port `_core.py` first, then each domain module.

The config tree writer (`configtree.go`) copies redacted config files to `config/` in the output directory.

**Function signature:**
```go
func Render(snap *schema.InspectionSnapshot, templateFS embed.FS, outputDir string) error
```

---

#### Task 28: HTML Report Renderer

**Files:**
- Create: `cmd/inspectah/internal/renderer/html.go`
- Create: `cmd/inspectah/internal/renderer/html_test.go`

**Python reference:** `src/inspectah/renderers/html_report.py`

Renders the interactive HTML report from `templates/report.html.j2` and its partials. This is a single-page app with PatternFly CSS and JavaScript. The renderer assembles the full snapshot data into a pongo2 template context and renders to `report.html`.

---

#### Task 29: Audit Report Renderer

**Python reference:** `src/inspectah/renderers/audit_report.py`

Renders a text/HTML audit report summarizing changes, risks, and recommendations.

---

#### Task 30: README Renderer

**Python reference:** `src/inspectah/renderers/readme.py`

Renders a plain-text README summarizing the scan output and next steps.

---

#### Task 31: Kickstart Renderer

**Python reference:** `src/inspectah/renderers/kickstart.py`

Renders a kickstart file for automated RHEL installation. Cross-references the Containerfile.

---

#### Task 32: Secrets Review Renderer

**Python reference:** `src/inspectah/renderers/secrets_review.py`

Renders a secrets review document listing all redaction findings and recommended actions.

---

#### Task 33: Merge Notes Renderer

**Python reference:** `src/inspectah/renderers/merge_notes.py`

Renders merge notes for fleet operations — documents what was merged and discrepancies.

---

### Task 34: Redaction engine and heuristic detection

**Files:**
- Create: `cmd/inspectah/internal/pipeline/redact.go`
- Create: `cmd/inspectah/internal/pipeline/redact_test.go`
- Create: `cmd/inspectah/internal/pipeline/heuristic.go`
- Create: `cmd/inspectah/internal/pipeline/heuristic_test.go`

**Python reference:** `src/inspectah/redact.py` and `src/inspectah/heuristic.py`

**Pattern-based redaction** (`redact.go`):
- Scans snapshot fields (config file content, shadow entries, container env vars) for known secret patterns
- Patterns: SSH private keys, certificates, password hashes, API keys, tokens
- Replaces inline with `[REDACTED-<type>]` tokens
- Records each finding as a RedactionFinding
- Handles three remediation types: `excluded` (remove file), `inline` (redact value), `flagged` (mark for review)

**Heuristic detection** (`heuristic.go`):
- Scans content for heuristic secret indicators (high-entropy strings, assignment patterns like `PASSWORD=`, `API_KEY=`)
- Produces HeuristicCandidate entries
- Skips subscription cert paths (`/etc/pki/entitlement/`, `/etc/rhsm/`)
- Sensitivity levels: `strict` (redact high-confidence) vs `moderate` (flag all)

**Key behavioral test:** After redaction, no secrets should appear in any output artifact. Write a test that creates a snapshot with known secrets, runs redaction, and verifies they're gone.

---

### Task 35: Subscription cert bundling and tarball packaging

**Files:**
- Create: `cmd/inspectah/internal/pipeline/subscription.go`
- Create: `cmd/inspectah/internal/pipeline/subscription_test.go`
- Create: `cmd/inspectah/internal/pipeline/packaging.go`
- Create: `cmd/inspectah/internal/pipeline/packaging_test.go`

**Python reference:** `src/inspectah/subscription.py` and `src/inspectah/packaging.py`

**Subscription** (`subscription.go`):
- Copies RHEL subscription certs from `/etc/pki/entitlement/` into the output directory
- Only runs when not in `--no-subscription` mode and not in `--from-snapshot` mode

**Packaging** (`packaging.go`):
- Creates `.tar.gz` tarball from output directory
- Naming convention: `inspectah-<hostname>-YYYYMMDD-HHMMSS.tar.gz`
- `get_output_stamp()` builds the hostname-timestamp stamp
- `sanitize_hostname()` removes unsafe filename characters

---

### Task 36: Pipeline orchestrator

**Files:**
- Create: `cmd/inspectah/internal/pipeline/run.go`
- Create: `cmd/inspectah/internal/pipeline/run_test.go`
- Create: `cmd/inspectah/internal/pipeline/github.go`
- Create: `cmd/inspectah/internal/pipeline/validate.go`

**Python reference:** `src/inspectah/pipeline.py`, `src/inspectah/git_github.py`, `src/inspectah/validate.py`

This is the main entry point that orchestrates the full scan pipeline:

1. Load from snapshot (`--from-snapshot`) or run inspectors
2. Run redaction pass (pattern + heuristic)
3. Run renderers (all 7)
4. Bundle subscription certs (optional)
5. Create tarball or write to directory
6. Optionally validate via `podman build`
7. Optionally push to GitHub

```go
package pipeline

type RunOptions struct {
	HostRoot         string
	FromSnapshotPath string
	InspectOnly      bool
	OutputFile       string
	OutputDir        string
	NoSubscription   bool
	Sensitivity      string
	NoRedaction      bool
	Validate         bool
	PushToGitHub     string
	GitHubToken      string
	Public           bool
	SkipConfirmation bool
	// Inspector options
	ConfigDiffs      bool
	DeepBinaryScan   bool
	QueryPodman      bool
	TargetVersion    string
	TargetImage      string
	NoBaseline       bool
	SkipPreflight    bool
	BaselinePackages string
	UserStrategy     string
}

func Run(opts RunOptions) (*schema.InspectionSnapshot, error) {
	// Main pipeline logic — see src/inspectah/pipeline.py
}
```

---

### Task 37: Scan command unlock

**Files:**
- Modify: `cmd/inspectah/internal/cli/scan.go`
- Modify: `cmd/inspectah/internal/cli/passthrough.go`

This is the cutover step for the `scan` command. All dependencies are now ported: schema, inspectors, baseline, renderers, redaction, packaging.

- [ ] **Step 1: Rewrite scan.go to call native pipeline instead of container**

Replace the container delegation in `scan.go` with a call to `pipeline.Run()`. Remove the `container.EnsureImage` and `container.BuildArgs` calls. Keep the same CLI flag interface — the flags from `passthrough.go` become real Go flags that feed into `pipeline.RunOptions`.

The scan command still requires Linux + root (no change to the platform checks).

- [ ] **Step 2: Run golden-file parity test**

Run both the Python (container) version and the Go (native) version on the same host. Compare outputs. They should produce identical snapshot JSON and identical rendered artifacts.

```bash
# Run Python version (container)
sudo inspectah-python scan --output-dir /tmp/python-output

# Run Go version (native)
sudo ./cmd/inspectah/inspectah scan --output-dir /tmp/go-output

# Compare
diff -r /tmp/python-output /tmp/go-output
```

- [ ] **Step 3: Run behavioral regression tests**

Run the behavioral test suite (see spec's testing strategy):
- Snapshot JSON round-trip
- Tarball inventory and naming
- Redaction completeness
- CLI compatibility (bare invocation, flag parsing)

- [ ] **Step 4: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add cmd/inspectah/internal/cli/scan.go cmd/inspectah/internal/cli/passthrough.go
git commit -m "feat(cli): unlock scan command — native Go pipeline

Scan no longer delegates to the container. Full pipeline runs
natively: inspectors → redaction → renderers → tarball.
Golden-file and behavioral tests validated."
```

---

## Phase 5: Refine Engine

### Task 38: Refine server

**Files:**
- Create: `cmd/inspectah/internal/refine/server.go`
- Create: `cmd/inspectah/internal/refine/server_test.go`
- Create: `cmd/inspectah/internal/refine/tarball.go`
- Create: `cmd/inspectah/internal/refine/tarball_test.go`

**Python reference:** `src/inspectah/refine.py`

The refine server:
1. Extracts a scan output tarball to a temp directory
2. Serves the HTML report over HTTP (Go `net/http`)
3. Watches for changes to `inspection-snapshot.json`
4. Re-renders all output when the snapshot is modified (by the report editor UI)
5. Supports `--from-snapshot` re-render: loads snapshot → runs pipeline with `refine_mode=true`

Port the HTTP handler, tarball extraction, re-render loop, and browser auto-open.

```go
package refine

func RunRefine(tarballPath string, port int, noBrowser bool) error
```

**Tarball helpers** (`tarball.go`):
- `ExtractTarball(path, destDir)` — extract .tar.gz
- `RepackTarball(srcDir, destPath)` — repack after modifications

---

### Task 39: Refine command unlock

**Files:**
- Modify: `cmd/inspectah/internal/cli/refine.go`

- [ ] **Step 1: Rewrite refine.go to call native refine server**

Replace container delegation with `refine.RunRefine()`. The refine command runs on any platform (not Linux-only).

- [ ] **Step 2: Test: extract a tarball, start refine, verify HTTP server responds**

- [ ] **Step 3: Commit**

---

## Phase 6: Fleet Merge

### Task 40: Fleet loader

**Files:**
- Create: `cmd/inspectah/internal/fleet/loader.go`
- Create: `cmd/inspectah/internal/fleet/loader_test.go`

**Python reference:** `src/inspectah/fleet/loader.py`

Loads multiple snapshots from a directory. Handles:
- `.tar.gz` files (extract, find snapshot JSON inside)
- `.json` files (direct load)
- Skip `fleet-snapshot.json` (prevents self-contamination)
- Validate: minimum 2 snapshots, matching schema versions, matching os_release

```go
package fleet

func LoadSnapshots(inputDir string) ([]*schema.InspectionSnapshot, error)
func ValidateSnapshots(snapshots []*schema.InspectionSnapshot) error
```

---

### Task 41: Fleet merge

**Files:**
- Create: `cmd/inspectah/internal/fleet/merge.go`
- Create: `cmd/inspectah/internal/fleet/merge_test.go`

**Python reference:** `src/inspectah/fleet/merge.py` (628 lines)

Merges multiple snapshots into a fleet-level aggregate:
- Union merge for packages, configs, services (with prevalence counts)
- Deduplication by name/path
- FleetPrevalence metadata on each item (count, total, host list)
- Min-prevalence filtering
- Fleet metadata section
- Handles tie-breaking for conflicting config files

```go
func MergeSnapshots(snapshots []*schema.InspectionSnapshot, minPrevalence int) (*schema.InspectionSnapshot, error)
```

**Golden-file test:** Create 3 sample snapshots with overlapping/divergent data. Merge. Compare merged JSON against golden file.

---

### Task 42: Fleet command unlock

**Files:**
- Modify: `cmd/inspectah/internal/cli/fleet.go`

- [ ] **Step 1: Rewrite fleet.go to call native loader + merger**

Replace container delegation. The fleet command works on any platform.

- [ ] **Step 2: Test with real scan outputs**

- [ ] **Step 3: Commit**

---

## Phase 7: Architect and Final Cutover

### Task 43: Architect analyzer

**Files:**
- Create: `cmd/inspectah/internal/architect/analyzer.go`
- Create: `cmd/inspectah/internal/architect/analyzer_test.go`

**Python reference:** `src/inspectah/architect/analyzer.py` (194 lines)

The analyzer takes fleet merge output and decomposes it into a layer topology:
- Identifies shared packages across all hosts (base layer)
- Groups role-specific packages (per-host or per-cluster layers)
- Produces a `LayerTopology` with parent/child relationships

```go
type FleetInput struct {
	Name     string
	Packages []string
	// ... other fleet data
}

type Layer struct {
	Name     string
	Parent   string
	Packages []string
	// ...
}

type LayerTopology struct {
	Layers []Layer
}

func Analyze(inputs []FleetInput) *LayerTopology
```

---

### Task 44: Architect loader

**Files:**
- Create: `cmd/inspectah/internal/architect/loader.go`
- Create: `cmd/inspectah/internal/architect/loader_test.go`

**Python reference:** `src/inspectah/architect/loader.py` (97 lines)

Loads fleet snapshots for the architect. Enforces the **same-major-version refusal**: snapshot's `schema_version` must match the binary's expected version. Mismatches produce a clear error message.

---

### Task 45: Architect export

**Files:**
- Create: `cmd/inspectah/internal/architect/export.go`
- Create: `cmd/inspectah/internal/architect/export_test.go`

**Python reference:** `src/inspectah/architect/export.py` (92 lines)

Exports a layer topology as a set of Containerfiles — one per layer, with proper `FROM` chains.

---

### Task 46: Architect server

**Files:**
- Create: `cmd/inspectah/internal/architect/server.go`
- Create: `cmd/inspectah/internal/architect/server_test.go`
- Create: `cmd/inspectah/internal/architect/static/architect.html`

**Python reference:** `src/inspectah/architect/server.py` (163 lines) and `src/inspectah/templates/architect/`

HTTP server that:
1. Renders the architect HTML from the template with topology data baked in
2. Serves the single-page app with embedded PatternFly CSS and CodeMirror JS
3. Exposes API endpoints for topology manipulation and Containerfile export
4. Auto-opens browser

For the Go port, the rendered HTML is embedded via `go:embed`. The server uses `net/http`.

---

### Task 47: Architect command unlock and container surface removal

**Files:**
- Modify: `cmd/inspectah/internal/cli/architect.go`
- Modify: `cmd/inspectah/internal/cli/root.go`
- Delete: `cmd/inspectah/internal/cli/image.go`
- Modify: `cmd/inspectah/internal/cli/passthrough.go` (remove or gut)
- Modify: `cmd/inspectah/main.go` (bump version to 0.7.0)

This is the final cutover.

- [ ] **Step 1: Rewrite architect.go to call native server**

- [ ] **Step 2: Remove container surface from root.go**

Remove `--image` and `--pull` persistent flags from root command. Remove the `PersistentPreRun` that resolves the container image.

```go
func NewRootCmd(ver, commit, date string) *cobra.Command {
	root := &cobra.Command{
		Use:   "inspectah",
		Short: "Inspect package-mode hosts and produce bootc image artifacts",
		// No more PersistentPreRun for image resolution
		SilenceUsage:  true,
		SilenceErrors: true,
	}
	// No more --image or --pull flags
	root.AddCommand(newVersionCmd(ver, commit, date))
	root.AddCommand(newScanCmd())     // no more GlobalOpts
	root.AddCommand(newFleetCmd())
	root.AddCommand(newRefineCmd())
	root.AddCommand(newArchitectCmd())
	root.AddCommand(newBuildCmd())
	// newImageCmd REMOVED
	return root
}
```

- [ ] **Step 3: Delete image.go**

```bash
rm cmd/inspectah/internal/cli/image.go cmd/inspectah/internal/cli/image_test.go
```

- [ ] **Step 4: Remove or gut passthrough.go**

If no commands still need passthrough flags, delete the file. If build still uses some, keep only what's needed.

- [ ] **Step 5: Bump version in main.go to 0.7.0**

```go
var (
	version = "0.7.0"
	commit  = "unknown"
	date    = "unknown"
)
```

- [ ] **Step 6: Update error messages**

In `cmd/inspectah/internal/errors/translate.go`, remove references to `inspectah image update` and replace with version-appropriate messaging.

- [ ] **Step 7: Run full test suite**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah
go test ./... -v
go vet ./...
```

- [ ] **Step 8: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add -A
git commit -m "feat: complete Go-native port — remove container surface

All commands now run natively. Remove image subcommand, --image
and --pull flags, container package, and passthrough delegation.
Bump version to 0.7.0."
```

---

## Post-Port: Cutover Sequence

After all tasks are complete, follow the cutover sequence from the spec:

1. All modules ported, all golden-file and behavioral tests passing
2. Tag `v0.7.0-rc1` on `go-port` branch
3. Manual smoke test on a real RHEL system (full pipeline)
4. Tag final container image `v0.6.x-final` on `main`
5. Merge `go-port` to `main`
6. Delete `src/inspectah/`, `Containerfile`, `build-image.yml`, `pyproject.toml`, `internal/container/` in merge commit
7. Tag `v0.7.0` on `main`
8. `package-release.yml` builds and publishes artifacts

The merge commit and tagging steps are done by Mark, not by the implementing agent.

---

## Appendix: Golden-File Testing Workflow

For each module, golden-file testing follows this workflow:

1. **Generate reference output** — Run the Python version on a test host, capture the JSON section output
2. **Save as golden file** — Store in `testdata/golden/<module>-<variant>.json`
3. **Write Go test** — Load golden file, run Go module with same inputs (via FakeExecutor + fixture data), compare JSON output
4. **First run creates golden** — If golden file doesn't exist, write it and mark test as "needs re-run"
5. **Update golden files** — When Python behavior changes, regenerate. When Go behavior intentionally differs, update golden and document why.

Golden files are committed to the repo and serve as the regression baseline after Python code is deleted.
