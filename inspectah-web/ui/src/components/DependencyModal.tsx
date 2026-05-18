import { Modal, ModalBody, ModalHeader } from "@patternfly/react-core";

export interface DependencyModalProps {
  packageId: string;
  dependencies: string[];
  isOpen: boolean;
  onClose: () => void;
}

export function DependencyModal({
  packageId,
  dependencies,
  isOpen,
  onClose,
}: DependencyModalProps) {
  if (!isOpen) return null;

  const sorted = [...dependencies].sort();

  return (
    <Modal
      isOpen={isOpen}
      onClose={onClose}
      aria-label={`Dependencies for ${packageId}`}
      variant="medium"
      data-testid="dependency-modal"
    >
      <ModalHeader title={`Dependencies: ${packageId}`} />
      <ModalBody>
        <p style={{ marginBottom: "var(--pf-t--global--spacer--sm)" }}>
          ({sorted.length} dependencies)
        </p>
        <ul
          role="list"
          aria-label={`Dependency list for ${packageId}`}
          style={{
            listStyle: "none",
            padding: 0,
            maxHeight: "60vh",
            overflowY: "auto",
          }}
        >
          {sorted.map((dep) => (
            <li
              key={dep}
              style={{
                padding: "var(--pf-t--global--spacer--xs) var(--pf-t--global--spacer--sm)",
                fontFamily: "var(--pf-t--global--font--family--mono)",
                borderBottom: "1px solid var(--pf-t--global--border--color--default)",
              }}
            >
              {dep}
            </li>
          ))}
        </ul>
      </ModalBody>
    </Modal>
  );
}
