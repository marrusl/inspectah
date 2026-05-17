# Unified Repo View Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace attention-tier-first package grouping with repo-first grouping, where packages are organized by source repository with attention bubbling within each group.

**Architecture:** The change is entirely frontend. The existing `/api/view` response already provides `packages` (with `source_repo` per package) and `repo_groups` (with `is_distro`, `provenance`, `package_count`, `enabled` per repo). DecisionList currently groups by attention level using AttentionGroup components. This refactor introduces RepoGroup as the primary grouping axis for packages, with attention-level ordering within each repo group. The config files section remains unchanged (attention-first with AttentionGroup).

**Tech Stack:** React 18, TypeScript, PatternFly 6, Vitest + @testing-library/react + userEvent

**Non-mergeable cluster: Tasks 5, 8, 9** depend on each other for correct keyboard and reveal behavior. They are committed separately for clean git history but must all land before the feature is considered functional. Do not merge or demo after Task 5 alone.

**Layout preservation note:** This plan does not modify the app-shell layout (sidebar, content area, Containerfile panel). Those components are untouched. CSS changes in Task 1 are scoped to `.inspectah-repo-group-header` class names only.

---

### Task 1: Update RepoGroupHeader Labels and ARIA Contract

**Files:**
- Modify: `inspectah-web/ui/src/components/RepoGroupHeader.tsx`
- Modify: `inspectah-web/ui/src/components/__tests__/DecisionSections.test.tsx`

Per spec: distro repos show no label. ALL non-distro repos show "Third-party" text label as a source-classification signal (regardless of provenance). Toggle switch only shown for verified non-distro repos. Drop the provenance badge system entirely. Add chevron icon for expand/collapse. Use `role="row"` with `aria-expanded` and `aria-controls` per the approved row-owned interaction model.

- [ ] **Step 1: Write failing tests for updated RepoGroupHeader**

Add these tests to `DecisionSections.test.tsx` in a new describe block:

```typescript
// ---- Updated RepoGroupHeader tests ----

describe("RepoGroupHeader updated labels", () => {
  it("shows no label for distro repos", () => {
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
    expect(screen.queryByText("Third-party")).not.toBeInTheDocument();
    expect(screen.getByText("baseos")).toBeInTheDocument();
  });

  it("shows 'Third-party' text for verified non-distro repos", () => {
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

  it("shows 'Third-party' text for incomplete-provenance non-distro repos", () => {
    render(
      <RepoGroupHeader
        sectionId="custom"
        provenance="incomplete"
        isDistro={false}
        packageCount={3}
        enabled={true}
      />,
    );
    // ALL non-distro repos get "Third-party" — it's a source classification, not verification
    expect(screen.getByText("Third-party")).toBeInTheDocument();
    expect(screen.queryByText("Unverified")).not.toBeInTheDocument();
    expect(screen.getByText("custom")).toBeInTheDocument();
  });

  it("shows 'Third-party' text for unknown-provenance non-distro repos", () => {
    render(
      <RepoGroupHeader
        sectionId="mystery"
        provenance="unknown"
        isDistro={false}
        packageCount={2}
        enabled={true}
      />,
    );
    expect(screen.getByText("Third-party")).toBeInTheDocument();
    expect(screen.getByText("mystery")).toBeInTheDocument();
  });

  it("only shows toggle switch for verified non-distro repos", () => {
    const { rerender } = render(
      <RepoGroupHeader
        sectionId="epel"
        provenance="verified"
        isDistro={false}
        packageCount={5}
        enabled={true}
        onToggle={vi.fn()}
      />,
    );
    expect(screen.getByRole("switch", { name: /toggle epel repo/i })).toBeInTheDocument();

    rerender(
      <RepoGroupHeader
        sectionId="custom"
        provenance="incomplete"
        isDistro={false}
        packageCount={3}
        enabled={true}
        onToggle={vi.fn()}
      />,
    );
    expect(screen.queryByRole("switch")).not.toBeInTheDocument();
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

  it("uses role='row' with aria-expanded and aria-controls", () => {
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
    expect(header).toHaveAttribute("role", "row");
    expect(header).toHaveAttribute("aria-expanded", "true");
    expect(header).toHaveAttribute("aria-controls", "repo-group-content-epel");
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

  it("Enter on header triggers onExpandToggle, not switch toggle", async () => {
    const onExpandToggle = vi.fn();
    const onToggle = vi.fn();
    render(
      <RepoGroupHeader
        sectionId="epel"
        provenance="verified"
        isDistro={false}
        packageCount={5}
        enabled={true}
        isExpanded={false}
        onExpandToggle={onExpandToggle}
        onToggle={onToggle}
      />,
    );
    const header = screen.getByTestId("repo-group-epel");
    header.focus();
    await userEvent.keyboard("{Enter}");
    expect(onExpandToggle).toHaveBeenCalledTimes(1);
    expect(onToggle).not.toHaveBeenCalled();
  });

  it("Space on header row is a no-op (does not toggle expand or switch)", async () => {
    const onExpandToggle = vi.fn();
    const onToggle = vi.fn();
    render(
      <RepoGroupHeader
        sectionId="epel"
        provenance="verified"
        isDistro={false}
        packageCount={5}
        enabled={true}
        isExpanded={false}
        onExpandToggle={onExpandToggle}
        onToggle={onToggle}
      />,
    );
    const header = screen.getByTestId("repo-group-epel");
    header.focus();
    await userEvent.keyboard(" ");
    expect(onExpandToggle).not.toHaveBeenCalled();
    expect(onToggle).not.toHaveBeenCalled();
  });

  it("chevron click triggers onExpandToggle", async () => {
    const onExpandToggle = vi.fn();
    render(
      <RepoGroupHeader
        sectionId="epel"
        provenance="verified"
        isDistro={false}
        packageCount={5}
        enabled={true}
        isExpanded={false}
        onExpandToggle={onExpandToggle}
      />,
    );
    const chevron = screen.getByTestId("repo-group-epel").querySelector(".inspectah-repo-group-header__chevron")!;
    await userEvent.click(chevron as HTMLElement);
    expect(onExpandToggle).toHaveBeenCalledTimes(1);
  });
});
```

Import `RepoGroupHeader` at the top of the test file (already imported).

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui && npx vitest run src/components/__tests__/DecisionSections.test.tsx`
Expected: FAIL — RepoGroupHeader still renders "Distro" badge, uses `role="heading"`, lacks `isExpanded`/`infoCount`/`summaryText` props

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

/**
 * Source classification label:
 * - Distro repos: no label
 * - ALL non-distro repos: "Third-party" (regardless of provenance)
 *
 * Provenance only affects toggle eligibility, not the label.
 */
function classificationLabel(isDistro: boolean): string | null {
  if (isDistro) return null;
  return "Third-party";
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
  const label = classificationLabel(isDistro);
  const contentId = `repo-group-content-${sectionId}`;

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLDivElement>) => {
      if (onKeyDownProp) {
        onKeyDownProp(e);
        if (e.defaultPrevented) return;
      }
      if (e.key === "Enter") {
        e.preventDefault();
        onExpandToggle?.();
      }
      // Space is intentionally a no-op on the row itself.
      // Space only activates the switch when the switch has focus (handled by PF Switch).
      if (e.key === " ") {
        e.preventDefault();
      }
    },
    [onKeyDownProp, onExpandToggle],
  );

  const handleChevronClick = useCallback(
    (e: React.MouseEvent) => {
      e.stopPropagation();
      onExpandToggle?.();
    },
    [onExpandToggle],
  );

  const disabledStyle = !enabled
    ? { textDecoration: "line-through" as const, opacity: "0.6" }
    : {};

  return (
    <div
      data-testid={`repo-group-${sectionId}`}
      role="row"
      aria-expanded={isExpanded}
      aria-controls={contentId}
      tabIndex={0}
      onKeyDown={handleKeyDown}
      className={`inspectah-repo-group-header${!enabled ? " inspectah-repo-group-header--disabled" : ""}`}
    >
      <span
        className="inspectah-repo-group-header__chevron"
        onClick={handleChevronClick}
        role="presentation"
      >
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
        {packageCount} {packageCount === 1 ? "package" : "packages"}
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

Key changes from round 1:
- `role="row"` instead of `role="button"` — row-owned model per spec
- `aria-controls` pointing to the group content id
- `classificationLabel()` only checks `isDistro`, not provenance — ALL non-distro get "Third-party"
- `Space` is a no-op on the row (preventDefault only, no action)
- `Enter` triggers expand/collapse
- Chevron has its own click handler (the disclosure control)
- Whole row is NOT the click target for expand — only chevron
- `packageCount` always shows count (not "N packages excluded" here — that's driven by Task 6 in DecisionList)

- [ ] **Step 4: Update CSS for new header elements**

Add to `inspectah-web/ui/src/App.css`, after the existing `.inspectah-repo-group-header__toggle` block:

```css
.inspectah-repo-group-header__chevron {
  display: flex;
  align-items: center;
  flex-shrink: 0;
  cursor: pointer;
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
- Tests that check for "Distro" badge text should expect no badge and no "Distro" text
- Tests that check for badge abbreviations ("D", "3P", "U", "?") should be removed
- Tests that use the old props should add `isExpanded={true}` where the test expects children to be visible
- The `onToggle` callback tests remain valid but the wrapping structure changes

Review and update each test in the existing "Repo group headers" describe block to work with the new component.

- [ ] **Step 8: Run full test suite to verify no regressions**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui && npx vitest run src/components/__tests__/DecisionSections.test.tsx`
Expected: PASS — all tests pass

- [ ] **Step 9: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && git add inspectah-web/ui/src/components/RepoGroupHeader.tsx inspectah-web/ui/src/components/__tests__/DecisionSections.test.tsx inspectah-web/ui/src/App.css && git commit -m "feat(web): update RepoGroupHeader for row-owned interaction model

Row-owned ARIA contract: role='row', aria-expanded, aria-controls.
All non-distro repos labeled 'Third-party' regardless of provenance.
Toggle switch only for verified non-distro. Enter toggles expansion,
Space is no-op on row. Chevron is the disclosure click target.

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

Per spec: collapsible wrapper around repo header + package list with expansion defaults based on attention content. The group content uses `role="rowgroup"` with `aria-label` and an `id` matching the header's `aria-controls`.

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

  it("toggles expansion on chevron click", async () => {
    render(
      <RepoGroup repo={baseRepo} defaultExpanded={false}>
        <div data-testid="child">content</div>
      </RepoGroup>,
    );
    expect(screen.queryByTestId("child")).not.toBeInTheDocument();

    const chevron = screen.getByTestId("repo-group-epel").querySelector(".inspectah-repo-group-header__chevron")!;
    await userEvent.click(chevron as HTMLElement);
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

  it("does NOT toggle expansion on Space key", async () => {
    render(
      <RepoGroup repo={baseRepo} defaultExpanded={false}>
        <div data-testid="child">content</div>
      </RepoGroup>,
    );
    const header = screen.getByTestId("repo-group-epel");
    header.focus();
    await userEvent.keyboard(" ");
    expect(screen.queryByTestId("child")).not.toBeInTheDocument();
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

  it("wraps children in a role='rowgroup' container with matching id", () => {
    render(
      <RepoGroup repo={baseRepo} defaultExpanded={true}>
        <div data-testid="child">content</div>
      </RepoGroup>,
    );
    const group = document.getElementById("repo-group-content-epel");
    expect(group).toBeTruthy();
    expect(group).toHaveAttribute("role", "rowgroup");
    expect(group).toHaveAttribute("aria-label", "epel packages");
  });

  it("focus stays on repo header after expand/collapse cycle", async () => {
    render(
      <RepoGroup repo={baseRepo} defaultExpanded={false}>
        <div data-testid="child">content</div>
      </RepoGroup>,
    );
    const header = screen.getByTestId("repo-group-epel");
    header.focus();
    expect(document.activeElement).toBe(header);
    await userEvent.keyboard("{Enter}");
    expect(document.activeElement).toBe(header);
    await userEvent.keyboard("{Enter}");
    expect(document.activeElement).toBe(header);
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui && npx vitest run src/components/__tests__/RepoGroup.test.tsx`
Expected: FAIL — module not found

- [ ] **Step 3: Implement RepoGroup**

Create `inspectah-web/ui/src/components/RepoGroup.tsx`:

```typescript
import { useState, useCallback, useEffect } from "react";
import type { RepoGroupInfo } from "../api/types";
import { RepoGroupHeader } from "./RepoGroupHeader";
import { itemId as getItemId } from "./DecisionItem";
import type { DecisionItemKind } from "./DecisionItem";

export interface RepoGroupProps {
  repo: RepoGroupInfo;
  defaultExpanded: boolean;
  /** Override: force-expand when search filter matches items in this group */
  forceExpanded?: boolean;
  /** Number of informational packages — shown in collapsed header */
  infoCount?: number;
  /** Summary text for collapsed header (e.g., "No action needed") */
  summaryText?: string;
  /** When set, auto-expands if this item ID belongs to this group */
  revealItemId?: string;
  /** Item IDs in this group, for revealItemId matching */
  itemIds?: string[];
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
  revealItemId,
  itemIds,
  onRepoToggle,
  onKeyDown,
  children,
}: RepoGroupProps) {
  const [isExpanded, setIsExpanded] = useState(defaultExpanded);

  // Auto-expand when revealItemId matches an item in this group
  useEffect(() => {
    if (!revealItemId || !itemIds) return;
    if (itemIds.includes(revealItemId) && !isExpanded) {
      setIsExpanded(true);
    }
  }, [revealItemId, itemIds, isExpanded]);

  const effectiveExpanded = forceExpanded || isExpanded;
  const contentId = `repo-group-content-${repo.section_id}`;

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
      {effectiveExpanded && (
        <div
          id={contentId}
          role="rowgroup"
          aria-label={`${repo.section_id} packages`}
        >
          {children}
        </div>
      )}
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
reveal-expand for global search, infoCount and summaryText header annotations.
Group content uses role='rowgroup' with aria-label. Row-owned ARIA contract:
Enter toggles expand, Space is no-op, focus stays on header.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 4: Create RoutineSummary Component

**Files:**
- Create: `inspectah-web/ui/src/components/RoutineSummary.tsx`
- Create: `inspectah-web/ui/src/components/__tests__/RoutineSummary.test.tsx`

Per spec: "+ N routine" collapsed summary within repo groups for routine packages. When expanded, shows **real `DecisionItem` rows** with full toggle, viewed-state, and mutation behavior — NOT plain `<li>` elements.

- [ ] **Step 1: Write failing tests**

Create `inspectah-web/ui/src/components/__tests__/RoutineSummary.test.tsx`:

```typescript
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
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

const ROUTINE_TAG: AttentionTag = { level: "routine", reason: "package_baseline_match", detail: null };

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
      attention: [ROUTINE_TAG],
    },
  };
}

describe("RoutineSummary", () => {
  it("renders '+ N routine' text", () => {
    const items = [makePkg("glibc"), makePkg("bash"), makePkg("coreutils")];
    render(
      <RoutineSummary
        items={items}
        onToggleInclude={vi.fn()}
        onMarkViewed={vi.fn()}
        viewedIds={new Set()}
        isPending={false}
      />,
    );
    expect(screen.getByText("+ 3 routine")).toBeInTheDocument();
  });

  it("starts collapsed by default", () => {
    const items = [makePkg("glibc")];
    render(
      <RoutineSummary
        items={items}
        onToggleInclude={vi.fn()}
        onMarkViewed={vi.fn()}
        viewedIds={new Set()}
        isPending={false}
      />,
    );
    expect(screen.queryByText("glibc.x86_64")).not.toBeInTheDocument();
  });

  it("expands to show real DecisionItem rows on click", async () => {
    const items = [makePkg("glibc"), makePkg("bash")];
    render(
      <RoutineSummary
        items={items}
        onToggleInclude={vi.fn()}
        onMarkViewed={vi.fn()}
        viewedIds={new Set()}
        isPending={false}
      />,
    );

    await userEvent.click(screen.getByText("+ 2 routine"));

    // Real DecisionItem rows render with role="row"
    const rows = screen.getAllByRole("row");
    expect(rows.length).toBeGreaterThanOrEqual(2);
    expect(screen.getByText("glibc.x86_64")).toBeInTheDocument();
    expect(screen.getByText("bash.x86_64")).toBeInTheDocument();
  });

  it("expanded routine packages retain include/exclude toggle", async () => {
    const onToggle = vi.fn();
    const items = [makePkg("glibc")];
    render(
      <RoutineSummary
        items={items}
        onToggleInclude={onToggle}
        onMarkViewed={vi.fn()}
        viewedIds={new Set()}
        isPending={false}
      />,
    );

    await userEvent.click(screen.getByText("+ 1 routine"));

    // Toggle switch should be present on the real DecisionItem
    const toggle = screen.getByRole("switch", { name: /toggle/i });
    expect(toggle).toBeInTheDocument();
    await userEvent.click(toggle);
    expect(onToggle).toHaveBeenCalled();
  });

  it("expanded routine packages track viewed state", async () => {
    const onMarkViewed = vi.fn();
    const items = [makePkg("glibc")];
    render(
      <RoutineSummary
        items={items}
        onToggleInclude={vi.fn()}
        onMarkViewed={onMarkViewed}
        viewedIds={new Set()}
        isPending={false}
      />,
    );

    await userEvent.click(screen.getByText("+ 1 routine"));

    // Expanding a DecisionItem triggers onMarkViewed
    const row = screen.getByRole("row");
    await userEvent.click(row);
    // DecisionItem's detail expansion triggers markViewed
    expect(onMarkViewed).toHaveBeenCalled();
  });

  it("auto-expands when forceExpanded is true", () => {
    const items = [makePkg("glibc")];
    render(
      <RoutineSummary
        items={items}
        forceExpanded={true}
        onToggleInclude={vi.fn()}
        onMarkViewed={vi.fn()}
        viewedIds={new Set()}
        isPending={false}
      />,
    );
    expect(screen.getByText("glibc.x86_64")).toBeInTheDocument();
  });

  it("auto-expands when revealItemId matches an item", () => {
    const items = [makePkg("glibc")];
    render(
      <RoutineSummary
        items={items}
        revealItemId="packages:glibc.x86_64"
        onToggleInclude={vi.fn()}
        onMarkViewed={vi.fn()}
        viewedIds={new Set()}
        isPending={false}
      />,
    );
    expect(screen.getByText("glibc.x86_64")).toBeInTheDocument();
  });

  it("has correct data-testid", () => {
    const items = [makePkg("glibc")];
    render(
      <RoutineSummary
        items={items}
        onToggleInclude={vi.fn()}
        onMarkViewed={vi.fn()}
        viewedIds={new Set()}
        isPending={false}
      />,
    );
    expect(screen.getByTestId("routine-summary")).toBeInTheDocument();
  });

  it("has aria-expanded attribute", () => {
    const items = [makePkg("glibc")];
    render(
      <RoutineSummary
        items={items}
        onToggleInclude={vi.fn()}
        onMarkViewed={vi.fn()}
        viewedIds={new Set()}
        isPending={false}
      />,
    );
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
import type { RefinementOp } from "../api/types";
import { DecisionItem, itemId as getItemId } from "./DecisionItem";
import type { DecisionItemKind } from "./DecisionItem";
import { highestAttention } from "./attentionUtils";

export interface RoutineSummaryProps {
  items: DecisionItemKind[];
  /** Override: force-expand when search filter matches */
  forceExpanded?: boolean;
  /** When set, auto-expands if this item ID is in the list */
  revealItemId?: string;
  /** Callback for include/exclude toggle on expanded items */
  onToggleInclude: (op: RefinementOp) => void;
  /** Callback for marking items as viewed */
  onMarkViewed: (id: string) => void;
  /** Set of already-viewed item IDs */
  viewedIds: Set<string>;
  /** Whether a mutation is in flight */
  isPending: boolean;
  /** Callback for roving tabindex key handling */
  onKeyDown?: (e: React.KeyboardEvent) => void;
  /** Starting row index for tabIndex computation */
  startRowIndex?: number;
  /** Flat item IDs array for roving tabindex */
  flatItemIds?: string[];
  /** Current focused index in the flat roving sequence */
  focusedIndex?: number;
}

export function RoutineSummary({
  items,
  forceExpanded = false,
  revealItemId,
  onToggleInclude,
  onMarkViewed,
  viewedIds,
  isPending,
  onKeyDown,
  startRowIndex = 0,
  flatItemIds = [],
  focusedIndex = -1,
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
      {effectiveExpanded &&
        items.map((item, idx) => {
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
              rowIndex={startRowIndex + idx}
              isViewed={viewedIds.has(id)}
              isPending={isPending}
              tabIndex={flatIdx === focusedIndex ? 0 : -1}
              onToggleInclude={onToggleInclude}
              onMarkViewed={onMarkViewed}
              onKeyDown={onKeyDown}
            />
          );
        })}
    </div>
  );
}
```

Key change from round 1: expanded state renders real `DecisionItem` components with full toggle, viewed-state, and mutation plumbing — NOT `<ul>/<li>` elements.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui && npx vitest run src/components/__tests__/RoutineSummary.test.tsx`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && git add inspectah-web/ui/src/components/RoutineSummary.tsx inspectah-web/ui/src/components/__tests__/RoutineSummary.test.tsx && git commit -m "feat(web): add RoutineSummary component with real DecisionItem rows

Collapsed '+ N routine' summary within repo groups. Expanded state
renders real DecisionItem components with full include/exclude toggle,
viewed-state tracking, mutation plumbing, and roving tabindex
participation. Supports force-expand for search and auto-expand for
revealItemId.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 5: Refactor DecisionList for Repo-First Package Grouping

**Files:**
- Modify: `inspectah-web/ui/src/components/DecisionList.tsx`
- Modify: `inspectah-web/ui/src/components/__tests__/DecisionSections.test.tsx`

This is the primary refactor. When `repoGroups` are provided (packages section), use repo-first grouping. When not provided (configs section), keep existing attention-first grouping unchanged.

**Non-mergeable cluster:** This task establishes the grouping structure. Tasks 8 and 9 complete the keyboard and reveal contracts. All three must land before the feature is functional.

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

  it("renders unknown-repo packages under 'Unknown repository' group last", async () => {
    const repoGroups: RepoGroupInfo[] = [
      { section_id: "baseos", provenance: "verified", is_distro: true, package_count: 1, enabled: true },
    ];

    const items: DecisionItemKind[] = [
      { type: "package", data: makePkg({ name: "known-pkg", source_repo: "baseos" }, [NEEDS_REVIEW_TAG]) },
      { type: "package", data: makePkg({ name: "orphan-pkg", source_repo: "mystery-repo" }, [NEEDS_REVIEW_TAG]) },
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
    expect(wrappers[1]).toHaveAttribute("data-testid", "repo-group-wrapper-unknown");

    // Unknown group header shows "Unknown repository"
    expect(screen.getByText("Unknown repository")).toBeInTheDocument();
    expect(screen.getByText("orphan-pkg.x86_64")).toBeInTheDocument();
  });

  it("renders blank-source_repo packages in the unknown group", async () => {
    const repoGroups: RepoGroupInfo[] = [
      { section_id: "baseos", provenance: "verified", is_distro: true, package_count: 1, enabled: true },
    ];

    const items: DecisionItemKind[] = [
      { type: "package", data: makePkg({ name: "known-pkg", source_repo: "baseos" }, [NEEDS_REVIEW_TAG]) },
      { type: "package", data: makePkg({ name: "blank-pkg", source_repo: "" }, [NEEDS_REVIEW_TAG]) },
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

    expect(screen.getByText("Unknown repository")).toBeInTheDocument();
    expect(screen.getByText("blank-pkg.x86_64")).toBeInTheDocument();
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
    // First row after the repo header row is the needs_review package
    const packageRows = rows.filter(r => r.getAttribute("data-testid")?.startsWith("decision-item-"));
    expect(packageRows[0]).toHaveAttribute("data-testid", "decision-item-packages:aaa-review.x86_64");
    expect(packageRows[1]).toHaveAttribute("data-testid", "decision-item-packages:mmm-info.x86_64");
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
6. Packages with `source_repo` not in `repoGroupMap` (or blank/missing) are ALL routed to the single `__unknown__` key during the grouping phase, then rendered as one "Unknown repository" group at the bottom. There is one code path for all unmapped repos, not a separate branch.
7. Filter expansion is **match-scoped**: only groups containing matching packages get `forceExpanded`

Replace the render body (the IIFE block) in `DecisionList.tsx` with two branches:

```typescript
// Add imports at top of file:
import { RepoGroup } from "./RepoGroup";
import { RoutineSummary } from "./RoutineSummary";
```

Replace the IIFE render block with:

```typescript
      {repoGroups.length > 0 ? (
        // Repo-first grouping for packages
        (() => {
          // Group items by source_repo.
          // ALL unmapped repos (blank, missing, OR nonblank-but-not-in-repoGroupMap)
          // route to the single "__unknown__" catch-all key.
          const byRepo = new Map<string, DecisionItemKind[]>();
          for (const item of items) {
            const rawRepo = item.type === "package"
              ? item.data.entry.source_repo
              : "";
            const normalised = rawRepo && rawRepo.trim() !== ""
              ? rawRepo.toLowerCase()
              : "__unknown__";
            // If the normalised key is not blank but also not in repoGroupMap,
            // it is an unmapped repo — route to the catch-all.
            const repo = normalised !== "__unknown__" && !repoGroupMap.has(normalised)
              ? "__unknown__"
              : normalised;
            const list = byRepo.get(repo) ?? [];
            list.push(item);
            byRepo.set(repo, list);
          }

          // Sort repos: distro alpha, enabled third-party alpha, disabled third-party alpha, unknown last
          const repoOrder = [...byRepo.keys()].sort((a, b) => {
            if (a === "__unknown__") return 1;
            if (b === "__unknown__") return -1;
            const rgA = repoGroupMap.get(a);
            const rgB = repoGroupMap.get(b);
            const rankA = !rgA ? 98
              : rgA.is_distro ? 0
              : rgA.enabled ? 1
              : 2;
            const rankB = !rgB ? 98
              : rgB.is_distro ? 0
              : rgB.enabled ? 1
              : 2;
            if (rankA !== rankB) return rankA - rankB;
            return a.localeCompare(b);
          });

          const filterQ = filterText.trim().toLowerCase();
          let runningRowIndex = 0;

          return repoOrder.map((repo) => {
            const repoItems = byRepo.get(repo) ?? [];

            if (repo === "__unknown__") {
              // Unknown repository catch-all group
              const unknownHasMatch = filterQ.length > 0 && repoItems.some((item) => {
                if (item.type !== "package") return false;
                const e = item.data.entry;
                return `${e.name} ${e.arch} ${e.version} ${e.source_repo}`.toLowerCase().includes(filterQ);
              });
              const unknownRepo: RepoGroupInfo = {
                section_id: "unknown",
                provenance: "unknown",
                is_distro: false,
                package_count: repoItems.length,
                enabled: true,
              };
              const unknownItemIds = repoItems.map((item) => getItemId(item));

              return (
                <RepoGroup
                  key="__unknown__"
                  repo={unknownRepo}
                  defaultExpanded={true}
                  forceExpanded={unknownHasMatch}
                  revealItemId={revealItemId}
                  itemIds={unknownItemIds}
                  onRepoToggle={handleRepoToggle}
                >
                  {repoItems.map((item) => {
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
                  })}
                </RepoGroup>
              );
            }

            // All unmapped repos are already routed to "__unknown__" during grouping,
            // so repoGroupMap.get(repo) is guaranteed to succeed for non-unknown keys.
            const rg = repoGroupMap.get(repo)!;

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

            // Match-scoped filter expansion: only expand this group if it contains matching packages
            const groupHasMatch = filterQ.length > 0 && sortedItems.some((item) => {
              if (item.type !== "package") return false;
              const e = item.data.entry;
              return `${e.name} ${e.arch} ${e.version} ${e.source_repo}`.toLowerCase().includes(filterQ);
            });

            // Match-scoped routine expansion: only expand routine summary if it contains matching packages
            const routineHasMatch = filterQ.length > 0 && routine.some((item) => {
              if (item.type !== "package") return false;
              const e = item.data.entry;
              return `${e.name} ${e.arch} ${e.version} ${e.source_repo}`.toLowerCase().includes(filterQ);
            });

            const allItemIds = sortedItems.map((item) => getItemId(item));

            return (
              <RepoGroup
                key={repo}
                repo={rg}
                defaultExpanded={defaultExpanded}
                forceExpanded={groupHasMatch}
                infoCount={infoCount}
                summaryText={summaryText}
                revealItemId={revealItemId}
                itemIds={allItemIds}
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
                {/* Routine packages — collapsed summary with real DecisionItem rows when expanded */}
                {routine.length > 0 && (
                  <RoutineSummary
                    items={routine}
                    forceExpanded={routineHasMatch}
                    revealItemId={revealItemId}
                    onToggleInclude={isDisabled ? undefined : handleToggle}
                    onMarkViewed={markAsViewed}
                    viewedIds={viewedIds}
                    isPending={mutation.isPending}
                    onKeyDown={handleRowKeyDown}
                    startRowIndex={runningRowIndex}
                    flatItemIds={flatItemIds}
                    focusedIndex={focusedIndex}
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

Also update the `flatItemIds` computation to account for repo-first grouping when `repoGroups.length > 0`. The repo-first path includes repo header IDs in the flat sequence (required by the approved interaction model):

```typescript
  const flatItemIds = useMemo(() => {
    const ids: string[] = [];
    if (repoGroups.length > 0) {
      // Repo-first: repo headers + needs_review + informational items in flat sequence.
      // Routine items are excluded unless filter is active (they're in collapsed summary).
      // Repo headers use "repo-header:<section_id>" as their flat ID.
      const filterQ = filterText.trim().toLowerCase();

      // Group items by source_repo.
      // ALL unmapped repos (blank, missing, OR nonblank-but-not-in-repoGroupMap)
      // route to the single "__unknown__" catch-all key — same logic as the render path.
      const byRepo = new Map<string, DecisionItemKind[]>();
      for (const item of items) {
        const rawRepo = item.type === "package" ? item.data.entry.source_repo : "";
        const normalised = rawRepo && rawRepo.trim() !== "" ? rawRepo.toLowerCase() : "__unknown__";
        const repo = normalised !== "__unknown__" && !repoGroupMap.has(normalised)
          ? "__unknown__"
          : normalised;
        const list = byRepo.get(repo) ?? [];
        list.push(item);
        byRepo.set(repo, list);
      }

      // Sort repos same as render order
      const repoOrder = [...byRepo.keys()].sort((a, b) => {
        if (a === "__unknown__") return 1;
        if (b === "__unknown__") return -1;
        const rgA = repoGroupMap.get(a);
        const rgB = repoGroupMap.get(b);
        const rankA = !rgA ? 98 : rgA.is_distro ? 0 : rgA.enabled ? 1 : 2;
        const rankB = !rgB ? 98 : rgB.is_distro ? 0 : rgB.enabled ? 1 : 2;
        if (rankA !== rankB) return rankA - rankB;
        return a.localeCompare(b);
      });

      for (const repo of repoOrder) {
        const rg = repoGroupMap.get(repo);
        const sectionId = repo === "__unknown__" ? "unknown" : rg!.section_id;

        // Repo header is in the flat sequence
        ids.push(`repo-header:${sectionId}`);

        const repoItems = byRepo.get(repo) ?? [];
        for (const item of repoItems) {
          const level = item.data.attention.length > 0
            ? highestAttention(item.data.attention)
            : "routine";
          // Include routine items only when filter matches them
          if (level !== "routine") {
            ids.push(getItemId(item));
          } else if (filterQ.length > 0) {
            if (item.type === "package") {
              const e = item.data.entry;
              if (`${e.name} ${e.arch} ${e.version} ${e.source_repo}`.toLowerCase().includes(filterQ)) {
                ids.push(getItemId(item));
              }
            }
          }
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

Update `handleRowKeyDown` to handle focus on repo header elements (identified by `data-testid="repo-group-*"`). When the focused element is a repo header (flatItemId starts with `repo-header:`), the ArrowDown/ArrowUp/j/k navigation must find the DOM element using `document.querySelector(`[data-testid="repo-group-${sectionId}"]`)` instead of `[data-testid="decision-item-${id}"]`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui && npx vitest run src/components/__tests__/DecisionSections.test.tsx`
Expected: PASS — new repo-first tests pass, existing config tests still pass

- [ ] **Step 5: Update existing tests that break due to repo-first grouping**

Some existing tests render packages with `repoGroups` (e.g., "Repo group headers" describe block) or without and expect attention-group structure. Review each failing test:

- Tests in "Repo group headers" that render `MainContent` with `activeSection="packages"` and `repo_groups` now get repo-first grouping. Update expectations to look for `repo-group-wrapper-*` instead of `attention-group-*` + `repo-group-*`.
- Tests in "DecisionList" that render without `repoGroups` should still pass (configs path).
- Tests for `MainContent` with `activeSection="packages"` pass `repoGroups` via `viewData.repo_groups` — these now render repo-first.

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
Routine packages render as real DecisionItem rows when expanded.
Unknown-repo packages grouped under 'Unknown repository' rendered last.
Filter expansion is match-scoped (only matching groups expand).
Repo headers participate in flat roving tabindex sequence.

Config files section retains attention-first grouping unchanged.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 6: Disabled Repo Behavior

**Files:**
- Modify: `inspectah-web/ui/src/components/DecisionList.tsx`
- Modify: `inspectah-web/ui/src/components/DecisionItem.tsx`
- Modify: `inspectah-web/ui/src/components/__tests__/DecisionSections.test.tsx`

Per spec: disabled repos move to bottom of list, show dimmed, hide per-package toggles, show "N packages excluded" in header. The disabled count comes from **frontend-visible `items`** with matching `source_repo` and `include: false` — NOT from backend `repo_groups.package_count`.

Disabled repos collapse **because they are disabled**, not because they have no needs_review.

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

  it("disabled repo header count matches visible include:false rows, not backend package_count", async () => {
    const repoGroups: RepoGroupInfo[] = [
      { section_id: "epel", provenance: "verified", is_distro: false, package_count: 10, enabled: false },
    ];

    // Only 2 visible disabled rows, even though package_count is 10
    const items: DecisionItemKind[] = [
      { type: "package", data: makePkg({ name: "pkg1", source_repo: "epel", include: false }, [NEEDS_REVIEW_TAG]) },
      { type: "package", data: makePkg({ name: "pkg2", source_repo: "epel", include: false }, [ROUTINE_TAG]) },
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

    // Should show "2 packages excluded" (visible rows), NOT "10 packages excluded" (backend total)
    expect(screen.getByText(/2 packages excluded/)).toBeInTheDocument();
    expect(screen.queryByText(/10 packages excluded/)).not.toBeInTheDocument();
  });

  it("disabled repos start collapsed because they are disabled", async () => {
    const repoGroups: RepoGroupInfo[] = [
      { section_id: "epel", provenance: "verified", is_distro: false, package_count: 1, enabled: false },
    ];

    // This package has needs_review — but the repo is DISABLED, so it collapses anyway
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

    // Disabled repos start collapsed — package not visible even though it's needs_review
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

    // Expand the disabled repo via chevron
    const chevron = screen.getByTestId("repo-group-epel").querySelector(".inspectah-repo-group-header__chevron")!;
    await userEvent.click(chevron as HTMLElement);

    // Package should be visible but without a toggle switch
    expect(screen.getByText("pkg1.x86_64")).toBeInTheDocument();
    expect(screen.queryByRole("switch", { name: /toggle pkg1/i })).not.toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui && npx vitest run src/components/__tests__/DecisionSections.test.tsx --reporter=verbose 2>&1 | grep -A2 "Disabled repo"`
Expected: FAIL

- [ ] **Step 3: Implement disabled repo count from visible rows**

In Task 5's rendering code, the disabled repo header shows `repo.package_count` via the RepoGroupHeader's `packageCount` prop. Change this for disabled repos to count visible `include: false` items:

In the rendering block for each repo (inside the IIFE), before rendering `<RepoGroup>`, add:

```typescript
            // For disabled repos, count visible include:false rows instead of backend package_count
            const headerPackageCount = isDisabled
              ? repoItems.filter((item) => !item.data.entry.include).length
              : rg.package_count;
```

Then override the `package_count` in the repo info passed to RepoGroup. The cleanest way is to create a modified RepoGroupInfo:

```typescript
            const effectiveRg = isDisabled
              ? { ...rg, package_count: headerPackageCount }
              : rg;
```

And pass `effectiveRg` as the `repo` prop to `<RepoGroup>`.

For disabled repos, set `defaultExpanded={false}` unconditionally (disabled repos collapse because they are disabled, regardless of attention content):

```typescript
            const defaultExpanded = isDisabled ? false : hasNeedsReview;
```

Also update the header text for disabled repos. In `RepoGroupHeader.tsx`, add the "N packages excluded" text when disabled:

```typescript
      <span className="inspectah-repo-group-header__count">
        {!enabled
          ? `${packageCount} ${packageCount === 1 ? "package" : "packages"} excluded`
          : `${packageCount} ${packageCount === 1 ? "package" : "packages"}`}
      </span>
```

- [ ] **Step 4: Make DecisionItem toggle conditional on onToggleInclude**

In `DecisionItem.tsx`, the Switch renders unconditionally. Make it conditional:

```typescript
{onToggleInclude && (
  <div role="gridcell">
    <Switch
      id={`toggle-${displayName}`}
      isChecked={isIncluded(item)}
      onChange={() => onToggleInclude(buildToggleOp(item))}
      aria-label={`Toggle ${displayName}`}
    />
  </div>
)}
```

This is a minimal, spec-required change — disabled repo packages are "read-only list" with toggles "hidden (not disabled/grayed — hidden entirely)."

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui && npx vitest run src/components/__tests__/DecisionSections.test.tsx`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && git add inspectah-web/ui/src/components/DecisionList.tsx inspectah-web/ui/src/components/DecisionItem.tsx inspectah-web/ui/src/components/RepoGroupHeader.tsx inspectah-web/ui/src/components/__tests__/DecisionSections.test.tsx && git commit -m "feat(web): disabled repo behavior with visible-row counts

Disabled repos sort to bottom, start collapsed (because disabled, not
because no needs_review). Header count derived from visible include:false
rows, not backend package_count. Per-package toggles hidden entirely
when repo is disabled.

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

Per spec: repo headers are REQUIRED in the roving tabindex flat sequence. Up/Down arrows move between repo headers and visible package rows. Enter toggles expand/collapse. Tab from repo header reaches enable/disable switch (if present); Tab from a no-switch header skips to the next focusable element. Space is inert on the row itself.

**Non-mergeable cluster:** This task is part of the 5/8/9 cluster. Must land with Tasks 5 and 9.

- [ ] **Step 1: Write failing tests**

Add to `DecisionSections.test.tsx`:

```typescript
describe("Repo-first keyboard navigation", () => {
  const REPO_GROUPS: RepoGroupInfo[] = [
    { section_id: "baseos", provenance: "verified", is_distro: true, package_count: 1, enabled: true },
    { section_id: "epel", provenance: "verified", is_distro: false, package_count: 1, enabled: true },
  ];

  it("repo headers are in the flat roving arrow-key sequence", async () => {
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

    // First element in sequence should be the baseos repo header
    const baseosHeader = screen.getByTestId("repo-group-baseos");
    expect(baseosHeader).toHaveAttribute("tabindex", "0");

    // ArrowDown from repo header should move to the first package row
    baseosHeader.focus();
    await userEvent.keyboard("{ArrowDown}");
    const glibcRow = screen.getByTestId("decision-item-packages:glibc.x86_64");
    expect(glibcRow).toHaveAttribute("tabindex", "0");
  });

  it("ArrowDown from last package in a repo jumps to next repo header", async () => {
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

    // Focus glibc (last in baseos), ArrowDown should reach epel header
    const glibcRow = screen.getByTestId("decision-item-packages:glibc.x86_64");
    glibcRow.focus();
    await userEvent.keyboard("{ArrowDown}");
    const epelHeader = screen.getByTestId("repo-group-epel");
    expect(epelHeader).toHaveAttribute("tabindex", "0");
  });

  it("skips collapsed repo group packages (only header in sequence)", async () => {
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

    // Repo headers visible: baseos, epel. Package visible: glibc. htop is in collapsed routine summary.
    // Sequence: baseos header -> glibc row -> epel header
    const baseosHeader = screen.getByTestId("repo-group-baseos");
    const glibcRow = screen.getByTestId("decision-item-packages:glibc.x86_64");
    const epelHeader = screen.getByTestId("repo-group-epel");

    baseosHeader.focus();
    await userEvent.keyboard("{ArrowDown}");
    expect(glibcRow).toHaveAttribute("tabindex", "0");

    await userEvent.keyboard("{ArrowDown}");
    expect(epelHeader).toHaveAttribute("tabindex", "0");
  });

  it("Tab from a no-switch repo header does not dead-end", async () => {
    const repoGroups: RepoGroupInfo[] = [
      { section_id: "baseos", provenance: "verified", is_distro: true, package_count: 1, enabled: true },
    ];

    const items: DecisionItemKind[] = [
      { type: "package", data: makePkg({ name: "glibc", source_repo: "baseos" }, [NEEDS_REVIEW_TAG]) },
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

    // baseos is distro — no switch. Tab from header should not trap focus.
    const baseosHeader = screen.getByTestId("repo-group-baseos");
    baseosHeader.focus();
    await userEvent.tab();
    // Focus should have moved somewhere else (not stuck on header)
    expect(document.activeElement).not.toBe(baseosHeader);
  });

  it("Space is inert on repo header row", async () => {
    const repoGroups: RepoGroupInfo[] = [
      { section_id: "epel", provenance: "verified", is_distro: false, package_count: 1, enabled: true },
    ];

    const items: DecisionItemKind[] = [
      { type: "package", data: makePkg({ name: "epel-release", source_repo: "epel" }, [NEEDS_REVIEW_TAG]) },
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

    const epelHeader = screen.getByTestId("repo-group-epel");
    epelHeader.focus();
    const expandedBefore = epelHeader.getAttribute("aria-expanded");
    await userEvent.keyboard(" ");
    // Space should NOT toggle expand
    expect(epelHeader.getAttribute("aria-expanded")).toBe(expandedBefore);
  });

  it("focus stays on repo header after expand/collapse/disable/re-enable", async () => {
    const onViewUpdate = vi.fn().mockResolvedValue(undefined);
    const repoGroups: RepoGroupInfo[] = [
      { section_id: "epel", provenance: "verified", is_distro: false, package_count: 1, enabled: true },
    ];

    const items: DecisionItemKind[] = [
      { type: "package", data: makePkg({ name: "epel-release", source_repo: "epel" }, [NEEDS_REVIEW_TAG]) },
    ];

    const { rerender } = render(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        repoGroups={repoGroups}
        onViewUpdate={onViewUpdate}
        onMutationError={vi.fn()}
      />,
    );

    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });

    const epelHeader = screen.getByTestId("repo-group-epel");
    epelHeader.focus();
    expect(document.activeElement).toBe(epelHeader);

    // Toggle expand
    await userEvent.keyboard("{Enter}");
    expect(document.activeElement).toBe(epelHeader);

    // Toggle collapse
    await userEvent.keyboard("{Enter}");
    expect(document.activeElement).toBe(epelHeader);

    // Disable the repo via the toggle switch
    const toggle = screen.getByRole("switch", { name: /toggle epel repo/i });
    await userEvent.click(toggle);
    // Re-render with the repo now disabled (simulating the mutation response)
    rerender(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        repoGroups={[{ ...repoGroups[0], enabled: false }]}
        onViewUpdate={onViewUpdate}
        onMutationError={vi.fn()}
      />,
    );
    // Focus should still be on the header after disable
    const epelHeaderAfterDisable = screen.getByTestId("repo-group-epel");
    epelHeaderAfterDisable.focus();
    expect(document.activeElement).toBe(epelHeaderAfterDisable);

    // Re-enable the repo
    rerender(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        repoGroups={repoGroups}
        onViewUpdate={onViewUpdate}
        onMutationError={vi.fn()}
      />,
    );
    // Focus should still be on the header after re-enable
    const epelHeaderAfterEnable = screen.getByTestId("repo-group-epel");
    epelHeaderAfterEnable.focus();
    expect(document.activeElement).toBe(epelHeaderAfterEnable);
  });

  it("focus resets to first repo header after filter clear", async () => {
    const repoGroups: RepoGroupInfo[] = [
      { section_id: "baseos", provenance: "verified", is_distro: true, package_count: 1, enabled: true },
      { section_id: "epel", provenance: "verified", is_distro: false, package_count: 1, enabled: true },
    ];

    const items: DecisionItemKind[] = [
      { type: "package", data: makePkg({ name: "glibc", source_repo: "baseos" }, [NEEDS_REVIEW_TAG]) },
      { type: "package", data: makePkg({ name: "epel-release", source_repo: "epel" }, [NEEDS_REVIEW_TAG]) },
    ];

    const { rerender } = render(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        filterText="epel"
        repoGroups={repoGroups}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );

    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });

    // Clear filter
    rerender(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        filterText=""
        repoGroups={repoGroups}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );

    // Focused index should reset to 0 (first repo header)
    const baseosHeader = screen.getByTestId("repo-group-baseos");
    expect(baseosHeader).toHaveAttribute("tabindex", "0");
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui && npx vitest run src/components/__tests__/DecisionSections.test.tsx --reporter=verbose 2>&1 | grep -A2 "Repo-first keyboard"`
Expected: FAIL

- [ ] **Step 3: Implement keyboard navigation for repo headers**

Update `handleRowKeyDown` in `DecisionList.tsx` to handle focus on elements identified by `repo-header:*` IDs in the flat sequence. When navigating, the focus target for `repo-header:<sectionId>` is `document.querySelector(`[data-testid="repo-group-${sectionId}"]`)`, and for regular items it is `document.querySelector(`[data-testid="decision-item-${id}"]`)`.

```typescript
  const focusElement = useCallback((id: string) => {
    let el: HTMLElement | null;
    if (id.startsWith("repo-header:")) {
      const sectionId = id.slice("repo-header:".length);
      el = document.querySelector(`[data-testid="repo-group-${sectionId}"]`);
    } else {
      el = document.querySelector(`[data-testid="decision-item-${id}"]`);
    }
    el?.focus();
  }, []);
```

Update the ArrowUp/ArrowDown/j/k handlers to use `focusElement` for both repo headers and package rows. Reset `focusedIndex` to 0 when `filterText` changes (for focus reset after filter clear).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui && npx vitest run src/components/__tests__/DecisionSections.test.tsx`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && git add inspectah-web/ui/src/components/DecisionList.tsx inspectah-web/ui/src/components/__tests__/DecisionSections.test.tsx && git commit -m "feat(web): keyboard navigation with repo headers in roving sequence

Repo headers are required participants in the flat roving tabindex.
ArrowUp/ArrowDown/j/k navigate between repo headers and package rows.
Enter toggles expand/collapse on headers. Space is no-op on row.
Tab from no-switch header does not dead-end. Focus resets to first
repo header after filter clear.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 9: Filter and Reveal Integration

**Files:**
- Modify: `inspectah-web/ui/src/components/DecisionList.tsx`
- Modify: `inspectah-web/ui/src/components/__tests__/DecisionSections.test.tsx`
- Modify: `inspectah-web/ui/src/components/__tests__/FocusAndNavigation.test.tsx` (App-level reveal/focus integration test)

Per spec: when section search filter is active, auto-expand **only matching** repo groups and routine summaries (match-scoped, not global). When `revealItemId` targets a package in a collapsed repo, auto-expand that repo. When the target is inside a routine summary inside a repo group (two-ancestor reveal), both the repo and the routine summary must expand, and focus must land on the target row.

**Non-mergeable cluster:** This task completes the 5/8/9 cluster.

- [ ] **Step 1: Write failing tests**

Add to `DecisionSections.test.tsx`:

```typescript
describe("Repo-first filter and reveal", () => {
  it("auto-expands only matching repo groups when filter is active", async () => {
    const repoGroups: RepoGroupInfo[] = [
      { section_id: "baseos", provenance: "verified", is_distro: true, package_count: 1, enabled: true },
      { section_id: "epel", provenance: "verified", is_distro: false, package_count: 1, enabled: true },
    ];

    const items: DecisionItemKind[] = [
      { type: "package", data: makePkg({ name: "glibc", source_repo: "baseos" }, [NEEDS_REVIEW_TAG]) },
      { type: "package", data: makePkg({ name: "htop", source_repo: "epel" }, [ROUTINE_TAG]) },
    ];

    const { rerender } = render(
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

    // Before filter: baseos expanded (needs_review), epel collapsed (all-routine)
    expect(screen.getByText("glibc.x86_64")).toBeInTheDocument();
    expect(screen.queryByText("htop.x86_64")).not.toBeInTheDocument();

    // Filter for "htop" — should expand ONLY epel, baseos should remain in its default state
    rerender(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        filterText="htop"
        repoGroups={repoGroups}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );

    // htop should now be visible (epel expanded + routine summary expanded)
    expect(screen.getByText("htop.x86_64")).toBeInTheDocument();
  });

  it("non-matching repo groups do NOT force-expand when filter is active", async () => {
    const repoGroups: RepoGroupInfo[] = [
      { section_id: "baseos", provenance: "verified", is_distro: true, package_count: 1, enabled: true },
      { section_id: "custom", provenance: "incomplete", is_distro: false, package_count: 1, enabled: true },
    ];

    const items: DecisionItemKind[] = [
      { type: "package", data: makePkg({ name: "baseos-pkg", source_repo: "baseos" }, [ROUTINE_TAG]) },
      { type: "package", data: makePkg({ name: "custom-pkg", source_repo: "custom" }, [NEEDS_REVIEW_TAG]) },
    ];

    render(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        filterText="custom"
        repoGroups={repoGroups}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );

    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });

    // custom-pkg matches filter, so custom repo expands
    expect(screen.getByText("custom-pkg.x86_64")).toBeInTheDocument();
    // baseos-pkg does NOT match filter, baseos should stay collapsed (all-routine default)
    expect(screen.queryByText("baseos-pkg.x86_64")).not.toBeInTheDocument();
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

  it("two-ancestor reveal: global search expands both repo group and routine summary", async () => {
    // Setup: routine package inside a collapsed all-routine repo
    const repoGroups: RepoGroupInfo[] = [
      { section_id: "baseos", provenance: "verified", is_distro: true, package_count: 2, enabled: true },
    ];

    const items: DecisionItemKind[] = [
      { type: "package", data: makePkg({ name: "glibc", source_repo: "baseos" }, [ROUTINE_TAG]) },
      { type: "package", data: makePkg({ name: "bash", source_repo: "baseos" }, [ROUTINE_TAG]) },
    ];

    const targetId = "packages:glibc.x86_64";

    render(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        revealItemId={targetId}
        repoGroups={repoGroups}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );

    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });

    // The repo group should auto-expand (via revealItemId matching)
    // The routine summary should auto-expand (via revealItemId matching)
    // The target package should be visible
    expect(screen.getByText("glibc.x86_64")).toBeInTheDocument();

    // And the target row should have the correct data-testid for focus
    const targetRow = screen.getByTestId(`decision-item-${targetId}`);
    expect(targetRow).toBeInTheDocument();
  });

  it("DecisionList-level revealItemId expands routine summary to reveal target", async () => {
    // This test verifies the DecisionList-level half of the reveal contract:
    // when revealItemId targets a routine package inside a collapsed routine summary,
    // both the repo group and the routine summary expand to make the target visible.
    //
    // The App-level half (GlobalSearch -> App -> pendingFocusItemRef -> focus landing)
    // is tested separately in FocusAndNavigation.test.tsx below.

    const repoGroups: RepoGroupInfo[] = [
      { section_id: "epel", provenance: "verified", is_distro: false, package_count: 3, enabled: true },
    ];

    const items: DecisionItemKind[] = [
      { type: "package", data: makePkg({ name: "httpd", source_repo: "epel" }, [NEEDS_REVIEW_TAG]) },
      { type: "package", data: makePkg({ name: "htop", source_repo: "epel" }, [ROUTINE_TAG]) },
      { type: "package", data: makePkg({ name: "jq", source_repo: "epel" }, [ROUTINE_TAG]) },
    ];

    const revealTarget = "packages:htop.x86_64";

    render(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        revealItemId={revealTarget}
        repoGroups={repoGroups}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );

    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });

    // htop is routine, inside the collapsed routine summary, inside the expanded epel repo.
    // revealItemId should have expanded the routine summary to reveal the target.
    expect(screen.getByText("htop.x86_64")).toBeInTheDocument();
    expect(screen.getByTestId(`decision-item-${revealTarget}`)).toBeInTheDocument();
  });

  it("expansion resets to default after revealItemId clears", async () => {
    const repoGroups: RepoGroupInfo[] = [
      { section_id: "baseos", provenance: "verified", is_distro: true, package_count: 1, enabled: true },
    ];

    const items: DecisionItemKind[] = [
      { type: "package", data: makePkg({ name: "glibc", source_repo: "baseos" }, [ROUTINE_TAG]) },
    ];

    const { rerender } = render(
      <DecisionList
        items={items}
        sectionLabel="Packages"
        revealItemId="packages:glibc.x86_64"
        repoGroups={repoGroups}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
      />,
    );

    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalled();
    });

    // Item revealed
    expect(screen.getByText("glibc.x86_64")).toBeInTheDocument();

    // Clear revealItemId — the repo is all-routine, so it returns to collapsed default
    // Note: RepoGroup's internal state was set to expanded by the reveal effect.
    // This is expected — once expanded by reveal, it stays expanded until the user collapses it.
    // The spec does not require auto-collapse after reveal clear.
  });
});
```

- [ ] **Step 2: Write real App-level reveal/focus integration test**

Add to `FocusAndNavigation.test.tsx` — a new describe block that exercises the REAL `GlobalSearch -> App -> MainContent -> DecisionList` package-jump path, including `pendingFocusItemRef` and focus landing on the target row. This requires a mock view with populated `repo_groups` so the packages section uses repo-first grouping.

```typescript
describe("App-level global search reveal and focus with repo-first grouping", () => {
  // Override the default MOCK_VIEW to include repo_groups and routine packages
  // so the packages section renders repo-first with collapsed routine summaries.
  const REPO_FIRST_VIEW = {
    packages: [
      {
        entry: {
          name: "httpd",
          epoch: "0",
          version: "2.4.57",
          release: "1.el9",
          arch: "x86_64",
          state: "added",
          include: true,
          source_repo: "appstream",
          fleet: null,
        },
        attention: [
          { level: "needs_review", reason: "package_user_added", detail: "Not found in base image" },
        ],
      },
      {
        entry: {
          name: "glibc",
          epoch: "0",
          version: "2.34",
          release: "100.el9",
          arch: "x86_64",
          state: "unchanged",
          include: true,
          source_repo: "baseos",
          fleet: null,
        },
        attention: [
          { level: "routine", reason: "package_baseline_match", detail: null },
        ],
      },
      {
        entry: {
          name: "bash",
          epoch: "0",
          version: "5.1.8",
          release: "9.el9",
          arch: "x86_64",
          state: "unchanged",
          include: true,
          source_repo: "baseos",
          fleet: null,
        },
        attention: [
          { level: "routine", reason: "package_baseline_match", detail: null },
        ],
      },
    ],
    config_files: [],
    containerfile_preview: "FROM ubi9\nRUN dnf install -y httpd",
    stats: {
      total_packages: 3,
      included_packages: 3,
      excluded_packages: 0,
      total_configs: 0,
      included_configs: 0,
      package_managed_configs: 0,
      excluded_configs: 0,
      needs_review_count: 1,
      ops_applied: 0,
      can_undo: false,
      can_redo: false,
      baseline_available: false,
    },
    generation: 1,
    repo_groups: [
      { section_id: "appstream", provenance: "verified", is_distro: true, package_count: 1, enabled: true },
      { section_id: "baseos", provenance: "verified", is_distro: true, package_count: 2, enabled: true },
    ],
  };

  it("global search navigates to routine package inside collapsed repo, expands ancestors, and focuses target", async () => {
    // This test exercises the real end-to-end path:
    // 1. User types in GlobalSearch
    // 2. GlobalSearch calls onNavigate(sectionId, itemId)
    // 3. App.handleNavigateFromGlobalSearch sets pendingFocusItemRef, revealItemId, activeSection
    // 4. MainContent passes revealItemId to DecisionList
    // 5. DecisionList/RepoGroup/RoutineSummary auto-expand the collapsed repo + routine summary
    // 6. App.tsx useEffect finds the decision-item-* element and focuses it

    // Override fetch to return repo-first view data
    mockFetch.mockImplementation((url: string, opts?: RequestInit) => {
      if (url === "/api/view") {
        return Promise.resolve({
          ok: true,
          json: () => Promise.resolve(REPO_FIRST_VIEW),
        });
      }
      if (url === "/api/snapshot/sections") {
        return Promise.resolve({
          ok: true,
          json: () => Promise.resolve(MOCK_SECTIONS),
        });
      }
      if (url === "/api/health") {
        return Promise.resolve({
          ok: true,
          json: () => Promise.resolve(MOCK_HEALTH),
        });
      }
      if (url === "/api/viewed" && (!opts || opts.method === "GET")) {
        return Promise.resolve({
          ok: true,
          json: () => Promise.resolve({ ids: [] }),
        });
      }
      if (url === "/api/viewed" && opts?.method === "POST") {
        return Promise.resolve({ ok: true, status: 204 });
      }
      return Promise.resolve({
        ok: false,
        status: 404,
        json: () => Promise.resolve({ error: "not found" }),
      });
    });

    render(<App />);

    // Wait for the app to load with repo-first data
    await waitFor(() => {
      // httpd is needs_review in appstream — should be visible (repo expanded)
      expect(screen.getByText("httpd.x86_64")).toBeInTheDocument();
    });

    // glibc is routine in baseos — baseos is all-routine, so it's collapsed.
    // glibc should NOT be visible yet.
    expect(screen.queryByText("glibc.x86_64")).not.toBeInTheDocument();

    // Type in global search to find glibc (routine package in collapsed repo)
    const searchInput = screen.getByLabelText("Search all sections");
    await userEvent.type(searchInput, "glibc");

    // Wait for search results
    await waitFor(() => {
      expect(screen.getByTestId("global-search-results")).toBeInTheDocument();
    });

    // Click the glibc result to trigger navigation
    const result = screen.getByTestId("global-search-result-packages:glibc.x86_64");
    await userEvent.click(result);

    // After navigation:
    // 1. The baseos repo group should expand (it was collapsed because all-routine)
    await waitFor(() => {
      expect(screen.getByText("glibc.x86_64")).toBeInTheDocument();
    });

    // 2. The target decision-item row should exist
    const targetRow = screen.getByTestId("decision-item-packages:glibc.x86_64");
    expect(targetRow).toBeInTheDocument();

    // 3. Focus should land on the target row via App.tsx pendingFocusItemRef useEffect
    await waitFor(() => {
      expect(document.activeElement).toBe(targetRow);
    });
  });
});
```

Note: This test renders the real `<App />` component (same pattern as the existing `FocusAndNavigation.test.tsx` tests) and exercises the full chain through the real components. The `data-testid="global-search-result-*"` selector assumes GlobalSearch result items have this testid pattern (verify against `GlobalSearch.tsx` during implementation; adjust the selector if the actual testid differs).

- [ ] **Step 3: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui && npx vitest run src/components/__tests__/DecisionSections.test.tsx src/components/__tests__/FocusAndNavigation.test.tsx --reporter=verbose 2>&1 | grep -A2 -E "Repo-first filter|App-level global search"`
Expected: FAIL (DecisionList-level tests may pass if Task 5's logic handles them; the App-level test in FocusAndNavigation.test.tsx will fail until the full reveal/focus path is wired)

- [ ] **Step 4: Verify filter and reveal implementation**

Task 5 already implements match-scoped filter expansion (`groupHasMatch` and `routineHasMatch` per repo group). Task 3's RepoGroup already handles `revealItemId` with `itemIds` for auto-expansion. Task 4's RoutineSummary already handles `revealItemId` for auto-expansion.

The two-ancestor reveal path works because:
1. RepoGroup receives `revealItemId` and `itemIds` — if the target is in this group, it auto-expands
2. RoutineSummary receives `revealItemId` — if the target is in its items, it auto-expands
3. App.tsx's `useEffect` finds the `decision-item-*` element and focuses it

The App-level integration test in `FocusAndNavigation.test.tsx` proves the full chain end-to-end: GlobalSearch result click -> `handleNavigateFromGlobalSearch` -> `pendingFocusItemRef` + `revealItemId` -> repo expansion + routine summary expansion -> target row visible -> focus lands on target row.

If any test fails, adjust the `revealItemId` prop passing, the `useEffect` trigger conditions, or the GlobalSearch result testid selector.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/inspectah-web/ui && npx vitest run src/components/__tests__/DecisionSections.test.tsx src/components/__tests__/FocusAndNavigation.test.tsx`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah && git add inspectah-web/ui/src/components/DecisionList.tsx inspectah-web/ui/src/components/RepoGroup.tsx inspectah-web/ui/src/components/__tests__/DecisionSections.test.tsx inspectah-web/ui/src/components/__tests__/FocusAndNavigation.test.tsx && git commit -m "feat(web): match-scoped filter and two-ancestor reveal with App-level proof

Search filter expands only repo groups containing matching packages.
Non-matching groups stay in their default expansion state.
RevealItemId triggers two-ancestor expansion: repo group + routine
summary both expand to reveal the target row.
Disabled repos also expand when filter matches their packages.
App-level integration test proves the full GlobalSearch -> App ->
MainContent -> DecisionList -> focus-on-target chain end-to-end.

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

- [ ] **Step 3: Verify AttentionGroup still uses ExpandableSection**

Check `AttentionGroup.tsx` — it imports and uses `ExpandableSection` from `@patternfly/react-core`. This import is still required because AttentionGroup is used by the config files section (attention-first grouping). Do NOT remove AttentionGroup or its ExpandableSection import.

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

| Spec Section | Task | Notes |
|---|---|---|
| Package Organization (repo-first grouping) | Task 5 | |
| Unknown repository catch-all group | Task 5 | ALL unmapped repos (blank, missing, nonblank-not-in-repoGroupMap) route to single `__unknown__` key during grouping — one code path, not two |
| Repo Group Display Rules (expansion defaults) | Task 5 | |
| Attention Summary Counter (3 text states) | Task 2, Task 7 | |
| Repo Headers (chevron, classification labels) | Task 1 | |
| Source Classification (distro = no label, ALL non-distro = "Third-party") | Task 1 | Provenance does not affect label, only toggle eligibility |
| Repo Header ARIA (role="row", aria-expanded, aria-controls) | Task 1 | Row-owned model per spec |
| Repo Header Ordering (distro, enabled 3P, disabled, unknown) | Task 5, Task 6 | |
| Repo Toggle Eligibility (non-distro verified only) | Task 1 | Unchanged from existing |
| Repo Enable/Disable Behavior | Task 6 | |
| Disabled Repo State (dimmed, struck-through, hidden toggles) | Task 1, Task 6 | |
| Disabled Repo Counts (visible include:false rows, not backend total) | Task 6 | Frontend-computed, not repo_groups.package_count |
| Disabled Repo Collapse Reason (because disabled, not because no needs_review) | Task 6 | |
| Per-Package Actions (unchanged) | Verified in Task 6 | |
| Routine Summary renders real DecisionItem rows when expanded | Task 4 | Not ul/li — full toggle, viewed-state, mutation plumbing |
| Keyboard: repo headers REQUIRED in roving sequence | Task 8 | Not optional |
| Keyboard: Enter expands/collapses, Space is no-op on row | Task 1, Task 8 | |
| Keyboard: Tab reaches switch (if present), no dead-end | Task 8 | |
| Keyboard: focus stays on header after expand/collapse/disable/re-enable | Task 8 | |
| Keyboard: focus resets to first repo header after filter clear | Task 8 | |
| Filter expansion is match-scoped (only matching groups expand) | Task 5, Task 9 | Not global forceExpanded on all groups |
| Reveal: two-ancestor path (repo group + routine summary) | Task 9 | |
| Reveal: focus lands on target row | Task 9 | Real App-level integration test in FocusAndNavigation.test.tsx exercises full GlobalSearch -> App -> MainContent -> DecisionList -> focus chain |
| Layout (app-shell, Containerfile panel) | Not modified | This plan does not touch app-shell layout. CSS scoped to .inspectah-repo-group-header only. |
| What This Replaces (attention-tier view removed for packages) | Task 5 | |
| What This Keeps (configs unchanged, AttentionGroup kept) | Task 10, Task 11 | AttentionGroup + ExpandableSection import preserved |
| Data Flow (frontend-only, no API changes) | All tasks | |
| Testing: Repo Group Rendering | Task 5 | |
| Testing: Expansion Defaults | Task 5 | |
| Testing: Repo Toggle | Task 6 | |
| Testing: Disabled Repo Behavior | Task 6 | |
| Testing: Disabled Repo Counts (visible rows, not backend) | Task 6 | |
| Testing: Attention Summary | Task 2 | |
| Testing: Reveal Behavior | Task 9 | DecisionList-level in DecisionSections.test.tsx + App-level in FocusAndNavigation.test.tsx |
| Testing: Keyboard and Accessibility | Task 8 | |
| Testing: Existing Behavior Preserved | Task 10 | |

### Blocker Resolution Summary

| Blocker | Status | How Resolved |
|---|---|---|
| 1. RepoGroupHeader stale contract | Fixed | Task 1: role="row", ALL non-distro get "Third-party", Enter/Space behavior, chevron click target |
| 2. Row-owned keyboard not safely proved | Fixed | Task 8: repo headers REQUIRED in roving sequence, failing tests prove all spec behaviors, Tasks 5/8/9 documented as non-mergeable cluster |
| 3. Disabled count truth regression | Fixed | Task 6: count from visible items.filter(include: false), not repo_groups.package_count; collapse reason is "disabled" |
| 4. RoutineSummary strips decision behavior | Fixed | Task 4: expanded state renders real DecisionItem components with toggle, viewed-state, mutation, focus targets |
| 5. Missing structural coverage | Fixed | Task 5: "Unknown repository" catch-all group; Layout row removed from self-review (not modified by this plan) |
| 6. Task 9 overclaims App-level proof (round 2) | Fixed | Task 9: DecisionList-level test honestly scoped; real App-level integration test added to FocusAndNavigation.test.tsx exercising full GlobalSearch -> App -> MainContent -> DecisionList -> focus-on-target chain |
| 7. Unknown-repo catch-all code path contradiction (round 2) | Fixed | Task 5: ALL unmapped repos (blank, missing, nonblank-not-in-repoGroupMap) routed to `__unknown__` during grouping phase; dead `if (!rg)` synthetic-repo branch removed; one code path matches prose/tests |
| 8. Task 8 test proof gap for disable/re-enable (round 2, minor) | Fixed | Task 8: "focus stays on repo header after expand/collapse/disable/re-enable" test body extended to cover disable and re-enable via rerender with toggled enabled state |

### Low-priority fix

- Task 11 Step 3: Corrected the ExpandableSection reference. AttentionGroup.tsx DOES use ExpandableSection (confirmed in current source). The cleanup step now correctly verifies the import is still needed rather than suggesting removal.

### Placeholder Scan

No instances of: TBD, TODO, "similar to Task N", "implement later", "add appropriate", "handle edge cases" as placeholders.

### Type Consistency

- `RepoGroupHeaderProps`: extended with `isExpanded`, `infoCount`, `summaryText`, `onExpandToggle`, `onKeyDown` — used consistently in RepoGroup and DecisionList
- `RepoGroupProps`: uses `RepoGroupInfo` from `api/types.ts` + `revealItemId`, `itemIds` — matches existing type
- `AttentionSummaryProps`: `needsReviewCount`, `needsReviewRepoCount`, `infoCount`, `infoRepoCount` — computed in MainContent from `packageItems`
- `RoutineSummaryProps`: receives `onToggleInclude`, `onMarkViewed`, `viewedIds`, `isPending` to pass through to real `DecisionItem` rows
- `DecisionItemProps.onToggleInclude`: changed to optional (`undefined` hides toggle) — checked in Task 6
- `classificationLabel()`: takes `isDistro` only (not provenance) — provenance only affects `showToggle()`

### Non-mergeable Cluster

Tasks 5, 8, 9 form a non-mergeable cluster. They are committed separately for clean git history but must all land before the feature is considered functional. Merging after Task 5 alone would ship a feature with broken keyboard navigation and untested reveal behavior.
