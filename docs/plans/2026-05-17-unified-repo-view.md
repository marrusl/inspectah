# Unified Repo View Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace attention-tier-first package grouping with repo-first grouping, where packages are organized by source repository with attention bubbling within each group.

**Architecture:** The change is entirely frontend. The existing `/api/view` response already provides `packages` (with `source_repo` per package) and `repo_groups` (with `is_distro`, `provenance`, `package_count`, `enabled` per repo). DecisionList currently groups by attention level using AttentionGroup components. This refactor introduces RepoGroup as the primary grouping axis for packages, with attention-level ordering within each repo group. The config files section remains unchanged (attention-first with AttentionGroup).

**Tech Stack:** React 18, TypeScript, PatternFly 6, Vitest + @testing-library/react + userEvent

---

### Task 1: Update RepoGroupHeader Labels

**Files:**
- Modify: `inspectah-web/ui/src/components/RepoGroupHeader.tsx`
- Modify: `inspectah-web/ui/src/components/__tests__/DecisionSections.test.tsx`

Per spec: distro repos show no label (remove badge entirely). Third-party repos show "Third-party" text label. Drop the provenance badge system (verified/incomplete/unknown badges). Add a chevron icon for expand/collapse. Add ARIA attributes for the collapsible repo group pattern.

- [ ] **Step 1: Write failing tests for updated RepoGroupHeader**

Add these tests to `DecisionSections.test.tsx` in a new describe block:

```typescript
// ---- Updated RepoGroupHeader tests ----

describe("RepoGroupHeader updated labels", () => {
  it("shows no badge for distro repos", () => {
    render(
      <RepoGroupHeader
        sectionId="baseos"
        provenance="verified"
        isDistro={true}
        packageCount={50}
        enabled={true}
      />,
    );
    expect(screen.queryByText("Distro")).not.toBeInTheDocument();
    expect(screen.queryByText("D")).not.toBeInTheDocument();
    expect(screen.getByText("baseos")).toBeInTheDocument();
  });

  it("shows 'Third-party' text for non-distro repos", () => {
    render(
      <RepoGroupHeader
        sectionId="epel"
        provenance="verified"
        isDistro={false}
        packageCount={5}
        enabled={true}
      />,
    );
    expect(screen.getByText("Third-party")).toBeInTheDocument();
    expect(screen.getByText("epel")).toBeInTheDocument();
  });

  it("shows no label for non-distro repos with incomplete provenance", () => {
    render(
      <RepoGroupHeader
        sectionId="custom"
        provenance="incomplete"
        isDistro={false}
        packageCount={3}
        enabled={true}
      />,
    );
    // No provenance badge at all — spec drops provenance badges from UI
    expect(screen.queryByText("Unverified")).not.toBeInTheDocument();
    expect(screen.queryByText("Third-party")).not.toBeInTheDocument();
    expect(screen.getByText("custom")).toBeInTheDocument();
  });

  it("renders chevron icon", () => {
    const { container } = render(
      <RepoGroupHeader
        sectionId="epel"
        provenance="verified"
        isDistro={false}
        packageCount={5}
        enabled={true}
        isExpanded={false}
      />,
    );
    // AngleRightIcon when collapsed
    expect(container.querySelector("svg")).toBeTruthy();
  });

  it("has aria-expanded attribute", () => {
    render(
      <RepoGroupHeader
        sectionId="epel"
        provenance="verified"
        isDistro={false}
        packageCount={5}
        enabled={true}
        isExpanded={true}
      />,
    );
    const header = screen.getByTestId("repo-group-epel");
    expect(header).toHaveAttribute("aria-expanded", "true");
  });

  it("shows 'N packages excluded' for disabled repos", () => {
    render(
      <RepoGroupHeader
        sectionId="epel"
        provenance="verified"
        isDistro={false}
        packageCount={5}
        enabled={false}
      />,
    );
    expect(screen.getByText("5 packages excluded")).toBeInTheDocument();
  });

  it("shows struck-through name and dimmed text for disabled repos", () => {
    render(
      <RepoGroupHeader
        sectionId="epel"
        provenance="verified"
        isDistro={false}
        packageCount={5}
        enabled={false}
      />,
    );
    const label = screen.getByText("epel");
    expect(label.style.textDecoration).toBe("line-through");
    expect(label.style.opacity).toBe("0.6");
  });

  it("shows informational count in header when provided", () => {
    render(
      <RepoGroupHeader
        sectionId="appstream"
        provenance="verified"
        isDistro={true}
        packageCount={20}
        enabled={true}
        infoCount={3}
      />,
    );
    expect(screen.getByText("3 informational")).toBeInTheDocument();
  });

  it("shows 'No action needed' for all-routine repos", () => {
    render(
      <RepoGroupHeader
        sectionId="baseos"
        provenance="verified"
        isDistro={true}
        packageCount={50}
        enabled={true}
        summaryText="No action needed"
      />,
    );
    expect(screen.getByText("No action needed")).toBeInTheDocument();
  });
});
```

Import `RepoGroupHeader` at the top of the test file (already imported).

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui && npx vitest run src/components/__tests__/DecisionSections.test.tsx`
Expected: FAIL — RepoGroupHeader still renders "Distro" badge, lacks `isExpanded`/`infoCount`/`summaryText` props

- [ ] **Step 3: Implement updated RepoGroupHeader**

Replace `inspectah-web/ui/src/components/RepoGroupHeader.tsx` with:

```typescript
import { useCallback } from "react";
import { Switch } from "@patternfly/react-core";
import { AngleRightIcon, AngleDownIcon } from "@patternfly/react-icons";
import type { RepoProvenance } from "../api/types";

export interface RepoGroupHeaderProps {
  sectionId: string;
  provenance: RepoProvenance;
  isDistro: boolean;
  packageCount: number;
  enabled: boolean;
  isExpanded?: boolean;
  /** Number of informational packages — shown in collapsed header */
  infoCount?: number;
  /** Summary text like "No action needed" for all-routine repos */
  summaryText?: string;
  onToggle?: (sectionId: string, enabled: boolean) => void;
  onExpandToggle?: () => void;
  onKeyDown?: (e: React.KeyboardEvent<HTMLDivElement>) => void;
}

/** Only verified non-distro repos are toggleable. */
const showToggle = (isDistro: boolean, provenance: RepoProvenance): boolean =>
  !isDistro && provenance === "verified";

/** Distro repos get no label. Verified non-distro repos get "Third-party". Others get nothing. */
function classificationLabel(isDistro: boolean, provenance: RepoProvenance): string | null {
  if (isDistro) return null;
  if (provenance === "verified") return "Third-party";
  return null;
}

export function RepoGroupHeader({
  sectionId,
  provenance,
  isDistro,
  packageCount,
  enabled,
  isExpanded = false,
  infoCount,
  summaryText,
  onToggle,
  onExpandToggle,
  onKeyDown: onKeyDownProp,
}: RepoGroupHeaderProps) {
  const canToggle = showToggle(isDistro, provenance);
  const label = classificationLabel(isDistro, provenance);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLDivElement>) => {
      if (onKeyDownProp) {
        onKeyDownProp(e);
        return;
      }
      if (e.key === "Enter") {
        e.preventDefault();
        onExpandToggle?.();
      }
    },
    [onKeyDownProp, onExpandToggle],
  );

  const handleClick = useCallback(() => {
    onExpandToggle?.();
  }, [onExpandToggle]);

  const disabledStyle = !enabled
    ? { textDecoration: "line-through" as const, opacity: "0.6" }
    : {};

  return (
    <div
      data-testid={`repo-group-${sectionId}`}
      role="button"
      aria-expanded={isExpanded}
      tabIndex={0}
      onKeyDown={handleKeyDown}
      onClick={handleClick}
      className={`inspectah-repo-group-header${!enabled ? " inspectah-repo-group-header--disabled" : ""}`}
    >
      <span className="inspectah-repo-group-header__chevron">
        {isExpanded ? <AngleDownIcon /> : <AngleRightIcon />}
      </span>
      <span className="inspectah-repo-group-header__label" style={disabledStyle}>
        {sectionId}
      </span>
      {label && (
        <span className="inspectah-repo-group-header__classification">
          {label}
        </span>
      )}
      <span className="inspectah-repo-group-header__count">
        {!enabled
          ? `${packageCount} packages excluded`
          : `${packageCount} ${packageCount === 1 ? "package" : "packages"}`}
      </span>
      {enabled && infoCount != null && infoCount > 0 && (
        <span className="inspectah-repo-group-header__info-count">
          {infoCount} informational
        </span>
      )}
      {enabled && summaryText && (
        <span className="inspectah-repo-group-header__summary">
          {summaryText}
        </span>
      )}
      {canToggle && (
        <span
          className="inspectah-repo-group-header__toggle"
          onClick={(e) => e.stopPropagation()}
        >
          <Switch
            id={`repo-toggle-${sectionId}`}
            label={enabled ? "Enabled" : "Disabled"}
            isChecked={enabled}
            onChange={() => onToggle?.(sectionId, !enabled)}
            aria-label={`Toggle ${sectionId} repo`}
          />
        </span>
      )}
    </div>
  );
}
```

- [ ] **Step 4: Update CSS for new header elements**

Add to `inspectah-web/ui/src/App.css`, after the existing `.inspectah-repo-group-header__toggle` block:

```css
.inspectah-repo-group-header__chevron {
  display: flex;
  align-items: center;
  flex-shrink: 0;
}

.inspectah-repo-group-header__classification {
  color: var(--pf-t--global--text--color--subtle);
  font-size: var(--pf-t--global--font--size--body--sm);
  font-style: italic;
}

.inspectah-repo-group-header__info-count {
  color: var(--pf-t--global--color--status--info--default);
  font-size: var(--pf-t--global--font--size--body--sm);
}

.inspectah-repo-group-header__summary {
  color: var(--pf-t--global--text--color--subtle);
  font-size: var(--pf-t--global--font--size--body--sm);
}

.inspectah-repo-group-header--disabled {
  opacity: 0.6;
}

.inspectah-repo-group-header--disabled .inspectah-repo-group-header__label {
  text-decoration: line-through;
}
```

- [ ] **Step 5: Remove old badge CSS**

Remove the badge-full/badge-abbrev CSS blocks and their responsive rules from `App.css` (the `.inspectah-repo-group-header__badge-full`, `.inspectah-repo-group-header__badge-abbrev`, and the `@media (max-width: 767px)` block for badge switching). These are replaced by the classification label.

- [ ] **Step 6: Run tests to verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui && npx vitest run src/components/__tests__/DecisionSections.test.tsx`
Expected: PASS — all new RepoGroupHeader tests pass

- [ ] **Step 7: Update existing RepoGroupHeader tests to match new API**

The existing "Repo group headers" describe block in the test file references old badge behavior (expects "Distro", "Third-party" as PatternFly Labels, expects old header props). Update those tests to match the new component API:
- Tests that check for "Distro" badge text should be updated to expect no badge
- Tests that use the old props should add `isExpanded={true}` where the test expects children to be visible
- The `onToggle` callback tests remain valid but the wrapping structure changes

Review and update each test in the existing "Repo group headers" describe block to work with the new component.

- [ ] **Step 8: Run full test suite to verify no regressions**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui && npx vitest run src/components/__tests__/DecisionSections.test.tsx`
Expected: PASS — all tests pass

- [ ] **Step 9: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && git add inspectah-web/ui/src/components/RepoGroupHeader.tsx inspectah-web/ui/src/components/__tests__/DecisionSections.test.tsx inspectah-web/ui/src/App.css && git commit -m "feat(web): update RepoGroupHeader for unified repo view

Drop provenance badges. Distro repos show no label, verified non-distro
shows 'Third-party' text. Add chevron, isExpanded, infoCount, summaryText
props. Disabled repos show struck-through name and 'N packages excluded'.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 2: Create AttentionSummary Component

**Files:**
- Create: `inspectah-web/ui/src/components/AttentionSummary.tsx`
- Create: `inspectah-web/ui/src/components/__tests__/AttentionSummary.test.tsx`

Per spec: a summary line at the top of the Packages section showing cross-repo attention signal. Three text states.

- [ ] **Step 1: Write failing tests**

Create `inspectah-web/ui/src/components/__tests__/AttentionSummary.test.tsx`:

```typescript
import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { AttentionSummary } from "../AttentionSummary";

describe("AttentionSummary", () => {
  it("shows review count and repo count when needs_review > 0", () => {
    render(
      <AttentionSummary needsReviewCount={3} needsReviewRepoCount={2} infoCount={5} infoRepoCount={3} />,
    );
    expect(screen.getByText("3 packages need review across 2 repos")).toBeInTheDocument();
  });

  it("shows singular 'package' and 'repo' when counts are 1", () => {
    render(
      <AttentionSummary needsReviewCount={1} needsReviewRepoCount={1} infoCount={0} infoRepoCount={0} />,
    );
    expect(screen.getByText("1 package needs review across 1 repo")).toBeInTheDocument();
  });

  it("shows informational fallback when needs_review is 0 but informational exists", () => {
    render(
      <AttentionSummary needsReviewCount={0} needsReviewRepoCount={0} infoCount={12} infoRepoCount={3} />,
    );
    const el = screen.getByTestId("attention-summary");
    expect(el.textContent).toContain("No packages flagged for review");
    expect(el.textContent).toContain("12 informational across 3 repos");
  });

  it("shows all-clear when both are 0", () => {
    render(
      <AttentionSummary needsReviewCount={0} needsReviewRepoCount={0} infoCount={0} infoRepoCount={0} />,
    );
    expect(screen.getByText("All actionable items reviewed")).toBeInTheDocument();
  });

  it("has correct data-testid", () => {
    render(
      <AttentionSummary needsReviewCount={0} needsReviewRepoCount={0} infoCount={0} infoRepoCount={0} />,
    );
    expect(screen.getByTestId("attention-summary")).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui && npx vitest run src/components/__tests__/AttentionSummary.test.tsx`
Expected: FAIL — module not found

- [ ] **Step 3: Implement AttentionSummary**

Create `inspectah-web/ui/src/components/AttentionSummary.tsx`:

```typescript
export interface AttentionSummaryProps {
  needsReviewCount: number;
  needsReviewRepoCount: number;
  infoCount: number;
  infoRepoCount: number;
}

export function AttentionSummary({
  needsReviewCount,
  needsReviewRepoCount,
  infoCount,
  infoRepoCount,
}: AttentionSummaryProps) {
  let text: string;

  if (needsReviewCount > 0) {
    const pkgWord = needsReviewCount === 1 ? "package" : "packages";
    const verb = needsReviewCount === 1 ? "needs" : "need";
    const repoWord = needsReviewRepoCount === 1 ? "repo" : "repos";
    text = `${needsReviewCount} ${pkgWord} ${verb} review across ${needsReviewRepoCount} ${repoWord}`;
  } else if (infoCount > 0) {
    const repoWord = infoRepoCount === 1 ? "repo" : "repos";
    text = `No packages flagged for review · ${infoCount} informational across ${infoRepoCount} ${repoWord}`;
  } else {
    text = "All actionable items reviewed";
  }

  return (
    <div
      data-testid="attention-summary"
      style={{
        padding: "var(--pf-t--global--spacer--sm) 0",
        fontSize: "var(--pf-t--global--font--size--body--default)",
        color: needsReviewCount > 0
          ? "var(--pf-t--global--color--status--danger--default)"
          : "var(--pf-t--global--text--color--subtle)",
        fontWeight: needsReviewCount > 0 ? 600 : 400,
      }}
    >
      {text}
    </div>
  );
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui && npx vitest run src/components/__tests__/AttentionSummary.test.tsx`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && git add inspectah-web/ui/src/components/AttentionSummary.tsx inspectah-web/ui/src/components/__tests__/AttentionSummary.test.tsx && git commit -m "feat(web): add AttentionSummary component

Cross-repo attention counter with three text states: review count,
informational fallback, and all-clear. Placed at top of Packages section.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 3: Create RepoGroup Collapsible Component

**Files:**
- Create: `inspectah-web/ui/src/components/RepoGroup.tsx`
- Create: `inspectah-web/ui/src/components/__tests__/RepoGroup.test.tsx`

Per spec: collapsible wrapper around repo header + package list with expansion defaults based on attention content.

- [ ] **Step 1: Write failing tests**

Create `inspectah-web/ui/src/components/__tests__/RepoGroup.test.tsx`:

```typescript
import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { RepoGroup } from "../RepoGroup";
import type { RepoGroupInfo } from "../../api/types";

const baseRepo: RepoGroupInfo = {
  section_id: "epel",
  provenance: "verified",
  is_distro: false,
  package_count: 5,
  enabled: true,
};

describe("RepoGroup", () => {
  it("renders header with repo name", () => {
    render(
      <RepoGroup repo={baseRepo} defaultExpanded={true}>
        <div data-testid="child">content</div>
      </RepoGroup>,
    );
    expect(screen.getByText("epel")).toBeInTheDocument();
  });

  it("shows children when defaultExpanded is true", () => {
    render(
      <RepoGroup repo={baseRepo} defaultExpanded={true}>
        <div data-testid="child">content</div>
      </RepoGroup>,
    );
    expect(screen.getByTestId("child")).toBeVisible();
  });

  it("hides children when defaultExpanded is false", () => {
    render(
      <RepoGroup repo={baseRepo} defaultExpanded={false}>
        <div data-testid="child">content</div>
      </RepoGroup>,
    );
    expect(screen.queryByTestId("child")).not.toBeInTheDocument();
  });

  it("toggles expansion on header click", async () => {
    render(
      <RepoGroup repo={baseRepo} defaultExpanded={false}>
        <div data-testid="child">content</div>
      </RepoGroup>,
    );
    expect(screen.queryByTestId("child")).not.toBeInTheDocument();

    const header = screen.getByTestId("repo-group-epel");
    await userEvent.click(header);
    expect(screen.getByTestId("child")).toBeVisible();
  });

  it("toggles expansion on Enter key", async () => {
    render(
      <RepoGroup repo={baseRepo} defaultExpanded={false}>
        <div data-testid="child">content</div>
      </RepoGroup>,
    );
    const header = screen.getByTestId("repo-group-epel");
    header.focus();
    await userEvent.keyboard("{Enter}");
    expect(screen.getByTestId("child")).toBeVisible();
  });

  it("force-expands when forceExpanded is true regardless of user toggle", async () => {
    const { rerender } = render(
      <RepoGroup repo={baseRepo} defaultExpanded={false}>
        <div data-testid="child">content</div>
      </RepoGroup>,
    );
    expect(screen.queryByTestId("child")).not.toBeInTheDocument();

    rerender(
      <RepoGroup repo={baseRepo} defaultExpanded={false} forceExpanded={true}>
        <div data-testid="child">content</div>
      </RepoGroup>,
    );
    expect(screen.getByTestId("child")).toBeVisible();
  });

  it("calls onRepoToggle when switch is clicked", async () => {
    const onRepoToggle = vi.fn();
    render(
      <RepoGroup repo={baseRepo} defaultExpanded={true} onRepoToggle={onRepoToggle}>
        <div>content</div>
      </RepoGroup>,
    );
    const toggle = screen.getByRole("switch", { name: /toggle epel repo/i });
    await userEvent.click(toggle);
    expect(onRepoToggle).toHaveBeenCalledWith("epel", false);
  });

  it("does not show toggle for distro repos", () => {
    const distroRepo: RepoGroupInfo = { ...baseRepo, section_id: "baseos", is_distro: true };
    render(
      <RepoGroup repo={distroRepo} defaultExpanded={true}>
        <div>content</div>
      </RepoGroup>,
    );
    expect(screen.queryByRole("switch")).not.toBeInTheDocument();
  });

  it("renders with disabled styling when repo is disabled", () => {
    const disabledRepo: RepoGroupInfo = { ...baseRepo, enabled: false };
    render(
      <RepoGroup repo={disabledRepo} defaultExpanded={false}>
        <div data-testid="child">content</div>
      </RepoGroup>,
    );
    const label = screen.getByText("epel");
    expect(label.style.textDecoration).toBe("line-through");
  });

  it("shows infoCount in collapsed header when provided", () => {
    render(
      <RepoGroup repo={baseRepo} defaultExpanded={false} infoCount={3}>
        <div>content</div>
      </RepoGroup>,
    );
    expect(screen.getByText("3 informational")).toBeInTheDocument();
  });

  it("shows summaryText in collapsed header when provided", () => {
    render(
      <RepoGroup repo={baseRepo} defaultExpanded={false} summaryText="No action needed">
        <div>content</div>
      </RepoGroup>,
    );
    expect(screen.getByText("No action needed")).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui && npx vitest run src/components/__tests__/RepoGroup.test.tsx`
Expected: FAIL — module not found

- [ ] **Step 3: Implement RepoGroup**

Create `inspectah-web/ui/src/components/RepoGroup.tsx`:

```typescript
import { useState, useCallback } from "react";
import type { RepoGroupInfo } from "../api/types";
import { RepoGroupHeader } from "./RepoGroupHeader";

export interface RepoGroupProps {
  repo: RepoGroupInfo;
  defaultExpanded: boolean;
  /** Override: force-expand when search filter is active */
  forceExpanded?: boolean;
  /** Number of informational packages — shown in collapsed header */
  infoCount?: number;
  /** Summary text for collapsed header (e.g., "No action needed") */
  summaryText?: string;
  onRepoToggle?: (sectionId: string, enabled: boolean) => void;
  onKeyDown?: (e: React.KeyboardEvent<HTMLDivElement>) => void;
  children: React.ReactNode;
}

export function RepoGroup({
  repo,
  defaultExpanded,
  forceExpanded = false,
  infoCount,
  summaryText,
  onRepoToggle,
  onKeyDown,
  children,
}: RepoGroupProps) {
  const [isExpanded, setIsExpanded] = useState(defaultExpanded);

  const effectiveExpanded = forceExpanded || isExpanded;

  const handleExpandToggle = useCallback(() => {
    setIsExpanded((prev) => !prev);
  }, []);

  return (
    <div data-testid={`repo-group-wrapper-${repo.section_id}`}>
      <RepoGroupHeader
        sectionId={repo.section_id}
        provenance={repo.provenance}
        isDistro={repo.is_distro}
        packageCount={repo.package_count}
        enabled={repo.enabled}
        isExpanded={effectiveExpanded}
        infoCount={infoCount}
        summaryText={summaryText}
        onToggle={onRepoToggle}
        onExpandToggle={handleExpandToggle}
        onKeyDown={onKeyDown}
      />
      {effectiveExpanded && children}
    </div>
  );
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui && npx vitest run src/components/__tests__/RepoGroup.test.tsx`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && git add inspectah-web/ui/src/components/RepoGroup.tsx inspectah-web/ui/src/components/__tests__/RepoGroup.test.tsx && git commit -m "feat(web): add RepoGroup collapsible component

Wraps RepoGroupHeader with expand/collapse state, force-expand for search,
infoCount and summaryText header annotations. Expansion defaults driven
by parent based on attention content.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 4: Create RoutineSummary Component

**Files:**
- Create: `inspectah-web/ui/src/components/RoutineSummary.tsx`
- Create: `inspectah-web/ui/src/components/__tests__/RoutineSummary.test.tsx`

Per spec: "+ N routine" collapsed summary within repo groups for routine packages.

- [ ] **Step 1: Write failing tests**

Create `inspectah-web/ui/src/components/__tests__/RoutineSummary.test.tsx`:

```typescript
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { RoutineSummary } from "../RoutineSummary";
import type { DecisionItemKind } from "../DecisionItem";
import type { RefinedPackage, AttentionTag } from "../../api/types";

// Mock fetch for useViewed
const mockFetch = vi.fn();
beforeEach(() => {
  mockFetch.mockReset();
  vi.stubGlobal("fetch", mockFetch);
  mockFetch.mockImplementation(() =>
    Promise.resolve({ ok: true, json: () => Promise.resolve({ ids: [] }) }),
  );
});
afterEach(() => {
  vi.restoreAllMocks();
});

function makePkg(name: string): DecisionItemKind {
  return {
    type: "package",
    data: {
      entry: {
        name,
        epoch: "0",
        version: "1.0",
        release: "1.el9",
        arch: "x86_64",
        state: "added",
        include: true,
        source_repo: "baseos",
        fleet: null,
      },
      attention: [{ level: "routine", reason: "package_baseline_match", detail: null }],
    },
  };
}

describe("RoutineSummary", () => {
  it("renders '+ N routine' text", () => {
    const items = [makePkg("glibc"), makePkg("bash"), makePkg("coreutils")];
    render(<RoutineSummary items={items} />);
    expect(screen.getByText("+ 3 routine")).toBeInTheDocument();
  });

  it("starts collapsed by default", () => {
    const items = [makePkg("glibc")];
    render(<RoutineSummary items={items} />);
    expect(screen.queryByText("glibc.x86_64")).not.toBeInTheDocument();
  });

  it("expands to show package names on click", async () => {
    const items = [makePkg("glibc"), makePkg("bash")];
    render(<RoutineSummary items={items} />);

    await userEvent.click(screen.getByText("+ 2 routine"));
    expect(screen.getByText("glibc.x86_64")).toBeInTheDocument();
    expect(screen.getByText("bash.x86_64")).toBeInTheDocument();
  });

  it("auto-expands when forceExpanded is true", () => {
    const items = [makePkg("glibc")];
    render(<RoutineSummary items={items} forceExpanded={true} />);
    expect(screen.getByText("glibc.x86_64")).toBeInTheDocument();
  });

  it("auto-expands when revealItemId matches an item", () => {
    const items = [makePkg("glibc")];
    render(<RoutineSummary items={items} revealItemId="pkg:glibc:x86_64" />);
    expect(screen.getByText("glibc.x86_64")).toBeInTheDocument();
  });

  it("has correct data-testid", () => {
    const items = [makePkg("glibc")];
    render(<RoutineSummary items={items} />);
    expect(screen.getByTestId("routine-summary")).toBeInTheDocument();
  });

  it("has aria-expanded attribute", () => {
    const items = [makePkg("glibc")];
    render(<RoutineSummary items={items} />);
    const button = screen.getByRole("button");
    expect(button).toHaveAttribute("aria-expanded", "false");
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui && npx vitest run src/components/__tests__/RoutineSummary.test.tsx`
Expected: FAIL — module not found

- [ ] **Step 3: Implement RoutineSummary**

Create `inspectah-web/ui/src/components/RoutineSummary.tsx`:

```typescript
import { useState, useEffect } from "react";
import { AngleRightIcon, AngleDownIcon } from "@patternfly/react-icons";
import type { DecisionItemKind } from "./DecisionItem";
import { itemId as getItemId } from "./DecisionItem";

export interface RoutineSummaryProps {
  items: DecisionItemKind[];
  /** Override: force-expand when search filter matches */
  forceExpanded?: boolean;
  /** When set, auto-expands if this item ID is in the list */
  revealItemId?: string;
}

export function RoutineSummary({
  items,
  forceExpanded = false,
  revealItemId,
}: RoutineSummaryProps) {
  const [isExpanded, setIsExpanded] = useState(false);

  // Auto-expand when revealItemId matches an item
  useEffect(() => {
    if (!revealItemId) return;
    const match = items.some((item) => getItemId(item) === revealItemId);
    if (match && !isExpanded) {
      setIsExpanded(true);
    }
  }, [revealItemId, items, isExpanded]);

  // Auto-expand when filter is active
  useEffect(() => {
    if (forceExpanded && !isExpanded) {
      setIsExpanded(true);
    }
  }, [forceExpanded]); // eslint-disable-line react-hooks/exhaustive-deps

  const effectiveExpanded = forceExpanded || isExpanded;

  return (
    <div data-testid="routine-summary" style={{ marginBottom: "var(--pf-t--global--spacer--sm)" }}>
      <button
        type="button"
        onClick={() => setIsExpanded((prev) => !prev)}
        aria-expanded={effectiveExpanded}
        style={{
          background: "none",
          border: "none",
          cursor: "pointer",
          padding: "var(--pf-t--global--spacer--xs) 0",
          fontSize: "var(--pf-t--global--font--size--body--default)",
          color: "var(--pf-t--global--text--color--subtle)",
          display: "flex",
          alignItems: "center",
          gap: "var(--pf-t--global--spacer--xs)",
        }}
      >
        {effectiveExpanded ? <AngleDownIcon /> : <AngleRightIcon />}
        + {items.length} routine
      </button>
      {effectiveExpanded && (
        <ul style={{ listStyle: "none", padding: 0, margin: 0 }}>
          {items.map((item) => {
            const id = getItemId(item);
            const name = item.type === "package"
              ? `${item.data.entry.name}.${(item.data as any).entry.arch}`
              : (item.data as any).entry.path;
            return (
              <li
                key={id}
                data-testid={`decision-item-${id}`}
                tabIndex={-1}
                style={{
                  padding: "var(--pf-t--global--spacer--xs) var(--pf-t--global--spacer--md)",
                  color: "var(--pf-t--global--text--color--subtle)",
                  fontSize: "var(--pf-t--global--font--size--body--sm)",
                }}
              >
                {name}
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui && npx vitest run src/components/__tests__/RoutineSummary.test.tsx`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && git add inspectah-web/ui/src/components/RoutineSummary.tsx inspectah-web/ui/src/components/__tests__/RoutineSummary.test.tsx && git commit -m "feat(web): add RoutineSummary component

Collapsed '+ N routine' summary within repo groups. Supports
force-expand for search filter and auto-expand for revealItemId.
Matches existing BaselineSummary/ConfigManagedSummary patterns.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 5: Refactor DecisionList for Repo-First Package Grouping

**Files:**
- Modify: `inspectah-web/ui/src/components/DecisionList.tsx`
- Modify: `inspectah-web/ui/src/components/__tests__/DecisionSections.test.tsx`

This is the primary refactor. When `repoGroups` are provided (packages section), use repo-first grouping. When not provided (configs section), keep existing attention-first grouping unchanged.

- [ ] **Step 1: Write failing tests for repo-first grouping**

Add to `DecisionSections.test.tsx`:

```typescript
// ---- Repo-first grouping tests ----

describe("Repo-first package grouping", () => {
  const REPO_GROUPS: RepoGroupInfo[] = [
    { section_id: "baseos", provenance: "verified", is_distro: true, package_count: 2, enabled: true },
    { section_id: "appstream", provenance: "verified", is_distro: true, package_count: 1, enabled: true },
    { section_id: "epel", provenance: "verified", is_distro: false, package_count: 2, enabled: true },
  ];

  it("groups packages by repo instead of attention level", async () => {
    const items: DecisionItemKind[] = [
      { type: "package", data: makePkg({ name: "glibc", source_repo: "baseos" }, [NEEDS_REVIEW_TAG]) },
      { type: "package", data: makePkg({ name: "kernel", source_repo: "baseos" }, [ROUTINE_TAG]) },
      { type: "package", data: makePkg({ name: "httpd", source_repo: "appstream" }, [INFO_TAG]) },
      { type: "package", data: makePkg({ name: "epel-release", source_repo: "epel" }, [NEEDS_REVIEW_TAG]) },
      { type: "package", data: makePkg({ name: "htop", source_repo: "epel" }, [ROUTINE_TAG]) },
    ];

    render(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        repoGroups={REPO_GROUPS}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );

    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });

    // Repo group wrappers should exist
    expect(screen.getByTestId("repo-group-wrapper-baseos")).toBeInTheDocument();
    expect(screen.getByTestId("repo-group-wrapper-appstream")).toBeInTheDocument();
    expect(screen.getByTestId("repo-group-wrapper-epel")).toBeInTheDocument();

    // No attention-level groups should exist
    expect(screen.queryByTestId("attention-group-needs_review")).not.toBeInTheDocument();
    expect(screen.queryByTestId("attention-group-informational")).not.toBeInTheDocument();
    expect(screen.queryByTestId("attention-group-routine")).not.toBeInTheDocument();
  });

  it("orders repos: distro alpha, enabled third-party alpha, disabled, unknown last", async () => {
    const repoGroups: RepoGroupInfo[] = [
      { section_id: "epel", provenance: "verified", is_distro: false, package_count: 1, enabled: true },
      { section_id: "baseos", provenance: "verified", is_distro: true, package_count: 1, enabled: true },
      { section_id: "appstream", provenance: "verified", is_distro: true, package_count: 1, enabled: true },
    ];

    const items: DecisionItemKind[] = [
      { type: "package", data: makePkg({ name: "pkg1", source_repo: "epel" }, [NEEDS_REVIEW_TAG]) },
      { type: "package", data: makePkg({ name: "pkg2", source_repo: "baseos" }, [NEEDS_REVIEW_TAG]) },
      { type: "package", data: makePkg({ name: "pkg3", source_repo: "appstream" }, [NEEDS_REVIEW_TAG]) },
    ];

    render(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        repoGroups={repoGroups}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );

    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });

    const wrappers = screen.getAllByTestId(/^repo-group-wrapper-/);
    expect(wrappers[0]).toHaveAttribute("data-testid", "repo-group-wrapper-appstream");
    expect(wrappers[1]).toHaveAttribute("data-testid", "repo-group-wrapper-baseos");
    expect(wrappers[2]).toHaveAttribute("data-testid", "repo-group-wrapper-epel");
  });

  it("expands repos with needs_review packages by default", async () => {
    const items: DecisionItemKind[] = [
      { type: "package", data: makePkg({ name: "glibc", source_repo: "baseos" }, [NEEDS_REVIEW_TAG]) },
    ];

    render(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        repoGroups={[{ section_id: "baseos", provenance: "verified", is_distro: true, package_count: 1, enabled: true }]}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );

    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });

    // Package should be visible (repo expanded)
    expect(screen.getByText("glibc.x86_64")).toBeInTheDocument();
  });

  it("collapses all-routine repos by default", async () => {
    const items: DecisionItemKind[] = [
      { type: "package", data: makePkg({ name: "glibc", source_repo: "baseos" }, [ROUTINE_TAG]) },
    ];

    render(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        repoGroups={[{ section_id: "baseos", provenance: "verified", is_distro: true, package_count: 1, enabled: true }]}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );

    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });

    // Package should NOT be visible (repo collapsed)
    expect(screen.queryByText("glibc.x86_64")).not.toBeInTheDocument();
    // Summary text should appear
    expect(screen.getByText("No action needed")).toBeInTheDocument();
  });

  it("shows '+ N routine' summary within expanded repos that have mixed attention", async () => {
    const items: DecisionItemKind[] = [
      { type: "package", data: makePkg({ name: "httpd", source_repo: "epel" }, [NEEDS_REVIEW_TAG]) },
      { type: "package", data: makePkg({ name: "htop", source_repo: "epel" }, [ROUTINE_TAG]) },
      { type: "package", data: makePkg({ name: "jq", source_repo: "epel" }, [ROUTINE_TAG]) },
    ];

    render(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        repoGroups={[{ section_id: "epel", provenance: "verified", is_distro: false, package_count: 3, enabled: true }]}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );

    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });

    // NeedsReview package visible
    expect(screen.getByText("httpd.x86_64")).toBeInTheDocument();
    // Routine packages collapsed
    expect(screen.getByText("+ 2 routine")).toBeInTheDocument();
    expect(screen.queryByText("htop.x86_64")).not.toBeInTheDocument();
  });

  it("sorts packages within repo: needs_review first, then informational, then routine", async () => {
    const items: DecisionItemKind[] = [
      { type: "package", data: makePkg({ name: "zzz-routine", source_repo: "epel" }, [ROUTINE_TAG]) },
      { type: "package", data: makePkg({ name: "aaa-review", source_repo: "epel" }, [NEEDS_REVIEW_TAG]) },
      { type: "package", data: makePkg({ name: "mmm-info", source_repo: "epel" }, [INFO_TAG]) },
    ];

    render(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        repoGroups={[{ section_id: "epel", provenance: "verified", is_distro: false, package_count: 3, enabled: true }]}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );

    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });

    // NeedsReview and Informational are shown individually; routine is in summary
    const rows = screen.getAllByRole("row");
    expect(rows[0]).toHaveAttribute("data-testid", "decision-item-pkg:aaa-review:x86_64");
    expect(rows[1]).toHaveAttribute("data-testid", "decision-item-pkg:mmm-info:x86_64");
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui && npx vitest run src/components/__tests__/DecisionSections.test.tsx --reporter=verbose 2>&1 | tail -30`
Expected: FAIL — DecisionList still renders attention-tier groups when repoGroups provided

- [ ] **Step 3: Implement repo-first grouping in DecisionList**

This is a significant refactor of `DecisionList.tsx`. The key changes:
1. Add a new rendering path when `repoGroups.length > 0`
2. Group items by `source_repo`, sort within each group by attention level
3. Determine expansion defaults per repo based on attention content
4. Render RepoGroup components with RoutineSummary for routine packages
5. Keep the existing attention-first path for configs (when `repoGroups` is empty)

Replace the render body (lines 393-564, the `{(() => { ... })()}` block) in `DecisionList.tsx` with two branches:

```typescript
// Add imports at top of file:
import { RepoGroup } from "./RepoGroup";
import { RoutineSummary } from "./RoutineSummary";
```

Replace the IIFE render block (the `{(() => {` block starting around line 415) with:

```typescript
      {repoGroups.length > 0 ? (
        // Repo-first grouping for packages
        (() => {
          // Group items by source_repo
          const byRepo = new Map<string, DecisionItemKind[]>();
          for (const item of items) {
            const repo = item.type === "package"
              ? item.data.entry.source_repo.toLowerCase()
              : "__other__";
            const list = byRepo.get(repo) ?? [];
            list.push(item);
            byRepo.set(repo, list);
          }

          // Sort repos: distro alpha, enabled third-party alpha, disabled third-party alpha, unknown last
          const repoOrder = [...byRepo.keys()].sort((a, b) => {
            const rgA = repoGroupMap.get(a);
            const rgB = repoGroupMap.get(b);
            const rankA = !rgA ? 99
              : rgA.is_distro ? 0
              : rgA.enabled ? 1
              : 2;
            const rankB = !rgB ? 99
              : rgB.is_distro ? 0
              : rgB.enabled ? 1
              : 2;
            if (rankA !== rankB) return rankA - rankB;
            return a.localeCompare(b);
          });

          const filterActive = filterText.trim().length > 0;
          let runningRowIndex = 0;

          return repoOrder.map((repo) => {
            const repoItems = byRepo.get(repo) ?? [];
            const rg = repoGroupMap.get(repo);
            if (!rg) {
              // Unknown repo — render items flat
              return repoItems.map((item) => {
                runningRowIndex++;
                const id = getItemId(item);
                const level = item.data.attention.length > 0
                  ? highestAttention(item.data.attention)
                  : "routine";
                const flatIdx = flatItemIds.indexOf(id);
                return (
                  <DecisionItem
                    key={id}
                    item={item}
                    level={level}
                    rowIndex={runningRowIndex}
                    isViewed={viewedIds.has(id)}
                    isPending={mutation.isPending}
                    tabIndex={flatIdx === focusedIndex ? 0 : -1}
                    onToggleInclude={handleToggle}
                    onMarkViewed={markAsViewed}
                    onKeyDown={handleRowKeyDown}
                  />
                );
              });
            }

            // Sort items within repo by attention priority
            const LEVEL_ORDER: Record<string, number> = {
              needs_review: 0,
              informational: 1,
              routine: 2,
            };
            const sortedItems = [...repoItems].sort((a, b) => {
              const la = a.data.attention.length > 0
                ? highestAttention(a.data.attention)
                : "routine";
              const lb = b.data.attention.length > 0
                ? highestAttention(b.data.attention)
                : "routine";
              return (LEVEL_ORDER[la] ?? 2) - (LEVEL_ORDER[lb] ?? 2);
            });

            // Partition into attention tiers
            const needsReview = sortedItems.filter(
              (item) => item.data.attention.length > 0 &&
                highestAttention(item.data.attention) === "needs_review",
            );
            const informational = sortedItems.filter(
              (item) => item.data.attention.length > 0 &&
                highestAttention(item.data.attention) === "informational",
            );
            const routine = sortedItems.filter(
              (item) => item.data.attention.length === 0 ||
                highestAttention(item.data.attention) === "routine",
            );

            // Determine expansion default
            const hasNeedsReview = needsReview.length > 0;
            const hasInfo = informational.length > 0;
            const defaultExpanded = hasNeedsReview;

            // Info count for collapsed header
            const infoCount = !hasNeedsReview && hasInfo ? informational.length : undefined;

            // Summary text for all-routine repos
            const summaryText =
              !hasNeedsReview && !hasInfo && routine.length > 0
                ? "No action needed"
                : undefined;

            // Disabled repos: show no toggle controls on individual packages
            const isDisabled = !rg.enabled;

            return (
              <RepoGroup
                key={repo}
                repo={rg}
                defaultExpanded={defaultExpanded}
                forceExpanded={filterActive}
                infoCount={infoCount}
                summaryText={summaryText}
                onRepoToggle={handleRepoToggle}
              >
                {/* NeedsReview packages — shown individually */}
                {needsReview.map((item) => {
                  runningRowIndex++;
                  const id = getItemId(item);
                  const flatIdx = flatItemIds.indexOf(id);
                  return (
                    <DecisionItem
                      key={id}
                      item={item}
                      level="needs_review"
                      rowIndex={runningRowIndex}
                      isViewed={viewedIds.has(id)}
                      isPending={mutation.isPending}
                      tabIndex={flatIdx === focusedIndex ? 0 : -1}
                      onToggleInclude={isDisabled ? undefined : handleToggle}
                      onMarkViewed={markAsViewed}
                      onKeyDown={handleRowKeyDown}
                    />
                  );
                })}
                {/* Informational packages — shown individually */}
                {informational.map((item) => {
                  runningRowIndex++;
                  const id = getItemId(item);
                  const flatIdx = flatItemIds.indexOf(id);
                  return (
                    <DecisionItem
                      key={id}
                      item={item}
                      level="informational"
                      rowIndex={runningRowIndex}
                      isViewed={viewedIds.has(id)}
                      isPending={mutation.isPending}
                      tabIndex={flatIdx === focusedIndex ? 0 : -1}
                      onToggleInclude={isDisabled ? undefined : handleToggle}
                      onMarkViewed={markAsViewed}
                      onKeyDown={handleRowKeyDown}
                    />
                  );
                })}
                {/* Routine packages — collapsed summary */}
                {routine.length > 0 && (
                  <RoutineSummary
                    items={routine}
                    forceExpanded={filterActive}
                    revealItemId={revealItemId}
                  />
                )}
              </RepoGroup>
            );
          });
        })()
      ) : (
        // Attention-first grouping for configs (existing behavior)
        (() => {
          let runningRowIndex = 0;
          return levels.map((level) => {
            const groupItems = grouped[level];
            if (groupItems.length === 0) return null;
            const forceExpanded = filterText.trim().length > 0 && groupItems.length > 0;

            if (level === "routine") {
              const baselineItems = groupItems.filter(
                (item) => item.data.attention.length > 0 &&
                  item.data.attention[0].reason === "package_baseline_match",
              );
              const configManagedItems = groupItems.filter(
                (item) => item.data.attention.length > 0 &&
                  CONFIG_MANAGED_REASONS.has(
                    typeof item.data.attention[0].reason === "string"
                      ? item.data.attention[0].reason
                      : "",
                  ),
              );
              const otherRoutine = groupItems.filter(
                (item) => item.data.attention.length === 0 ||
                  (item.data.attention[0].reason !== "package_baseline_match" &&
                    !CONFIG_MANAGED_REASONS.has(
                      typeof item.data.attention[0].reason === "string"
                        ? item.data.attention[0].reason
                        : "",
                    )),
              );

              return (
                <AttentionGroup key={level} level={level} count={groupItems.length} forceExpanded={forceExpanded}>
                  {baselineItems.length > 0 && (
                    <BaselineSummary count={baselineItems.length} items={baselineItems} revealItemId={revealItemId} filterActive={forceExpanded} />
                  )}
                  {configManagedItems.length > 0 && (
                    <ConfigManagedSummary count={configManagedItems.length} items={configManagedItems} revealItemId={revealItemId} filterActive={forceExpanded} />
                  )}
                  {otherRoutine.map((item) => {
                    runningRowIndex++;
                    const id = getItemId(item);
                    const flatIdx = flatItemIds.indexOf(id);
                    return (
                      <DecisionItem
                        key={id}
                        item={item}
                        level={level}
                        rowIndex={runningRowIndex}
                        isViewed={viewedIds.has(id)}
                        isPending={mutation.isPending}
                        tabIndex={flatIdx === focusedIndex ? 0 : -1}
                        onToggleInclude={handleToggle}
                        onMarkViewed={markAsViewed}
                        onKeyDown={handleRowKeyDown}
                      />
                    );
                  })}
                </AttentionGroup>
              );
            }

            return (
              <AttentionGroup key={level} level={level} count={groupItems.length} forceExpanded={forceExpanded}>
                {groupItems.map((item) => {
                  runningRowIndex++;
                  const id = getItemId(item);
                  const flatIdx = flatItemIds.indexOf(id);
                  return (
                    <DecisionItem
                      key={id}
                      item={item}
                      level={level}
                      rowIndex={runningRowIndex}
                      isViewed={viewedIds.has(id)}
                      isPending={mutation.isPending}
                      tabIndex={flatIdx === focusedIndex ? 0 : -1}
                      onToggleInclude={handleToggle}
                      onMarkViewed={markAsViewed}
                      onKeyDown={handleRowKeyDown}
                    />
                  );
                })}
              </AttentionGroup>
            );
          });
        })()
      )}
```

Also update the `flatItemIds` computation to account for repo-first grouping when `repoGroups.length > 0`. The existing logic groups by attention level; the new path needs to group by repo and include only visible items:

```typescript
  const flatItemIds = useMemo(() => {
    const ids: string[] = [];
    if (repoGroups.length > 0) {
      // Repo-first: include needs_review and informational items (routine are in collapsed summary)
      for (const item of items) {
        const level = item.data.attention.length > 0
          ? highestAttention(item.data.attention)
          : "routine";
        if (level !== "routine" || filterText.trim().length > 0) {
          ids.push(getItemId(item));
        }
      }
    } else {
      // Attention-first: existing logic for configs
      for (const level of levels) {
        const groupItems = grouped[level];
        if (level === "routine" && summariesCollapsed) {
          for (const item of groupItems) {
            const reason = item.data.attention.length > 0
              ? item.data.attention[0].reason
              : "";
            const isBaseline = reason === "package_baseline_match";
            const isConfigManaged = CONFIG_MANAGED_REASONS.has(
              typeof reason === "string" ? reason : "",
            );
            if (!isBaseline && !isConfigManaged) {
              ids.push(getItemId(item));
            }
          }
        } else {
          for (const item of groupItems) {
            ids.push(getItemId(item));
          }
        }
      }
    }
    return ids;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [items, summariesCollapsed, repoGroups.length, filterText]);
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui && npx vitest run src/components/__tests__/DecisionSections.test.tsx`
Expected: PASS — new repo-first tests pass, existing config tests still pass

- [ ] **Step 5: Update existing tests that break due to repo-first grouping**

Some existing tests render packages with `repoGroups` (e.g., "Repo group headers" describe block) or without and expect attention-group structure. Review each failing test:

- Tests in "Repo group headers" that render `MainContent` with `activeSection="packages"` and `repo_groups` now get repo-first grouping. Update expectations to look for `repo-group-wrapper-*` instead of `attention-group-*` + `repo-group-*`.
- Tests in "DecisionList" that render without `repoGroups` should still pass (configs path).
- Tests for `MainContent` with `activeSection="packages"` pass `repoGroups` via `viewData.repo_groups` -- these now render repo-first.

Fix each test individually. The key pattern: when `repo_groups` is non-empty in `viewData` and `activeSection` is "packages", the packages section renders repo-first.

- [ ] **Step 6: Run full test suite**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui && npx vitest run src/components/__tests__/DecisionSections.test.tsx`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && git add inspectah-web/ui/src/components/DecisionList.tsx inspectah-web/ui/src/components/__tests__/DecisionSections.test.tsx && git commit -m "feat(web): refactor DecisionList for repo-first package grouping

When repoGroups are provided (packages section), group by source_repo
with attention-level ordering within each group. Repos with needs_review
expanded by default; all-routine repos collapsed with 'No action needed'.
Routine packages within expanded repos collapse to '+ N routine' summary.

Config files section retains attention-first grouping unchanged.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 6: Disabled Repo Behavior

**Files:**
- Modify: `inspectah-web/ui/src/components/DecisionList.tsx`
- Modify: `inspectah-web/ui/src/components/__tests__/DecisionSections.test.tsx`

Per spec: disabled repos move to bottom of list, show dimmed, hide per-package toggles, show "N packages excluded" in header.

- [ ] **Step 1: Write failing tests**

Add to `DecisionSections.test.tsx`:

```typescript
describe("Disabled repo behavior", () => {
  it("disabled repos sort after enabled repos", async () => {
    const repoGroups: RepoGroupInfo[] = [
      { section_id: "epel", provenance: "verified", is_distro: false, package_count: 1, enabled: false },
      { section_id: "baseos", provenance: "verified", is_distro: true, package_count: 1, enabled: true },
    ];

    const items: DecisionItemKind[] = [
      { type: "package", data: makePkg({ name: "epel-pkg", source_repo: "epel", include: false }, [NEEDS_REVIEW_TAG]) },
      { type: "package", data: makePkg({ name: "baseos-pkg", source_repo: "baseos" }, [NEEDS_REVIEW_TAG]) },
    ];

    render(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        repoGroups={repoGroups}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );

    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });

    const wrappers = screen.getAllByTestId(/^repo-group-wrapper-/);
    expect(wrappers[0]).toHaveAttribute("data-testid", "repo-group-wrapper-baseos");
    expect(wrappers[1]).toHaveAttribute("data-testid", "repo-group-wrapper-epel");
  });

  it("disabled repos show 'N packages excluded' in header", async () => {
    const repoGroups: RepoGroupInfo[] = [
      { section_id: "epel", provenance: "verified", is_distro: false, package_count: 3, enabled: false },
    ];

    const items: DecisionItemKind[] = [
      { type: "package", data: makePkg({ name: "pkg1", source_repo: "epel", include: false }, [NEEDS_REVIEW_TAG]) },
    ];

    render(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        repoGroups={repoGroups}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );

    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });

    expect(screen.getByText("3 packages excluded")).toBeInTheDocument();
  });

  it("disabled repos start collapsed", async () => {
    const repoGroups: RepoGroupInfo[] = [
      { section_id: "epel", provenance: "verified", is_distro: false, package_count: 1, enabled: false },
    ];

    const items: DecisionItemKind[] = [
      { type: "package", data: makePkg({ name: "pkg1", source_repo: "epel", include: false }, [NEEDS_REVIEW_TAG]) },
    ];

    render(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        repoGroups={repoGroups}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );

    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });

    // Disabled repos start collapsed — package not visible
    expect(screen.queryByText("pkg1.x86_64")).not.toBeInTheDocument();
  });

  it("hides per-package toggles in disabled repos when expanded", async () => {
    const repoGroups: RepoGroupInfo[] = [
      { section_id: "epel", provenance: "verified", is_distro: false, package_count: 1, enabled: false },
    ];

    const items: DecisionItemKind[] = [
      { type: "package", data: makePkg({ name: "pkg1", source_repo: "epel", include: false }, [NEEDS_REVIEW_TAG]) },
    ];

    render(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        repoGroups={repoGroups}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );

    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });

    // Expand the disabled repo
    const header = screen.getByTestId("repo-group-epel");
    await userEvent.click(header);

    // Package should be visible but without a toggle switch
    expect(screen.getByText("pkg1.x86_64")).toBeInTheDocument();
    expect(screen.queryByRole("switch", { name: /toggle pkg1/i })).not.toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui && npx vitest run src/components/__tests__/DecisionSections.test.tsx --reporter=verbose 2>&1 | grep -A2 "Disabled repo"`
Expected: FAIL

- [ ] **Step 3: Verify disabled repo implementation**

The disabled repo behavior is already wired in Task 5's implementation:
- Sorting: disabled repos have rank 2, after enabled third-party (rank 1)
- Expansion: `defaultExpanded` is `false` for disabled repos (they have no `needs_review`)
- Header text: RepoGroupHeader shows "N packages excluded" when `enabled === false`
- Per-package toggles: `onToggleInclude={isDisabled ? undefined : handleToggle}` hides toggle

If tests pass without changes, this task is a verification step. If any test fails, adjust the implementation in DecisionList.tsx accordingly.

The `onToggleInclude` prop on `DecisionItem` needs to handle `undefined` by hiding the switch. Check `DecisionItem.tsx` — if the Switch always renders regardless of `onToggleInclude`, we need to conditionally hide it. The current code:

```typescript
// In DecisionItem.tsx, the Switch renders unconditionally.
// It needs: {onToggleInclude && <Switch ... />}
```

If the toggle still renders, update `DecisionItem.tsx` to conditionally render the Switch:

```typescript
{onToggleInclude && (
  <Switch
    id={`toggle-${displayName}`}
    // ... existing props
  />
)}
```

This is a minimal, spec-required change — disabled repo packages are "read-only list" with toggles "hidden (not disabled/grayed -- hidden entirely)."

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui && npx vitest run src/components/__tests__/DecisionSections.test.tsx`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && git add inspectah-web/ui/src/components/DecisionList.tsx inspectah-web/ui/src/components/DecisionItem.tsx inspectah-web/ui/src/components/__tests__/DecisionSections.test.tsx && git commit -m "feat(web): disabled repo behavior

Disabled repos sort to bottom, start collapsed, show 'N packages excluded'.
Per-package toggles hidden (not grayed) when repo is disabled.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 7: Wire AttentionSummary into MainContent

**Files:**
- Modify: `inspectah-web/ui/src/components/MainContent.tsx`
- Modify: `inspectah-web/ui/src/components/__tests__/DecisionSections.test.tsx`

Per spec: attention summary counter appears at top of Packages section, between the baseline banner and the DecisionList.

- [ ] **Step 1: Write failing tests**

Add to `DecisionSections.test.tsx`:

```typescript
describe("AttentionSummary in MainContent", () => {
  it("shows attention summary on packages section", () => {
    const view = makeViewResponse({
      packages: [
        makePkg({ name: "httpd", source_repo: "epel" }, [NEEDS_REVIEW_TAG]),
        makePkg({ name: "glibc", source_repo: "baseos" }, [ROUTINE_TAG]),
      ],
      repo_groups: [
        { section_id: "epel", provenance: "verified", is_distro: false, package_count: 1, enabled: true },
        { section_id: "baseos", provenance: "verified", is_distro: true, package_count: 1, enabled: true },
      ],
    });
    render(<MainContent {...defaultMainContentProps} viewData={view} />);
    expect(screen.getByTestId("attention-summary")).toBeInTheDocument();
    expect(screen.getByText("1 package needs review across 1 repo")).toBeInTheDocument();
  });

  it("shows all-clear when no review items", () => {
    const view = makeViewResponse({
      packages: [
        makePkg({ name: "glibc", source_repo: "baseos" }, [ROUTINE_TAG]),
      ],
      repo_groups: [
        { section_id: "baseos", provenance: "verified", is_distro: true, package_count: 1, enabled: true },
      ],
    });
    render(<MainContent {...defaultMainContentProps} viewData={view} />);
    expect(screen.getByText("All actionable items reviewed")).toBeInTheDocument();
  });

  it("does not show attention summary on configs section", () => {
    const view = makeViewResponse({
      config_files: [
        makeConfig({}, [NEEDS_REVIEW_TAG]),
      ],
    });
    render(<MainContent {...defaultMainContentProps} activeSection="configs" viewData={view} />);
    expect(screen.queryByTestId("attention-summary")).not.toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui && npx vitest run src/components/__tests__/DecisionSections.test.tsx --reporter=verbose 2>&1 | grep -A2 "AttentionSummary in"`
Expected: FAIL — AttentionSummary not rendered in MainContent

- [ ] **Step 3: Add AttentionSummary to MainContent**

In `MainContent.tsx`, add the import:

```typescript
import { AttentionSummary } from "./AttentionSummary";
import { highestAttention } from "./attentionUtils";
```

In the `activeSection === "packages"` branch, after the `banner` variable and before the `SectionSearch`, compute the attention counts and render the summary:

```typescript
    // Compute attention summary counts
    const needsReviewPkgs = packageItems.filter(
      (item) => item.data.attention.length > 0 &&
        highestAttention(item.data.attention) === "needs_review",
    );
    const infoPkgs = packageItems.filter(
      (item) => item.data.attention.length > 0 &&
        highestAttention(item.data.attention) === "informational",
    );
    const needsReviewRepos = new Set(
      needsReviewPkgs
        .filter((item) => item.type === "package")
        .map((item) => (item.data as any).entry.source_repo),
    );
    const infoRepos = new Set(
      infoPkgs
        .filter((item) => item.type === "package")
        .map((item) => (item.data as any).entry.source_repo),
    );
```

Then in the JSX, between `{banner}` and the `SectionSearch`:

```typescript
        <AttentionSummary
          needsReviewCount={needsReviewPkgs.length}
          needsReviewRepoCount={needsReviewRepos.size}
          infoCount={infoPkgs.length}
          infoRepoCount={infoRepos.size}
        />
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui && npx vitest run src/components/__tests__/DecisionSections.test.tsx`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && git add inspectah-web/ui/src/components/MainContent.tsx inspectah-web/ui/src/components/__tests__/DecisionSections.test.tsx && git commit -m "feat(web): wire AttentionSummary into Packages section

Cross-repo attention counter shown between baseline banner and package
list. Computes needs_review and informational counts from package items.
Not shown on config files section.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 8: Keyboard and Accessibility

**Files:**
- Modify: `inspectah-web/ui/src/components/DecisionList.tsx`
- Modify: `inspectah-web/ui/src/components/__tests__/DecisionSections.test.tsx`

Per spec: repo headers participate in roving tabindex. Up/Down arrows move between repo headers and visible package rows in flat sequence. Enter toggles expand/collapse. Tab from repo header lands on enable/disable switch if present.

- [ ] **Step 1: Write failing tests**

Add to `DecisionSections.test.tsx`:

```typescript
describe("Repo-first keyboard navigation", () => {
  const REPO_GROUPS: RepoGroupInfo[] = [
    { section_id: "baseos", provenance: "verified", is_distro: true, package_count: 1, enabled: true },
    { section_id: "epel", provenance: "verified", is_distro: false, package_count: 1, enabled: true },
  ];

  it("ArrowDown from repo header moves to first visible package row", async () => {
    const items: DecisionItemKind[] = [
      { type: "package", data: makePkg({ name: "glibc", source_repo: "baseos" }, [NEEDS_REVIEW_TAG]) },
      { type: "package", data: makePkg({ name: "epel-release", source_repo: "epel" }, [NEEDS_REVIEW_TAG]) },
    ];

    render(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        repoGroups={REPO_GROUPS}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );

    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });

    // Focus on a package row, then ArrowDown should move to next
    const rows = screen.getAllByRole("row");
    rows[0].focus();
    await userEvent.keyboard("{ArrowDown}");
    expect(rows[1]).toHaveAttribute("tabindex", "0");
  });

  it("skips collapsed repo groups when navigating with arrows", async () => {
    const repoGroups: RepoGroupInfo[] = [
      { section_id: "baseos", provenance: "verified", is_distro: true, package_count: 1, enabled: true },
      { section_id: "epel", provenance: "verified", is_distro: false, package_count: 1, enabled: true },
    ];

    const items: DecisionItemKind[] = [
      // baseos has needs_review (expanded), epel is all-routine (collapsed)
      { type: "package", data: makePkg({ name: "glibc", source_repo: "baseos" }, [NEEDS_REVIEW_TAG]) },
      { type: "package", data: makePkg({ name: "htop", source_repo: "epel" }, [ROUTINE_TAG]) },
    ];

    render(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        repoGroups={repoGroups}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );

    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });

    // Only one row should be in the roving tabindex (glibc is needs_review)
    const rows = screen.getAllByRole("row");
    expect(rows).toHaveLength(1);
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui && npx vitest run src/components/__tests__/DecisionSections.test.tsx --reporter=verbose 2>&1 | grep -A2 "Repo-first keyboard"`
Expected: FAIL

- [ ] **Step 3: Implement keyboard navigation updates**

The `flatItemIds` computation from Task 5 already excludes routine items from the roving tabindex in repo-first mode. The existing `handleRowKeyDown` with ArrowUp/ArrowDown/j/k navigation works on `flatItemIds`. Verify the implementation handles the repo-first case correctly.

If tests fail, the likely issue is that `flatItemIds` includes routine items or doesn't properly account for collapsed repos. The Task 5 implementation already handles this:

```typescript
if (repoGroups.length > 0) {
  for (const item of items) {
    const level = item.data.attention.length > 0
      ? highestAttention(item.data.attention)
      : "routine";
    if (level !== "routine" || filterText.trim().length > 0) {
      ids.push(getItemId(item));
    }
  }
}
```

This ensures routine items in collapsed summaries are excluded from keyboard navigation. If more granular control is needed (e.g., repo headers in the focus sequence), that requires adding repo header IDs to `flatItemIds` and handling focus on `div[role="button"]` elements in addition to `[role="row"]` elements. Implement if tests require it.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui && npx vitest run src/components/__tests__/DecisionSections.test.tsx`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && git add inspectah-web/ui/src/components/DecisionList.tsx inspectah-web/ui/src/components/__tests__/DecisionSections.test.tsx && git commit -m "feat(web): keyboard navigation for repo-first grouping

Roving tabindex includes needs_review and informational package rows.
Routine packages in collapsed summaries excluded from arrow-key navigation.
Collapsed repo groups skipped entirely.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 9: Filter and Reveal Integration

**Files:**
- Modify: `inspectah-web/ui/src/components/DecisionList.tsx`
- Modify: `inspectah-web/ui/src/components/__tests__/DecisionSections.test.tsx`

Per spec: when section search filter is active, auto-expand matching repo groups and routine summaries. When `revealItemId` targets a package in a collapsed repo, auto-expand that repo.

- [ ] **Step 1: Write failing tests**

Add to `DecisionSections.test.tsx`:

```typescript
describe("Repo-first filter and reveal", () => {
  it("auto-expands collapsed repo groups when filter is active", async () => {
    const items: DecisionItemKind[] = [
      { type: "package", data: makePkg({ name: "htop", source_repo: "epel" }, [ROUTINE_TAG]) },
    ];

    const { rerender } = render(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        repoGroups={[{ section_id: "epel", provenance: "verified", is_distro: false, package_count: 1, enabled: true }]}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );

    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });

    // All-routine repo starts collapsed
    expect(screen.queryByText("htop.x86_64")).not.toBeInTheDocument();

    // Filter activates — repo should force-expand
    rerender(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        filterText="htop"
        repoGroups={[{ section_id: "epel", provenance: "verified", is_distro: false, package_count: 1, enabled: true }]}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );

    // Routine summary also force-expands, so individual package is visible
    expect(screen.getByText("htop.x86_64")).toBeInTheDocument();
  });

  it("auto-expands disabled repos when filter matches their packages", async () => {
    const items: DecisionItemKind[] = [
      { type: "package", data: makePkg({ name: "htop", source_repo: "epel", include: false }, [NEEDS_REVIEW_TAG]) },
    ];

    const { rerender } = render(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        repoGroups={[{ section_id: "epel", provenance: "verified", is_distro: false, package_count: 1, enabled: false }]}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );

    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });

    // Disabled repo starts collapsed
    expect(screen.queryByText("htop.x86_64")).not.toBeInTheDocument();

    // Filter matches — disabled repo should force-expand
    rerender(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        filterText="htop"
        repoGroups={[{ section_id: "epel", provenance: "verified", is_distro: false, package_count: 1, enabled: false }]}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );

    expect(screen.getByText("htop.x86_64")).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui && npx vitest run src/components/__tests__/DecisionSections.test.tsx --reporter=verbose 2>&1 | grep -A2 "Repo-first filter"`
Expected: FAIL (or PASS if Task 5's `forceExpanded={filterActive}` already handles this)

- [ ] **Step 3: Verify filter integration**

The Task 5 implementation passes `forceExpanded={filterActive}` to `RepoGroup`, which overrides the collapsed state. The `RoutineSummary` also receives `forceExpanded={filterActive}`. This should be sufficient for filter-driven expansion.

For `revealItemId`, the `RoutineSummary` already handles auto-expansion via its `useEffect`. For repo-level auto-expansion, we need the `RepoGroup` to auto-expand when `revealItemId` targets one of its items. This requires passing `revealItemId` through `RepoGroup` and adding a `useEffect` similar to `RoutineSummary`'s.

Update `RepoGroup.tsx` to accept `revealItemId` and auto-expand:

```typescript
// Add to RepoGroupProps:
/** When set, auto-expands if this item ID belongs to this group */
revealItemId?: string;
/** Item IDs in this group, for revealItemId matching */
itemIds?: string[];
```

Add useEffect in RepoGroup:

```typescript
// Auto-expand when revealItemId matches an item in this group
useEffect(() => {
  if (!revealItemId || !itemIds) return;
  if (itemIds.includes(revealItemId) && !isExpanded) {
    setIsExpanded(true);
  }
}, [revealItemId, itemIds, isExpanded]);
```

Pass `revealItemId` and `itemIds` from DecisionList when rendering each RepoGroup.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui && npx vitest run src/components/__tests__/DecisionSections.test.tsx`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && git add inspectah-web/ui/src/components/DecisionList.tsx inspectah-web/ui/src/components/RepoGroup.tsx inspectah-web/ui/src/components/__tests__/DecisionSections.test.tsx && git commit -m "feat(web): filter and reveal integration for repo-first view

Search filter force-expands matching repo groups and routine summaries.
RevealItemId auto-expands the repo group containing the target package.
Disabled repos also force-expand when filter matches their packages.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 10: Existing Config Section Preservation Verification

**Files:**
- Modify: `inspectah-web/ui/src/components/__tests__/DecisionSections.test.tsx` (add verification tests)

Per spec: "What This Keeps — Config files section behavior unchanged." This task verifies no regressions in config section behavior.

- [ ] **Step 1: Write verification tests**

Add to `DecisionSections.test.tsx`:

```typescript
describe("Config section unchanged after repo-first refactor", () => {
  it("config section still uses attention-level grouping", () => {
    const view = makeViewResponse({
      config_files: [
        makeConfig({ path: "/etc/review.conf" }, [{ level: "needs_review", reason: "config_modified", detail: null }]),
        makeConfig({ path: "/etc/info.conf" }, [{ level: "informational", reason: "config_unowned", detail: null }]),
        makeConfig({ path: "/etc/routine.conf" }, [{ level: "routine", reason: "config_default", detail: null }]),
      ],
    });
    render(<MainContent {...defaultMainContentProps} activeSection="configs" viewData={view} />);

    // Config section should still render AttentionGroup, not RepoGroup
    expect(screen.getByTestId("attention-group-needs_review")).toBeInTheDocument();
    expect(screen.queryByTestId(/^repo-group-wrapper-/)).not.toBeInTheDocument();
  });

  it("config section does not show attention summary", () => {
    const view = makeViewResponse({
      config_files: [
        makeConfig({ path: "/etc/test.conf" }, [{ level: "needs_review", reason: "config_modified", detail: null }]),
      ],
    });
    render(<MainContent {...defaultMainContentProps} activeSection="configs" viewData={view} />);
    expect(screen.queryByTestId("attention-summary")).not.toBeInTheDocument();
  });

  it("config section Tier 1 summary still works", () => {
    const view = makeViewResponse({
      config_files: [
        makeConfig({ path: "/etc/default.conf", kind: "rpm_owned_default" },
          [{ level: "routine", reason: "config_default", detail: null }]),
      ],
    });
    render(<MainContent {...defaultMainContentProps} activeSection="configs" viewData={view} />);
    expect(screen.getByText(/managed by packages/i)).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run verification tests**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui && npx vitest run src/components/__tests__/DecisionSections.test.tsx`
Expected: PASS — all config section tests pass without changes

- [ ] **Step 3: Commit verification tests**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && git add inspectah-web/ui/src/components/__tests__/DecisionSections.test.tsx && git commit -m "test(web): verify config section unchanged after repo-first refactor

Regression tests confirming config files section retains attention-level
grouping, Tier 1 summaries, and no AttentionSummary counter.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 11: Full Integration Test Run and Cleanup

**Files:**
- All modified files from Tasks 1-10

- [ ] **Step 1: Run full test suite**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui && npx vitest run`
Expected: PASS — all test files pass

- [ ] **Step 2: Run type check**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui && npx tsc --noEmit`
Expected: No type errors

- [ ] **Step 3: Remove dead imports from DecisionList**

After the refactor, the `ExpandableSection` import from `@patternfly/react-core` in `AttentionGroup.tsx` might no longer be used (it was already replaced with a custom button). Check and clean up any unused imports in modified files. Do not remove `AttentionGroup` itself — it is still used by the configs section.

- [ ] **Step 4: Clean up old informational-tier repo grouping code**

The informational tier's repo sub-grouping in the old DecisionList code (the `if (level === "informational" && repoGroups.length > 0)` block) should now be dead code in the attention-first branch since that branch only runs when `repoGroups.length === 0`. Remove it from the attention-first branch if present.

- [ ] **Step 5: Run tests one final time**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui && npx vitest run`
Expected: PASS

- [ ] **Step 6: Commit cleanup**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && git add -u inspectah-web/ui/src/ && git commit -m "refactor(web): remove dead code after repo-first migration

Remove informational-tier repo sub-grouping from attention-first branch
(now unreachable for packages). Clean up unused imports.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Self-Review

### Spec Coverage Checklist

| Spec Section | Task |
|---|---|
| Package Organization (repo-first grouping) | Task 5 |
| Repo Group Display Rules (expansion defaults) | Task 5 |
| Attention Summary Counter (3 text states) | Task 2, Task 7 |
| Repo Headers (chevron, classification labels) | Task 1 |
| Source Classification (distro = no label, third-party) | Task 1 |
| Repo Header Ordering (distro, enabled 3P, disabled, unknown) | Task 5, Task 6 |
| Repo Toggle Eligibility (non-distro verified only) | Task 1 (unchanged from existing) |
| Repo Enable/Disable Behavior | Task 6 |
| Disabled Repo State (dimmed, struck-through, hidden toggles) | Task 1, Task 6 |
| Per-Package Actions (unchanged) | Verified in Task 6 |
| Keyboard and Accessibility | Task 8 |
| Layout (CSS updates) | Task 1 |
| What This Replaces (attention-tier view removed for packages) | Task 5 |
| What This Keeps (configs unchanged, AttentionGroup kept) | Task 10 |
| Data Flow (frontend-only, no API changes) | All tasks |
| Reveal Behavior (search filter, programmatic navigation) | Task 9 |
| Routine summary ("+ N routine") | Task 4 |
| Testing: Repo Group Rendering | Task 5 |
| Testing: Expansion Defaults | Task 5 |
| Testing: Repo Toggle | Task 6 |
| Testing: Disabled Repo Behavior | Task 6 |
| Testing: Disabled Repo Counts | Task 6 |
| Testing: Attention Summary | Task 2 |
| Testing: Reveal Behavior | Task 9 |
| Testing: Keyboard and Accessibility | Task 8 |
| Testing: Existing Behavior Preserved | Task 10 |

### Placeholder Scan

No instances of: TBD, TODO, "similar to Task N", "implement later", "add appropriate", "handle edge cases" as placeholders.

### Type Consistency

- `RepoGroupHeaderProps`: extended with `isExpanded`, `infoCount`, `summaryText`, `onExpandToggle`, `onKeyDown` -- used consistently in RepoGroup and DecisionList
- `RepoGroupProps`: uses `RepoGroupInfo` from `api/types.ts` -- matches existing type
- `AttentionSummaryProps`: `needsReviewCount`, `needsReviewRepoCount`, `infoCount`, `infoRepoCount` -- computed in MainContent from `packageItems`
- `RoutineSummaryProps`: uses `DecisionItemKind[]` -- matches existing pattern from BaselineSummary/ConfigManagedSummary
- `DecisionItemProps.onToggleInclude`: changed to optional (`undefined` hides toggle) -- checked in Task 6
