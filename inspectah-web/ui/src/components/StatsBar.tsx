import { useState, useCallback } from "react";
import {
  Toolbar,
  ToolbarContent,
  ToolbarItem,
  ToolbarGroup,
  Button,
  Content,
  Label,
  Popover,
  Badge,
} from "@patternfly/react-core";
import { UndoIcon, RedoIcon, ExportIcon, SunIcon, MoonIcon, CopyIcon } from "@patternfly/react-icons";
import type { RefineStats } from "../api/types";

function ThemeToggle() {
  const [dark, setDark] = useState(
    () => document.documentElement.classList.contains("pf-v6-theme-dark"),
  );
  const toggle = useCallback(() => {
    const next = !dark;
    document.documentElement.classList.toggle("pf-v6-theme-dark", next);
    if (next === window.matchMedia("(prefers-color-scheme: dark)").matches) {
      localStorage.removeItem("inspectah-theme");
    } else {
      localStorage.setItem("inspectah-theme", next ? "dark" : "light");
    }
    setDark(next);
  }, [dark]);
  return (
    <Button
      variant="plain"
      aria-label={dark ? "Switch to light mode" : "Switch to dark mode"}
      onClick={toggle}
      icon={dark ? <SunIcon /> : <MoonIcon />}
    />
  );
}

function HostnamePopover({ hostCount, hostnames }: { hostCount: number; hostnames: string[] }) {
  const [copied, setCopied] = useState(false);
  const sorted = [...hostnames].sort((a, b) => a.localeCompare(b));

  const handleCopy = useCallback(() => {
    navigator.clipboard.writeText(sorted.join("\n")).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    });
  }, [sorted]);

  return (
    <Popover
      aria-label="Fleet hosts"
      headerContent={
        <span>
          Fleet Hosts <Badge isRead>{hostCount}</Badge>
        </span>
      }
      bodyContent={
        <div>
          <div className="fleet-hostname-list" data-testid="fleet-hostname-list">
            {sorted.map((h) => (
              <div key={h} className="fleet-hostname-entry">{h}</div>
            ))}
          </div>
          <Button
            variant="link"
            icon={<CopyIcon />}
            onClick={handleCopy}
            className="fleet-hostname-copy"
            data-testid="fleet-hostname-copy"
            size="sm"
          >
            {copied ? "Copied!" : "Copy all"}
          </Button>
        </div>
      }
    >
      <Button
        variant="link"
        isInline
        className="fleet-host-trigger"
        data-testid="fleet-host-trigger"
      >
        <strong>{hostCount}</strong> hosts
      </Button>
    </Popover>
  );
}

export interface FleetSummary {
  hostCount: number;
  hostnames: string[];
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
                <HostnamePopover
                  hostCount={fleetSummary.hostCount}
                  hostnames={fleetSummary.hostnames}
                />{" · "}
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
          <ToolbarItem>
            <ThemeToggle />
          </ToolbarItem>
        </ToolbarGroup>
      </ToolbarContent>
    </Toolbar>
  );
}
