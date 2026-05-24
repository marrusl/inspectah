import { useCallback, useEffect, useRef, useState } from "react";
import { ExclamationTriangleIcon } from "@patternfly/react-icons";
import type { RepoSourceEntry } from "../../api/types";

export interface RepoConflictPopoverProps {
  packageName: string;
  identityKey: string;
  entries: RepoSourceEntry[];
  isDismissed: boolean;
  onDismiss: (key: string) => void;
}

export function RepoConflictPopover({
  packageName,
  identityKey,
  entries,
  isDismissed,
  onDismiss,
}: RepoConflictPopoverProps) {
  const [isOpen, setIsOpen] = useState(false);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const dismissRef = useRef<HTMLButtonElement>(null);

  // Nothing to show
  if (isDismissed || entries.length === 0) return null;

  const accessibleName = `Repo conflict for ${packageName} — ${entries.length} sources`;

  const open = () => {
    setIsOpen(true);
  };

  const closeToTrigger = () => {
    setIsOpen(false);
    // Return focus to trigger after close
    triggerRef.current?.focus();
  };

  const dismiss = () => {
    setIsOpen(false);
    onDismiss(identityKey);
  };

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === "Escape" && isOpen) {
        e.stopPropagation();
        closeToTrigger();
      }
    },
    [isOpen],
  );

  return (
    <span
      className="repo-conflict-popover"
      style={{ position: "relative", display: "inline-block" }}
      onKeyDown={handleKeyDown}
    >
      <button
        ref={triggerRef}
        type="button"
        className="repo-conflict-popover__trigger"
        aria-haspopup="dialog"
        aria-expanded={isOpen ? "true" : "false"}
        aria-label={accessibleName}
        onClick={open}
      >
        <ExclamationTriangleIcon />
      </button>

      {isOpen && (
        <PopoverDialog
          packageName={packageName}
          entries={entries}
          dismissRef={dismissRef}
          onClose={closeToTrigger}
          onDismiss={dismiss}
        />
      )}
    </span>
  );
}

function PopoverDialog({
  packageName,
  entries,
  dismissRef,
  onClose,
  onDismiss,
}: {
  packageName: string;
  entries: RepoSourceEntry[];
  dismissRef: React.RefObject<HTMLButtonElement | null>;
  onClose: () => void;
  onDismiss: () => void;
}) {
  // Focus dismiss button on mount
  useEffect(() => {
    dismissRef.current?.focus();
  }, [dismissRef]);

  return (
    <div
      role="dialog"
      aria-label={`Repo source conflict for ${packageName}`}
      className="repo-conflict-popover__dialog"
    >
      <ul className="repo-conflict-popover__list">
        {entries.map((entry) => (
          <li key={entry.repo}>
            <strong>{entry.repo}</strong> &mdash;{" "}
            {entry.host_count} {entry.host_count === 1 ? "host" : "hosts"}
          </li>
        ))}
      </ul>
      <div className="repo-conflict-popover__actions">
        <button
          ref={dismissRef}
          type="button"
          className="repo-conflict-popover__dismiss"
          onClick={onDismiss}
        >
          Dismiss
        </button>
      </div>
    </div>
  );
}
