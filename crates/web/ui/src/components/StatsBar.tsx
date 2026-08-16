import { useState, useCallback } from "react";
import {
  Toolbar,
  ToolbarContent,
  ToolbarItem,
  ToolbarGroup,
  Button,
  Content,
  Popover,
  Badge,
  Modal,
  ModalBody,
  ModalFooter,
  ModalHeader,
} from "@patternfly/react-core";
import {
  UndoIcon,
  RedoIcon,
  ExportIcon,
  SunIcon,
  MoonIcon,
  CopyIcon,
} from "@patternfly/react-icons";
import type { RefineStats, SectionStats } from "../api/types";

function ThemeToggle() {
  const [dark, setDark] = useState(() =>
    document.documentElement.classList.contains("pf-v6-theme-dark"),
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

function HostnamePopover({
  hostCount,
  hostnames,
}: {
  hostCount: number;
  hostnames: string[];
}) {
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
      aria-label="Aggregate hosts"
      headerContent={
        <span>
          Aggregate Hosts <Badge isRead>{hostCount}</Badge>
        </span>
      }
      bodyContent={
        <div>
          <div
            className="aggregate-hostname-list"
            data-testid="aggregate-hostname-list"
          >
            {sorted.map((h) => (
              <div key={h} className="aggregate-hostname-entry">
                {h}
              </div>
            ))}
          </div>
          <Button
            variant="link"
            icon={<CopyIcon />}
            onClick={handleCopy}
            className="aggregate-hostname-copy"
            data-testid="aggregate-hostname-copy"
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
        className="aggregate-host-trigger"
        data-testid="aggregate-host-trigger"
      >
        <strong>{hostCount}</strong> hosts
      </Button>
    </Popover>
  );
}

export interface AggregateSummary {
  hostCount: number;
  hostnames: string[];
  totalItems: number;
  needsReviewCount: number;
}

export interface StatsBarProps {
  stats: RefineStats | null;
  onUndo: () => void;
  onRedo: () => void;
  onExport: () => void;
  onReset?: () => void;
  isPending: boolean;
  /** Hamburger menu button rendered at < 1024px. */
  hamburger?: React.ReactNode;
  /** When set, render a aggregate-oriented one-line summary instead of single-host counters. */
  aggregateSummary?: AggregateSummary;
}

function stat(value: number | null | undefined, fallback = "-"): string {
  return value != null ? String(value) : fallback;
}

/**
 * One section's counters. `included` and `excluded` are the user's
 * decisions; the advisory bucket is what the tool reported without ever
 * offering a decision, so it is counted apart rather than folded into
 * `excluded`. It is omitted when empty — most hosts have no advisories,
 * and a permanent "0 advisory" would be noise on all of them.
 */
function SectionCounts({
  label,
  section,
}: {
  label: string;
  section: SectionStats | undefined;
}) {
  const advisory = section?.advisory ?? 0;
  return (
    <Content component="small">
      <strong>{label}:</strong> {stat(section?.included)} included /{" "}
      {stat(section?.excluded)} excluded
      {advisory > 0 && <> / {advisory} advisory</>}
    </Content>
  );
}

export function StatsBar({
  stats,
  onUndo,
  onRedo,
  onExport,
  onReset,
  isPending,
  hamburger,
  aggregateSummary,
}: StatsBarProps) {
  const [resetConfirmOpen, setResetConfirmOpen] = useState(false);
  const hasOps = (stats?.ops_applied ?? 0) > 0;

  return (
    <>
      <Toolbar className="inspectah-statsbar" isSticky>
        <ToolbarContent>
          {hamburger && <ToolbarItem>{hamburger}</ToolbarItem>}
          <ToolbarGroup align={{ default: "alignStart" }}>
            {aggregateSummary ? (
              <ToolbarItem>
                <Content component="small" data-testid="aggregate-stats-summary">
                  <HostnamePopover
                    hostCount={aggregateSummary.hostCount}
                    hostnames={aggregateSummary.hostnames}
                  />
                  {" · "}
                  <strong>
                    {aggregateSummary.totalItems.toLocaleString()}
                  </strong>{" "}
                  items
                </Content>
              </ToolbarItem>
            ) : (
              <>
                <ToolbarItem>
                  <SectionCounts
                    label="Packages"
                    section={stats?.sections?.find((s) => s.kind === "package")}
                  />
                </ToolbarItem>
                <ToolbarItem>
                  <SectionCounts
                    label="Configs"
                    section={stats?.sections?.find((s) => s.kind === "config")}
                  />
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
            {onReset && (
              <>
                <ToolbarItem variant="separator" />
                <ToolbarItem>
                  <Button
                    variant="link"
                    isDisabled={!hasOps || isPending}
                    onClick={() => setResetConfirmOpen(true)}
                    data-testid="reset-defaults-btn"
                  >
                    Reset to defaults
                  </Button>
                </ToolbarItem>
              </>
            )}
            <ToolbarItem variant="separator" />
            <ToolbarItem>
              <Button variant="primary" onClick={onExport} icon={<ExportIcon />}>
                Export
              </Button>
            </ToolbarItem>
            <ToolbarItem>
              <ThemeToggle />
            </ToolbarItem>
          </ToolbarGroup>
        </ToolbarContent>
      </Toolbar>
      {resetConfirmOpen && (
        <Modal
          isOpen
          onClose={() => setResetConfirmOpen(false)}
          aria-label="Reset confirmation"
          variant="small"
          data-testid="reset-confirm-modal"
        >
          <ModalHeader title="Reset to defaults?" />
          <ModalBody>
            Reset all items to analysis defaults? This will revert all
            include/exclude changes you have made.
          </ModalBody>
          <ModalFooter>
            <Button
              variant="primary"
              onClick={() => {
                setResetConfirmOpen(false);
                onReset?.();
              }}
              data-testid="reset-confirm-yes"
            >
              Reset to defaults
            </Button>
            <Button
              variant="link"
              onClick={() => setResetConfirmOpen(false)}
            >
              Cancel
            </Button>
          </ModalFooter>
        </Modal>
      )}
    </>
  );
}
