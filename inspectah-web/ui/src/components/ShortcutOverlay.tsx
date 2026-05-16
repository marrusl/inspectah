import { Modal, ModalBody, ModalHeader } from "@patternfly/react-core";

interface ShortcutGroup {
  title: string;
  shortcuts: { keys: string; description: string }[];
}

const SHORTCUT_GROUPS: ShortcutGroup[] = [
  {
    title: "Navigation",
    shortcuts: [
      { keys: "j / ArrowDown", description: "Next item" },
      { keys: "k / ArrowUp", description: "Previous item" },
      { keys: "g", description: "First item" },
      { keys: "G", description: "Last item" },
      { keys: "1-9", description: "Jump to section by index" },
    ],
  },
  {
    title: "Actions",
    shortcuts: [
      { keys: "Space / x", description: "Toggle include/exclude" },
      { keys: "Enter", description: "Expand/collapse detail" },
    ],
  },
  {
    title: "Global",
    shortcuts: [
      { keys: "/", description: "Open section search" },
      { keys: "Ctrl+Z", description: "Undo" },
      { keys: "Ctrl+Shift+Z", description: "Redo" },
      { keys: "Ctrl+E", description: "Toggle Containerfile panel" },
      { keys: "Ctrl+Shift+E", description: "Export" },
      { keys: "?", description: "Show keyboard shortcuts" },
      { keys: "Escape", description: "Close search / overlay" },
    ],
  },
];

export interface ShortcutOverlayProps {
  isOpen: boolean;
  onClose: () => void;
}

/**
 * Modal overlay listing all keyboard shortcuts, organized by category.
 * Opened by pressing `?`, closed by `Escape` or `?` again.
 */
export function ShortcutOverlay({ isOpen, onClose }: ShortcutOverlayProps) {
  if (!isOpen) return null;

  return (
    <Modal
      isOpen={isOpen}
      onClose={onClose}
      aria-label="Keyboard shortcuts"
      variant="medium"
      data-testid="shortcut-overlay"
    >
      <ModalHeader title="Keyboard Shortcuts" />
      <ModalBody>
        {SHORTCUT_GROUPS.map((group) => (
          <div
            key={group.title}
            style={{ marginBottom: "var(--pf-t--global--spacer--lg)" }}
          >
            <h4
              style={{
                marginBottom: "var(--pf-t--global--spacer--sm)",
                fontWeight: 600,
              }}
            >
              {group.title}
            </h4>
            <table
              style={{ width: "100%", borderCollapse: "collapse" }}
              data-testid={`shortcuts-${group.title.toLowerCase()}`}
            >
              <tbody>
                {group.shortcuts.map((sc) => (
                  <tr key={sc.keys}>
                    <td
                      style={{
                        padding: "var(--pf-t--global--spacer--xs) var(--pf-t--global--spacer--sm)",
                        fontFamily: "var(--pf-t--global--font--family--mono)",
                        fontSize: "var(--pf-t--global--font--size--sm)",
                        whiteSpace: "nowrap",
                        width: "40%",
                      }}
                    >
                      {sc.keys}
                    </td>
                    <td
                      style={{
                        padding: "var(--pf-t--global--spacer--xs) var(--pf-t--global--spacer--sm)",
                      }}
                    >
                      {sc.description}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ))}
      </ModalBody>
    </Modal>
  );
}
