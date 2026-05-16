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
  onUndo: () => void;
  onRedo: () => void;
  onExport: () => void;
  isPending: boolean;
}

function stat(value: number | null | undefined, fallback = "-"): string {
  return value != null ? String(value) : fallback;
}

export function StatsBar({
  stats,
  onUndo,
  onRedo,
  onExport,
  isPending,
}: StatsBarProps) {
  const remaining = stats
    ? stat(stats.needs_review_count)
    : "-";
  const totalDecisions = stats
    ? stats.total_packages + stats.total_configs
    : null;
  return (
    <Toolbar className="inspectah-statsbar" isSticky>
      <ToolbarContent>
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
              {remaining} of{" "}
              {totalDecisions != null ? String(totalDecisions) : "-"}{" "}
              remaining
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
