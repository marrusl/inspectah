import {
  Toolbar,
  ToolbarContent,
  ToolbarItem,
  ToolbarGroup,
  Button,
  Content,
  Label,
} from "@patternfly/react-core";
import { UndoIcon, RedoIcon, ExportIcon } from "@patternfly/react-icons";
import type { RefineStats } from "../api/types";

export interface FleetSummary {
  hostCount: number;
  totalItems: number;
  needsReviewCount: number;
}

export interface StatsBarProps {
  stats: RefineStats | null;
  /** Number of NeedsReview items the user has viewed/triaged. */
  viewedNeedsReviewCount?: number;
  onUndo: () => void;
  onRedo: () => void;
  onExport: () => void;
  isPending: boolean;
  /** Hamburger menu button rendered at < 1024px. */
  hamburger?: React.ReactNode;
  /** When set, render a fleet-oriented one-line summary instead of single-host counters. */
  fleetSummary?: FleetSummary;
}

function stat(value: number | null | undefined, fallback = "-"): string {
  return value != null ? String(value) : fallback;
}

export function StatsBar({
  stats,
  viewedNeedsReviewCount = 0,
  onUndo,
  onRedo,
  onExport,
  isPending,
  hamburger,
  fleetSummary,
}: StatsBarProps) {
  const needsReviewTotal = stats?.needs_review_count ?? null;
  const remaining = needsReviewTotal != null
    ? Math.max(0, needsReviewTotal - viewedNeedsReviewCount)
    : null;

  // Completion signal logic
  const showCompletionSignal = needsReviewTotal != null && remaining !== null;
  const isComplete = showCompletionSignal && remaining === 0;

  return (
    <Toolbar className="inspectah-statsbar" isSticky>
      <ToolbarContent>
        {hamburger && (
          <ToolbarItem>{hamburger}</ToolbarItem>
        )}
        <ToolbarGroup align={{ default: "alignStart" }}>
          {fleetSummary ? (
            <ToolbarItem>
              <Content component="small" data-testid="fleet-stats-summary">
                <strong>{fleetSummary.hostCount}</strong> hosts{" · "}
                <strong>{fleetSummary.totalItems.toLocaleString()}</strong> items{" · "}
                {fleetSummary.needsReviewCount > 0 ? (
                  <Label color="blue">{fleetSummary.needsReviewCount} need review</Label>
                ) : (
                  <Label color="green">All reviewed</Label>
                )}
              </Content>
            </ToolbarItem>
          ) : (
            <>
              <ToolbarItem>
                <Content component="small">
                  <strong>Packages:</strong>{" "}
                  {stat(stats?.sections?.find(s => s.kind === "package")?.included)} included /{" "}
                  {stat(stats?.sections?.find(s => s.kind === "package")?.excluded)} excluded
                </Content>
              </ToolbarItem>
              <ToolbarItem>
                <Content component="small">
                  <strong>Configs:</strong>{" "}
                  {stat(stats?.sections?.find(s => s.kind === "config")?.included)} included /{" "}
                  {stat(stats?.sections?.find(s => s.kind === "config")?.excluded)} excluded
                </Content>
              </ToolbarItem>
              <ToolbarItem>
                <Content component="small">
                  <strong>Triage:</strong>{" "}
                  {showCompletionSignal ? (
                    isComplete ? (
                      <Label color="green">All actionable items reviewed</Label>
                    ) : (
                      <Label color="blue">{remaining} items remaining</Label>
                    )
                  ) : (
                    <>
                      {remaining != null ? String(remaining) : "-"} of{" "}
                      {needsReviewTotal != null ? String(needsReviewTotal) : "-"}{" "}
                      to review
                    </>
                  )}
                </Content>
              </ToolbarItem>
            </>
          )}
        </ToolbarGroup>
        <ToolbarGroup align={{ default: "alignEnd" }}>
          <ToolbarItem>
            <Button
              variant="plain"
              aria-label="Undo"
              isDisabled={!stats?.can_undo || isPending}
              onClick={onUndo}
              icon={<UndoIcon />}
            />
          </ToolbarItem>
          <ToolbarItem>
            <Button
              variant="plain"
              aria-label="Redo"
              isDisabled={!stats?.can_redo || isPending}
              onClick={onRedo}
              icon={<RedoIcon />}
            />
          </ToolbarItem>
          <ToolbarItem variant="separator" />
          <ToolbarItem>
            <Button
              variant="primary"
              onClick={onExport}
              icon={<ExportIcon />}
            >
              Export
            </Button>
          </ToolbarItem>
        </ToolbarGroup>
      </ToolbarContent>
    </Toolbar>
  );
}
