# Non-RPM Replication Plan 3: Refine UI Changes

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add Language Packages and Unmanaged Files decision sections to the single-host refine UI, implement the RPM upload modal for repo-less packages wired to Plan 2's backend upload endpoint, update sidebar/keyboard navigation, and extend global search and focus plumbing to cover the new sections.

**Scope boundary — single-host only.** This plan covers single-host refine UI exclusively. Aggregate UI for the new sections (searchable aggregate metadata, detail-pane coverage, variant comparison for language environments and unmanaged files) is Plan 4's responsibility. Plan 3 builds the components and interaction contracts; Plan 4 wires them into aggregate mode. The aggregate tasks (Task 12) from the prior revision are removed. See "Aggregate Handoff" section at the end.

**Architecture:** Four layers change: (1) TypeScript types gain new section/item interfaces including Plan 2's `ProvenanceSignals` and repo-less RPM backend fields, (2) new React components render the Language Packages and Unmanaged Files decision sections with per-environment and per-item toggles, (3) the RPM upload modal adds a file-upload workflow that calls Plan 2's `POST /api/upload-rpm` endpoint so uploads are durable and exportable, (4) `MainContent.tsx` gains section rendering and search/reveal plumbing, `AppShell.tsx` gains focus management, and `useKeyboard.ts` gains shortcut remapping for the new sections.

**Tech Stack:** React 18, TypeScript, PatternFly v6 (components + icons), Vitest, React Testing Library. CSS follows existing `App.css` BEM conventions.

**Spec:** `process-docs/specs/proposed/2026-06-27-non-rpm-replication.md` — read fresh before implementation. This plan covers the "Refine UI: Section Topology" spec section and the per-tier Refine UI subsections, single-host mode only.

**Plan 1 Contracts:** This plan consumes the shared contracts defined in Plan 1's "Shared Contracts for Plans 2-4" section. Use `ItemId::LanguageEnv { ecosystem, path }`, `ItemId::UnmanagedFile { path }`, the `method` string table, and the confidence rendering gate exactly as specified there.

**Plan 2 Contracts:** This plan consumes Plan 2's backend contracts:
- `PackageEntry.repoless_annotation` and `PackageEntry.repoless_cached` fields for deriving RPM row states
- `POST /api/upload-rpm` endpoint for durable RPM upload (not just UI hook state)
- `ProvenanceSignals` struct fields (`mutability`, `writable_mount`, `service_working_dir`, `last_modified`, `uid`, `gid`, `permissions`) carried through the DTO for Unmanaged Files reviewability

**Thorn Checkpoints:** After Tasks 4, 8, 13.

## Global Constraints

- Lint clean: `npm run lint` with zero warnings.
- Format: `npx prettier --check` must pass.
- No team member names in code or commits.
- Commit format: `type(scope): description`. Attribution: `Assisted-by: Claude Code (Opus 4.6)`.
- PatternFly v6 components only — no v5 imports.
- All new components are functional with hooks.
- Tests use Vitest + React Testing Library. No Enzyme, no `act()` wrapping unless React demands it.
- Existing tests must keep passing throughout. Run `npm test` after each task.
- CSS class naming follows BEM: `.inspectah-{component}__{element}--{modifier}`.
- Every new section must work in both light and dark themes.

## File Map

### Modified files

| File | What changes |
|------|-------------|
| `crates/web/ui/src/api/types.ts` | New `LanguagePackageEnv`, `UnmanagedFileItem`, `UnmanagedFileGroup`, `ProvenanceSignals` interfaces; `RpmUploadRowState` type; extend `PackageEntry` with `repoless_annotation`, `repoless_cached`; extend `ViewResponse` |
| `crates/web/ui/src/components/Sidebar.tsx` | Add Language Packages + Unmanaged Files to review sections; move System Tuning to reference; discoverability hint |
| `crates/web/ui/src/hooks/useKeyboard.ts` | Update `SINGLE_HOST_SECTION_IDS` with new sections after `containers` |
| `crates/web/ui/src/components/GlobalSearch.tsx` | Add `SECTION_LABELS` entries; accept + search new section items |
| `crates/web/ui/src/components/PackageList.tsx` | RPM upload icon in blocked rows; muted styling; post-upload transition; repo-text changes; row-level `aria-live` |
| `crates/web/ui/src/components/StatsBar.tsx` | Upload RPMs toolbar button when blocked packages exist |
| `crates/web/ui/src/components/MainContent.tsx` | Render Language Packages and Unmanaged Files sections; SectionSearch integration; first-item focus; reveal highlighting |
| `crates/web/ui/src/components/AppShell.tsx` | Shell-level focus management for new sections; ArrowDown-to-first-item; section search state |
| `crates/web/ui/src/App.tsx` | Wire new components, pass data to MainContent/Sidebar/GlobalSearch; RPM upload backend calls |
| `crates/web/ui/src/App.css` | New styles for upload rows, unmanaged file groups, language package rows, `/var` warning, provenance badges |

### New files

| File | Purpose |
|------|---------|
| `crates/web/ui/src/components/LanguagePackageList.tsx` | Language Packages decision section component |
| `crates/web/ui/src/components/UnmanagedFileList.tsx` | Unmanaged Files decision section with directory grouping |
| `crates/web/ui/src/components/RpmUploadModal.tsx` | Single-RPM upload modal with NEVRA validation |
| `crates/web/ui/src/components/RpmBatchUploadModal.tsx` | Multi-RPM batch upload modal with auto-matching and conflicts view |
| `crates/web/ui/src/hooks/useRpmUpload.ts` | Upload state machine hook (5 row states) wired to `POST /api/upload-rpm` |
| `crates/web/ui/src/components/__tests__/LanguagePackageList.test.tsx` | Tests for Language Packages section |
| `crates/web/ui/src/components/__tests__/UnmanagedFileList.test.tsx` | Tests for Unmanaged Files section including grouped accessibility |
| `crates/web/ui/src/components/__tests__/RpmUploadModal.test.tsx` | Tests for single + batch upload modals including focus trap and conflicts |
| `crates/web/ui/src/components/__tests__/useRpmUpload.test.ts` | Tests for upload state machine hook |
| `crates/web/ui/src/components/__tests__/SectionPlumbing.test.tsx` | Tests for MainContent/AppShell search/focus/reveal plumbing |

---

## Task 1: TypeScript Type Extensions

**Files:**
- Modify: `crates/web/ui/src/api/types.ts`
- Test: `crates/web/ui/src/components/__tests__/LanguagePackageList.test.tsx` (type import verification)

**Interfaces:**
- Produces: `LanguagePackageEnv`, `ProvenanceSignals`, `UnmanagedFileItem`, `UnmanagedFileGroup`, `RpmUploadRowState`, extended `PackageEntry`, extended `ViewResponse`
- Consumed by: Tasks 2-13

- [ ] **Step 1: Add LanguagePackageEnv interface**

In `crates/web/ui/src/api/types.ts`, add after the existing `NonRpmItem` interface (or at the end of the decision item types section):

```typescript
/**
 * A language package environment (pip venv, npm project, gem project).
 * Identity contract: matches Plan 1's ItemId::LanguageEnv { ecosystem, path }.
 */
export interface LanguagePackageEnv {
  /** Ecosystem identifier: "pip" | "npm" | "gem". */
  ecosystem: "pip" | "npm" | "gem";
  /** Absolute path to the environment root. */
  path: string;
  /** Method string from Plan 1 contract. */
  method: string;
  /** Package names in this environment. */
  packages: string[];
  /** Confidence level from collector. */
  confidence: "high" | "medium" | "low";
  /** How the environment was discovered (e.g., "requirements.txt", "dist-info", "package-lock.json", "Gemfile.lock"). */
  manifest_basis: string;
  /** Whether to include in export. */
  include: boolean;
}
```

- [ ] **Step 2: Add ProvenanceSignals interface**

Below `LanguagePackageEnv`, add the provenance signals that Plan 2's backend provides:

```typescript
/**
 * Provenance signals for an unmanaged file.
 * Matches Plan 2's ProvenanceSignals struct — carry through for
 * reviewability, don't narrow at the DTO layer.
 */
export interface ProvenanceSignals {
  /** File type classification. */
  file_type: "elf_binary" | "jar" | "script" | "data_file" | "config" | "symlink" | "other";
  /** Last-modified timestamp (seconds since epoch). */
  last_modified: number;
  /** Filesystem UID. */
  uid: number;
  /** Filesystem GID. */
  gid: number;
  /** Octal file permissions (e.g., "0755"). */
  permissions: string;
  /** True when file's mtime is newer than system install date. */
  mutability: boolean;
  /** True when file lives on a read-write mount point. */
  writable_mount: boolean;
  /** True when file is under a systemd service's WorkingDirectory. */
  service_working_dir: boolean;
}
```

- [ ] **Step 3: Add UnmanagedFileItem and UnmanagedFileGroup interfaces**

Below `ProvenanceSignals`, add:

```typescript
/** A single unmanaged file discovered by --include-unmanaged. */
export interface UnmanagedFileItem {
  /** Absolute file path (matches Plan 1's ItemId::UnmanagedFile { path }). */
  path: string;
  /** File size in bytes. */
  size: number;
  /** Whether path is under /var. */
  is_var_path: boolean;
  /** Whether to include in export. */
  include: boolean;
  /** Provenance signals from Plan 2's backend. */
  provenance: ProvenanceSignals;
}

/** Directory group for unmanaged files. */
export interface UnmanagedFileGroup {
  /** Parent directory path. */
  directory: string;
  /** Items in this directory. */
  items: UnmanagedFileItem[];
}
```

- [ ] **Step 4: Add RpmUploadRowState type and extend PackageEntry**

Below the unmanaged file types, add:

```typescript
/**
 * Row state for repo-less RPM packages.
 * Derived from Plan 2's backend fields (repoless_annotation, repoless_cached)
 * combined with local upload state. See spec "RPM Upload Row Contract".
 */
export type RpmUploadRowState =
  | "cached_excluded"    // Cached RPM found, pre-excluded (no GPG)
  | "cached_included"    // Cached RPM found, user-included
  | "needs_upload"       // No RPM anywhere, needs user upload
  | "uploaded_excluded"  // RPM uploaded via POST /api/upload-rpm, pre-excluded
  | "uploaded_included"; // RPM uploaded, user-included
```

Extend `PackageEntry` (or the relevant package type) with Plan 2's backend fields:

```typescript
// Add to the existing PackageEntry / package item interface:
  /** Triage annotation for repo-less packages (from Plan 2 backend). */
  repoless_annotation?: string;
  /** True if cached RPM was found in /var/cache/dnf/ (from Plan 2 backend). */
  repoless_cached?: boolean;
```

- [ ] **Step 5: Extend ViewResponse with new section data**

Find the `ViewResponse` interface and add these optional fields:

```typescript
  /** Language package environments (Tier 1 non-RPM). */
  language_packages?: LanguagePackageEnv[];
  /** Unmanaged file groups (Tier 2, flag-gated). Present only when --include-unmanaged was used. */
  unmanaged_files?: UnmanagedFileGroup[];
  /** Whether --include-unmanaged was used at scan time. Drives discoverability hint. */
  has_unmanaged_scan?: boolean;
```

- [ ] **Step 6: Write type import verification test**

Create `crates/web/ui/src/components/__tests__/LanguagePackageList.test.tsx` with an initial scaffold:

```typescript
import { describe, it, expect } from "vitest";
import type {
  LanguagePackageEnv,
  UnmanagedFileItem,
  UnmanagedFileGroup,
  ProvenanceSignals,
  RpmUploadRowState,
} from "../../api/types";

// --- Test data factories ---

const DEFAULT_PROVENANCE: ProvenanceSignals = {
  file_type: "elf_binary",
  last_modified: 1700000000,
  uid: 0,
  gid: 0,
  permissions: "0755",
  mutability: false,
  writable_mount: false,
  service_working_dir: false,
};

function makeLangEnv(
  ecosystem: LanguagePackageEnv["ecosystem"],
  path: string,
  packages: string[],
  overrides?: Partial<LanguagePackageEnv>,
): LanguagePackageEnv {
  return {
    ecosystem,
    path,
    method: ecosystem === "pip" ? "pip list" : ecosystem === "npm" ? "npm lockfile" : "gem lockfile",
    packages,
    confidence: "high",
    manifest_basis: ecosystem === "pip" ? "requirements.txt" : ecosystem === "npm" ? "package-lock.json" : "Gemfile.lock",
    include: true,
    ...overrides,
  };
}

function makeUnmanagedFile(
  path: string,
  overrides?: Partial<UnmanagedFileItem>,
): UnmanagedFileItem {
  return {
    path,
    size: 1024,
    is_var_path: path.startsWith("/var/"),
    include: true,
    provenance: { ...DEFAULT_PROVENANCE },
    ...overrides,
  };
}

describe("Type contracts", () => {
  it("LanguagePackageEnv factory produces valid shape", () => {
    const env = makeLangEnv("pip", "/opt/myapp/venv", ["flask", "requests"]);
    expect(env.ecosystem).toBe("pip");
    expect(env.path).toBe("/opt/myapp/venv");
    expect(env.packages).toHaveLength(2);
    expect(env.confidence).toBe("high");
  });

  it("UnmanagedFileItem factory carries provenance signals", () => {
    const regular = makeUnmanagedFile("/opt/splunk/bin/splunkd", {
      provenance: {
        ...DEFAULT_PROVENANCE,
        mutability: true,
        writable_mount: true,
      },
    });
    expect(regular.is_var_path).toBe(false);
    expect(regular.provenance.mutability).toBe(true);
    expect(regular.provenance.writable_mount).toBe(true);
    expect(regular.provenance.service_working_dir).toBe(false);

    const varFile = makeUnmanagedFile("/var/lib/myapp/data.db");
    expect(varFile.is_var_path).toBe(true);
  });

  it("RpmUploadRowState covers all 5 states", () => {
    const states: RpmUploadRowState[] = [
      "cached_excluded",
      "cached_included",
      "needs_upload",
      "uploaded_excluded",
      "uploaded_included",
    ];
    expect(states).toHaveLength(5);
  });
});
```

- [ ] **Step 7: Run tests, verify pass, commit**

```bash
cd crates/web/ui && npm test -- --run
git add -A && git commit -m "feat(web): add TypeScript types for language packages, unmanaged files, and RPM upload states

Includes ProvenanceSignals matching Plan 2's backend struct and
repoless_annotation/repoless_cached fields on PackageEntry.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 2: Language Packages Decision Section Component

**Files:**
- Create: `crates/web/ui/src/components/LanguagePackageList.tsx`
- Test: `crates/web/ui/src/components/__tests__/LanguagePackageList.test.tsx`

**Interfaces:**
- Consumes: `LanguagePackageEnv` from types.ts, confidence rendering gate from Plan 1
- Produces: `LanguagePackageList` component with per-environment toggles

- [ ] **Step 1: Write failing test — renders environment list with toggles**

Append to `LanguagePackageList.test.tsx`:

```typescript
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { vi } from "vitest";
import { LanguagePackageList } from "../LanguagePackageList";

describe("LanguagePackageList", () => {
  const envs: LanguagePackageEnv[] = [
    makeLangEnv("pip", "/opt/myapp/venv", ["flask", "requests", "gunicorn"]),
    makeLangEnv("npm", "/srv/webapp", ["express", "lodash"], {
      confidence: "medium",
      include: false,
    }),
    makeLangEnv("gem", "/opt/rails-app", ["rails", "puma"]),
  ];

  it("renders one row per environment with ecosystem label", () => {
    render(
      <LanguagePackageList
        environments={envs}
        onToggle={vi.fn()}
        isPending={false}
      />,
    );
    expect(screen.getByText("/opt/myapp/venv")).toBeInTheDocument();
    expect(screen.getByText("/srv/webapp")).toBeInTheDocument();
    expect(screen.getByText("/opt/rails-app")).toBeInTheDocument();
    expect(screen.getByText("pip")).toBeInTheDocument();
    expect(screen.getByText("npm")).toBeInTheDocument();
    expect(screen.getByText("gem")).toBeInTheDocument();
  });

  it("renders package count badge per environment", () => {
    render(
      <LanguagePackageList
        environments={envs}
        onToggle={vi.fn()}
        isPending={false}
      />,
    );
    expect(screen.getByText("3 packages")).toBeInTheDocument();
    expect(screen.getAllByText("2 packages")).toHaveLength(2);
  });

  it("shows confidence label with correct color", () => {
    render(
      <LanguagePackageList
        environments={envs}
        onToggle={vi.fn()}
        isPending={false}
      />,
    );
    const highBadges = screen.getAllByText("high");
    expect(highBadges.length).toBeGreaterThanOrEqual(2);
    expect(screen.getByText("medium")).toBeInTheDocument();
  });

  it("checkbox reflects include state", () => {
    render(
      <LanguagePackageList
        environments={envs}
        onToggle={vi.fn()}
        isPending={false}
      />,
    );
    const checkboxes = screen.getAllByRole("checkbox");
    // pip and gem are included, npm is not
    expect(checkboxes[0]).toBeChecked();
    expect(checkboxes[1]).not.toBeChecked();
    expect(checkboxes[2]).toBeChecked();
  });
});
```

- [ ] **Step 2: Verify test fails (component does not exist yet)**

```bash
cd crates/web/ui && npm test -- --run LanguagePackageList
```

- [ ] **Step 3: Implement LanguagePackageList component**

Create `crates/web/ui/src/components/LanguagePackageList.tsx`:

```tsx
import { useCallback } from "react";
import { Badge, Label } from "@patternfly/react-core";
import type { LanguagePackageEnv } from "../api/types";

/** Confidence-to-PatternFly Label color mapping per Plan 1 gate. */
const CONFIDENCE_COLOR: Record<string, "green" | "orange" | "grey"> = {
  high: "green",
  medium: "orange",
  low: "grey",
};

/** Human-readable manifest basis labels. */
const MANIFEST_LABELS: Record<string, string> = {
  "requirements.txt": "from requirements.txt",
  "dist-info": "from dist-info",
  "package-lock.json": "from package-lock.json",
  "Gemfile.lock": "from Gemfile.lock",
};

export interface LanguagePackageListProps {
  environments: LanguagePackageEnv[];
  /** Toggle callback. Receives { ecosystem, path } matching ItemId::LanguageEnv. */
  onToggle: (ecosystem: string, path: string) => void;
  isPending: boolean;
  /** Item to scroll into view (from global search). Format: "ecosystem:path". */
  revealItemId?: string;
}

export function LanguagePackageList({
  environments,
  onToggle,
  isPending,
  revealItemId,
}: LanguagePackageListProps) {
  const handleToggle = useCallback(
    (ecosystem: string, path: string) => {
      if (!isPending) onToggle(ecosystem, path);
    },
    [onToggle, isPending],
  );

  return (
    <div
      className="inspectah-lang-pkg-list"
      role="list"
      aria-label="Language package environments"
      data-testid="language-package-list"
    >
      {environments.map((env) => {
        const itemKey = `${env.ecosystem}:${env.path}`;
        return (
          <div
            key={itemKey}
            role="listitem"
            tabIndex={-1}
            data-testid={`lang-env-row-${itemKey}`}
            className="inspectah-lang-pkg-row"
            data-revealed={revealItemId === itemKey ? "true" : undefined}
          >
            <div className="inspectah-lang-pkg-row__main">
              <div className="inspectah-lang-pkg-row__toggle">
                <input
                  type="checkbox"
                  role="checkbox"
                  checked={env.include}
                  disabled={isPending}
                  aria-label={`Toggle ${env.ecosystem} environment at ${env.path}`}
                  onChange={() => handleToggle(env.ecosystem, env.path)}
                />
              </div>
              <div className="inspectah-lang-pkg-row__info">
                <div className="inspectah-lang-pkg-row__header">
                  <Label
                    className="inspectah-lang-pkg-row__ecosystem"
                    isCompact
                  >
                    {env.ecosystem}
                  </Label>
                  <span className="inspectah-lang-pkg-row__path">
                    {env.path}
                  </span>
                </div>
                <div className="inspectah-lang-pkg-row__meta">
                  <Badge isRead>
                    {env.packages.length} package{env.packages.length !== 1 ? "s" : ""}
                  </Badge>
                  <Label
                    color={CONFIDENCE_COLOR[env.confidence] ?? "grey"}
                    isCompact
                  >
                    {env.confidence}
                  </Label>
                  <span className="inspectah-lang-pkg-row__basis">
                    {MANIFEST_LABELS[env.manifest_basis] ?? env.manifest_basis}
                  </span>
                </div>
              </div>
            </div>
          </div>
        );
      })}
    </div>
  );
}
```

- [ ] **Step 4: Verify test passes**

```bash
cd crates/web/ui && npm test -- --run LanguagePackageList
```

- [ ] **Step 5: Write failing test — toggle calls onToggle with ecosystem + path**

Append to the `LanguagePackageList` describe block:

```typescript
  it("calls onToggle with ecosystem and path when checkbox is clicked", async () => {
    const onToggle = vi.fn();
    render(
      <LanguagePackageList
        environments={envs}
        onToggle={onToggle}
        isPending={false}
      />,
    );
    const user = userEvent.setup();
    const checkboxes = screen.getAllByRole("checkbox");
    await user.click(checkboxes[1]); // npm env
    expect(onToggle).toHaveBeenCalledWith("npm", "/srv/webapp");
  });

  it("disables toggles when isPending is true", () => {
    render(
      <LanguagePackageList
        environments={envs}
        onToggle={vi.fn()}
        isPending={true}
      />,
    );
    const checkboxes = screen.getAllByRole("checkbox");
    checkboxes.forEach((cb) => expect(cb).toBeDisabled());
  });
```

- [ ] **Step 6: Verify tests pass, commit**

```bash
cd crates/web/ui && npm test -- --run LanguagePackageList
git add -A && git commit -m "feat(web): add LanguagePackageList decision section component

Per-environment toggles for pip/npm/gem environments. onToggle
emits { ecosystem, path } matching Plan 1's ItemId::LanguageEnv
contract. Confidence labels and package count badges.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 3: Unmanaged Files Decision Section Component

**Files:**
- Create: `crates/web/ui/src/components/UnmanagedFileList.tsx`
- Test: `crates/web/ui/src/components/__tests__/UnmanagedFileList.test.tsx`

**Interfaces:**
- Consumes: `UnmanagedFileGroup`, `UnmanagedFileItem`, `ProvenanceSignals` from types.ts
- Produces: `UnmanagedFileList` component with directory grouping, per-item toggles, size rollup, provenance signal rendering

- [ ] **Step 1: Write failing test — renders grouped files with provenance signals**

Create `crates/web/ui/src/components/__tests__/UnmanagedFileList.test.tsx`:

```typescript
import { describe, it, expect, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { UnmanagedFileList } from "../UnmanagedFileList";
import type { UnmanagedFileGroup, UnmanagedFileItem, ProvenanceSignals } from "../../api/types";

// --- Test data factories ---

const DEFAULT_PROVENANCE: ProvenanceSignals = {
  file_type: "elf_binary",
  last_modified: 1700000000,
  uid: 0,
  gid: 0,
  permissions: "0755",
  mutability: false,
  writable_mount: false,
  service_working_dir: false,
};

function makeFile(
  path: string,
  overrides?: Partial<UnmanagedFileItem>,
): UnmanagedFileItem {
  return {
    path,
    size: 1024 * 100,
    is_var_path: path.startsWith("/var/"),
    include: true,
    provenance: { ...DEFAULT_PROVENANCE },
    ...overrides,
  };
}

const groups: UnmanagedFileGroup[] = [
  {
    directory: "/opt/splunk",
    items: [
      makeFile("/opt/splunk/bin/splunkd", {
        size: 50 * 1024 * 1024,
        provenance: { ...DEFAULT_PROVENANCE, mutability: true },
      }),
      makeFile("/opt/splunk/etc/system.conf", {
        size: 2048,
        provenance: { ...DEFAULT_PROVENANCE, file_type: "config" },
      }),
      makeFile("/opt/splunk/lib/libcrypto.so", { size: 5 * 1024 * 1024 }),
    ],
  },
  {
    directory: "/srv/myapp",
    items: [
      makeFile("/srv/myapp/app.jar", {
        size: 120 * 1024 * 1024,
        provenance: {
          ...DEFAULT_PROVENANCE,
          file_type: "jar",
          service_working_dir: true,
        },
      }),
      makeFile("/srv/myapp/start.sh", {
        size: 512,
        provenance: { ...DEFAULT_PROVENANCE, file_type: "script" },
      }),
    ],
  },
  {
    directory: "/var/lib/custom",
    items: [
      makeFile("/var/lib/custom/data.db", {
        size: 200 * 1024 * 1024,
        provenance: {
          ...DEFAULT_PROVENANCE,
          file_type: "data_file",
          writable_mount: true,
          mutability: true,
        },
      }),
    ],
  },
];

describe("UnmanagedFileList", () => {
  it("renders directory group headers", () => {
    render(
      <UnmanagedFileList
        groups={groups}
        onToggleItem={vi.fn()}
        onToggleGroup={vi.fn()}
        isPending={false}
      />,
    );
    expect(screen.getByText("/opt/splunk")).toBeInTheDocument();
    expect(screen.getByText("/srv/myapp")).toBeInTheDocument();
    expect(screen.getByText("/var/lib/custom")).toBeInTheDocument();
  });

  it("renders item count per group", () => {
    render(
      <UnmanagedFileList
        groups={groups}
        onToggleItem={vi.fn()}
        onToggleGroup={vi.fn()}
        isPending={false}
      />,
    );
    expect(screen.getByText("3 items")).toBeInTheDocument();
    expect(screen.getByText("2 items")).toBeInTheDocument();
    expect(screen.getByText("1 item")).toBeInTheDocument();
  });

  it("shows /var warning for items under /var", () => {
    render(
      <UnmanagedFileList
        groups={groups}
        onToggleItem={vi.fn()}
        onToggleGroup={vi.fn()}
        isPending={false}
      />,
    );
    expect(
      screen.getByText(/persistent, mutable/),
    ).toBeInTheDocument();
  });

  it("renders provenance signals on file rows", () => {
    render(
      <UnmanagedFileList
        groups={groups}
        onToggleItem={vi.fn()}
        onToggleGroup={vi.fn()}
        isPending={false}
      />,
    );
    // /srv/myapp/app.jar has service_working_dir: true
    expect(screen.getByText(/service workdir/i)).toBeInTheDocument();
    // /var/lib/custom/data.db has writable_mount: true
    expect(screen.getByText(/writable mount/i)).toBeInTheDocument();
    // /opt/splunk/bin/splunkd has mutability: true
    expect(screen.getByText(/modified since install/i)).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Verify test fails**

```bash
cd crates/web/ui && npm test -- --run UnmanagedFileList
```

- [ ] **Step 3: Implement UnmanagedFileList component**

Create `crates/web/ui/src/components/UnmanagedFileList.tsx`:

```tsx
import { useState, useCallback, useMemo } from "react";
import { Badge, Button, Content, Label } from "@patternfly/react-core";
import {
  AngleRightIcon,
  AngleDownIcon,
} from "@patternfly/react-icons";
import type { UnmanagedFileGroup, UnmanagedFileItem, ProvenanceSignals } from "../api/types";

/** Format bytes into human-readable size. */
function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  if (bytes < 1024 * 1024 * 1024)
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
}

/** Render provenance signal badges for a file. */
function ProvenanceBadges({ signals }: { signals: ProvenanceSignals }) {
  return (
    <span className="inspectah-unmanaged-row__provenance">
      {signals.mutability && (
        <Label color="orange" isCompact>modified since install</Label>
      )}
      {signals.writable_mount && (
        <Label color="orange" isCompact>writable mount</Label>
      )}
      {signals.service_working_dir && (
        <Label color="blue" isCompact>service workdir</Label>
      )}
    </span>
  );
}

export interface UnmanagedFileListProps {
  groups: UnmanagedFileGroup[];
  /** Toggle callback. Receives absolute file path matching ItemId::UnmanagedFile { path }. */
  onToggleItem: (path: string) => void;
  onToggleGroup: (directory: string, include: boolean) => void;
  isPending: boolean;
  onIncludeNone?: () => void;
  onResetAll?: () => void;
  /** Item path to scroll into view (from global search). */
  revealItemId?: string;
}

function FileRow({
  item,
  onToggle,
  isPending,
  isRevealed,
}: {
  item: UnmanagedFileItem;
  onToggle: (path: string) => void;
  isPending: boolean;
  isRevealed: boolean;
}) {
  return (
    <div
      className="inspectah-unmanaged-row"
      data-testid={`unmanaged-file-${item.path}`}
      data-revealed={isRevealed ? "true" : undefined}
      role="listitem"
      tabIndex={-1}
      aria-label={item.path}
    >
      <input
        type="checkbox"
        role="checkbox"
        checked={item.include}
        disabled={isPending}
        aria-label={`Toggle ${item.path}`}
        onChange={() => onToggle(item.path)}
      />
      <span className="inspectah-unmanaged-row__path">
        {item.path.split("/").pop()}
      </span>
      <span className="inspectah-unmanaged-row__type">{item.provenance.file_type}</span>
      <span className="inspectah-unmanaged-row__size">
        {formatSize(item.size)}
      </span>
      <ProvenanceBadges signals={item.provenance} />
      {item.is_var_path && (
        <span
          className="inspectah-unmanaged-row__var-warning"
          title="This path is under /var (persistent, mutable). Changes at runtime will not be reset by image updates."
        >
          /var — persistent, mutable
        </span>
      )}
    </div>
  );
}

function DirectoryGroup({
  group,
  onToggleItem,
  onToggleGroup,
  isPending,
  revealItemId,
}: {
  group: UnmanagedFileGroup;
  onToggleItem: (path: string) => void;
  onToggleGroup: (directory: string, include: boolean) => void;
  isPending: boolean;
  revealItemId?: string;
}) {
  const hasRevealedChild = group.items.some((i) => i.path === revealItemId);
  const [isExpanded, setIsExpanded] = useState(true);
  const shouldExpand = isExpanded || hasRevealedChild;

  const allIncluded = group.items.every((i) => i.include);
  const noneIncluded = group.items.every((i) => !i.include);
  const groupIncludedCount = group.items.filter((i) => i.include).length;
  const groupSize = group.items.reduce((sum, i) => sum + i.size, 0);
  const includedSize = group.items
    .filter((i) => i.include)
    .reduce((sum, i) => sum + i.size, 0);

  const handleGroupToggle = useCallback(() => {
    onToggleGroup(group.directory, noneIncluded || !allIncluded);
  }, [group.directory, allIncluded, noneIncluded, onToggleGroup]);

  const handleExpandCollapse = useCallback(() => {
    setIsExpanded((prev) => !prev);
  }, []);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        handleExpandCollapse();
      }
      if (e.key === "ArrowRight" && !shouldExpand) {
        e.preventDefault();
        setIsExpanded(true);
      }
      if (e.key === "ArrowLeft" && shouldExpand) {
        e.preventDefault();
        setIsExpanded(false);
      }
    },
    [shouldExpand, handleExpandCollapse],
  );

  const isVarGroup = group.directory.startsWith("/var/");

  return (
    <div
      className={`inspectah-unmanaged-group${isVarGroup ? " inspectah-unmanaged-group--var" : ""}`}
      data-testid={`unmanaged-group-${group.directory}`}
      role="group"
      aria-label={`${group.directory} file group`}
    >
      <div
        className="inspectah-unmanaged-group__header"
        onClick={handleExpandCollapse}
        onKeyDown={handleKeyDown}
        tabIndex={0}
        role="button"
        aria-expanded={shouldExpand}
      >
        <span className="inspectah-unmanaged-group__chevron">
          {shouldExpand ? <AngleDownIcon /> : <AngleRightIcon />}
        </span>
        <input
          type="checkbox"
          role="checkbox"
          checked={allIncluded}
          ref={(el) => {
            if (el) el.indeterminate = !allIncluded && !noneIncluded;
          }}
          disabled={isPending}
          aria-label={`Toggle all files in ${group.directory}`}
          onChange={handleGroupToggle}
          onClick={(e) => e.stopPropagation()}
        />
        <span className="inspectah-unmanaged-group__name">
          {group.directory}
        </span>
        <Badge isRead>
          {group.items.length} item{group.items.length !== 1 ? "s" : ""}
        </Badge>
        <span className="inspectah-unmanaged-group__rollup" aria-live="polite">
          {groupIncludedCount} of {group.items.length} included, ~{formatSize(includedSize)} of ~{formatSize(groupSize)}
        </span>
        {isVarGroup && (
          <span className="inspectah-unmanaged-group__var-badge">
            /var
          </span>
        )}
      </div>
      {shouldExpand && (
        <div
          className="inspectah-unmanaged-group__items"
          role="list"
          aria-label={`Files in ${group.directory}`}
        >
          {group.items.map((item) => (
            <FileRow
              key={item.path}
              item={item}
              onToggle={onToggleItem}
              isPending={isPending}
              isRevealed={revealItemId === item.path}
            />
          ))}
        </div>
      )}
      <div
        className="inspectah-unmanaged-group__toggle-announce"
        aria-live="polite"
        role="status"
      />
    </div>
  );
}

export function UnmanagedFileList({
  groups,
  onToggleItem,
  onToggleGroup,
  isPending,
  onIncludeNone,
  onResetAll,
  revealItemId,
}: UnmanagedFileListProps) {
  const allItems = useMemo(
    () => groups.flatMap((g) => g.items),
    [groups],
  );

  const totalCount = allItems.length;
  const includedCount = allItems.filter((i) => i.include).length;
  const totalSize = allItems.reduce((sum, i) => sum + i.size, 0);
  const includedSize = allItems
    .filter((i) => i.include)
    .reduce((sum, i) => sum + i.size, 0);

  return (
    <div
      className="inspectah-unmanaged-list"
      data-testid="unmanaged-file-list"
    >
      <div className="inspectah-unmanaged-list__header">
        <Content component="small" data-testid="unmanaged-rollup" aria-live="polite">
          {includedCount} of {totalCount} items included, ~{formatSize(includedSize)} of ~{formatSize(totalSize)}
        </Content>
        <div className="inspectah-unmanaged-list__actions">
          {onIncludeNone && (
            <Button
              variant="link"
              isSmall
              onClick={onIncludeNone}
              isDisabled={isPending || includedCount === 0}
            >
              Include None
            </Button>
          )}
          {onResetAll && (
            <Button
              variant="link"
              isSmall
              onClick={onResetAll}
              isDisabled={isPending || includedCount === totalCount}
            >
              Reset to All
            </Button>
          )}
        </div>
      </div>
      {groups.map((group) => (
        <DirectoryGroup
          key={group.directory}
          group={group}
          onToggleItem={onToggleItem}
          onToggleGroup={onToggleGroup}
          isPending={isPending}
          revealItemId={revealItemId}
        />
      ))}
    </div>
  );
}
```

- [ ] **Step 4: Verify test passes**

```bash
cd crates/web/ui && npm test -- --run UnmanagedFileList
```

- [ ] **Step 5: Write failing tests — group toggle, size rollup, grouped accessibility**

Append to `UnmanagedFileList.test.tsx`:

```typescript
  it("calls onToggleGroup when group checkbox is clicked", async () => {
    const onToggleGroup = vi.fn();
    render(
      <UnmanagedFileList
        groups={groups}
        onToggleItem={vi.fn()}
        onToggleGroup={onToggleGroup}
        isPending={false}
      />,
    );
    const user = userEvent.setup();
    const groupCb = screen.getByLabelText("Toggle all files in /opt/splunk");
    await user.click(groupCb);
    expect(onToggleGroup).toHaveBeenCalledWith("/opt/splunk", expect.any(Boolean));
  });

  it("shows running size rollup in header", () => {
    render(
      <UnmanagedFileList
        groups={groups}
        onToggleItem={vi.fn()}
        onToggleGroup={vi.fn()}
        isPending={false}
      />,
    );
    const rollup = screen.getByTestId("unmanaged-rollup");
    expect(rollup.textContent).toMatch(/6 of 6 items included/);
  });

  it("calls onToggleItem with file path when individual file checkbox is clicked", async () => {
    const onToggleItem = vi.fn();
    render(
      <UnmanagedFileList
        groups={groups}
        onToggleItem={onToggleItem}
        onToggleGroup={vi.fn()}
        isPending={false}
      />,
    );
    const user = userEvent.setup();
    const fileCb = screen.getByLabelText("Toggle /opt/splunk/bin/splunkd");
    await user.click(fileCb);
    expect(onToggleItem).toHaveBeenCalledWith("/opt/splunk/bin/splunkd");
  });

  it("Include None button calls onIncludeNone", async () => {
    const onIncludeNone = vi.fn();
    render(
      <UnmanagedFileList
        groups={groups}
        onToggleItem={vi.fn()}
        onToggleGroup={vi.fn()}
        isPending={false}
        onIncludeNone={onIncludeNone}
        onResetAll={vi.fn()}
      />,
    );
    const user = userEvent.setup();
    await user.click(screen.getByText("Include None"));
    expect(onIncludeNone).toHaveBeenCalled();
  });

  // --- Grouped accessibility ---

  it("group header has role='button' and aria-expanded", () => {
    render(
      <UnmanagedFileList
        groups={groups}
        onToggleItem={vi.fn()}
        onToggleGroup={vi.fn()}
        isPending={false}
      />,
    );
    const groupHeader = screen.getByLabelText("/opt/splunk file group")
      .querySelector("[role='button']")!;
    expect(groupHeader).toHaveAttribute("aria-expanded", "true");
  });

  it("group rollup has aria-live='polite' for debounced size announcements", () => {
    render(
      <UnmanagedFileList
        groups={groups}
        onToggleItem={vi.fn()}
        onToggleGroup={vi.fn()}
        isPending={false}
      />,
    );
    const rollup = screen.getByTestId("unmanaged-rollup");
    expect(rollup).toHaveAttribute("aria-live", "polite");
  });

  it("ArrowRight on collapsed group expands it", async () => {
    render(
      <UnmanagedFileList
        groups={groups}
        onToggleItem={vi.fn()}
        onToggleGroup={vi.fn()}
        isPending={false}
      />,
    );
    const user = userEvent.setup();
    // First collapse a group by clicking the header
    const groupHeader = screen.getByLabelText("/opt/splunk file group")
      .querySelector("[role='button']")! as HTMLElement;
    await user.click(groupHeader);
    expect(groupHeader).toHaveAttribute("aria-expanded", "false");
    // ArrowRight should expand it
    groupHeader.focus();
    await user.keyboard("{ArrowRight}");
    expect(groupHeader).toHaveAttribute("aria-expanded", "true");
  });

  it("ArrowLeft on expanded group collapses it", async () => {
    render(
      <UnmanagedFileList
        groups={groups}
        onToggleItem={vi.fn()}
        onToggleGroup={vi.fn()}
        isPending={false}
      />,
    );
    const user = userEvent.setup();
    const groupHeader = screen.getByLabelText("/opt/splunk file group")
      .querySelector("[role='button']")! as HTMLElement;
    expect(groupHeader).toHaveAttribute("aria-expanded", "true");
    groupHeader.focus();
    await user.keyboard("{ArrowLeft}");
    expect(groupHeader).toHaveAttribute("aria-expanded", "false");
  });

  // --- Arrow-key navigation between groups and items ---

  it("ArrowDown from group header moves focus to first item in group", async () => {
    render(
      <UnmanagedFileList
        groups={groups}
        onToggleItem={vi.fn()}
        onToggleGroup={vi.fn()}
        isPending={false}
      />,
    );
    const user = userEvent.setup();
    const groupHeader = screen.getByLabelText("/opt/splunk file group")
      .querySelector("[role='button']")! as HTMLElement;
    groupHeader.focus();
    await user.keyboard("{ArrowDown}");
    expect(document.activeElement).toBe(
      screen.getByTestId("unmanaged-item-/opt/splunk/bin/splunkd"),
    );
  });

  it("ArrowDown from last item in group moves focus to next group header", async () => {
    // groups fixture has /opt/splunk (2 items) and /opt/datadog (1 item)
    render(
      <UnmanagedFileList
        groups={twoGroupFixture}
        onToggleItem={vi.fn()}
        onToggleGroup={vi.fn()}
        isPending={false}
      />,
    );
    const user = userEvent.setup();
    const lastItem = screen.getByTestId("unmanaged-item-/opt/splunk/etc/config.yml");
    lastItem.focus();
    await user.keyboard("{ArrowDown}");
    const nextGroupHeader = screen.getByLabelText("/opt/datadog file group")
      .querySelector("[role='button']")! as HTMLElement;
    expect(document.activeElement).toBe(nextGroupHeader);
  });

  it("ArrowUp from first item in group moves focus back to group header", async () => {
    render(
      <UnmanagedFileList
        groups={groups}
        onToggleItem={vi.fn()}
        onToggleGroup={vi.fn()}
        isPending={false}
      />,
    );
    const user = userEvent.setup();
    const firstItem = screen.getByTestId("unmanaged-item-/opt/splunk/bin/splunkd");
    firstItem.focus();
    await user.keyboard("{ArrowUp}");
    const groupHeader = screen.getByLabelText("/opt/splunk file group")
      .querySelector("[role='button']")! as HTMLElement;
    expect(document.activeElement).toBe(groupHeader);
  });

  // --- Polite announcements for group and item toggles ---

  it("announces group toggle via aria-live", async () => {
    render(
      <UnmanagedFileList
        groups={groups}
        onToggleItem={vi.fn()}
        onToggleGroup={vi.fn()}
        isPending={false}
      />,
    );
    const user = userEvent.setup();
    const groupCb = screen.getByLabelText("Toggle all files in /opt/splunk");
    await user.click(groupCb);
    const liveRegion = screen.getByTestId("unmanaged-group-announce-/opt/splunk");
    expect(liveRegion).toHaveAttribute("aria-live", "polite");
    expect(liveRegion.textContent).toMatch(/Excluded \d+ files in \/opt\/splunk/);
  });

  it("announces item toggle via aria-live", async () => {
    render(
      <UnmanagedFileList
        groups={groups}
        onToggleItem={vi.fn()}
        onToggleGroup={vi.fn()}
        isPending={false}
      />,
    );
    const user = userEvent.setup();
    const fileCb = screen.getByLabelText("Toggle /opt/splunk/bin/splunkd");
    await user.click(fileCb);
    const liveRegion = screen.getByTestId("unmanaged-item-announce-/opt/splunk/bin/splunkd");
    expect(liveRegion).toHaveAttribute("aria-live", "polite");
    expect(liveRegion.textContent).toMatch(/Excluded \/opt\/splunk\/bin\/splunkd/);
  });

  // --- Debounced size-rollup announcement ---

  it("debounces size-rollup announcement after rapid toggles", async () => {
    vi.useFakeTimers();
    render(
      <UnmanagedFileList
        groups={groups}
        onToggleItem={vi.fn()}
        onToggleGroup={vi.fn()}
        isPending={false}
      />,
    );
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    const rollupAnnounce = screen.getByTestId("unmanaged-rollup-announce");

    // Toggle two items rapidly
    await user.click(screen.getByLabelText("Toggle /opt/splunk/bin/splunkd"));
    await user.click(screen.getByLabelText("Toggle /opt/splunk/etc/config.yml"));

    // Before debounce fires, announcement should not have updated
    expect(rollupAnnounce.textContent).toBe("");

    // After 500ms debounce, announcement should fire once
    vi.advanceTimersByTime(500);
    expect(rollupAnnounce.textContent).toMatch(/\d+ of \d+ items included, ~[\d.]+ [KMGT]?B/);

    vi.useRealTimers();
  });
```

**Implementation note for the debounced rollup:** The `UnmanagedFileList`
component needs a separate `aria-live="polite"` region
(`data-testid="unmanaged-rollup-announce"`) that is updated via a 500ms
debounced callback after any toggle. The visible rollup text
(`data-testid="unmanaged-rollup"`) updates immediately; the announcement
region updates on the debounce to avoid spamming screen readers during
rapid toggling.

- [ ] **Step 6: Verify tests pass, commit**

```bash
cd crates/web/ui && npm test -- --run UnmanagedFileList
git add -A && git commit -m "feat(web): add UnmanagedFileList with directory grouping and provenance signals

Grouped by parent directory with per-item and per-group toggles,
running size rollup, /var path warnings, provenance signal badges
(mutability, writable mount, service workdir), Include None/Reset
All bulk actions, and ArrowLeft/ArrowRight keyboard expand/collapse.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 4: RPM Upload State Machine Hook

**Files:**
- Create: `crates/web/ui/src/hooks/useRpmUpload.ts`
- Test: `crates/web/ui/src/components/__tests__/useRpmUpload.test.ts`

**Interfaces:**
- Consumes: `RpmUploadRowState` from types.ts, `PackageEntry.repoless_annotation`, `PackageEntry.repoless_cached` from Plan 2
- Produces: `useRpmUpload` hook managing the 5-state row machine. Uploads call `POST /api/upload-rpm`, not just local state.

Row state derivation contract (from Plan 2 backend fields):

| `repoless_annotation` | `repoless_cached` | Local upload? | Row State |
|------------------------|-------------------|---------------|-----------|
| non-empty | `true` | no | `cached_excluded` (toggleable) |
| non-empty | `true` | no, user-included | `cached_included` |
| non-empty | `false` | no | `needs_upload` |
| non-empty | `false` | yes | `uploaded_excluded` |
| non-empty | `false` | yes, user-included | `uploaded_included` |

- [ ] **Step 1: Write failing test — state machine transitions with backend integration**

Create `crates/web/ui/src/components/__tests__/useRpmUpload.test.ts`:

```typescript
import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook, act } from "@testing-library/react";
import { useRpmUpload } from "../../hooks/useRpmUpload";

// Mock fetch for POST /api/upload-rpm
const mockFetch = vi.fn();
globalThis.fetch = mockFetch;

beforeEach(() => {
  mockFetch.mockReset();
  mockFetch.mockResolvedValue({ ok: true, json: async () => ({ ok: true }) });
});

describe("useRpmUpload", () => {
  it("derives cached_excluded state from backend fields", () => {
    const { result } = renderHook(() => useRpmUpload());
    act(() => {
      result.current.initFromBackend([
        { name: "custom-tool", arch: "x86_64", repoless_annotation: "No repo source — cached RPM bundled", repoless_cached: true },
      ]);
    });
    expect(result.current.getRowState("custom-tool")).toBe("cached_excluded");
  });

  it("derives needs_upload state from backend fields when not cached", () => {
    const { result } = renderHook(() => useRpmUpload());
    act(() => {
      result.current.initFromBackend([
        { name: "my-agent", arch: "x86_64", repoless_annotation: "No repo source — manual resolution needed", repoless_cached: false },
      ]);
    });
    expect(result.current.getRowState("my-agent")).toBe("needs_upload");
  });

  it("returns undefined for non-repoless packages", () => {
    const { result } = renderHook(() => useRpmUpload());
    act(() => {
      result.current.initFromBackend([]);
    });
    expect(result.current.getRowState("normal-package")).toBeUndefined();
  });

  it("uploadRpm calls POST /api/upload-rpm and transitions state", async () => {
    const { result } = renderHook(() => useRpmUpload());
    act(() => {
      result.current.initFromBackend([
        { name: "nginx", arch: "x86_64", repoless_annotation: "manual resolution", repoless_cached: false },
      ]);
    });
    const mockFile = new File(["rpm-content"], "nginx-1.24-1.el9.x86_64.rpm", {
      type: "application/x-rpm",
    });
    await act(async () => {
      await result.current.uploadRpm("nginx", mockFile);
    });
    // Verify POST was called
    expect(mockFetch).toHaveBeenCalledWith(
      "/api/upload-rpm",
      expect.objectContaining({ method: "POST" }),
    );
    expect(result.current.getRowState("nginx")).toBe("uploaded_excluded");
  });

  it("removeUpload transitions back to needs_upload", async () => {
    const { result } = renderHook(() => useRpmUpload());
    act(() => {
      result.current.initFromBackend([
        { name: "nginx", arch: "x86_64", repoless_annotation: "manual resolution", repoless_cached: false },
      ]);
    });
    const mockFile = new File(["rpm-content"], "nginx-1.24-1.el9.x86_64.rpm");
    await act(async () => {
      await result.current.uploadRpm("nginx", mockFile);
    });
    expect(result.current.getRowState("nginx")).toBe("uploaded_excluded");
    act(() => {
      result.current.removeUpload("nginx");
    });
    expect(result.current.getRowState("nginx")).toBe("needs_upload");
  });

  it("validateFilename accepts matching NEVRA", () => {
    const { result } = renderHook(() => useRpmUpload());
    expect(
      result.current.validateFilename("nginx", "x86_64", "nginx-1.24-1.el9.x86_64.rpm"),
    ).toEqual({ valid: true });
  });

  it("validateFilename rejects wrong package name", () => {
    const { result } = renderHook(() => useRpmUpload());
    const validation = result.current.validateFilename(
      "nginx",
      "x86_64",
      "httpd-2.4-1.el9.x86_64.rpm",
    );
    expect(validation.valid).toBe(false);
    expect(validation.error).toContain("nginx");
  });

  it("validateFilename rejects non-.rpm extension", () => {
    const { result } = renderHook(() => useRpmUpload());
    const validation = result.current.validateFilename(
      "nginx",
      "x86_64",
      "nginx-1.24-1.el9.x86_64.tar.gz",
    );
    expect(validation.valid).toBe(false);
    expect(validation.error).toContain(".rpm");
  });

  it("needsUploadCount counts packages still needing uploads", async () => {
    const { result } = renderHook(() => useRpmUpload());
    act(() => {
      result.current.initFromBackend([
        { name: "nginx", arch: "x86_64", repoless_annotation: "manual", repoless_cached: false },
        { name: "custom-agent", arch: "x86_64", repoless_annotation: "manual", repoless_cached: false },
        { name: "my-tool", arch: "x86_64", repoless_annotation: "manual", repoless_cached: false },
      ]);
    });
    expect(result.current.needsUploadCount).toBe(3);
    const mockFile = new File(["rpm"], "nginx-1.0-1.el9.x86_64.rpm");
    await act(async () => {
      await result.current.uploadRpm("nginx", mockFile);
    });
    expect(result.current.needsUploadCount).toBe(2);
  });

  it("batchMatch matches files to packages by name prefix", () => {
    const { result } = renderHook(() => useRpmUpload());
    act(() => {
      result.current.initFromBackend([
        { name: "nginx", arch: "x86_64", repoless_annotation: "manual", repoless_cached: false },
        { name: "custom-agent", arch: "x86_64", repoless_annotation: "manual", repoless_cached: false },
      ]);
    });
    const files = [
      new File(["rpm1"], "nginx-1.24-1.el9.x86_64.rpm"),
      new File(["rpm2"], "custom-agent-2.0-1.el9.x86_64.rpm"),
      new File(["rpm3"], "unrelated-3.0-1.el9.x86_64.rpm"),
    ];
    let matchResult: ReturnType<typeof result.current.batchMatch>;
    act(() => {
      matchResult = result.current.batchMatch(files);
    });
    expect(matchResult!.matched).toHaveLength(2);
    expect(matchResult!.unmatched).toHaveLength(1);
    expect(matchResult!.conflicts).toHaveLength(0);
  });

  it("batchMatch detects conflicts when multiple files match same package", () => {
    const { result } = renderHook(() => useRpmUpload());
    act(() => {
      result.current.initFromBackend([
        { name: "nginx", arch: "x86_64", repoless_annotation: "manual", repoless_cached: false },
      ]);
    });
    const files = [
      new File(["rpm1"], "nginx-1.24-1.el9.x86_64.rpm"),
      new File(["rpm2"], "nginx-1.25-1.el9.x86_64.rpm"),
    ];
    let matchResult: ReturnType<typeof result.current.batchMatch>;
    act(() => {
      matchResult = result.current.batchMatch(files);
    });
    expect(matchResult!.conflicts).toHaveLength(1);
    expect(matchResult!.conflicts[0].packageName).toBe("nginx");
    expect(matchResult!.conflicts[0].files).toHaveLength(2);
  });
});
```

- [ ] **Step 2: Verify test fails**

```bash
cd crates/web/ui && npm test -- --run useRpmUpload
```

- [ ] **Step 3: Implement useRpmUpload hook**

Create `crates/web/ui/src/hooks/useRpmUpload.ts`:

```typescript
import { useState, useCallback, useMemo } from "react";
import type { RpmUploadRowState } from "../api/types";

interface RepolessEntry {
  name: string;
  arch: string;
  repoless_annotation: string;
  repoless_cached: boolean;
}

interface ValidationResult {
  valid: boolean;
  error?: string;
}

interface BatchMatchResult {
  matched: Array<{ packageName: string; file: File }>;
  unmatched: File[];
  conflicts: Array<{ packageName: string; files: File[] }>;
}

export interface UseRpmUploadResult {
  /** Initialize from backend PackageEntry fields. */
  initFromBackend: (entries: RepolessEntry[]) => void;
  /** Get the current row state for a package. Returns undefined for non-repoless packages. */
  getRowState: (packageName: string) => RpmUploadRowState | undefined;
  /** Upload an RPM via POST /api/upload-rpm. Transitions to uploaded_excluded on success. */
  uploadRpm: (packageName: string, file: File) => Promise<void>;
  /** Remove a local upload, reverting to needs_upload. */
  removeUpload: (packageName: string) => void;
  /** Validate a filename against expected NEVRA. */
  validateFilename: (packageName: string, arch: string, filename: string) => ValidationResult;
  /** Number of packages still needing uploads. */
  needsUploadCount: number;
  /** Match multiple files to registered packages by name prefix. */
  batchMatch: (files: File[]) => BatchMatchResult;
  /** Apply a batch match result — upload all matched files via backend. */
  applyBatchMatch: (matched: BatchMatchResult["matched"]) => Promise<void>;
  /** Get names of packages that need RPM uploads. */
  needsUploadPackages: string[];
}

/** Extract the package name prefix from an RPM filename (before first hyphen followed by a digit). */
function extractPackageName(filename: string): string | null {
  const match = filename.match(/^(.+?)-\d/);
  return match ? match[1] : null;
}

export function useRpmUpload(): UseRpmUploadResult {
  const [repolessMap, setRepolessMap] = useState<Map<string, RepolessEntry>>(
    () => new Map(),
  );
  const [uploadedSet, setUploadedSet] = useState<Set<string>>(
    () => new Set(),
  );

  const initFromBackend = useCallback((entries: RepolessEntry[]) => {
    const map = new Map<string, RepolessEntry>();
    for (const e of entries) {
      map.set(e.name, e);
    }
    setRepolessMap(map);
  }, []);

  const getRowState = useCallback(
    (packageName: string): RpmUploadRowState | undefined => {
      const entry = repolessMap.get(packageName);
      if (!entry) return undefined;

      if (uploadedSet.has(packageName)) {
        return "uploaded_excluded";
      }
      if (entry.repoless_cached) {
        return "cached_excluded";
      }
      return "needs_upload";
    },
    [repolessMap, uploadedSet],
  );

  const uploadRpm = useCallback(
    async (packageName: string, file: File) => {
      const formData = new FormData();
      formData.append("file", file);

      const response = await fetch("/api/upload-rpm", {
        method: "POST",
        body: formData,
      });

      if (!response.ok) {
        throw new Error(`Upload failed: ${response.statusText}`);
      }

      setUploadedSet((prev) => {
        const next = new Set(prev);
        next.add(packageName);
        return next;
      });
    },
    [],
  );

  const removeUpload = useCallback(
    (packageName: string) => {
      setUploadedSet((prev) => {
        const next = new Set(prev);
        next.delete(packageName);
        return next;
      });
    },
    [],
  );

  const validateFilename = useCallback(
    (packageName: string, arch: string, filename: string): ValidationResult => {
      if (!filename.endsWith(".rpm")) {
        return {
          valid: false,
          error: `File must end in .rpm, got "${filename}"`,
        };
      }

      const extractedName = extractPackageName(filename);
      if (!extractedName || extractedName !== packageName) {
        return {
          valid: false,
          error: `Filename must match package "${packageName}", got "${extractedName ?? filename}"`,
        };
      }

      const archPattern = `.${arch}.rpm`;
      if (!filename.endsWith(archPattern) && !filename.endsWith(".noarch.rpm")) {
        return {
          valid: false,
          error: `Expected architecture "${arch}" or "noarch", check filename`,
        };
      }

      return { valid: true };
    },
    [],
  );

  const needsUploadCount = useMemo(() => {
    let count = 0;
    for (const [name, entry] of repolessMap) {
      if (!entry.repoless_cached && !uploadedSet.has(name)) {
        count++;
      }
    }
    return count;
  }, [repolessMap, uploadedSet]);

  const needsUploadPackages = useMemo(() => {
    const pkgs: string[] = [];
    for (const [name, entry] of repolessMap) {
      if (!entry.repoless_cached && !uploadedSet.has(name)) {
        pkgs.push(name);
      }
    }
    return pkgs;
  }, [repolessMap, uploadedSet]);

  const batchMatch = useCallback(
    (files: File[]): BatchMatchResult => {
      const matched: BatchMatchResult["matched"] = [];
      const unmatched: File[] = [];
      const conflictMap = new Map<string, File[]>();

      for (const file of files) {
        const extractedName = extractPackageName(file.name);
        if (!extractedName || !repolessMap.has(extractedName)) {
          unmatched.push(file);
          continue;
        }

        const existing = conflictMap.get(extractedName);
        if (existing) {
          existing.push(file);
        } else if (matched.some((m) => m.packageName === extractedName)) {
          const prev = matched.find((m) => m.packageName === extractedName)!;
          conflictMap.set(extractedName, [prev.file, file]);
          matched.splice(matched.indexOf(prev), 1);
        } else {
          matched.push({ packageName: extractedName, file });
        }
      }

      return {
        matched,
        unmatched,
        conflicts: Array.from(conflictMap.entries()).map(
          ([packageName, conflictFiles]) => ({
            packageName,
            files: conflictFiles,
          }),
        ),
      };
    },
    [repolessMap],
  );

  const applyBatchMatch = useCallback(
    async (matched: BatchMatchResult["matched"]) => {
      for (const { packageName, file } of matched) {
        await uploadRpm(packageName, file);
      }
    },
    [uploadRpm],
  );

  return {
    initFromBackend,
    getRowState,
    uploadRpm,
    removeUpload,
    validateFilename,
    needsUploadCount,
    batchMatch,
    applyBatchMatch,
    needsUploadPackages,
  };
}
```

- [ ] **Step 4: Verify tests pass, commit**

```bash
cd crates/web/ui && npm test -- --run useRpmUpload
git add -A && git commit -m "feat(web): add useRpmUpload hook wired to POST /api/upload-rpm

Row states derived from Plan 2's repoless_annotation/repoless_cached
backend fields. Uploads call POST /api/upload-rpm so RPMs are durable
and exportable, not just held in UI hook state. Batch matching with
conflict detection for multi-file uploads.

Assisted-by: Claude Code (Opus 4.6)"
```

**Thorn Checkpoint: Tasks 1-4** — Types (including ProvenanceSignals and repoless backend fields), LanguagePackageList (with ecosystem+path toggle contract), UnmanagedFileList (with provenance signals, grouped accessibility), and useRpmUpload (wired to backend). Verify all tests pass with `npm test -- --run`. Verify: LanguagePackageList onToggle emits `(ecosystem, path)` not opaque `id`; UnmanagedFileList renders provenance badges; useRpmUpload calls `fetch("/api/upload-rpm")`.

---

## Task 5: RPM Upload Modal (Single File)

**Files:**
- Create: `crates/web/ui/src/components/RpmUploadModal.tsx`
- Test: `crates/web/ui/src/components/__tests__/RpmUploadModal.test.tsx`

**Interfaces:**
- Consumes: `useRpmUpload` hook, PatternFly `Modal`, `FileUpload`
- Produces: `RpmUploadModal` component with focus trap, focus return, tab order per spec

- [ ] **Step 1: Write failing test — modal with focus and accessibility**

Create `crates/web/ui/src/components/__tests__/RpmUploadModal.test.tsx`:

```typescript
import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { RpmUploadModal } from "../RpmUploadModal";

describe("RpmUploadModal", () => {
  const defaultProps = {
    isOpen: true,
    packageName: "custom-agent",
    packageArch: "x86_64",
    onUpload: vi.fn(),
    onClose: vi.fn(),
    triggerRef: { current: null } as React.RefObject<HTMLElement | null>,
  };

  it("renders modal with package name in title", () => {
    render(<RpmUploadModal {...defaultProps} />);
    expect(
      screen.getByText(/Upload RPM for custom-agent/),
    ).toBeInTheDocument();
  });

  it("shows expected NEVRA pattern", () => {
    render(<RpmUploadModal {...defaultProps} />);
    expect(
      screen.getByText(/custom-agent.*x86_64\.rpm/),
    ).toBeInTheDocument();
  });

  it("confirm button is disabled when no file is selected", () => {
    render(<RpmUploadModal {...defaultProps} />);
    const confirmBtn = screen.getByRole("button", { name: /confirm|upload/i });
    expect(confirmBtn).toBeDisabled();
  });

  it("does not render when isOpen is false", () => {
    render(<RpmUploadModal {...defaultProps} isOpen={false} />);
    expect(screen.queryByText(/Upload RPM/)).not.toBeInTheDocument();
  });

  it("has accessible modal label", () => {
    render(<RpmUploadModal {...defaultProps} />);
    expect(
      screen.getByRole("dialog", { name: /Upload RPM for custom-agent/ }),
    ).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Verify test fails**

```bash
cd crates/web/ui && npm test -- --run RpmUploadModal
```

- [ ] **Step 3: Implement RpmUploadModal with focus management**

Create `crates/web/ui/src/components/RpmUploadModal.tsx`:

```tsx
import { useState, useCallback, useEffect, useRef } from "react";
import {
  Modal,
  ModalVariant,
  Button,
  FileUpload,
  HelperText,
  HelperTextItem,
  Content,
} from "@patternfly/react-core";
import { CheckCircleIcon, ExclamationCircleIcon } from "@patternfly/react-icons";

export interface RpmUploadModalProps {
  isOpen: boolean;
  packageName: string;
  packageArch: string;
  onUpload: (packageName: string, file: File) => void;
  onClose: () => void;
  /** Ref to the trigger element for focus return on close. */
  triggerRef: React.RefObject<HTMLElement | null>;
}

function validateRpmFile(
  packageName: string,
  arch: string,
  filename: string,
): { valid: boolean; error?: string } {
  if (!filename.endsWith(".rpm")) {
    return { valid: false, error: "File must be an .rpm package" };
  }
  const match = filename.match(/^(.+?)-\d/);
  const extractedName = match ? match[1] : null;
  if (!extractedName || extractedName !== packageName) {
    return {
      valid: false,
      error: `Expected package "${packageName}", filename suggests "${extractedName ?? "unknown"}"`,
    };
  }
  const validArch = filename.endsWith(`.${arch}.rpm`) || filename.endsWith(".noarch.rpm");
  if (!validArch) {
    return { valid: false, error: `Expected architecture "${arch}" or "noarch"` };
  }
  return { valid: true };
}

export function RpmUploadModal({
  isOpen,
  packageName,
  packageArch,
  onUpload,
  onClose,
  triggerRef,
}: RpmUploadModalProps) {
  const [file, setFile] = useState<File | null>(null);
  const [filename, setFilename] = useState("");
  const [validation, setValidation] = useState<{
    valid: boolean;
    error?: string;
  } | null>(null);
  const uploadAreaRef = useRef<HTMLDivElement>(null);

  // Focus the upload area on open
  useEffect(() => {
    if (isOpen) {
      // Defer to let modal mount
      const timer = setTimeout(() => {
        uploadAreaRef.current?.focus();
      }, 50);
      return () => clearTimeout(timer);
    }
  }, [isOpen]);

  const handleFileChange = useCallback(
    (_event: unknown, selectedFile: File | undefined) => {
      if (!selectedFile) {
        setFile(null);
        setFilename("");
        setValidation(null);
        return;
      }
      setFile(selectedFile);
      setFilename(selectedFile.name);
      setValidation(validateRpmFile(packageName, packageArch, selectedFile.name));
    },
    [packageName, packageArch],
  );

  const handleClear = useCallback(() => {
    setFile(null);
    setFilename("");
    setValidation(null);
  }, []);

  const handleConfirm = useCallback(() => {
    if (file && validation?.valid) {
      onUpload(packageName, file);
      handleClear();
      onClose();
      // Return focus to trigger element
      triggerRef.current?.focus();
    }
  }, [file, validation, packageName, onUpload, onClose, handleClear, triggerRef]);

  const handleClose = useCallback(() => {
    handleClear();
    onClose();
    // Return focus to trigger element
    triggerRef.current?.focus();
  }, [onClose, handleClear, triggerRef]);

  if (!isOpen) return null;

  return (
    <Modal
      variant={ModalVariant.medium}
      title={`Upload RPM for ${packageName}`}
      isOpen={isOpen}
      onClose={handleClose}
      actions={[
        <Button
          key="confirm"
          variant="primary"
          onClick={handleConfirm}
          isDisabled={!file || !validation?.valid}
          aria-label="Confirm upload"
        >
          Upload
        </Button>,
        <Button key="cancel" variant="link" onClick={handleClose}>
          Cancel
        </Button>,
      ]}
      aria-label={`Upload RPM for ${packageName}`}
    >
      <Content component="p">
        Expected filename pattern:{" "}
        <code>
          {packageName}-*-*.{packageArch}.rpm
        </code>
      </Content>
      <div ref={uploadAreaRef} tabIndex={-1}>
        <FileUpload
          id={`rpm-upload-${packageName}`}
          value={file ?? undefined}
          filename={filename}
          onChange={handleFileChange}
          onClearClick={handleClear}
          browseButtonText="Choose RPM"
          dropzoneProps={{
            accept: { "application/x-rpm": [".rpm"] },
          }}
          aria-label={`Upload RPM for ${packageName}`}
        />
      </div>
      {validation && (
        <HelperText>
          <HelperTextItem
            variant={validation.valid ? "success" : "error"}
            icon={
              validation.valid ? <CheckCircleIcon /> : <ExclamationCircleIcon />
            }
          >
            {validation.valid
              ? `${filename} matches ${packageName}`
              : validation.error}
          </HelperTextItem>
        </HelperText>
      )}
    </Modal>
  );
}
```

- [ ] **Step 4: Verify tests pass, commit**

```bash
cd crates/web/ui && npm test -- --run RpmUploadModal
git add -A && git commit -m "feat(web): add RpmUploadModal with NEVRA validation and focus management

Focus moves to upload area on open, returns to trigger element on
close (success or cancel). Tab order: upload area -> file picker
button -> cancel -> confirm per spec.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 6: RPM Batch Upload Modal with Conflicts View

**Files:**
- Create: `crates/web/ui/src/components/RpmBatchUploadModal.tsx`
- Test: `crates/web/ui/src/components/__tests__/RpmUploadModal.test.tsx` (append)

**Interfaces:**
- Consumes: `useRpmUpload.batchMatch`, PatternFly `Modal`, `MultipleFileUpload`
- Produces: `RpmBatchUploadModal` with matched/unmatched/conflicts view before confirm

- [ ] **Step 1: Write failing test — batch modal renders with conflicts view**

Append to `RpmUploadModal.test.tsx`:

```typescript
import { RpmBatchUploadModal } from "../RpmBatchUploadModal";

describe("RpmBatchUploadModal", () => {
  const defaultBatchProps = {
    isOpen: true,
    needsUploadPackages: ["nginx", "custom-agent", "my-tool"],
    onBatchUpload: vi.fn(),
    onClose: vi.fn(),
  };

  it("renders modal with package count", () => {
    render(<RpmBatchUploadModal {...defaultBatchProps} />);
    expect(
      screen.getByText(/Upload RPMs.*3 packages/i),
    ).toBeInTheDocument();
  });

  it("confirm button is disabled when no files are dropped", () => {
    render(<RpmBatchUploadModal {...defaultBatchProps} />);
    const confirmBtn = screen.getByRole("button", { name: /confirm|upload/i });
    expect(confirmBtn).toBeDisabled();
  });

  it("does not render when isOpen is false", () => {
    render(<RpmBatchUploadModal {...defaultBatchProps} isOpen={false} />);
    expect(screen.queryByText(/Upload RPMs/)).not.toBeInTheDocument();
  });

  it("has accessible modal label", () => {
    render(<RpmBatchUploadModal {...defaultBatchProps} />);
    expect(
      screen.getByRole("dialog", { name: /Upload RPMs/i }),
    ).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Verify test fails**

```bash
cd crates/web/ui && npm test -- --run RpmUploadModal
```

- [ ] **Step 3: Implement RpmBatchUploadModal with conflicts view**

Create `crates/web/ui/src/components/RpmBatchUploadModal.tsx`:

```tsx
import { useState, useCallback, useMemo } from "react";
import {
  Modal,
  ModalVariant,
  Button,
  MultipleFileUpload,
  MultipleFileUploadMain,
  MultipleFileUploadStatusItem,
  MultipleFileUploadStatus,
  Content,
  Label,
  Alert,
} from "@patternfly/react-core";
import {
  CheckCircleIcon,
  ExclamationCircleIcon,
  ExclamationTriangleIcon,
  InProgressIcon,
} from "@patternfly/react-icons";

/** Extract the package name prefix from an RPM filename. */
function extractPackageName(filename: string): string | null {
  const match = filename.match(/^(.+?)-\d/);
  return match ? match[1] : null;
}

interface MatchResult {
  matched: Array<{ packageName: string; file: File }>;
  unmatched: File[];
  conflicts: Array<{ packageName: string; files: File[] }>;
}

export interface RpmBatchUploadModalProps {
  isOpen: boolean;
  needsUploadPackages: string[];
  onBatchUpload: (matched: Array<{ packageName: string; file: File }>) => void;
  onClose: () => void;
}

export function RpmBatchUploadModal({
  isOpen,
  needsUploadPackages,
  onBatchUpload,
  onClose,
}: RpmBatchUploadModalProps) {
  const [files, setFiles] = useState<File[]>([]);
  const packageSet = useMemo(
    () => new Set(needsUploadPackages),
    [needsUploadPackages],
  );

  const matchResult: MatchResult = useMemo(() => {
    const matched: MatchResult["matched"] = [];
    const unmatched: File[] = [];
    const conflictMap = new Map<string, File[]>();

    for (const file of files) {
      if (!file.name.endsWith(".rpm")) {
        unmatched.push(file);
        continue;
      }
      const name = extractPackageName(file.name);
      if (!name || !packageSet.has(name)) {
        unmatched.push(file);
        continue;
      }

      const existing = conflictMap.get(name);
      if (existing) {
        existing.push(file);
      } else if (matched.some((m) => m.packageName === name)) {
        const prev = matched.find((m) => m.packageName === name)!;
        conflictMap.set(name, [prev.file, file]);
        matched.splice(matched.indexOf(prev), 1);
      } else {
        matched.push({ packageName: name, file });
      }
    }

    return {
      matched,
      unmatched,
      conflicts: Array.from(conflictMap.entries()).map(
        ([packageName, conflictFiles]) => ({
          packageName,
          files: conflictFiles,
        }),
      ),
    };
  }, [files, packageSet]);

  const handleFileDrop = useCallback((_event: unknown, droppedFiles: File[]) => {
    setFiles((prev) => [...prev, ...droppedFiles]);
  }, []);

  const handleRemoveFile = useCallback((removedFile: File) => {
    setFiles((prev) => prev.filter((f) => f !== removedFile));
  }, []);

  const handleConfirm = useCallback(() => {
    if (matchResult.matched.length > 0) {
      onBatchUpload(matchResult.matched);
      setFiles([]);
      onClose();
    }
  }, [matchResult, onBatchUpload, onClose]);

  const handleClose = useCallback(() => {
    setFiles([]);
    onClose();
  }, [onClose]);

  if (!isOpen) return null;

  const summary = `${matchResult.matched.length} of ${files.length} RPMs matched`;

  return (
    <Modal
      variant={ModalVariant.large}
      title={`Upload RPMs (${needsUploadPackages.length} packages need RPMs)`}
      isOpen={isOpen}
      onClose={handleClose}
      actions={[
        <Button
          key="confirm"
          variant="primary"
          onClick={handleConfirm}
          isDisabled={matchResult.matched.length === 0}
          aria-label="Confirm upload"
        >
          Upload {matchResult.matched.length > 0 ? `(${matchResult.matched.length})` : ""}
        </Button>,
        <Button key="cancel" variant="link" onClick={handleClose}>
          Cancel
        </Button>,
      ]}
      aria-label={`Upload RPMs for ${needsUploadPackages.length} packages`}
    >
      <MultipleFileUpload
        onFileDrop={handleFileDrop}
        dropzoneProps={{
          accept: { "application/x-rpm": [".rpm"] },
        }}
      >
        <MultipleFileUploadMain
          titleIcon={<InProgressIcon />}
          titleText="Drag and drop RPM files here"
          titleTextSeparator="or"
          infoText="Accepted file types: .rpm"
        />
        {files.length > 0 && (
          <MultipleFileUploadStatus
            statusToggleText={summary}
            statusToggleIcon={
              matchResult.matched.length > 0 ? "success" : "danger"
            }
          >
            {matchResult.matched.map(({ packageName, file }) => (
              <MultipleFileUploadStatusItem
                key={packageName}
                file={file}
                onClearClick={() => handleRemoveFile(file)}
                progressValue={100}
                progressVariant="success"
              >
                <Label color="green" isCompact icon={<CheckCircleIcon />}>
                  Matched: {packageName}
                </Label>
              </MultipleFileUploadStatusItem>
            ))}
            {matchResult.unmatched.map((file, idx) => (
              <MultipleFileUploadStatusItem
                key={`unmatched-${idx}`}
                file={file}
                onClearClick={() => handleRemoveFile(file)}
                progressValue={100}
                progressVariant="danger"
              >
                <Label color="red" isCompact icon={<ExclamationCircleIcon />}>
                  No match
                </Label>
              </MultipleFileUploadStatusItem>
            ))}
          </MultipleFileUploadStatus>
        )}
      </MultipleFileUpload>
      {/* Conflicts view — shown before confirm when multiple files match the same package */}
      {matchResult.conflicts.length > 0 && (
        <Alert
          variant="warning"
          isInline
          title="Conflicting uploads"
          className="inspectah-batch-upload__conflicts"
        >
          <Content component="small">
            {matchResult.conflicts.map((c) => (
              <div key={c.packageName}>
                <ExclamationTriangleIcon /> <strong>{c.packageName}</strong>: {c.files.length} files match.
                Remove duplicates to resolve: {c.files.map((f) => f.name).join(", ")}
              </div>
            ))}
          </Content>
        </Alert>
      )}
      <Content component="small" aria-live="polite">
        {files.length > 0 && summary}
      </Content>
    </Modal>
  );
}
```

- [ ] **Step 4: Verify tests pass, commit**

```bash
cd crates/web/ui && npm test -- --run RpmUploadModal
git add -A && git commit -m "feat(web): add RpmBatchUploadModal with conflicts view

Batch upload with auto-matching, matched/unmatched/conflicts
breakdown shown before confirm. Conflicts alert with per-package
duplicate file listing prevents ambiguous uploads.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 7: PackageList RPM Upload Row Integration

**Files:**
- Modify: `crates/web/ui/src/components/PackageList.tsx`
- Modify: `crates/web/ui/src/App.css`
- Test: `crates/web/ui/src/components/__tests__/PackageList.test.tsx` (append)

**Interfaces:**
- Consumes: `useRpmUpload` hook, `RpmUploadModal`, spec's RPM Upload Row Contract (all 5 states)
- Produces: Modified package rows with 5-state upload behavior, repo-text transitions, row-level `aria-live`

- [ ] **Step 1: Write failing tests — full 5-state row contract**

Append to `PackageList.test.tsx`:

```typescript
  // --- RPM upload row states (full 5-state contract) ---

  describe("RPM upload rows", () => {
    it("renders upload icon instead of checkbox for needs_upload packages", () => {
      const pkgs = [makePkg("custom-agent", "none", false)];
      render(
        <PackageList
          mode="single"
          packages={pkgs}
          repoGroups={allRepos}
          onToggle={vi.fn()}
          onRepoToggle={vi.fn()}
          rpmRowStates={{ "custom-agent": "needs_upload" }}
        />,
      );
      const row = screen.getByTestId("package-row-custom-agent");
      expect(within(row).queryByRole("checkbox")).not.toBeInTheDocument();
      expect(
        within(row).getByLabelText("Upload RPM for custom-agent"),
      ).toBeInTheDocument();
    });

    it("shows orange 'RPM needed' label for needs_upload state", () => {
      const pkgs = [makePkg("custom-agent", "none", false)];
      render(
        <PackageList
          mode="single"
          packages={pkgs}
          repoGroups={allRepos}
          onToggle={vi.fn()}
          onRepoToggle={vi.fn()}
          rpmRowStates={{ "custom-agent": "needs_upload" }}
        />,
      );
      expect(screen.getByText("RPM needed")).toBeInTheDocument();
    });

    it("shows 'none' as repo text for needs_upload state", () => {
      const pkgs = [makePkg("custom-agent", "none", false)];
      render(
        <PackageList
          mode="single"
          packages={pkgs}
          repoGroups={allRepos}
          onToggle={vi.fn()}
          onRepoToggle={vi.fn()}
          rpmRowStates={{ "custom-agent": "needs_upload" }}
        />,
      );
      const row = screen.getByTestId("package-row-custom-agent");
      expect(within(row).getByText("none")).toBeInTheDocument();
    });

    it("shows checkbox and green label after upload (uploaded_excluded)", () => {
      const pkgs = [makePkg("custom-agent", "none", false)];
      render(
        <PackageList
          mode="single"
          packages={pkgs}
          repoGroups={allRepos}
          onToggle={vi.fn()}
          onRepoToggle={vi.fn()}
          rpmRowStates={{ "custom-agent": "uploaded_excluded" }}
        />,
      );
      const row = screen.getByTestId("package-row-custom-agent");
      expect(within(row).getByRole("checkbox")).toBeInTheDocument();
      expect(screen.getByText("RPM provided")).toBeInTheDocument();
    });

    it("shows 'uploaded' as repo text for uploaded state", () => {
      const pkgs = [makePkg("custom-agent", "none", false)];
      render(
        <PackageList
          mode="single"
          packages={pkgs}
          repoGroups={allRepos}
          onToggle={vi.fn()}
          onRepoToggle={vi.fn()}
          rpmRowStates={{ "custom-agent": "uploaded_excluded" }}
        />,
      );
      const row = screen.getByTestId("package-row-custom-agent");
      expect(within(row).getByText("uploaded")).toBeInTheDocument();
    });

    it("shows orange 'No repo' label for cached_excluded state", () => {
      const pkgs = [makePkg("custom-tool", "disabled-repo", false)];
      render(
        <PackageList
          mode="single"
          packages={pkgs}
          repoGroups={allRepos}
          onToggle={vi.fn()}
          onRepoToggle={vi.fn()}
          rpmRowStates={{ "custom-tool": "cached_excluded" }}
        />,
      );
      expect(screen.getByText("No repo")).toBeInTheDocument();
      const row = screen.getByTestId("package-row-custom-tool");
      expect(within(row).getByRole("checkbox")).toBeInTheDocument();
    });

    it("mutes row opacity for needs_upload state", () => {
      const pkgs = [makePkg("custom-agent", "none", false)];
      render(
        <PackageList
          mode="single"
          packages={pkgs}
          repoGroups={allRepos}
          onToggle={vi.fn()}
          onRepoToggle={vi.fn()}
          rpmRowStates={{ "custom-agent": "needs_upload" }}
        />,
      );
      const row = screen.getByTestId("package-row-custom-agent");
      expect(row.className).toContain("--blocked");
    });

    it("row has aria-live region for state transition announcements", () => {
      const pkgs = [makePkg("custom-agent", "none", false)];
      render(
        <PackageList
          mode="single"
          packages={pkgs}
          repoGroups={allRepos}
          onToggle={vi.fn()}
          onRepoToggle={vi.fn()}
          rpmRowStates={{ "custom-agent": "uploaded_excluded" }}
        />,
      );
      const row = screen.getByTestId("package-row-custom-agent");
      const liveRegion = within(row).getByRole("status");
      expect(liveRegion).toHaveAttribute("aria-live", "polite");
    });

    // --- Upload success: focus returns to new checkbox ---

    it("focuses checkbox after upload success replaces upload trigger", async () => {
      const pkgs = [makePkg("custom-agent", "none", false)];
      const { rerender } = render(
        <PackageList
          mode="single"
          packages={pkgs}
          repoGroups={allRepos}
          onToggle={vi.fn()}
          onRepoToggle={vi.fn()}
          rpmRowStates={{ "custom-agent": "needs_upload" }}
        />,
      );
      // Simulate upload success — row transitions to uploaded_excluded
      rerender(
        <PackageList
          mode="single"
          packages={pkgs}
          repoGroups={allRepos}
          onToggle={vi.fn()}
          onRepoToggle={vi.fn()}
          rpmRowStates={{ "custom-agent": "uploaded_excluded" }}
        />,
      );
      const row = screen.getByTestId("package-row-custom-agent");
      const checkbox = within(row).getByRole("checkbox");
      expect(document.activeElement).toBe(checkbox);
    });

    it("announces upload success via aria-live", async () => {
      const pkgs = [makePkg("custom-agent", "none", false)];
      const { rerender } = render(
        <PackageList
          mode="single"
          packages={pkgs}
          repoGroups={allRepos}
          onToggle={vi.fn()}
          onRepoToggle={vi.fn()}
          rpmRowStates={{ "custom-agent": "needs_upload" }}
        />,
      );
      rerender(
        <PackageList
          mode="single"
          packages={pkgs}
          repoGroups={allRepos}
          onToggle={vi.fn()}
          onRepoToggle={vi.fn()}
          rpmRowStates={{ "custom-agent": "uploaded_excluded" }}
        />,
      );
      const row = screen.getByTestId("package-row-custom-agent");
      const liveRegion = within(row).getByRole("status");
      expect(liveRegion.textContent).toMatch(
        /RPM provided for custom-agent/,
      );
    });

    // --- Upload removal: focus and announcement ---

    it("announces RPM removal and returns to blocked state", async () => {
      const onRemoveUpload = vi.fn();
      const pkgs = [makePkg("custom-agent", "none", false)];
      const { rerender } = render(
        <PackageList
          mode="single"
          packages={pkgs}
          repoGroups={allRepos}
          onToggle={vi.fn()}
          onRepoToggle={vi.fn()}
          rpmRowStates={{ "custom-agent": "uploaded_excluded" }}
          onRemoveRpmUpload={onRemoveUpload}
        />,
      );
      const user = userEvent.setup();
      // Click the remove button on the "RPM provided" label
      const removeBtn = screen.getByLabelText("Remove uploaded RPM for custom-agent");
      await user.click(removeBtn);
      expect(onRemoveUpload).toHaveBeenCalledWith("custom-agent");

      // Simulate state reverting to needs_upload
      rerender(
        <PackageList
          mode="single"
          packages={pkgs}
          repoGroups={allRepos}
          onToggle={vi.fn()}
          onRepoToggle={vi.fn()}
          rpmRowStates={{ "custom-agent": "needs_upload" }}
          onRemoveRpmUpload={onRemoveUpload}
        />,
      );
      const row = screen.getByTestId("package-row-custom-agent");
      const liveRegion = within(row).getByRole("status");
      expect(liveRegion.textContent).toMatch(
        /RPM removed for custom-agent.*upload required/,
      );
      // Checkbox should be hidden again, upload icon back
      expect(within(row).queryByRole("checkbox")).not.toBeInTheDocument();
      expect(
        within(row).getByLabelText("Upload RPM for custom-agent"),
      ).toBeInTheDocument();
    });
  });
```

- [ ] **Step 2: Verify tests fail**

```bash
cd crates/web/ui && npm test -- --run PackageList
```

- [ ] **Step 3: Add rpmRowStates prop to PackageList and implement full row contract**

In `PackageList.tsx`, add the new props:

```typescript
  /** Per-package RPM row state derived from backend + upload state. */
  rpmRowStates?: Record<string, RpmUploadRowState>;
  /** Callback when upload icon is clicked on a blocked row. */
  onUploadClick?: (packageName: string) => void;
  /** Callback when remove button is clicked on an uploaded row. */
  onRemoveUpload?: (packageName: string) => void;
```

In the row rendering, add state derivation and the full 5-state contract:

```typescript
const rowState = rpmRowStates?.[pkg.name];
const isBlocked = rowState === "needs_upload";
const isUploaded = rowState === "uploaded_excluded" || rowState === "uploaded_included";
const isCached = rowState === "cached_excluded" || rowState === "cached_included";

// Repo text transitions
const repoText = isBlocked ? "none" : isUploaded ? "uploaded" : pkg.source_repo;

// Row-level aria-live announcement — covers both success and revert
// Use a ref to track the previous state so announcements only fire
// on transitions, not on initial render.
const prevRowStateRef = useRef(rowState);
const [liveAnnouncement, setLiveAnnouncement] = useState("");
useEffect(() => {
  const prev = prevRowStateRef.current;
  prevRowStateRef.current = rowState;
  if (prev === "needs_upload" && (rowState === "uploaded_excluded" || rowState === "uploaded_included")) {
    setLiveAnnouncement(`RPM provided for ${pkg.name}, now available for inclusion`);
    // Focus the new checkbox after the upload trigger is replaced
    requestAnimationFrame(() => checkboxRef.current?.focus());
  } else if ((prev === "uploaded_excluded" || prev === "uploaded_included") && rowState === "needs_upload") {
    setLiveAnnouncement(`RPM removed for ${pkg.name}, upload required`);
  }
}, [rowState, pkg.name]);
```

Replace the checkbox with conditional rendering per spec's blocked-row layout:

```tsx
{isBlocked ? (
  <Button
    variant="plain"
    aria-label={`Upload RPM for ${pkg.name}`}
    onClick={(e) => {
      e.stopPropagation();
      onUploadClick?.(pkg.name);
    }}
    icon={<UploadIcon />}
    className="inspectah-package-row__upload-btn"
  />
) : (
  <input
    ref={checkboxRef}
    type="checkbox"
    role="checkbox"
    checked={pkg.include}
    aria-label={pkg.name}
    onChange={() => onToggle(pkg.name)}
  />
)}
```

Add upload-state badges:

```tsx
{isBlocked && (
  <Label color="orange" isCompact>RPM needed</Label>
)}
{isUploaded && (
  <Label color="green" isCompact>
    RPM provided
    <Button
      variant="plain"
      isSmall
      aria-label={`Remove uploaded RPM for ${pkg.name}`}
      onClick={(e) => {
        e.stopPropagation();
        onRemoveUpload?.(pkg.name);
      }}
    >
      x
    </Button>
  </Label>
)}
{isCached && (
  <Label color="orange" isCompact>No repo</Label>
)}
```

Add row-level `aria-live` region inside the row:

```tsx
<span role="status" aria-live="polite" className="inspectah-package-row__live">
  {liveAnnouncement}
</span>
```

Add blocked class modifier:

```tsx
className={`inspectah-package-row${isBlocked ? " inspectah-package-row--blocked" : ""}`}
```

Import `Button`, `Label` from PatternFly and `UploadIcon` from `@patternfly/react-icons`.

- [ ] **Step 4: Add CSS for blocked row state**

Append to `App.css`:

```css
/* ─── RPM upload blocked row ────────────────────────────────────────── */

.inspectah-package-row--blocked {
  opacity: 0.7;
}

.inspectah-package-row__upload-btn {
  flex-shrink: 0;
  padding: 2px;
}

.inspectah-package-row__live {
  position: absolute;
  width: 1px;
  height: 1px;
  overflow: hidden;
  clip: rect(0, 0, 0, 0);
}
```

- [ ] **Step 5: Verify tests pass, commit**

```bash
cd crates/web/ui && npm test -- --run PackageList
git add -A && git commit -m "feat(web): integrate full 5-state RPM upload contract into PackageList

All 5 row states from spec: cached_excluded/included, needs_upload,
uploaded_excluded/included. Repo-text transitions (none -> uploaded).
Row-level aria-live for upload/remove announcements. Blocked rows
hide checkbox and show upload icon.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 8: Sidebar Updates + Keyboard Navigation

**Files:**
- Modify: `crates/web/ui/src/components/Sidebar.tsx`
- Modify: `crates/web/ui/src/hooks/useKeyboard.ts`
- Test: new `crates/web/ui/src/components/__tests__/Sidebar.test.tsx` or append to existing

**Interfaces:**
- Consumes: `ViewResponse.language_packages`, `ViewResponse.has_unmanaged_scan`, `ViewResponse.unmanaged_files`
- Produces: Updated sidebar with new review sections, updated keyboard shortcuts

- [ ] **Step 1: Write failing test — sidebar shows Language Packages section**

Create sidebar tests:

```typescript
import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import { Sidebar } from "../Sidebar";

describe("Sidebar section ordering", () => {
  it("shows Language Packages in review group after Containers", () => {
    render(
      <Sidebar
        activeSection="packages"
        onSelect={vi.fn()}
        referenceSections={[]}
        hasLanguagePackages={true}
        hasUnmanagedFiles={false}
        hasUnmanagedScan={false}
      />,
    );
    expect(screen.getByText("Language Packages")).toBeInTheDocument();
  });

  it("shows discoverability hint when unmanaged scan was not used", () => {
    render(
      <Sidebar
        activeSection="packages"
        onSelect={vi.fn()}
        referenceSections={[]}
        hasLanguagePackages={false}
        hasUnmanagedFiles={false}
        hasUnmanagedScan={false}
      />,
    );
    expect(
      screen.getByText(/Re-run with.*--include-unmanaged/),
    ).toBeInTheDocument();
  });

  it("shows Unmanaged Files section when scan data exists", () => {
    render(
      <Sidebar
        activeSection="packages"
        onSelect={vi.fn()}
        referenceSections={[]}
        hasLanguagePackages={false}
        hasUnmanagedFiles={true}
        hasUnmanagedScan={true}
      />,
    );
    expect(screen.getByText("Unmanaged Files")).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Verify test fails**

```bash
cd crates/web/ui && npm test -- --run Sidebar
```

- [ ] **Step 3: Update Sidebar.tsx — add new review sections**

Add new props to `SidebarProps`:

```typescript
  hasLanguagePackages?: boolean;
  hasUnmanagedFiles?: boolean;
  hasUnmanagedScan?: boolean;
```

Build the review sections list dynamically. The new sections go after `containers`:

```typescript
const reviewSections = useMemo(() => {
  const base = [
    { id: "packages", label: "Packages" },
    { id: "configs", label: "Config Files" },
    { id: "users_groups", label: "Users & Groups" },
    { id: "services", label: "Services" },
    { id: "containers", label: "Containers" },
  ];
  if (hasLanguagePackages) {
    base.push({ id: "language_packages", label: "Language Packages" });
  }
  if (hasUnmanagedFiles) {
    base.push({ id: "unmanaged_files", label: "Unmanaged Files" });
  }
  return base;
}, [hasLanguagePackages, hasUnmanagedFiles]);
```

Add discoverability hint when `hasUnmanagedScan === false`:

```tsx
{!hasUnmanagedScan && (
  <Content
    component="small"
    className="inspectah-sidebar__hint"
    data-testid="unmanaged-hint"
  >
    Unmanaged files not scanned. Re-run with{" "}
    <code>--include-unmanaged</code> to review.
  </Content>
)}
```

- [ ] **Step 4: Update useKeyboard.ts — insert new section IDs per spec shortcut map**

In `useKeyboard.ts`, update `SINGLE_HOST_SECTION_IDS` to match the spec's full shortcut map:

```typescript
const SINGLE_HOST_SECTION_IDS = [
  "packages",           // 1
  "configs",            // 2
  "users_groups",       // 3
  "services",           // 4
  "containers",         // 5
  "language_packages",  // 6 (new — was: version_changes)
  "unmanaged_files",    // 7 (new — was: compose)
  "version_changes",    // 8 (was 6 → now 8)
  "compose",            // 9 (was 7 → now 9)
  // network, storage lose shortcuts (were 8, 9)
];
```

Key 7 (`unmanaged_files`) is a no-op when the section is not visible — the `onSectionChange` callback handles missing sections gracefully.

- [ ] **Step 5: Verify tests pass, commit**

```bash
cd crates/web/ui && npm test -- --run
git add -A && git commit -m "feat(web): add Language Packages and Unmanaged Files to sidebar

New review sections after Containers with keyboard shortcuts 6-7.
Discoverability hint when --include-unmanaged was not used.
Keys 6-9 remapped per spec shortcut map: Language Packages=6,
Unmanaged Files=7, Version Changes=8, Compose=9. Network and
Storage lose shortcuts.

Assisted-by: Claude Code (Opus 4.6)"
```

**Thorn Checkpoint: Tasks 5-8** — Upload modals (with focus trap and conflicts view), PackageList integration (all 5 row states, repo-text transitions, aria-live), and sidebar/keyboard changes complete. Run `npm test -- --run` and verify all existing tests still pass. Check keyboard shortcuts 1-9 against the spec's shortcut map.

---

## Task 9: MainContent Section Rendering + Search/Focus Plumbing

**Files:**
- Modify: `crates/web/ui/src/components/MainContent.tsx`
- Test: `crates/web/ui/src/components/__tests__/SectionPlumbing.test.tsx` (new)

**Interfaces:**
- Consumes: `LanguagePackageList`, `UnmanagedFileList`, `SectionSearch`, `ViewResponse`
- Produces: Section rendering in MainContent with search filtering, match counts, first-item focus, reveal highlighting

This task addresses the review finding that search/focus plumbing belongs to MainContent.tsx (which owns section rendering, SectionSearch integration, and the `revealItemId` prop) — not to the list components themselves.

- [ ] **Step 1: Write failing tests for section plumbing**

Create `crates/web/ui/src/components/__tests__/SectionPlumbing.test.tsx`:

```typescript
import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

// These test the integration of new sections into MainContent's
// existing search/focus plumbing. They verify:
// 1. Section renders when activeSection matches
// 2. SectionSearch filters items and shows match count
// 3. First item receives focus when section is selected
// 4. Reveal highlighting works via revealItemId prop

describe("MainContent — Language Packages section", () => {
  it("renders LanguagePackageList when activeSection is language_packages", () => {
    // Render MainContent with activeSection="language_packages" and
    // viewData containing language_packages array.
    // Assert: language-package-list test ID is in the document.
    // Implementation: MainContent adds an `if (activeSection === "language_packages")` block.
  });

  it("renders SectionSearch for language_packages section", () => {
    // Render MainContent with activeSection="language_packages",
    // sectionSearchOpen=true.
    // Assert: SectionSearch component is rendered.
  });

  it("focuses first language package row when section is selected", () => {
    // Render MainContent with activeSection="language_packages".
    // Assert: document.querySelector('[data-testid^="lang-env-row-"]')
    //   receives focus (matching existing firstItem focus pattern at line ~286).
  });
});

describe("MainContent — Unmanaged Files section", () => {
  it("renders UnmanagedFileList when activeSection is unmanaged_files", () => {
    // Same pattern as above.
  });

  it("renders SectionSearch for unmanaged_files section", () => {
    // Same pattern as above.
  });

  it("focuses first unmanaged file group when section is selected", () => {
    // Same pattern as above.
  });
});
```

Note: These tests follow the same pattern as MainContent's existing section tests (lines 304-509 in the current file). The exact test implementation depends on MainContent's test harness — adapt to match the existing `MainContent.test.tsx` patterns for rendering with mock ViewResponse data.

- [ ] **Step 2: Add SECTION_LABELS entries to MainContent**

In `MainContent.tsx`, add to the existing `SECTION_LABELS` record:

```typescript
const SECTION_LABELS: Record<string, string> = {
  // ... existing entries ...
  language_packages: "Language Packages",
  unmanaged_files: "Unmanaged Files",
};
```

- [ ] **Step 3: Add Language Packages section rendering**

Add a new `if (activeSection === "language_packages")` block following the pattern of existing sections (e.g., the `configs` block around line 367). This block:

1. Renders a section header with the label
2. Renders `SectionSearch` when `sectionSearchOpen` is true (matching existing pattern)
3. Renders `LanguagePackageList` with `revealItemId` prop from MainContent's existing reveal plumbing
4. Focuses the first item when the section mounts (matching the existing `firstItem?.focus()` pattern at line ~286)

```typescript
if (activeSection === "language_packages") {
  const langPkgs = viewData?.language_packages ?? [];
  return (
    <div className="inspectah-main-content" data-testid="section-language-packages">
      <h2 className="inspectah-main-content__heading">
        {SECTION_LABELS.language_packages}
      </h2>
      {sectionSearchOpen && (
        <SectionSearch
          // ... match existing SectionSearch pattern from configs section
        />
      )}
      <LanguagePackageList
        environments={langPkgs}
        onToggle={(ecosystem, path) => {
          onToggleLangEnv?.(ecosystem, path);
        }}
        isPending={isPending}
        revealItemId={revealItemId}
      />
    </div>
  );
}
```

- [ ] **Step 4: Add Unmanaged Files section rendering**

Same pattern. Add a new `if (activeSection === "unmanaged_files")` block:

```typescript
if (activeSection === "unmanaged_files") {
  const groups = viewData?.unmanaged_files ?? [];
  return (
    <div className="inspectah-main-content" data-testid="section-unmanaged-files">
      <h2 className="inspectah-main-content__heading">
        {SECTION_LABELS.unmanaged_files}
      </h2>
      {sectionSearchOpen && (
        <SectionSearch
          // ... match existing SectionSearch pattern
        />
      )}
      <UnmanagedFileList
        groups={groups}
        onToggleItem={onToggleUnmanagedFile}
        onToggleGroup={onToggleUnmanagedGroup}
        isPending={isPending}
        onIncludeNone={onUnmanagedIncludeNone}
        onResetAll={onUnmanagedResetAll}
        revealItemId={revealItemId}
      />
    </div>
  );
}
```

- [ ] **Step 5: Add first-item focus for new sections**

In MainContent's existing focus effect (around line 286), extend the query selector to handle the new sections:

```typescript
// Existing pattern:
const firstItem = document.querySelector(
  `[data-testid="section-${activeSection}"] [tabindex="-1"], [data-testid="section-${activeSection}"] [role="listitem"]`,
) as HTMLElement | null;
firstItem?.focus();
```

This already works generically if the section test IDs follow the `section-{id}` pattern and list items have `tabIndex={-1}`.

Write explicit tests in `MainContent.test.tsx` (or create
`MainContent.newSections.test.tsx`):

```typescript
it("focuses first language package item when language_packages section is selected", () => {
  const { rerender } = render(
    <MainContent activeSection="packages" {...defaultProps} />,
  );
  rerender(<MainContent activeSection="language_packages" {...defaultProps} />);
  const firstItem = screen.getByTestId("section-language_packages")
    .querySelector("[tabindex='-1']");
  expect(document.activeElement).toBe(firstItem);
});

it("focuses first unmanaged file group when unmanaged_files section is selected", () => {
  const { rerender } = render(
    <MainContent activeSection="packages" {...defaultProps} />,
  );
  rerender(<MainContent activeSection="unmanaged_files" {...defaultProps} />);
  const firstGroup = screen.getByTestId("section-unmanaged_files")
    .querySelector("[role='button']");
  expect(document.activeElement).toBe(firstGroup);
});

it("SectionSearch filters language package environments by path", async () => {
  render(<MainContent activeSection="language_packages" {...defaultProps} />);
  const user = userEvent.setup();
  const searchInput = screen.getByPlaceholderText("Filter items...");
  await user.type(searchInput, "myapp");
  expect(screen.getByText("/opt/myapp/venv")).toBeInTheDocument();
  expect(screen.queryByText("/opt/other/venv")).not.toBeInTheDocument();
});

it("SectionSearch filters unmanaged files and auto-expands matching groups", async () => {
  render(<MainContent activeSection="unmanaged_files" {...defaultProps} />);
  const user = userEvent.setup();
  const searchInput = screen.getByPlaceholderText("Filter items...");
  await user.type(searchInput, "splunkd");
  // Group containing splunkd should be expanded
  const groupHeader = screen.getByLabelText("/opt/splunk file group")
    .querySelector("[role='button']")!;
  expect(groupHeader).toHaveAttribute("aria-expanded", "true");
  expect(screen.getByText("/opt/splunk/bin/splunkd")).toBeInTheDocument();
});

it("reveal highlighting scrolls to and highlights search result", async () => {
  render(<MainContent activeSection="language_packages" {...defaultProps}
    revealItemId="pip:/opt/myapp/venv" />);
  const item = screen.getByTestId("langpkg-item-pip:/opt/myapp/venv");
  expect(item).toHaveClass("inspectah-reveal-highlight");
});
```

- [ ] **Step 6: Add MainContent props for new section callbacks**

Add to `MainContentProps`:

```typescript
  onToggleLangEnv?: (ecosystem: string, path: string) => void;
  onToggleUnmanagedFile?: (path: string) => void;
  onToggleUnmanagedGroup?: (directory: string, include: boolean) => void;
  onUnmanagedIncludeNone?: () => void;
  onUnmanagedResetAll?: () => void;
  isPending?: boolean;
```

- [ ] **Step 7: Verify tests pass, commit**

```bash
cd crates/web/ui && npm test -- --run
git add -A && git commit -m "feat(web): add Language Packages and Unmanaged Files to MainContent

Section rendering, SectionSearch integration, first-item focus,
and reveal highlighting for new sections. Follows existing
MainContent section rendering pattern.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 10: AppShell Focus Management + Shortcut Help

**Files:**
- Modify: `crates/web/ui/src/components/AppShell.tsx`
- Modify: `crates/web/ui/src/hooks/useKeyboard.ts` (shortcut help overlay data)

**Interfaces:**
- Consumes: section IDs from Sidebar, `onSectionChange` callback
- Produces: ArrowDown-to-first-item behavior for new sections, shortcut help overlay updates

- [ ] **Step 1: Verify AppShell's existing focus contract extends to new sections**

AppShell owns shell-level focus management (line 68: `activeSection` prop, line 142: useEffect on `activeSection`). The existing `onSectionChange` callback already handles arbitrary section IDs. Verify:

1. When `activeSection` changes to `"language_packages"` or `"unmanaged_files"`, the shell-level focus effect fires correctly (it should, since it's generic)
2. ArrowDown from the sidebar navigates to the first item in the active section (line ~189: `onSectionChange` callback)

If the existing code already handles this generically (likely, since it uses `activeSection` without section-specific branching), document that in a test:

```typescript
// In AppShell tests or SectionPlumbing.test.tsx:
it("ArrowDown from sidebar moves focus to first item in language_packages section", () => {
  // This tests the existing generic behavior works for new section IDs.
});
```

- [ ] **Step 2: Update shortcut help overlay data**

In `useKeyboard.ts`, if there is a shortcut help mapping (for the `?` overlay), update it to reflect the new key assignments:

```typescript
const SHORTCUT_HELP = [
  { key: "1", section: "Packages" },
  { key: "2", section: "Config Files" },
  { key: "3", section: "Users & Groups" },
  { key: "4", section: "Services" },
  { key: "5", section: "Containers" },
  { key: "6", section: "Language Packages" },   // new
  { key: "7", section: "Unmanaged Files" },      // new
  { key: "8", section: "Version Changes" },      // was 6
  { key: "9", section: "Compose" },              // was 7
];
```

- [ ] **Step 3: Verify tests pass, commit**

```bash
cd crates/web/ui && npm test -- --run
git add -A && git commit -m "feat(web): extend AppShell focus management and shortcut help for new sections

ArrowDown-to-first-item behavior works generically for new section
IDs. Shortcut help overlay updated to show new key assignments
(6=Language Packages, 7=Unmanaged Files).

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 11: Global Search Integration

**Files:**
- Modify: `crates/web/ui/src/components/GlobalSearch.tsx`
- Test: `crates/web/ui/src/components/__tests__/GlobalSearch.test.tsx` (append)

**Interfaces:**
- Consumes: `LanguagePackageEnv[]`, `UnmanagedFileGroup[]` from ViewResponse
- Produces: Search results for new sections in global search, with navigation to correct section and reveal highlighting

- [ ] **Step 1: Write failing tests — global search finds new section items**

Append to `GlobalSearch.test.tsx`:

```typescript
  it("finds language package environments by path", async () => {
    const langEnvs = [
      {
        ecosystem: "pip" as const,
        path: "/opt/myapp/venv",
        method: "pip list",
        packages: ["flask", "requests"],
        confidence: "high" as const,
        manifest_basis: "requirements.txt",
        include: true,
      },
    ];
    render(
      <GlobalSearch
        packageItems={[]}
        configItems={[]}
        referenceSections={[]}
        languagePackageEnvs={langEnvs}
        onNavigate={vi.fn()}
      />,
    );
    const user = userEvent.setup();
    const input = screen.getByRole("searchbox");
    await user.type(input, "myapp");
    expect(screen.getByText("/opt/myapp/venv")).toBeInTheDocument();
  });

  it("finds language package environments by package name", async () => {
    const langEnvs = [
      {
        ecosystem: "pip" as const,
        path: "/opt/myapp/venv",
        method: "pip list",
        packages: ["flask", "requests"],
        confidence: "high" as const,
        manifest_basis: "requirements.txt",
        include: true,
      },
    ];
    render(
      <GlobalSearch
        packageItems={[]}
        configItems={[]}
        referenceSections={[]}
        languagePackageEnvs={langEnvs}
        onNavigate={vi.fn()}
      />,
    );
    const user = userEvent.setup();
    const input = screen.getByRole("searchbox");
    await user.type(input, "flask");
    // Should match the environment that contains flask
    expect(screen.getByText(/flask.*pip.*\/opt\/myapp\/venv/)).toBeInTheDocument();
  });

  it("finds unmanaged files by path", async () => {
    const unmanagedGroups = [
      {
        directory: "/opt/splunk",
        items: [
          {
            path: "/opt/splunk/bin/splunkd",
            size: 1024,
            is_var_path: false,
            include: true,
            provenance: {
              file_type: "elf_binary" as const,
              last_modified: 1700000000,
              uid: 0,
              gid: 0,
              permissions: "0755",
              mutability: false,
              writable_mount: false,
              service_working_dir: false,
            },
          },
        ],
      },
    ];
    render(
      <GlobalSearch
        packageItems={[]}
        configItems={[]}
        referenceSections={[]}
        unmanagedFileGroups={unmanagedGroups}
        onNavigate={vi.fn()}
      />,
    );
    const user = userEvent.setup();
    const input = screen.getByRole("searchbox");
    await user.type(input, "splunkd");
    expect(screen.getByText("/opt/splunk/bin/splunkd")).toBeInTheDocument();
  });
```

- [ ] **Step 2: Verify tests fail**

```bash
cd crates/web/ui && npm test -- --run GlobalSearch
```

- [ ] **Step 3: Update GlobalSearch.tsx — add new section items**

Add `SECTION_LABELS` entries:

```typescript
const SECTION_LABELS: Record<string, string> = {
  // ... existing entries ...
  language_packages: "Language Packages",
  unmanaged_files: "Unmanaged Files",
};
```

Add new props:

```typescript
  languagePackageEnvs?: LanguagePackageEnv[];
  unmanagedFileGroups?: UnmanagedFileGroup[];
```

In the `searchableItems` useMemo, add entries. Language package search matches on environment path, individual package names, and ecosystem:

```typescript
// Language package environments
if (languagePackageEnvs) {
  for (const env of languagePackageEnvs) {
    const itemId = `${env.ecosystem}:${env.path}`;
    items.push({
      sectionId: "language_packages",
      sectionLabel: "Language Packages",
      title: env.path,
      itemId,
    });
    for (const pkg of env.packages) {
      items.push({
        sectionId: "language_packages",
        sectionLabel: "Language Packages",
        title: `${pkg} (${env.ecosystem} in ${env.path})`,
        itemId,
      });
    }
  }
}

// Unmanaged files
if (unmanagedFileGroups) {
  for (const group of unmanagedFileGroups) {
    for (const file of group.items) {
      items.push({
        sectionId: "unmanaged_files",
        sectionLabel: "Unmanaged Files",
        title: file.path,
        itemId: file.path,
      });
    }
  }
}
```

- [ ] **Step 4: Verify tests pass, commit**

```bash
cd crates/web/ui && npm test -- --run GlobalSearch
git add -A && git commit -m "feat(web): extend global search to language packages and unmanaged files

Search matches on environment paths, package names within environments,
ecosystem type, and unmanaged file paths. Results navigate to the
correct section with reveal highlighting.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 12: App.tsx Wiring — Single-Host Mode

**Files:**
- Modify: `crates/web/ui/src/App.tsx`

**Interfaces:**
- Consumes: `ViewResponse` with new fields, `MainContent`, `Sidebar`, `GlobalSearch`, `RpmUploadModal`, `RpmBatchUploadModal`, `useRpmUpload`
- Produces: Full single-host wiring: data flow from ViewResponse through components, RPM upload backend calls, ItemId contract compliance

- [ ] **Step 1: Import new components and hook**

In `App.tsx` (likely in `SingleHostApp`), add imports:

```typescript
import { RpmUploadModal } from "./components/RpmUploadModal";
import { RpmBatchUploadModal } from "./components/RpmBatchUploadModal";
import { useRpmUpload } from "./hooks/useRpmUpload";
```

- [ ] **Step 2: Initialize useRpmUpload from backend data**

In the SingleHostApp component body, add:

```typescript
const rpmUpload = useRpmUpload();
```

Initialize from backend data when view loads. Use Plan 2's `repoless_annotation` field — a package is repo-less when `repoless_annotation` is non-empty:

```typescript
useEffect(() => {
  if (view?.packages) {
    const repolessEntries = view.packages
      .filter((p) => p.repoless_annotation)
      .map((p) => ({
        name: p.name,
        arch: p.arch,
        repoless_annotation: p.repoless_annotation!,
        repoless_cached: p.repoless_cached ?? false,
      }));
    if (repolessEntries.length > 0) {
      rpmUpload.initFromBackend(repolessEntries);
    }
  }
}, [view?.packages]);
```

- [ ] **Step 3: Build rpmRowStates for PackageList**

Derive the row states record from the hook for passing to PackageList:

```typescript
const rpmRowStates = useMemo(() => {
  const states: Record<string, RpmUploadRowState> = {};
  for (const pkg of view?.packages ?? []) {
    const state = rpmUpload.getRowState(pkg.name);
    if (state) {
      // Include/exclude state comes from the view data (refine toggle)
      if (state === "cached_excluded" && pkg.include) {
        states[pkg.name] = "cached_included";
      } else if (state === "uploaded_excluded" && pkg.include) {
        states[pkg.name] = "uploaded_included";
      } else {
        states[pkg.name] = state;
      }
    }
  }
  return states;
}, [view?.packages, rpmUpload]);
```

- [ ] **Step 4: Add upload modal state and callbacks**

```typescript
const [uploadTarget, setUploadTarget] = useState<string | null>(null);
const [batchUploadOpen, setBatchUploadOpen] = useState(false);
const uploadTriggerRef = useRef<HTMLElement | null>(null);
```

- [ ] **Step 5: Wire MainContent callbacks for new sections**

Pass toggle callbacks that dispatch `SetInclude` ops with correct `ItemId` shapes:

```typescript
// Language Packages — uses ItemId::LanguageEnv { ecosystem, path }
const handleToggleLangEnv = useCallback((ecosystem: string, path: string) => {
  const env = view?.language_packages?.find(
    (e) => e.ecosystem === ecosystem && e.path === path,
  );
  if (env) {
    applyOp({
      op: "SetInclude",
      item_id: { LanguageEnv: { ecosystem, path } },
      include: !env.include,
    });
  }
}, [view?.language_packages, applyOp]);

// Unmanaged Files — uses ItemId::UnmanagedFile { path }
const handleToggleUnmanagedFile = useCallback((filePath: string) => {
  const allItems = view?.unmanaged_files?.flatMap((g) => g.items) ?? [];
  const item = allItems.find((i) => i.path === filePath);
  if (item) {
    applyOp({
      op: "SetInclude",
      item_id: { UnmanagedFile: { path: filePath } },
      include: !item.include,
    });
  }
}, [view?.unmanaged_files, applyOp]);

const handleToggleUnmanagedGroup = useCallback((directory: string, include: boolean) => {
  const group = view?.unmanaged_files?.find((g) => g.directory === directory);
  if (group) {
    for (const item of group.items) {
      applyOp({
        op: "SetInclude",
        item_id: { UnmanagedFile: { path: item.path } },
        include,
      });
    }
  }
}, [view?.unmanaged_files, applyOp]);

const handleUnmanagedIncludeNone = useCallback(() => {
  const allItems = view?.unmanaged_files?.flatMap((g) => g.items) ?? [];
  for (const item of allItems) {
    if (item.include) {
      applyOp({
        op: "SetInclude",
        item_id: { UnmanagedFile: { path: item.path } },
        include: false,
      });
    }
  }
}, [view?.unmanaged_files, applyOp]);

const handleUnmanagedResetAll = useCallback(() => {
  const allItems = view?.unmanaged_files?.flatMap((g) => g.items) ?? [];
  for (const item of allItems) {
    if (!item.include) {
      applyOp({
        op: "SetInclude",
        item_id: { UnmanagedFile: { path: item.path } },
        include: true,
      });
    }
  }
}, [view?.unmanaged_files, applyOp]);
```

- [ ] **Step 6: Pass props to MainContent, Sidebar, GlobalSearch**

Update `Sidebar` props:

```typescript
<Sidebar
  // ... existing props ...
  hasLanguagePackages={!!view?.language_packages?.length}
  hasUnmanagedFiles={!!view?.unmanaged_files?.length}
  hasUnmanagedScan={view?.has_unmanaged_scan ?? false}
/>
```

Update `MainContent` props with new callbacks:

```typescript
<MainContent
  // ... existing props ...
  onToggleLangEnv={handleToggleLangEnv}
  onToggleUnmanagedFile={handleToggleUnmanagedFile}
  onToggleUnmanagedGroup={handleToggleUnmanagedGroup}
  onUnmanagedIncludeNone={handleUnmanagedIncludeNone}
  onUnmanagedResetAll={handleUnmanagedResetAll}
/>
```

Pass PackageList upload props through MainContent (or directly if MainContent passes them through):

```typescript
// PackageList receives these via MainContent's rendering:
rpmRowStates={rpmRowStates}
onUploadClick={(name) => {
  // Capture trigger ref for focus return
  const triggerEl = document.querySelector(
    `[aria-label="Upload RPM for ${name}"]`,
  ) as HTMLElement | null;
  uploadTriggerRef.current = triggerEl;
  setUploadTarget(name);
}}
onRemoveUpload={(name) => rpmUpload.removeUpload(name)}
```

Update `GlobalSearch` props:

```typescript
<GlobalSearch
  // ... existing props ...
  languagePackageEnvs={view?.language_packages}
  unmanagedFileGroups={view?.unmanaged_files}
/>
```

- [ ] **Step 7: Render upload modals**

Add at the bottom of SingleHostApp JSX:

```tsx
<RpmUploadModal
  isOpen={uploadTarget !== null}
  packageName={uploadTarget ?? ""}
  packageArch={view?.packages?.find((p) => p.name === uploadTarget)?.arch ?? "x86_64"}
  onUpload={async (name, file) => {
    await rpmUpload.uploadRpm(name, file);
    setUploadTarget(null);
  }}
  onClose={() => setUploadTarget(null)}
  triggerRef={uploadTriggerRef}
/>

<RpmBatchUploadModal
  isOpen={batchUploadOpen}
  needsUploadPackages={rpmUpload.needsUploadPackages}
  onBatchUpload={async (matched) => {
    await rpmUpload.applyBatchMatch(matched);
    setBatchUploadOpen(false);
  }}
  onClose={() => setBatchUploadOpen(false)}
/>
```

- [ ] **Step 8: Add Upload RPMs button to StatsBar/toolbar**

Conditionally render when packages need uploads:

```tsx
{rpmUpload.needsUploadCount > 0 && (
  <ToolbarItem>
    <Button
      variant="secondary"
      onClick={() => setBatchUploadOpen(true)}
      icon={<UploadIcon />}
    >
      Upload RPMs ({rpmUpload.needsUploadCount})
    </Button>
  </ToolbarItem>
)}
```

- [ ] **Step 9: Verify all tests pass, commit**

```bash
cd crates/web/ui && npm test -- --run
git add -A && git commit -m "feat(web): wire language packages, unmanaged files, and RPM upload into App

ItemId::LanguageEnv { ecosystem, path } for language package toggles.
ItemId::UnmanagedFile { path } for unmanaged file toggles.
RPM row states derived from Plan 2's repoless_annotation/repoless_cached.
Upload modals call POST /api/upload-rpm via useRpmUpload hook.
Batch upload button in toolbar when repo-less packages need RPMs.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 13: CSS Styling for New Components

**Files:**
- Modify: `crates/web/ui/src/App.css`

**Interfaces:**
- Consumes: BEM class names from LanguagePackageList, UnmanagedFileList, Sidebar hint, provenance badges
- Produces: Styled components matching existing design language

- [ ] **Step 1: Add Language Package List styles**

Append to `App.css`:

```css
/* ─── Language package list (decision section) ───────────────────────── */

.inspectah-lang-pkg-list {
  padding: 0;
}

.inspectah-lang-pkg-list__empty {
  padding: var(--pf-t--global--spacer--md);
  color: var(--pf-t--global--text--color--subtle);
  font-style: italic;
}

.inspectah-lang-pkg-row {
  padding: var(--pf-t--global--spacer--xs) var(--pf-t--global--spacer--md);
  border-bottom: 1px solid var(--pf-t--global--border--color--default);
  transition: background-color 150ms ease;
}

.inspectah-lang-pkg-row:hover {
  background: var(--pf-t--global--background--color--secondary--default);
}

.inspectah-lang-pkg-row:focus-visible {
  outline: 2px solid var(--pf-t--global--color--brand--default);
  outline-offset: -2px;
  border-radius: var(--pf-t--global--border--radius--small);
}

.inspectah-lang-pkg-row__main {
  display: flex;
  align-items: flex-start;
  gap: var(--pf-t--global--spacer--sm);
}

.inspectah-lang-pkg-row__toggle {
  flex-shrink: 0;
  padding-top: 2px;
}

.inspectah-lang-pkg-row__info {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.inspectah-lang-pkg-row__header {
  display: flex;
  align-items: center;
  gap: var(--pf-t--global--spacer--sm);
}

.inspectah-lang-pkg-row__ecosystem {
  flex-shrink: 0;
}

.inspectah-lang-pkg-row__path {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-family: var(--pf-t--global--font--family--mono);
  font-size: var(--pf-t--global--font--size--body--sm);
}

.inspectah-lang-pkg-row__meta {
  display: flex;
  align-items: center;
  gap: var(--pf-t--global--spacer--sm);
  font-size: var(--pf-t--global--font--size--body--sm);
}

.inspectah-lang-pkg-row__basis {
  color: var(--pf-t--global--text--color--subtle);
  font-style: italic;
}
```

- [ ] **Step 2: Add Unmanaged File List styles (including provenance badges)**

```css
/* ─── Unmanaged file list (decision section) ─────────────────────────── */

.inspectah-unmanaged-list__header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--pf-t--global--spacer--sm) var(--pf-t--global--spacer--md);
  border-bottom: 1px solid var(--pf-t--global--border--color--default);
  background: var(--pf-t--global--background--color--secondary--default);
}

.inspectah-unmanaged-list__actions {
  display: flex;
  gap: var(--pf-t--global--spacer--sm);
}

.inspectah-unmanaged-group {
  border-left: 3px solid var(--pf-t--global--color--brand--default);
  margin-bottom: var(--pf-t--global--spacer--xs);
}

.inspectah-unmanaged-group--var {
  border-left-color: var(--pf-t--global--color--status--warning--default);
}

.inspectah-unmanaged-group__header {
  display: flex;
  align-items: center;
  gap: var(--pf-t--global--spacer--sm);
  padding: var(--pf-t--global--spacer--xs) var(--pf-t--global--spacer--md);
  border-bottom: 1px solid var(--pf-t--global--border--color--default);
  cursor: pointer;
  transition: background-color 150ms ease;
}

.inspectah-unmanaged-group__header:hover {
  background: var(--pf-t--global--background--color--secondary--default);
}

.inspectah-unmanaged-group__header:focus-visible {
  outline: 2px solid var(--pf-t--global--color--brand--default);
  outline-offset: -2px;
}

.inspectah-unmanaged-group__chevron {
  background: none;
  border: none;
  padding: 0;
  display: flex;
  align-items: center;
  color: var(--pf-t--global--text--color--regular);
  flex-shrink: 0;
}

.inspectah-unmanaged-group__name {
  font-weight: var(--pf-t--global--font--weight--body--bold);
  font-family: var(--pf-t--global--font--family--mono);
  font-size: var(--pf-t--global--font--size--body--sm);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.inspectah-unmanaged-group__rollup {
  font-size: var(--pf-t--global--font--size--body--sm);
  color: var(--pf-t--global--text--color--subtle);
  margin-left: auto;
  flex-shrink: 0;
}

.inspectah-unmanaged-group__var-badge {
  font-size: var(--pf-t--global--font--size--body--sm);
  color: var(--pf-t--global--color--status--warning--default);
  font-weight: var(--pf-t--global--font--weight--body--bold);
  flex-shrink: 0;
}

.inspectah-unmanaged-group__items {
  padding-left: calc(var(--pf-t--global--spacer--md) + 22px);
}

.inspectah-unmanaged-group__toggle-announce {
  position: absolute;
  width: 1px;
  height: 1px;
  overflow: hidden;
  clip: rect(0, 0, 0, 0);
}

.inspectah-unmanaged-row {
  display: flex;
  align-items: center;
  gap: var(--pf-t--global--spacer--sm);
  padding: 2px 0;
  font-size: var(--pf-t--global--font--size--body--sm);
}

.inspectah-unmanaged-row:focus-visible {
  outline: 2px solid var(--pf-t--global--color--brand--default);
  outline-offset: -2px;
}

.inspectah-unmanaged-row__path {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-family: var(--pf-t--global--font--family--mono);
}

.inspectah-unmanaged-row__type {
  color: var(--pf-t--global--text--color--subtle);
  flex-shrink: 0;
}

.inspectah-unmanaged-row__size {
  color: var(--pf-t--global--text--color--subtle);
  flex-shrink: 0;
  min-width: 60px;
  text-align: right;
}

.inspectah-unmanaged-row__provenance {
  display: flex;
  gap: var(--pf-t--global--spacer--xs);
  flex-shrink: 0;
}

.inspectah-unmanaged-row__var-warning {
  font-size: var(--pf-t--global--font--size--body--sm);
  color: var(--pf-t--global--color--status--warning--default);
  flex-shrink: 0;
}

/* ─── Sidebar hint ───────────────────────────────────────────────────── */

.inspectah-sidebar__hint {
  padding: var(--pf-t--global--spacer--sm) var(--pf-t--global--spacer--md);
  color: var(--pf-t--global--text--color--subtle);
  font-style: italic;
}

.inspectah-sidebar__hint code {
  font-family: var(--pf-t--global--font--family--mono);
  background: var(--pf-t--global--background--color--secondary--default);
  padding: 1px 4px;
  border-radius: var(--pf-t--global--border--radius--small);
}

/* ─── Batch upload conflicts ─────────────────────────────────────────── */

.inspectah-batch-upload__conflicts {
  margin-top: var(--pf-t--global--spacer--sm);
}
```

- [ ] **Step 3: Add dark mode overrides**

```css
/* Dark mode: /var warning and provenance badge contrast */
.pf-v6-theme-dark .inspectah-unmanaged-group--var {
  border-left-color: var(--pf-t--global--color--status--warning--default);
}

.pf-v6-theme-dark .inspectah-unmanaged-row__var-warning {
  color: var(--pf-t--global--color--status--warning--default);
}
```

- [ ] **Step 4: Verify visual rendering (manual check), commit**

```bash
cd crates/web/ui && npm test -- --run
git add -A && git commit -m "style(web): add CSS for language packages, unmanaged files, and provenance

BEM-scoped styles matching existing decision row patterns. Provenance
signal badges for mutability, writable mount, service workdir. /var
path warning uses warning color. Dark mode overrides included.

Assisted-by: Claude Code (Opus 4.6)"
```

**Thorn Checkpoint: Tasks 9-13** — MainContent section rendering, AppShell focus management, global search, App.tsx wiring, and CSS styling complete. Run full test suite with `npm test -- --run`. Verify:
1. All existing tests still pass
2. Language Packages section appears in sidebar (single-host only)
3. Unmanaged Files section appears with flag gate and discoverability hint
4. Keyboard shortcuts 1-9 match the spec's shortcut map
5. Global search finds items in new sections
6. RPM upload icon appears on blocked package rows; upload calls `POST /api/upload-rpm`
7. Language package toggle sends `{ LanguageEnv: { ecosystem, path } }` (not opaque `{ id }`)
8. Unmanaged file rows render provenance signal badges
9. RPM batch upload modal shows conflicts before confirm
10. Modal focus trap works: focus on open, return on close

---

## Aggregate Handoff to Plan 4

Plan 3 is scoped to single-host mode only. The following aggregate UI work is explicitly handed off to Plan 4:

| Aggregate requirement | Status in Plan 3 | Plan 4 responsibility |
|----------------------|-------------------|----------------------|
| Aggregate sections in sidebar | Not in scope | Wire `AggregateSidebar` (data-driven, may need no changes) |
| Aggregate row metadata (ecosystem, confidence for lang pkgs; file type, size for unmanaged files) | Components built in Plan 3 | Add `sectionId`-aware rendering to `AggregateItemRow` |
| Searchable aggregate metadata | Not in scope | Extend aggregate search to new sections |
| Detail pane for new sections | Not in scope | Add detail pane coverage for language envs and unmanaged files |
| Variant comparison (package-list diff for lang envs, content-hash comparison for unmanaged files) | Not in scope | Implement variant views per spec's aggregate decision-support contract |
| Aggregate identity model (`ecosystem:path` for lang pkgs, `path` for unmanaged files) | Identity contracts established in Plan 1 | Wire aggregate identity using Plan 1's contracts |

**Dependency note:** Plan 4's aggregate row work for new sections depends on backend aggregate metadata (aggregate sections, prevalence data) that Plan 4's backend tasks will establish. Plan 3 has no dependency on this data.

---

## Shared Contracts Consumed from Plan 1

### ItemId Variants Used

| Plan 3 Context | ItemId Variant | Identity Key | Toggle Payload Shape |
|---------------|---------------|--------------|---------------------|
| Language Package toggle | `ItemId::LanguageEnv { ecosystem, path }` | `"pip:/opt/myapp/venv"` | `{ LanguageEnv: { ecosystem: "pip", path: "/opt/myapp/venv" } }` |
| Unmanaged File toggle | `ItemId::UnmanagedFile { path }` | `"/opt/splunk/bin/splunkd"` | `{ UnmanagedFile: { path: "/opt/splunk/bin/splunkd" } }` |
| RPM Package toggle | `ItemId::Package` (existing) | Package name | Existing shape (unchanged) |

### Method Strings Referenced

| Method | Where Used in Plan 3 |
|--------|---------------------|
| `"pip list"` | `LanguagePackageList` default method for pip ecosystems |
| `"pip dist-info"` | `LanguagePackageList` manifest_basis rendering |
| `"venv"` | `LanguagePackageList` manifest_basis rendering |
| `"npm lockfile"` | `LanguagePackageList` default method for npm ecosystems |
| `"gem lockfile"` | `LanguagePackageList` default method for gem ecosystems |

### Confidence Rendering Gate

| Confidence | UI Behavior in Plan 3 |
|-----------|----------------------|
| `"high"` | Green label, `include: true` default |
| `"medium"` | Orange label, `include: false` default |
| `"low"` | Grey label, `include: false` default |

## Shared Contracts Consumed from Plan 2

### Backend Fields for RPM Row State Derivation

| Field | Source | Plan 3 Usage |
|-------|--------|-------------|
| `PackageEntry.repoless_annotation` | Plan 2 Task 5 | Non-empty = package is repo-less |
| `PackageEntry.repoless_cached` | Plan 2 Task 5 | `true` = cached RPM found, row is cached_excluded |
| `POST /api/upload-rpm` | Plan 2 Task 11 | Upload endpoint for durable RPM staging |

### Provenance Signals for Unmanaged Files

| Signal | Source | Plan 3 Rendering |
|--------|--------|-----------------|
| `mutability` | `ProvenanceSignals` (Plan 2 Task 1) | Orange "modified since install" badge |
| `writable_mount` | `ProvenanceSignals` (Plan 2 Task 1) | Orange "writable mount" badge |
| `service_working_dir` | `ProvenanceSignals` (Plan 2 Task 1) | Blue "service workdir" badge |
| `last_modified`, `uid`, `gid`, `permissions` | `ProvenanceSignals` (Plan 2 Task 1) | Carried in DTO, available for detail rendering |
