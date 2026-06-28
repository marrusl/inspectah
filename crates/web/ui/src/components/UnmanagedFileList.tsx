import { useState, useCallback, useMemo, useRef, useEffect } from "react";
import { Badge, Button, Content, Label } from "@patternfly/react-core";
import { AngleRightIcon, AngleDownIcon } from "@patternfly/react-icons";
import type {
  UnmanagedFileGroup,
  UnmanagedFileItem,
  ProvenanceSignals,
} from "../api/types";

/** Debounce delay for rollup screen-reader announcement (ms). */
const ROLLUP_ANNOUNCE_DELAY_MS = 500;

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
        <Label color="orange" isCompact>
          modified since install
        </Label>
      )}
      {signals.writable_mount && (
        <Label color="orange" isCompact>
          writable mount
        </Label>
      )}
      {signals.service_working_dir && (
        <Label color="blue" isCompact>
          service workdir
        </Label>
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

/**
 * Collect all focusable element refs in document order:
 * [group0-header, group0-item0, group0-item1, ..., group1-header, ...].
 * Only includes items from expanded groups.
 */
function getFocusableElements(
  containerRef: React.RefObject<HTMLDivElement | null>,
): HTMLElement[] {
  if (!containerRef.current) return [];
  return Array.from(
    containerRef.current.querySelectorAll<HTMLElement>(
      "[data-focusable='true']",
    ),
  );
}

function FileRow({
  item,
  onToggle,
  isPending,
  isRevealed,
  onKeyDown,
  announceText,
}: {
  item: UnmanagedFileItem;
  onToggle: (path: string) => void;
  isPending: boolean;
  isRevealed: boolean;
  onKeyDown: (e: React.KeyboardEvent) => void;
  announceText: string;
}) {
  const handleToggle = useCallback(() => {
    onToggle(item.path);
  }, [onToggle, item.path]);

  return (
    <div
      className="inspectah-unmanaged-row"
      data-testid={`unmanaged-item-${item.path}`}
      data-revealed={isRevealed ? "true" : undefined}
      data-focusable="true"
      role="listitem"
      tabIndex={-1}
      aria-label={item.path}
      onKeyDown={onKeyDown}
    >
      <input
        type="checkbox"
        role="checkbox"
        checked={item.include}
        disabled={isPending}
        aria-label={`Toggle ${item.path}`}
        onChange={handleToggle}
      />
      <span className="inspectah-unmanaged-row__path">
        {item.path.split("/").pop()}
      </span>
      <span className="inspectah-unmanaged-row__type">
        {item.provenance.file_type}
      </span>
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
      <div
        className="inspectah-unmanaged-row__announce"
        data-testid={`unmanaged-item-announce-${item.path}`}
        aria-live="polite"
        role="status"
      >
        {announceText}
      </div>
    </div>
  );
}

function DirectoryGroup({
  group,
  onToggleItem,
  onToggleGroup,
  isPending,
  revealItemId,
  onKeyDownHeader,
  onKeyDownItem,
  groupAnnounceText,
  itemAnnounceTexts,
}: {
  group: UnmanagedFileGroup;
  onToggleItem: (path: string) => void;
  onToggleGroup: (directory: string, include: boolean) => void;
  isPending: boolean;
  revealItemId?: string;
  onKeyDownHeader: (e: React.KeyboardEvent) => void;
  onKeyDownItem: (e: React.KeyboardEvent) => void;
  groupAnnounceText: string;
  itemAnnounceTexts: Record<string, string>;
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

  const handleHeaderKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        handleExpandCollapse();
        return;
      }
      if (e.key === "ArrowRight" && !shouldExpand) {
        e.preventDefault();
        setIsExpanded(true);
        return;
      }
      if (e.key === "ArrowLeft" && shouldExpand) {
        e.preventDefault();
        setIsExpanded(false);
        return;
      }
      // Delegate ArrowUp/ArrowDown to parent handler
      onKeyDownHeader(e);
    },
    [shouldExpand, handleExpandCollapse, onKeyDownHeader],
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
        onKeyDown={handleHeaderKeyDown}
        tabIndex={0}
        role="button"
        aria-expanded={shouldExpand}
        data-focusable="true"
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
          {groupIncludedCount} of {group.items.length} included, ~
          {formatSize(includedSize)} of ~{formatSize(groupSize)}
        </span>
        {isVarGroup && (
          <span className="inspectah-unmanaged-group__var-badge">/var</span>
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
              onKeyDown={onKeyDownItem}
              announceText={itemAnnounceTexts[item.path] ?? ""}
            />
          ))}
        </div>
      )}
      <div
        className="inspectah-unmanaged-group__announce"
        data-testid={`unmanaged-group-announce-${group.directory}`}
        aria-live="polite"
        role="status"
      >
        {groupAnnounceText}
      </div>
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
  const containerRef = useRef<HTMLDivElement>(null);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const announceTimeoutsRef = useRef<Map<string, ReturnType<typeof setTimeout>>>(new Map());
  const [rollupAnnounceText, setRollupAnnounceText] = useState("");
  const [groupAnnounces, setGroupAnnounces] = useState<Record<string, string>>(
    {},
  );
  const [itemAnnounces, setItemAnnounces] = useState<Record<string, string>>(
    {},
  );

  const allItems = useMemo(() => groups.flatMap((g) => g.items), [groups]);

  const totalCount = allItems.length;
  const includedCount = allItems.filter((i) => i.include).length;
  const totalSize = allItems.reduce((sum, i) => sum + i.size, 0);
  const includedSize = allItems
    .filter((i) => i.include)
    .reduce((sum, i) => sum + i.size, 0);

  /** Schedule a debounced rollup announcement for screen readers. */
  const scheduleRollupAnnounce = useCallback(() => {
    if (debounceRef.current) clearTimeout(debounceRef.current);
    debounceRef.current = setTimeout(() => {
      setRollupAnnounceText(
        `${includedCount} of ${totalCount} items included, ~${formatSize(includedSize)} of ~${formatSize(totalSize)}`,
      );
    }, ROLLUP_ANNOUNCE_DELAY_MS);
  }, [includedCount, totalCount, includedSize, totalSize]);

  // Clean up debounce and announce timeouts on unmount.
  useEffect(() => {
    return () => {
      if (debounceRef.current) clearTimeout(debounceRef.current);
      announceTimeoutsRef.current.forEach((timeout) => clearTimeout(timeout));
      announceTimeoutsRef.current.clear();
    };
  }, []);

  /** Wrap onToggleItem to fire announcement + schedule rollup. */
  const handleToggleItem = useCallback(
    (path: string) => {
      const item = allItems.find((i) => i.path === path);
      if (item) {
        const action = item.include ? "Excluded" : "Included";
        setItemAnnounces((prev) => ({
          ...prev,
          [path]: `${action} ${path}`,
        }));

        // Clear any existing timeout for this item
        const existingTimeout = announceTimeoutsRef.current.get(path);
        if (existingTimeout) clearTimeout(existingTimeout);

        // Schedule new timeout to clear announce text after 3s
        const timeout = setTimeout(() => {
          setItemAnnounces((prev) => ({ ...prev, [path]: "" }));
          announceTimeoutsRef.current.delete(path);
        }, 3000);
        announceTimeoutsRef.current.set(path, timeout);
      }
      onToggleItem(path);
      scheduleRollupAnnounce();
    },
    [allItems, onToggleItem, scheduleRollupAnnounce],
  );

  /** Wrap onToggleGroup to fire announcement + schedule rollup. */
  const handleToggleGroup = useCallback(
    (directory: string, include: boolean) => {
      const group = groups.find((g) => g.directory === directory);
      const count = group?.items.length ?? 0;
      const action = include ? "Included" : "Excluded";
      setGroupAnnounces((prev) => ({
        ...prev,
        [directory]: `${action} ${count} files in ${directory}`,
      }));

      // Clear any existing timeout for this group
      const existingTimeout = announceTimeoutsRef.current.get(directory);
      if (existingTimeout) clearTimeout(existingTimeout);

      // Schedule new timeout to clear announce text after 3s
      const timeout = setTimeout(() => {
        setGroupAnnounces((prev) => ({ ...prev, [directory]: "" }));
        announceTimeoutsRef.current.delete(directory);
      }, 3000);
      announceTimeoutsRef.current.set(directory, timeout);

      onToggleGroup(directory, include);
      scheduleRollupAnnounce();
    },
    [groups, onToggleGroup, scheduleRollupAnnounce],
  );

  /** Move focus to the next/previous focusable element relative to current. */
  const moveFocus = useCallback((direction: "up" | "down") => {
    const focusable = getFocusableElements(containerRef);
    const current = document.activeElement as HTMLElement;
    const idx = focusable.indexOf(current);
    if (idx === -1) return;
    const next = direction === "down" ? focusable[idx + 1] : focusable[idx - 1];
    if (next) next.focus();
  }, []);

  const handleKeyDownHeader = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === "ArrowDown") {
        e.preventDefault();
        moveFocus("down");
      }
      if (e.key === "ArrowUp") {
        e.preventDefault();
        moveFocus("up");
      }
    },
    [moveFocus],
  );

  const handleKeyDownItem = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === "ArrowDown") {
        e.preventDefault();
        moveFocus("down");
      }
      if (e.key === "ArrowUp") {
        e.preventDefault();
        moveFocus("up");
      }
    },
    [moveFocus],
  );

  return (
    <div
      className="inspectah-unmanaged-list"
      data-testid="unmanaged-file-list"
      ref={containerRef}
    >
      <div className="inspectah-unmanaged-list__header">
        <Content
          component="small"
          data-testid="unmanaged-rollup"
          aria-live="polite"
        >
          {includedCount} of {totalCount} items included, ~
          {formatSize(includedSize)} of ~{formatSize(totalSize)}
        </Content>
        <div className="inspectah-unmanaged-list__actions">
          {onIncludeNone && (
            <Button
              variant="link"
              size="sm"
              onClick={onIncludeNone}
              isDisabled={isPending || includedCount === 0}
            >
              Include None
            </Button>
          )}
          {onResetAll && (
            <Button
              variant="link"
              size="sm"
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
          onToggleItem={handleToggleItem}
          onToggleGroup={handleToggleGroup}
          isPending={isPending}
          revealItemId={revealItemId}
          onKeyDownHeader={handleKeyDownHeader}
          onKeyDownItem={handleKeyDownItem}
          groupAnnounceText={groupAnnounces[group.directory] ?? ""}
          itemAnnounceTexts={itemAnnounces}
        />
      ))}
      <div
        className="inspectah-unmanaged-list__rollup-announce"
        data-testid="unmanaged-rollup-announce"
        aria-live="polite"
        role="status"
      >
        {rollupAnnounceText}
      </div>
    </div>
  );
}
