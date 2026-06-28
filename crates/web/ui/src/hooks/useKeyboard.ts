import { useEffect, useCallback } from "react";

/** Single-host sidebar section IDs in display order (for 1-9 jump). */
const SINGLE_HOST_SECTION_IDS = [
  "packages", // 1
  "configs", // 2
  "users_groups", // 3
  "services", // 4
  "containers", // 5
  "language_packages", // 6
  "unmanaged_files", // 7
  "version_changes", // 8
  "compose", // 9
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
  /** Override section IDs for 1-9 keyboard navigation. When omitted,
   *  defaults to single-host section IDs. Aggregate mode passes its
   *  own section list since the IDs differ. */
  sectionIds?: string[];
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
 * ALL shortcuts are suppressed when a modal dialog is open.
 * Single-key shortcuts (/, ?, 1-9) are also suppressed when focus is in a text input.
 * Ctrl-chord shortcuts fire in text inputs but not in dialogs.
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
    sectionIds = SINGLE_HOST_SECTION_IDS,
  } = options;

  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      // --- Dialog guard: suppress ALL shortcuts when a dialog is open ---
      if (isDialogOpen()) return;

      const inTextInput = isTextInput(e.target);

      // --- Ctrl-chord shortcuts (active even in text inputs) ---
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

      // --- Single-key shortcuts (suppressed in text inputs) ---
      if (inTextInput) return;

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
      if (num >= 1 && num <= 9 && num <= sectionIds.length) {
        e.preventDefault();
        onSectionChange(sectionIds[num - 1]);
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
      sectionIds,
    ],
  );

  useEffect(() => {
    document.addEventListener("keydown", handleKeyDown);
    return () => {
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, [handleKeyDown]);
}
