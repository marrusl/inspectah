# Non-RPM Replication Plan 3: Refine UI Changes

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add Language Packages and Unmanaged Files decision sections to both single-host and aggregate refine UIs, implement the RPM upload modal for repo-less packages, update keyboard navigation, and extend global search to cover the new sections.

**Architecture:** Four layers change: (1) TypeScript types gain new section/item interfaces, (2) new React components render the Language Packages and Unmanaged Files decision sections with per-environment and per-item toggles, (3) the RPM upload modal adds a file-upload workflow to blocked package rows, (4) keyboard navigation, global search, and aggregate mode extend to cover the new sections.

**Tech Stack:** React 18, TypeScript, PatternFly v6 (components + icons), Vitest, React Testing Library. CSS follows existing `App.css` BEM conventions.

**Spec:** `process-docs/specs/proposed/2026-06-27-non-rpm-replication.md` — read fresh before implementation. This plan covers the "Refine UI: Section Topology" spec section and the per-tier Refine UI subsections.

**Plan 1 Contracts:** This plan consumes the shared contracts defined in Plan 1's "Shared Contracts for Plans 2-4" section. Use `ItemId::LanguageEnv`, `ItemId::UnmanagedFile`, the `method` string table, and the confidence rendering gate exactly as specified there.

**Thorn Checkpoints:** After Tasks 4, 8, 12.

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
| `crates/web/ui/src/api/types.ts` | New `LanguagePackageEnv`, `UnmanagedFileItem`, `UnmanagedFileGroup` interfaces; extend `ViewResponse` |
| `crates/web/ui/src/components/Sidebar.tsx` | Add Language Packages + Unmanaged Files to `REVIEW_SECTIONS`; move System Tuning to reference; discoverability hint |
| `crates/web/ui/src/hooks/useKeyboard.ts` | Update `SINGLE_HOST_SECTION_IDS` with new sections after `containers` |
| `crates/web/ui/src/components/GlobalSearch.tsx` | Add `SECTION_LABELS` entries; accept + search new section items |
| `crates/web/ui/src/components/PackageList.tsx` | RPM upload icon in blocked rows; muted styling; post-upload transition |
| `crates/web/ui/src/components/StatsBar.tsx` | Upload RPMs toolbar button when blocked packages exist |
| `crates/web/ui/src/App.tsx` | Wire new sections into `MainContent` rendering; pass data to `GlobalSearch` |
| `crates/web/ui/src/components/aggregate/AggregateSidebar.tsx` | No code change needed — data-driven via `sections` prop |
| `crates/web/ui/src/components/aggregate/AggregateItemRow.tsx` | Render ecosystem/confidence metadata for language packages; type/size/var-warning for unmanaged files |
| `crates/web/ui/src/App.css` | New styles for upload rows, unmanaged file groups, language package rows, `/var` warning |

### New files

| File | Purpose |
|------|---------|
| `crates/web/ui/src/components/LanguagePackageList.tsx` | Language Packages decision section component |
| `crates/web/ui/src/components/UnmanagedFileList.tsx` | Unmanaged Files decision section with directory grouping |
| `crates/web/ui/src/components/RpmUploadModal.tsx` | Single-RPM upload modal with NEVRA validation |
| `crates/web/ui/src/components/RpmBatchUploadModal.tsx` | Multi-RPM batch upload modal with auto-matching |
| `crates/web/ui/src/hooks/useRpmUpload.ts` | Upload state machine hook (5 row states) |
| `crates/web/ui/src/components/__tests__/LanguagePackageList.test.tsx` | Tests for Language Packages section |
| `crates/web/ui/src/components/__tests__/UnmanagedFileList.test.tsx` | Tests for Unmanaged Files section |
| `crates/web/ui/src/components/__tests__/RpmUploadModal.test.tsx` | Tests for single + batch upload modals |
| `crates/web/ui/src/components/__tests__/useRpmUpload.test.ts` | Tests for upload state machine hook |

---

## Task 1: TypeScript Type Extensions

**Files:**
- Modify: `crates/web/ui/src/api/types.ts`
- Test: `crates/web/ui/src/components/__tests__/LanguagePackageList.test.tsx` (type import verification)

**Interfaces:**
- Produces: `LanguagePackageEnv`, `UnmanagedFileItem`, `UnmanagedFileGroup`, `RpmUploadState`, extended `ViewResponse`
- Consumed by: Tasks 2-12

- [ ] **Step 1: Add LanguagePackageEnv interface**

In `crates/web/ui/src/api/types.ts`, add after the existing `NonRpmItem` interface (or at the end of the decision item types section):

```typescript
/** A language package environment (pip venv, npm project, gem project). */
export interface LanguagePackageEnv {
  /** Canonical ID: "ecosystem:path" (e.g., "pip:/opt/myapp/venv"). */
  id: string;
  /** Ecosystem: "pip" | "npm" | "gem". */
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

- [ ] **Step 2: Add UnmanagedFileItem and UnmanagedFileGroup interfaces**

Below `LanguagePackageEnv`, add:

```typescript
/** A single unmanaged file discovered by --include-unmanaged. */
export interface UnmanagedFileItem {
  /** Canonical ID: absolute file path (e.g., "/opt/splunk/bin/splunkd"). */
  id: string;
  /** Absolute file path. */
  path: string;
  /** File size in bytes. */
  size: number;
  /** File type: "elf_binary" | "jar" | "script" | "data" | "config" | "symlink" | "other". */
  file_type: string;
  /** Whether path is under /var. */
  is_var_path: boolean;
  /** Whether to include in export. */
  include: boolean;
}

/** Directory group for unmanaged files. */
export interface UnmanagedFileGroup {
  /** Parent directory path. */
  directory: string;
  /** Items in this directory. */
  items: UnmanagedFileItem[];
}
```

- [ ] **Step 3: Add RpmUploadState type**

Below the unmanaged file types, add:

```typescript
/**
 * Row state for repo-less RPM packages.
 * See spec "RPM Upload Row Contract" for the 5-state machine.
 */
export type RpmUploadState =
  | "cached_excluded"    // Cached RPM, pre-excluded
  | "cached_included"    // Cached RPM, user-included
  | "needs_upload"       // No RPM, needs upload
  | "uploaded_excluded"  // RPM uploaded, pre-excluded
  | "uploaded_included"; // RPM uploaded, user-included
```

- [ ] **Step 4: Extend ViewResponse with new section data**

Find the `ViewResponse` interface and add these optional fields:

```typescript
  /** Language package environments (Tier 1 non-RPM). */
  language_packages?: LanguagePackageEnv[];
  /** Unmanaged file groups (Tier 2, flag-gated). Present only when --include-unmanaged was used. */
  unmanaged_files?: UnmanagedFileGroup[];
  /** Whether --include-unmanaged was used at scan time. Drives discoverability hint. */
  has_unmanaged_scan?: boolean;
```

- [ ] **Step 5: Write type import verification test**

Create `crates/web/ui/src/components/__tests__/LanguagePackageList.test.tsx` with an initial scaffold:

```typescript
import { describe, it, expect } from "vitest";
import type {
  LanguagePackageEnv,
  UnmanagedFileItem,
  UnmanagedFileGroup,
  RpmUploadState,
} from "../../api/types";

// --- Test data factories ---

function makeLangEnv(
  ecosystem: LanguagePackageEnv["ecosystem"],
  path: string,
  packages: string[],
  overrides?: Partial<LanguagePackageEnv>,
): LanguagePackageEnv {
  return {
    id: `${ecosystem}:${path}`,
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
    id: path,
    path,
    size: 1024,
    file_type: "elf_binary",
    is_var_path: path.startsWith("/var/"),
    include: true,
    ...overrides,
  };
}

describe("Type contracts", () => {
  it("LanguagePackageEnv factory produces valid shape", () => {
    const env = makeLangEnv("pip", "/opt/myapp/venv", ["flask", "requests"]);
    expect(env.id).toBe("pip:/opt/myapp/venv");
    expect(env.ecosystem).toBe("pip");
    expect(env.packages).toHaveLength(2);
    expect(env.confidence).toBe("high");
  });

  it("UnmanagedFileItem factory detects /var paths", () => {
    const regular = makeUnmanagedFile("/opt/splunk/bin/splunkd");
    expect(regular.is_var_path).toBe(false);

    const varFile = makeUnmanagedFile("/var/lib/myapp/data.db");
    expect(varFile.is_var_path).toBe(true);
  });

  it("RpmUploadState covers all 5 states", () => {
    const states: RpmUploadState[] = [
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

- [ ] **Step 6: Run tests, verify pass, commit**

```bash
cd crates/web/ui && npm test -- --run
git add -A && git commit -m "feat(web): add TypeScript types for language packages, unmanaged files, and RPM upload states

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
    // High confidence = green-ish label
    const highBadges = screen.getAllByText("high");
    expect(highBadges.length).toBeGreaterThanOrEqual(2);
    // Medium confidence = orange-ish label
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
  onToggle: (envId: string) => void;
  isPending: boolean;
  /** Item ID to scroll into view (from global search). */
  revealItemId?: string;
  /** Whether section search filter is active. */
  filterActive?: boolean;
  /** Current section search query for highlighting. */
  filterQuery?: string;
}

function matchesFilter(env: LanguagePackageEnv, query: string): boolean {
  if (!query) return true;
  const q = query.toLowerCase();
  return (
    env.path.toLowerCase().includes(q) ||
    env.ecosystem.toLowerCase().includes(q) ||
    env.packages.some((p) => p.toLowerCase().includes(q)) ||
    env.manifest_basis.toLowerCase().includes(q)
  );
}

export function LanguagePackageList({
  environments,
  onToggle,
  isPending,
  revealItemId,
  filterActive = false,
  filterQuery = "",
}: LanguagePackageListProps) {
  const handleToggle = useCallback(
    (envId: string) => {
      if (!isPending) onToggle(envId);
    },
    [onToggle, isPending],
  );

  const filtered = filterActive && filterQuery
    ? environments.filter((env) => matchesFilter(env, filterQuery))
    : environments;

  if (filtered.length === 0 && filterActive) {
    return (
      <div
        className="inspectah-lang-pkg-list"
        data-testid="language-package-list"
      >
        <p className="inspectah-lang-pkg-list__empty">
          No environments match the current filter.
        </p>
      </div>
    );
  }

  return (
    <div
      className="inspectah-lang-pkg-list"
      role="list"
      aria-label="Language package environments"
      data-testid="language-package-list"
    >
      {filtered.map((env) => (
        <div
          key={env.id}
          role="listitem"
          tabIndex={-1}
          data-testid={`lang-env-row-${env.id}`}
          className="inspectah-lang-pkg-row"
          data-revealed={revealItemId === env.id ? "true" : undefined}
        >
          <div className="inspectah-lang-pkg-row__main">
            <div className="inspectah-lang-pkg-row__toggle">
              <input
                type="checkbox"
                role="checkbox"
                checked={env.include}
                disabled={isPending}
                aria-label={`Toggle ${env.ecosystem} environment at ${env.path}`}
                onChange={() => handleToggle(env.id)}
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
      ))}
    </div>
  );
}
```

- [ ] **Step 4: Verify test passes**

```bash
cd crates/web/ui && npm test -- --run LanguagePackageList
```

- [ ] **Step 5: Write failing test — toggle calls onToggle with env ID**

Append to the `LanguagePackageList` describe block:

```typescript
  it("calls onToggle with env ID when checkbox is clicked", async () => {
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
    expect(onToggle).toHaveBeenCalledWith("npm:/srv/webapp");
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

Per-environment toggles for pip/npm/gem environments with confidence
labels and package count badges.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 3: Unmanaged Files Decision Section Component

**Files:**
- Create: `crates/web/ui/src/components/UnmanagedFileList.tsx`
- Test: `crates/web/ui/src/components/__tests__/UnmanagedFileList.test.tsx`

**Interfaces:**
- Consumes: `UnmanagedFileGroup`, `UnmanagedFileItem` from types.ts
- Produces: `UnmanagedFileList` component with directory grouping, per-item toggles, size rollup

- [ ] **Step 1: Write failing test — renders grouped files with directory headers**

Create `crates/web/ui/src/components/__tests__/UnmanagedFileList.test.tsx`:

```typescript
import { describe, it, expect, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { UnmanagedFileList } from "../UnmanagedFileList";
import type { UnmanagedFileGroup, UnmanagedFileItem } from "../../api/types";

// --- Test data factories ---

function makeFile(
  path: string,
  overrides?: Partial<UnmanagedFileItem>,
): UnmanagedFileItem {
  return {
    id: path,
    path,
    size: 1024 * 100, // 100 KB
    file_type: "elf_binary",
    is_var_path: path.startsWith("/var/"),
    include: true,
    ...overrides,
  };
}

const groups: UnmanagedFileGroup[] = [
  {
    directory: "/opt/splunk",
    items: [
      makeFile("/opt/splunk/bin/splunkd", { size: 50 * 1024 * 1024 }),
      makeFile("/opt/splunk/etc/system.conf", {
        size: 2048,
        file_type: "config",
      }),
      makeFile("/opt/splunk/lib/libcrypto.so", { size: 5 * 1024 * 1024 }),
    ],
  },
  {
    directory: "/srv/myapp",
    items: [
      makeFile("/srv/myapp/app.jar", {
        size: 120 * 1024 * 1024,
        file_type: "jar",
      }),
      makeFile("/srv/myapp/start.sh", { size: 512, file_type: "script" }),
    ],
  },
  {
    directory: "/var/lib/custom",
    items: [
      makeFile("/var/lib/custom/data.db", {
        size: 200 * 1024 * 1024,
        file_type: "data",
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
import { Badge, Button, Content } from "@patternfly/react-core";
import {
  AngleRightIcon,
  AngleDownIcon,
} from "@patternfly/react-icons";
import type { UnmanagedFileGroup, UnmanagedFileItem } from "../api/types";

/** Format bytes into human-readable size. */
function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  if (bytes < 1024 * 1024 * 1024)
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
}

export interface UnmanagedFileListProps {
  groups: UnmanagedFileGroup[];
  onToggleItem: (itemId: string) => void;
  onToggleGroup: (directory: string, include: boolean) => void;
  isPending: boolean;
  /** Bulk action: exclude all items. */
  onIncludeNone?: () => void;
  /** Bulk action: reset all items to included. */
  onResetAll?: () => void;
  /** Item ID to scroll into view (from global search). */
  revealItemId?: string;
  /** Whether section search filter is active. */
  filterActive?: boolean;
  /** Current section search query. */
  filterQuery?: string;
}

function itemMatchesFilter(item: UnmanagedFileItem, query: string): boolean {
  if (!query) return true;
  const q = query.toLowerCase();
  return (
    item.path.toLowerCase().includes(q) ||
    item.file_type.toLowerCase().includes(q)
  );
}

function FileRow({
  item,
  onToggle,
  isPending,
  isRevealed,
}: {
  item: UnmanagedFileItem;
  onToggle: (id: string) => void;
  isPending: boolean;
  isRevealed: boolean;
}) {
  return (
    <div
      className="inspectah-unmanaged-row"
      data-testid={`unmanaged-file-${item.id}`}
      data-revealed={isRevealed ? "true" : undefined}
      role="listitem"
    >
      <input
        type="checkbox"
        role="checkbox"
        checked={item.include}
        disabled={isPending}
        aria-label={`Toggle ${item.path}`}
        onChange={() => onToggle(item.id)}
      />
      <span className="inspectah-unmanaged-row__path">
        {item.path.split("/").pop()}
      </span>
      <span className="inspectah-unmanaged-row__type">{item.file_type}</span>
      <span className="inspectah-unmanaged-row__size">
        {formatSize(item.size)}
      </span>
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
  forceExpanded,
  matchingItemIds,
}: {
  group: UnmanagedFileGroup;
  onToggleItem: (id: string) => void;
  onToggleGroup: (directory: string, include: boolean) => void;
  isPending: boolean;
  revealItemId?: string;
  forceExpanded: boolean;
  matchingItemIds?: Set<string>;
}) {
  const hasRevealedChild = group.items.some((i) => i.id === revealItemId);
  const hasMatchingChildren =
    matchingItemIds && group.items.some((i) => matchingItemIds.has(i.id));
  const [isExpanded, setIsExpanded] = useState(true);

  // Auto-expand when search matches children or reveal targets a child
  const shouldExpand =
    isExpanded || forceExpanded || hasRevealedChild || !!hasMatchingChildren;

  const allIncluded = group.items.every((i) => i.include);
  const noneIncluded = group.items.every((i) => !i.include);
  const groupIncludedCount = group.items.filter((i) => i.include).length;
  const groupSize = group.items.reduce((sum, i) => sum + i.size, 0);
  const includedSize = group.items
    .filter((i) => i.include)
    .reduce((sum, i) => sum + i.size, 0);

  const handleGroupToggle = useCallback(() => {
    // Toggle to the opposite of majority state
    onToggleGroup(group.directory, noneIncluded || !allIncluded);
  }, [group.directory, allIncluded, noneIncluded, onToggleGroup]);

  const isVarGroup = group.directory.startsWith("/var/");

  const items = matchingItemIds
    ? group.items.filter((i) => matchingItemIds.has(i.id))
    : group.items;

  return (
    <div
      className={`inspectah-unmanaged-group${isVarGroup ? " inspectah-unmanaged-group--var" : ""}`}
      data-testid={`unmanaged-group-${group.directory}`}
      aria-label={`${group.directory} file group`}
    >
      <div
        className="inspectah-unmanaged-group__header"
        onClick={() => setIsExpanded(!shouldExpand)}
        onKeyDown={(e) => {
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            setIsExpanded(!shouldExpand);
          }
        }}
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
        <span className="inspectah-unmanaged-group__rollup">
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
          {items.map((item) => (
            <FileRow
              key={item.id}
              item={item}
              onToggle={onToggleItem}
              isPending={isPending}
              isRevealed={revealItemId === item.id}
            />
          ))}
        </div>
      )}
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
  filterActive = false,
  filterQuery = "",
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

  // Build set of matching item IDs for search filtering
  const matchingItemIds = useMemo(() => {
    if (!filterActive || !filterQuery) return undefined;
    const ids = new Set<string>();
    for (const group of groups) {
      for (const item of group.items) {
        if (itemMatchesFilter(item, filterQuery)) {
          ids.add(item.id);
        }
      }
    }
    return ids;
  }, [groups, filterActive, filterQuery]);

  // Filter groups to those with matching items
  const visibleGroups = matchingItemIds
    ? groups.filter((g) => g.items.some((i) => matchingItemIds.has(i.id)))
    : groups;

  return (
    <div
      className="inspectah-unmanaged-list"
      data-testid="unmanaged-file-list"
    >
      <div
        className="inspectah-unmanaged-list__header"
        aria-live="polite"
      >
        <Content component="small" data-testid="unmanaged-rollup">
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
      {visibleGroups.map((group) => (
        <DirectoryGroup
          key={group.directory}
          group={group}
          onToggleItem={onToggleItem}
          onToggleGroup={onToggleGroup}
          isPending={isPending}
          revealItemId={revealItemId}
          forceExpanded={filterActive && !!filterQuery}
          matchingItemIds={matchingItemIds}
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

- [ ] **Step 5: Write failing test — group toggle and size rollup**

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
    // Group checkboxes have aria-label "Toggle all files in ..."
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

  it("calls onToggleItem when individual file checkbox is clicked", async () => {
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
```

- [ ] **Step 6: Verify tests pass, commit**

```bash
cd crates/web/ui && npm test -- --run UnmanagedFileList
git add -A && git commit -m "feat(web): add UnmanagedFileList decision section with directory grouping

Grouped by parent directory with per-item and per-group toggles,
running size rollup, /var path warnings, and Include None/Reset All
bulk actions.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 4: RPM Upload State Machine Hook

**Files:**
- Create: `crates/web/ui/src/hooks/useRpmUpload.ts`
- Test: `crates/web/ui/src/components/__tests__/useRpmUpload.test.ts`

**Interfaces:**
- Consumes: `RpmUploadState` from types.ts
- Produces: `useRpmUpload` hook managing the 5-state row machine per spec's RPM Upload Row Contract

- [ ] **Step 1: Write failing test — state machine transitions**

Create `crates/web/ui/src/components/__tests__/useRpmUpload.test.ts`:

```typescript
import { describe, it, expect } from "vitest";
import { renderHook, act } from "@testing-library/react";
import { useRpmUpload } from "../../hooks/useRpmUpload";

describe("useRpmUpload", () => {
  it("initializes with empty uploads map", () => {
    const { result } = renderHook(() => useRpmUpload());
    expect(result.current.uploads).toEqual(new Map());
  });

  it("getState returns 'needs_upload' for unknown package", () => {
    const { result } = renderHook(() => useRpmUpload());
    expect(result.current.getState("unknown-pkg")).toBe("needs_upload");
  });

  it("uploadRpm transitions from needs_upload to uploaded_excluded", () => {
    const { result } = renderHook(() => useRpmUpload());
    const mockFile = new File(["rpm-content"], "nginx-1.24-1.el9.x86_64.rpm", {
      type: "application/x-rpm",
    });
    act(() => {
      result.current.uploadRpm("nginx", mockFile);
    });
    expect(result.current.getState("nginx")).toBe("uploaded_excluded");
    expect(result.current.getFile("nginx")).toBe(mockFile);
  });

  it("removeUpload transitions back to needs_upload", () => {
    const { result } = renderHook(() => useRpmUpload());
    const mockFile = new File(["rpm-content"], "nginx-1.24-1.el9.x86_64.rpm", {
      type: "application/x-rpm",
    });
    act(() => {
      result.current.uploadRpm("nginx", mockFile);
    });
    expect(result.current.getState("nginx")).toBe("uploaded_excluded");
    act(() => {
      result.current.removeUpload("nginx");
    });
    expect(result.current.getState("nginx")).toBe("needs_upload");
    expect(result.current.getFile("nginx")).toBeUndefined();
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

  it("needsUploadCount returns correct count", () => {
    const { result } = renderHook(() => useRpmUpload());
    // Register packages that need uploads
    act(() => {
      result.current.registerNeedsUpload(["nginx", "custom-agent", "my-tool"]);
    });
    expect(result.current.needsUploadCount).toBe(3);
    // Upload one
    const mockFile = new File(["rpm"], "nginx-1.0-1.el9.x86_64.rpm");
    act(() => {
      result.current.uploadRpm("nginx", mockFile);
    });
    expect(result.current.needsUploadCount).toBe(2);
  });

  it("batchUpload matches files to packages by name prefix", () => {
    const { result } = renderHook(() => useRpmUpload());
    act(() => {
      result.current.registerNeedsUpload(["nginx", "custom-agent"]);
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

interface UploadEntry {
  file: File;
  state: "uploaded_excluded" | "uploaded_included";
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
  /** Map of package name → upload entry. */
  uploads: Map<string, UploadEntry>;
  /** Get the current state for a package. Returns "needs_upload" if no upload exists. */
  getState: (packageName: string) => string;
  /** Get the uploaded file for a package. */
  getFile: (packageName: string) => File | undefined;
  /** Upload an RPM for a specific package. Transitions to uploaded_excluded. */
  uploadRpm: (packageName: string, file: File) => void;
  /** Remove an upload, reverting to needs_upload. */
  removeUpload: (packageName: string) => void;
  /** Validate a filename against expected NEVRA. */
  validateFilename: (
    packageName: string,
    arch: string,
    filename: string,
  ) => ValidationResult;
  /** Register packages that need uploads. */
  registerNeedsUpload: (packageNames: string[]) => void;
  /** Number of packages still needing uploads. */
  needsUploadCount: number;
  /** Match multiple files to registered packages by name prefix. */
  batchMatch: (files: File[]) => BatchMatchResult;
  /** Apply a batch match result — upload all matched files. */
  applyBatchMatch: (matched: BatchMatchResult["matched"]) => void;
}

/** Extract the package name prefix from an RPM filename (before first hyphen followed by a digit). */
function extractPackageName(filename: string): string | null {
  // RPM naming: name-version-release.arch.rpm
  // Package name can contain hyphens, so find first hyphen followed by a digit
  const match = filename.match(/^(.+?)-\d/);
  return match ? match[1] : null;
}

export function useRpmUpload(): UseRpmUploadResult {
  const [uploads, setUploads] = useState<Map<string, UploadEntry>>(
    () => new Map(),
  );
  const [needsUploadSet, setNeedsUploadSet] = useState<Set<string>>(
    () => new Set(),
  );

  const getState = useCallback(
    (packageName: string): string => {
      const entry = uploads.get(packageName);
      if (entry) return entry.state;
      return "needs_upload";
    },
    [uploads],
  );

  const getFile = useCallback(
    (packageName: string): File | undefined => {
      return uploads.get(packageName)?.file;
    },
    [uploads],
  );

  const uploadRpm = useCallback(
    (packageName: string, file: File) => {
      setUploads((prev) => {
        const next = new Map(prev);
        next.set(packageName, { file, state: "uploaded_excluded" });
        return next;
      });
    },
    [],
  );

  const removeUpload = useCallback(
    (packageName: string) => {
      setUploads((prev) => {
        const next = new Map(prev);
        next.delete(packageName);
        return next;
      });
    },
    [],
  );

  const validateFilename = useCallback(
    (
      packageName: string,
      arch: string,
      filename: string,
    ): ValidationResult => {
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

      // Check architecture
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

  const registerNeedsUpload = useCallback(
    (packageNames: string[]) => {
      setNeedsUploadSet(new Set(packageNames));
    },
    [],
  );

  const needsUploadCount = useMemo(() => {
    let count = 0;
    for (const name of needsUploadSet) {
      if (!uploads.has(name)) count++;
    }
    return count;
  }, [needsUploadSet, uploads]);

  const batchMatch = useCallback(
    (files: File[]): BatchMatchResult => {
      const matched: BatchMatchResult["matched"] = [];
      const unmatched: File[] = [];
      const conflicts = new Map<string, File[]>();

      for (const file of files) {
        const extractedName = extractPackageName(file.name);
        if (!extractedName || !needsUploadSet.has(extractedName)) {
          unmatched.push(file);
          continue;
        }

        const existing = conflicts.get(extractedName);
        if (existing) {
          existing.push(file);
        } else if (matched.some((m) => m.packageName === extractedName)) {
          // Move from matched to conflicts
          const prev = matched.find((m) => m.packageName === extractedName)!;
          conflicts.set(extractedName, [prev.file, file]);
          matched.splice(matched.indexOf(prev), 1);
        } else {
          matched.push({ packageName: extractedName, file });
        }
      }

      return {
        matched,
        unmatched,
        conflicts: Array.from(conflicts.entries()).map(
          ([packageName, conflictFiles]) => ({
            packageName,
            files: conflictFiles,
          }),
        ),
      };
    },
    [needsUploadSet],
  );

  const applyBatchMatch = useCallback(
    (matched: BatchMatchResult["matched"]) => {
      setUploads((prev) => {
        const next = new Map(prev);
        for (const { packageName, file } of matched) {
          next.set(packageName, { file, state: "uploaded_excluded" });
        }
        return next;
      });
    },
    [],
  );

  return {
    uploads,
    getState,
    getFile,
    uploadRpm,
    removeUpload,
    validateFilename,
    registerNeedsUpload,
    needsUploadCount,
    batchMatch,
    applyBatchMatch,
  };
}
```

- [ ] **Step 4: Verify tests pass, commit**

```bash
cd crates/web/ui && npm test -- --run useRpmUpload
git add -A && git commit -m "feat(web): add useRpmUpload hook for RPM upload state machine

Five-state row machine (cached_excluded, cached_included, needs_upload,
uploaded_excluded, uploaded_included) with NEVRA validation and batch
matching for multi-file uploads.

Assisted-by: Claude Code (Opus 4.6)"
```

**Thorn Checkpoint: Tasks 1-4** — Types, LanguagePackageList, UnmanagedFileList, and useRpmUpload hook are complete. Verify all tests pass with `npm test -- --run`.

---

## Task 5: RPM Upload Modal (Single File)

**Files:**
- Create: `crates/web/ui/src/components/RpmUploadModal.tsx`
- Test: `crates/web/ui/src/components/__tests__/RpmUploadModal.test.tsx`

**Interfaces:**
- Consumes: `useRpmUpload` hook, PatternFly `Modal`, `FileUpload`
- Produces: `RpmUploadModal` component for single-package RPM upload

- [ ] **Step 1: Write failing test — modal renders with expected NEVRA**

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
});
```

- [ ] **Step 2: Verify test fails**

```bash
cd crates/web/ui && npm test -- --run RpmUploadModal
```

- [ ] **Step 3: Implement RpmUploadModal component**

Create `crates/web/ui/src/components/RpmUploadModal.tsx`:

```tsx
import { useState, useCallback } from "react";
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
}

/** Validate RPM filename against expected package name and architecture. */
function validateRpmFile(
  packageName: string,
  arch: string,
  filename: string,
): { valid: boolean; error?: string } {
  if (!filename.endsWith(".rpm")) {
    return { valid: false, error: "File must be an .rpm package" };
  }

  // Extract package name from NEVRA: name-version-release.arch.rpm
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
    return {
      valid: false,
      error: `Expected architecture "${arch}" or "noarch"`,
    };
  }

  return { valid: true };
}

export function RpmUploadModal({
  isOpen,
  packageName,
  packageArch,
  onUpload,
  onClose,
}: RpmUploadModalProps) {
  const [file, setFile] = useState<File | null>(null);
  const [filename, setFilename] = useState("");
  const [validation, setValidation] = useState<{
    valid: boolean;
    error?: string;
  } | null>(null);

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
    }
  }, [file, validation, packageName, onUpload, onClose, handleClear]);

  const handleClose = useCallback(() => {
    handleClear();
    onClose();
  }, [onClose, handleClear]);

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
git add -A && git commit -m "feat(web): add RpmUploadModal with NEVRA validation

Single-file RPM upload modal with drag-and-drop, filename validation
against expected NEVRA pattern, and accessible focus management.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 6: RPM Batch Upload Modal

**Files:**
- Create: `crates/web/ui/src/components/RpmBatchUploadModal.tsx`
- Test: `crates/web/ui/src/components/__tests__/RpmUploadModal.test.tsx` (append)

**Interfaces:**
- Consumes: `useRpmUpload.batchMatch`, PatternFly `Modal`, `MultipleFileUpload`
- Produces: `RpmBatchUploadModal` component for multi-package RPM upload

- [ ] **Step 1: Write failing test — batch modal renders match table**

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
});
```

- [ ] **Step 2: Verify test fails**

```bash
cd crates/web/ui && npm test -- --run RpmUploadModal
```

- [ ] **Step 3: Implement RpmBatchUploadModal component**

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
} from "@patternfly/react-core";
import { CheckCircleIcon, ExclamationCircleIcon, InProgressIcon } from "@patternfly/react-icons";

/** Extract the package name prefix from an RPM filename. */
function extractPackageName(filename: string): string | null {
  const match = filename.match(/^(.+?)-\d/);
  return match ? match[1] : null;
}

interface MatchResult {
  matched: Array<{ packageName: string; file: File }>;
  unmatched: File[];
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

    for (const file of files) {
      if (!file.name.endsWith(".rpm")) {
        unmatched.push(file);
        continue;
      }
      const name = extractPackageName(file.name);
      if (name && packageSet.has(name) && !matched.some((m) => m.packageName === name)) {
        matched.push({ packageName: name, file });
      } else {
        unmatched.push(file);
      }
    }
    return { matched, unmatched };
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
git add -A && git commit -m "feat(web): add RpmBatchUploadModal for multi-RPM upload

PatternFly MultipleFileUpload with auto-matching of dropped RPM files
to expected packages. Shows matched/unmatched status per file.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 7: PackageList RPM Upload Row Integration

**Files:**
- Modify: `crates/web/ui/src/components/PackageList.tsx`
- Modify: `crates/web/ui/src/App.css`
- Test: `crates/web/ui/src/components/__tests__/PackageList.test.tsx` (append)

**Interfaces:**
- Consumes: `useRpmUpload` hook, `RpmUploadModal`, spec's RPM Upload Row Contract
- Produces: Modified package rows with 5-state upload behavior

- [ ] **Step 1: Write failing test — blocked row hides checkbox and shows upload icon**

Append to `PackageList.test.tsx`:

```typescript
  // --- RPM upload row states ---

  describe("RPM upload rows", () => {
    it("renders upload icon instead of checkbox for needs_upload packages", () => {
      const pkgs = [
        makePkg("custom-agent", "none", false),
      ];
      render(
        <PackageList
          mode="single"
          packages={pkgs}
          repoGroups={allRepos}
          onToggle={vi.fn()}
          onRepoToggle={vi.fn()}
          rpmUploadState={{ "custom-agent": "needs_upload" }}
        />,
      );
      const row = screen.getByTestId("package-row-custom-agent");
      // Checkbox should be hidden
      expect(within(row).queryByRole("checkbox")).not.toBeInTheDocument();
      // Upload icon button should be present
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
          rpmUploadState={{ "custom-agent": "needs_upload" }}
        />,
      );
      expect(screen.getByText("RPM needed")).toBeInTheDocument();
    });

    it("shows checkbox and green label after upload", () => {
      const pkgs = [makePkg("custom-agent", "none", false)];
      render(
        <PackageList
          mode="single"
          packages={pkgs}
          repoGroups={allRepos}
          onToggle={vi.fn()}
          onRepoToggle={vi.fn()}
          rpmUploadState={{ "custom-agent": "uploaded_excluded" }}
        />,
      );
      const row = screen.getByTestId("package-row-custom-agent");
      expect(within(row).getByRole("checkbox")).toBeInTheDocument();
      expect(screen.getByText("RPM provided")).toBeInTheDocument();
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
          rpmUploadState={{ "custom-agent": "needs_upload" }}
        />,
      );
      const row = screen.getByTestId("package-row-custom-agent");
      expect(row.className).toContain("--blocked");
    });
  });
```

- [ ] **Step 2: Verify test fails**

```bash
cd crates/web/ui && npm test -- --run PackageList
```

- [ ] **Step 3: Add rpmUploadState prop to PackageList and modify row rendering**

In `PackageList.tsx`, add the `rpmUploadState` optional prop to the component's props interface:

```typescript
  /** Per-package RPM upload state. Keys are package names, values are RpmUploadState strings. */
  rpmUploadState?: Record<string, string>;
  /** Callback when upload icon is clicked on a blocked row. */
  onUploadClick?: (packageName: string) => void;
```

In the row rendering, before the checkbox, add upload-state branching:

```typescript
const uploadState = rpmUploadState?.[pkg.name];
const isBlocked = uploadState === "needs_upload";
const isUploaded = uploadState === "uploaded_excluded" || uploadState === "uploaded_included";
```

Replace the checkbox rendering with conditional logic:

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

Replace the provenance badge with upload-state labels when applicable:

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
```

Add the `--blocked` class modifier to the row:

```tsx
className={`inspectah-package-row${isBlocked ? " inspectah-package-row--blocked" : ""}`}
```

Import `Button`, `Label` from PatternFly and `UploadIcon` from `@patternfly/react-icons` (use `OutlinedUploadIcon` or `UploadIcon`).

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
```

- [ ] **Step 5: Verify tests pass, commit**

```bash
cd crates/web/ui && npm test -- --run PackageList
git add -A && git commit -m "feat(web): integrate RPM upload states into PackageList rows

Blocked rows hide checkbox and show upload icon. Post-upload rows
show green 'RPM provided' label with remove button. Muted opacity
for needs_upload state per spec's RPM Upload Row Contract.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 8: Sidebar Updates + Keyboard Navigation

**Files:**
- Modify: `crates/web/ui/src/components/Sidebar.tsx`
- Modify: `crates/web/ui/src/hooks/useKeyboard.ts`
- Modify: `crates/web/ui/src/components/AppShell.tsx`
- Test: `crates/web/ui/src/components/__tests__/DecisionSections.test.tsx` (append sidebar tests) or new `Sidebar.test.tsx`

**Interfaces:**
- Consumes: `ViewResponse.language_packages`, `ViewResponse.has_unmanaged_scan`, `ViewResponse.unmanaged_files`
- Produces: Updated sidebar with new review sections, updated keyboard shortcuts

- [ ] **Step 1: Write failing test — sidebar shows Language Packages section**

Create or append to a sidebar test file:

```typescript
import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import { Sidebar } from "../Sidebar";

describe("Sidebar section ordering", () => {
  it("shows Language Packages in review group after Containers", () => {
    // Render Sidebar with hasLanguagePackages=true
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

In `Sidebar.tsx`, modify `REVIEW_SECTIONS` to make language packages and unmanaged files conditional. Add new props:

```typescript
export interface SidebarProps {
  // ... existing props ...
  /** Whether language package environments exist in the data. */
  hasLanguagePackages?: boolean;
  /** Whether unmanaged file data exists. */
  hasUnmanagedFiles?: boolean;
  /** Whether --include-unmanaged was used at scan time. */
  hasUnmanagedScan?: boolean;
}
```

Build the review sections list dynamically:

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
  // System Tuning moves to reference (was review)
  return base;
}, [hasLanguagePackages, hasUnmanagedFiles]);
```

Move `system_tuning` from `REVIEW_SECTIONS` to `REFERENCE_SECTIONS` (it was already in reference per the spec's sidebar inventory — verify current placement and adjust if needed).

Add discoverability hint below the review NavGroup when `hasUnmanagedScan === false`:

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

- [ ] **Step 4: Update useKeyboard.ts — insert new section IDs**

In `useKeyboard.ts`, update `SINGLE_HOST_SECTION_IDS`:

```typescript
const SINGLE_HOST_SECTION_IDS = [
  "packages",           // 1
  "configs",            // 2
  "users_groups",       // 3
  "services",           // 4
  "containers",         // 5
  "language_packages",  // 6 (new)
  "unmanaged_files",    // 7 (new)
  "version_changes",    // 8 (was 6)
  "compose",            // 9 (was 7)
  // network, storage — no longer have shortcuts (were 8, 9)
  "scheduled_tasks",
  "non_rpm_software",
  "kernel_boot",
  "selinux",
];
```

Key 7 (`unmanaged_files`) is a no-op when the section is not visible — the `onSectionChange` callback already handles missing sections gracefully (no element to scroll to).

- [ ] **Step 5: Verify tests pass, commit**

```bash
cd crates/web/ui && npm test -- --run
git add -A && git commit -m "feat(web): add Language Packages and Unmanaged Files to sidebar

New review sections after Containers with keyboard shortcuts 6-7.
Discoverability hint when --include-unmanaged was not used.
Keys 6-9 remapped per spec's shortcut table.

Assisted-by: Claude Code (Opus 4.6)"
```

**Thorn Checkpoint: Tasks 5-8** — Upload modals, PackageList integration, and sidebar/keyboard changes complete. Run `npm test -- --run` and verify all existing tests still pass. Check keyboard shortcuts 1-9 against the spec's shortcut map.

---

## Task 9: Global Search Integration

**Files:**
- Modify: `crates/web/ui/src/components/GlobalSearch.tsx`
- Test: `crates/web/ui/src/components/__tests__/GlobalSearch.test.tsx` (append)

**Interfaces:**
- Consumes: `LanguagePackageEnv[]`, `UnmanagedFileGroup[]` from ViewResponse
- Produces: Search results for new sections in global search

- [ ] **Step 1: Write failing test — global search finds language package environments**

Append to `GlobalSearch.test.tsx`:

```typescript
  it("finds language package environments by path", async () => {
    // Add language package items to the search props
    const langEnvs = [
      {
        id: "pip:/opt/myapp/venv",
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

  it("finds unmanaged files by path", async () => {
    const unmanagedGroups = [
      {
        directory: "/opt/splunk",
        items: [
          {
            id: "/opt/splunk/bin/splunkd",
            path: "/opt/splunk/bin/splunkd",
            size: 1024,
            file_type: "elf_binary",
            is_var_path: false,
            include: true,
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

- [ ] **Step 2: Verify test fails**

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
export interface GlobalSearchProps {
  // ... existing props ...
  /** Language package environments for search. */
  languagePackageEnvs?: LanguagePackageEnv[];
  /** Unmanaged file groups for search. */
  unmanagedFileGroups?: UnmanagedFileGroup[];
}
```

In the `searchableItems` useMemo, add entries for language packages and unmanaged files:

```typescript
// Language package environments
if (languagePackageEnvs) {
  for (const env of languagePackageEnvs) {
    items.push({
      sectionId: "language_packages",
      sectionLabel: "Language Packages",
      title: env.path,
      itemId: env.id,
    });
    // Also index individual package names for deep search
    for (const pkg of env.packages) {
      items.push({
        sectionId: "language_packages",
        sectionLabel: "Language Packages",
        title: `${pkg} (${env.ecosystem} in ${env.path})`,
        itemId: env.id,
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
        itemId: file.id,
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
and unmanaged file paths. Results navigate to the correct section.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 10: App.tsx Wiring — Single-Host Mode

**Files:**
- Modify: `crates/web/ui/src/App.tsx`
- Test: `crates/web/ui/src/__tests__/App.routing.test.tsx` (append) or visual verification

**Interfaces:**
- Consumes: `ViewResponse` with new fields, `LanguagePackageList`, `UnmanagedFileList`, `RpmUploadModal`, `RpmBatchUploadModal`, `useRpmUpload`
- Produces: New sections rendered in single-host main content area

- [ ] **Step 1: Import new components and hook**

In `App.tsx`, add imports:

```typescript
import { LanguagePackageList } from "./components/LanguagePackageList";
import { UnmanagedFileList } from "./components/UnmanagedFileList";
import { RpmUploadModal } from "./components/RpmUploadModal";
import { RpmBatchUploadModal } from "./components/RpmBatchUploadModal";
import { useRpmUpload } from "./hooks/useRpmUpload";
```

- [ ] **Step 2: Initialize useRpmUpload hook**

In the App component body, add:

```typescript
const rpmUpload = useRpmUpload();
```

Register repo-less packages that need uploads when view data loads (in a useEffect or after data fetch):

```typescript
useEffect(() => {
  if (view?.packages) {
    const needsUpload = view.packages
      .filter((p) => p.source_repo === "none" && !p.cached_rpm)
      .map((p) => p.name);
    if (needsUpload.length > 0) {
      rpmUpload.registerNeedsUpload(needsUpload);
    }
  }
}, [view?.packages]);
```

- [ ] **Step 3: Add RPM upload state to PackageList**

Pass `rpmUploadState` and callbacks to the existing `PackageList` render:

```typescript
<PackageList
  // ... existing props ...
  rpmUploadState={Object.fromEntries(
    Array.from(rpmUpload.uploads.entries()).map(([name, entry]) => [
      name,
      entry.state,
    ]),
  )}
  onUploadClick={(name) => setUploadTarget(name)}
  onRemoveUpload={(name) => rpmUpload.removeUpload(name)}
/>
```

Add state for upload modal:

```typescript
const [uploadTarget, setUploadTarget] = useState<string | null>(null);
const [batchUploadOpen, setBatchUploadOpen] = useState(false);
```

- [ ] **Step 4: Render Language Packages section in main content**

In the section rendering logic (where `activeSection` determines what to show), add:

```typescript
{activeSection === "language_packages" && view?.language_packages && (
  <LanguagePackageList
    environments={view.language_packages}
    onToggle={(envId) => {
      // Dispatch SetInclude op for ItemId::LanguageEnv
      applyOp({
        op: "SetInclude",
        item_id: { LanguageEnv: { id: envId } },
        include: !view.language_packages!.find((e) => e.id === envId)?.include,
      });
    }}
    isPending={isPending}
    revealItemId={revealItemId}
    filterActive={sectionSearchOpen}
    filterQuery={sectionSearchQuery}
  />
)}
```

- [ ] **Step 5: Render Unmanaged Files section in main content**

```typescript
{activeSection === "unmanaged_files" && view?.unmanaged_files && (
  <UnmanagedFileList
    groups={view.unmanaged_files}
    onToggleItem={(itemId) => {
      const allItems = view.unmanaged_files!.flatMap((g) => g.items);
      const item = allItems.find((i) => i.id === itemId);
      applyOp({
        op: "SetInclude",
        item_id: { UnmanagedFile: { path: itemId } },
        include: !item?.include,
      });
    }}
    onToggleGroup={(directory, include) => {
      const group = view.unmanaged_files!.find((g) => g.directory === directory);
      if (group) {
        for (const item of group.items) {
          applyOp({
            op: "SetInclude",
            item_id: { UnmanagedFile: { path: item.id } },
            include,
          });
        }
      }
    }}
    isPending={isPending}
    onIncludeNone={() => {
      const allItems = view.unmanaged_files!.flatMap((g) => g.items);
      for (const item of allItems) {
        if (item.include) {
          applyOp({
            op: "SetInclude",
            item_id: { UnmanagedFile: { path: item.id } },
            include: false,
          });
        }
      }
    }}
    onResetAll={() => {
      const allItems = view.unmanaged_files!.flatMap((g) => g.items);
      for (const item of allItems) {
        if (!item.include) {
          applyOp({
            op: "SetInclude",
            item_id: { UnmanagedFile: { path: item.id } },
            include: true,
          });
        }
      }
    }}
    revealItemId={revealItemId}
    filterActive={sectionSearchOpen}
    filterQuery={sectionSearchQuery}
  />
)}
```

- [ ] **Step 6: Render upload modals**

Add at the bottom of the App component JSX:

```tsx
<RpmUploadModal
  isOpen={uploadTarget !== null}
  packageName={uploadTarget ?? ""}
  packageArch={view?.packages?.find((p) => p.name === uploadTarget)?.arch ?? "x86_64"}
  onUpload={(name, file) => {
    rpmUpload.uploadRpm(name, file);
    setUploadTarget(null);
  }}
  onClose={() => setUploadTarget(null)}
/>

<RpmBatchUploadModal
  isOpen={batchUploadOpen}
  needsUploadPackages={Array.from(
    view?.packages
      ?.filter((p) => p.source_repo === "none" && !rpmUpload.uploads.has(p.name))
      .map((p) => p.name) ?? [],
  )}
  onBatchUpload={(matched) => {
    rpmUpload.applyBatchMatch(matched);
    setBatchUploadOpen(false);
  }}
  onClose={() => setBatchUploadOpen(false)}
/>
```

- [ ] **Step 7: Pass new data to Sidebar and GlobalSearch**

Update `Sidebar` props:

```typescript
<Sidebar
  // ... existing props ...
  hasLanguagePackages={!!view?.language_packages?.length}
  hasUnmanagedFiles={!!view?.unmanaged_files?.length}
  hasUnmanagedScan={view?.has_unmanaged_scan ?? false}
/>
```

Update `GlobalSearch` props:

```typescript
<GlobalSearch
  // ... existing props ...
  languagePackageEnvs={view?.language_packages}
  unmanagedFileGroups={view?.unmanaged_files}
/>
```

- [ ] **Step 8: Add Upload RPMs button to StatsBar when blocked packages exist**

In the `StatsBar` or toolbar area, conditionally render:

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

New decision sections render in single-host main content. RPM upload
modal opens from blocked package rows. Batch upload button in toolbar
when repo-less packages need RPMs. Sidebar and global search receive
new section data.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 11: CSS Styling for New Components

**Files:**
- Modify: `crates/web/ui/src/App.css`

**Interfaces:**
- Consumes: BEM class names from LanguagePackageList, UnmanagedFileList, Sidebar hint
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

- [ ] **Step 2: Add Unmanaged File List styles**

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

.inspectah-unmanaged-row {
  display: flex;
  align-items: center;
  gap: var(--pf-t--global--spacer--sm);
  padding: 2px 0;
  font-size: var(--pf-t--global--font--size--body--sm);
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
```

- [ ] **Step 3: Add dark mode overrides for /var warning**

```css
/* Dark mode: /var warning styles */
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
git add -A && git commit -m "style(web): add CSS for language packages, unmanaged files, and sidebar hint

BEM-scoped styles matching existing decision row patterns. /var path
warning uses warning color. Dark mode overrides included.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Task 12: Aggregate Mode Support

**Files:**
- Modify: `crates/web/ui/src/components/aggregate/AggregateItemRow.tsx`
- Modify: `crates/web/ui/src/App.css` (aggregate-specific styles)
- Test: visual verification + existing aggregate tests should still pass

**Interfaces:**
- Consumes: `AggregateSection` with `language_packages` and `unmanaged_files` IDs from backend
- Produces: Enriched aggregate rows for new section types

The aggregate sidebar (`AggregateSidebar.tsx`) is fully data-driven — it renders whatever sections the backend provides via the `sections` prop, keyed by `is_decision_section`. No code changes needed there. The new sections will appear in the Review group automatically once the backend emits them.

`AggregateSection.tsx` is also data-driven — it renders zones (consensus/near-consensus/divergent) using `AggregateItemRow` for each item. The zone layout works without modification.

The work here is in `AggregateItemRow.tsx` — adding metadata rendering for the two new section types.

- [ ] **Step 1: Write failing test — aggregate row shows ecosystem for language packages**

Add to existing aggregate tests or create a new test:

```typescript
import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import { AggregateItemRow } from "../aggregate/AggregateItemRow";

describe("AggregateItemRow — language packages", () => {
  it("renders ecosystem label and package count for language_packages section", () => {
    render(
      <AggregateItemRow
        item={{
          id: "pip:/opt/myapp/venv",
          name: "/opt/myapp/venv",
          include: true,
          prevalence: { count: 8, total: 10 },
          variants: 1,
          metadata: {
            ecosystem: "pip",
            confidence: "high",
            manifest_basis: "requirements.txt",
            package_count: 12,
          },
        }}
        sectionId="language_packages"
        onToggle={vi.fn()}
        onSelect={vi.fn()}
        isSelected={false}
        isPending={false}
      />,
    );
    expect(screen.getByText("pip")).toBeInTheDocument();
    expect(screen.getByText("12 packages")).toBeInTheDocument();
  });
});

describe("AggregateItemRow — unmanaged files", () => {
  it("renders file type and size for unmanaged_files section", () => {
    render(
      <AggregateItemRow
        item={{
          id: "/opt/splunk/bin/splunkd",
          name: "/opt/splunk/bin/splunkd",
          include: true,
          prevalence: { count: 10, total: 10 },
          variants: 1,
          metadata: {
            file_type: "elf_binary",
            size: 52428800,
            is_var_path: false,
          },
        }}
        sectionId="unmanaged_files"
        onToggle={vi.fn()}
        onSelect={vi.fn()}
        isSelected={false}
        isPending={false}
      />,
    );
    expect(screen.getByText("elf_binary")).toBeInTheDocument();
    expect(screen.getByText(/50.*MB/)).toBeInTheDocument();
  });

  it("shows /var warning badge for var paths", () => {
    render(
      <AggregateItemRow
        item={{
          id: "/var/lib/custom/data.db",
          name: "/var/lib/custom/data.db",
          include: true,
          prevalence: { count: 5, total: 10 },
          variants: 2,
          metadata: {
            file_type: "data",
            size: 209715200,
            is_var_path: true,
          },
        }}
        sectionId="unmanaged_files"
        onToggle={vi.fn()}
        onSelect={vi.fn()}
        isSelected={false}
        isPending={false}
      />,
    );
    expect(screen.getByText("/var")).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Verify test fails**

```bash
cd crates/web/ui && npm test -- --run AggregateItemRow
```

- [ ] **Step 3: Add section-aware metadata rendering to AggregateItemRow**

In `AggregateItemRow.tsx`, the component receives an `item` with generic fields. Add conditional metadata rendering based on `sectionId`:

```typescript
// Add sectionId to props if not already present
export interface AggregateItemRowProps {
  // ... existing props ...
  /** Section ID — used for section-specific metadata rendering. */
  sectionId?: string;
}
```

Add a metadata rendering function:

```tsx
function SectionMetadata({
  sectionId,
  metadata,
}: {
  sectionId?: string;
  metadata?: Record<string, unknown>;
}) {
  if (!metadata || !sectionId) return null;

  if (sectionId === "language_packages") {
    return (
      <span className="aggregate-item-row__section-meta">
        <Label isCompact>{metadata.ecosystem as string}</Label>
        <Badge isRead>
          {metadata.package_count as number} packages
        </Badge>
        <Label
          color={
            metadata.confidence === "high"
              ? "green"
              : metadata.confidence === "medium"
                ? "orange"
                : "grey"
          }
          isCompact
        >
          {metadata.confidence as string}
        </Label>
      </span>
    );
  }

  if (sectionId === "unmanaged_files") {
    const size = metadata.size as number;
    const formattedSize =
      size < 1024 * 1024
        ? `${(size / 1024).toFixed(0)} KB`
        : `${(size / (1024 * 1024)).toFixed(1)} MB`;

    return (
      <span className="aggregate-item-row__section-meta">
        <span className="aggregate-item-row__file-type">
          {metadata.file_type as string}
        </span>
        <span className="aggregate-item-row__file-size">
          {formattedSize}
        </span>
        {metadata.is_var_path && (
          <span className="aggregate-item-row__var-badge">/var</span>
        )}
      </span>
    );
  }

  return null;
}
```

Render `<SectionMetadata>` in the row layout, after the name area:

```tsx
<SectionMetadata sectionId={sectionId} metadata={item.metadata} />
```

- [ ] **Step 4: Add aggregate-specific CSS**

Append to `App.css`:

```css
/* ─── Aggregate item row — section-specific metadata ─────────────────── */

.aggregate-item-row__section-meta {
  display: flex;
  align-items: center;
  gap: var(--pf-t--global--spacer--xs);
  margin-left: var(--pf-t--global--spacer--sm);
}

.aggregate-item-row__file-type {
  font-size: var(--pf-t--global--font--size--body--sm);
  color: var(--pf-t--global--text--color--subtle);
}

.aggregate-item-row__file-size {
  font-size: var(--pf-t--global--font--size--body--sm);
  color: var(--pf-t--global--text--color--subtle);
}

.aggregate-item-row__var-badge {
  font-size: var(--pf-t--global--font--size--body--sm);
  color: var(--pf-t--global--color--status--warning--default);
  font-weight: var(--pf-t--global--font--weight--body--bold);
}
```

- [ ] **Step 5: Verify tests pass, commit**

```bash
cd crates/web/ui && npm test -- --run
git add -A && git commit -m "feat(web): add aggregate row metadata for language packages and unmanaged files

Ecosystem, package count, and confidence for language packages.
File type, size, and /var warning for unmanaged files. Zone-based
layout works without modification via existing AggregateSection.

Assisted-by: Claude Code (Opus 4.6)"
```

**Thorn Checkpoint: Tasks 9-12** — Global search, App.tsx wiring, CSS styling, and aggregate support complete. Run full test suite with `npm test -- --run`. Verify:
1. All existing tests still pass
2. Language Packages section appears in sidebar (single-host + aggregate)
3. Unmanaged Files section appears with flag gate and discoverability hint
4. Keyboard shortcuts 1-9 match the spec's shortcut map
5. Global search finds items in new sections
6. RPM upload icon appears on blocked package rows
7. Aggregate mode shows section-specific metadata

---

## Shared Contracts Consumed from Plan 1

### ItemId Variants Used

| Plan 3 Context | ItemId Variant | Identity Key |
|---------------|---------------|--------------|
| Language Package toggle | `ItemId::LanguageEnv { ecosystem, path }` | `"pip:/opt/myapp/venv"` |
| Unmanaged File toggle | `ItemId::UnmanagedFile { path }` | `"/opt/splunk/bin/splunkd"` |
| RPM Package toggle | `ItemId::Package` (existing) | Package name |

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
