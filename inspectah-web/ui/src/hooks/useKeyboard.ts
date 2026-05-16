import { useEffect, useCallback } from "react";

/** All sidebar section IDs in display order (for 1-9 jump). */
const SECTION_IDS = [
  "packages",
  "configs",
  "services",
  "containers",
  "users_groups",
  "network",
  "storage",
  "scheduled_tasks",
  "non_rpm_software",
  "kernel_boot",
  "selinux",
];

export interface UseKeyboardOptions {
  onUndo: () => void;
  onRedo: () => void;
  onTogglePanel: () => void;
  onExport: () => void;
  onSectionChange: (sectionId: string) => void;
  onOpenSearch: () => void;
  onOpenGlobalSearch: () => void;
  onOpenShortcuts: () => void;
}

/** Returns true if the event target is a text input where single-key shortcuts should be suppressed. */
function isTextInput(target: EventTarget | null): boolean {
  if (!target || !(target instanceof HTMLElement)) return false;
  const tag = target.tagName.toLowerCase();
  if (tag === "input" || tag === "textarea" || tag === "select") return true;
  if (target.isContentEditable) return true;
  return false;
}

/** Returns true if any modal/dialog is currently open in the DOM. */
function isDialogOpen(): boolean {
  return document.querySelector('[role="dialog"]') !== null;
}

/**
 * Global keyboard handler for the inspectah refine UI.
 *
 * Attaches a document-level keydown listener that handles:
 * - Ctrl+Z / Ctrl+Shift+Z: undo / redo
 * - Ctrl+E: toggle Containerfile panel
 * - Ctrl+Shift+E: export
 * - Ctrl+K: open global search
 * - /: open section search
 * - ?: open shortcut overlay
 * - 1-9: jump to section by index
 *
 * Single-key shortcuts (/, ?, 1-9) are suppressed when focus is in a text input.
 * Ctrl-chord shortcuts always fire.
 */
export function useKeyboard(options: UseKeyboardOptions): void {
  const {
    onUndo,
    onRedo,
    onTogglePanel,
    onExport,
    onSectionChange,
    onOpenSearch,
    onOpenGlobalSearch,
    onOpenShortcuts,
  } = options;

  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      const inTextInput = isTextInput(e.target);

      // --- Ctrl-chord shortcuts (always active) ---
      if ((e.ctrlKey || e.metaKey) && !e.altKey) {
        if (e.key === "z" && !e.shiftKey) {
          e.preventDefault();
          onUndo();
          return;
        }
        if (e.key === "z" && e.shiftKey) {
          e.preventDefault();
          onRedo();
          return;
        }
        if (e.key === "Z") {
          // Shift+Z on some keyboards sends uppercase Z
          e.preventDefault();
          onRedo();
          return;
        }
        if (e.key === "e" && !e.shiftKey) {
          e.preventDefault();
          onTogglePanel();
          return;
        }
        if (e.key === "e" && e.shiftKey) {
          e.preventDefault();
          onExport();
          return;
        }
        if (e.key === "E") {
          e.preventDefault();
          onExport();
          return;
        }
        if (e.key === "k") {
          e.preventDefault();
          onOpenGlobalSearch();
          return;
        }
      }

      // --- Single-key shortcuts (suppressed in text inputs and behind dialogs) ---
      if (inTextInput || isDialogOpen()) return;

      if (e.key === "/") {
        e.preventDefault();
        onOpenSearch();
        return;
      }

      if (e.key === "?") {
        e.preventDefault();
        onOpenShortcuts();
        return;
      }

      // 1-9 jump to section by index
      const num = parseInt(e.key, 10);
      if (num >= 1 && num <= 9 && num <= SECTION_IDS.length) {
        e.preventDefault();
        onSectionChange(SECTION_IDS[num - 1]);
        return;
      }
    },
    [
      onUndo,
      onRedo,
      onTogglePanel,
      onExport,
      onSectionChange,
      onOpenSearch,
      onOpenGlobalSearch,
      onOpenShortcuts,
    ],
  );

  useEffect(() => {
    document.addEventListener("keydown", handleKeyDown);
    return () => {
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, [handleKeyDown]);
}
