import {
  Toolbar,
  ToolbarContent,
  ToolbarItem,
  ToolbarGroup,
  Button,
  Content,
} from "@patternfly/react-core";
import { UndoIcon, RedoIcon, ExportIcon } from "@patternfly/react-icons";
import type { RefineStats } from "../api/types";

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
}: StatsBarProps) {
  const needsReviewTotal = stats?.needs_review_count ?? null;
  const remaining = needsReviewTotal != null
    ? String(Math.max(0, needsReviewTotal - viewedNeedsReviewCount))
    : "-";
  return (
    <Toolbar className="inspectah-statsbar" isSticky>
      <ToolbarContent>
        {hamburger && (
          <ToolbarItem>{hamburger}</ToolbarItem>
        )}
        <ToolbarGroup align={{ default: "alignStart" }}>
          <ToolbarItem>
            <Content component="small">
              <strong>Packages:</strong>{" "}
              {stat(stats?.included_packages)} included /{" "}
              {stat(stats?.excluded_packages)} excluded
            </Content>
          </ToolbarItem>
          <ToolbarItem>
            <Content component="small">
              <strong>Configs:</strong>{" "}
              {stat(stats?.included_configs)} included /{" "}
              {stat(stats?.excluded_configs)} excluded
            </Content>
          </ToolbarItem>
          <ToolbarItem>
            <Content component="small">
              <strong>Triage:</strong>{" "}
              {needsReviewTotal != null && needsReviewTotal > 0 && viewedNeedsReviewCount >= needsReviewTotal
                ? "All items have been triaged"
                : <>
                    {remaining} of{" "}
                    {needsReviewTotal != null ? String(needsReviewTotal) : "-"}{" "}
                    to review
                  </>
              }
            </Content>
          </ToolbarItem>
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
