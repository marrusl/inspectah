import { EmptyState } from "@patternfly/react-core";
import { CubesIcon } from "@patternfly/react-icons";
import { Table, Thead, Tbody, Tr, Th, Td } from "@patternfly/react-table";
import type { VersionChangeEntry } from "../api/types";
import { formatEvrPair } from "./evrFormat";

export interface VersionChangesTableProps {
  entries: VersionChangeEntry[];
  emptyReason?: string | null;
  revealItemId?: string;
}

export function VersionChangesTable({
  entries,
  emptyReason,
  revealItemId,
}: VersionChangesTableProps) {
  if (entries.length === 0) {
    const copyMap: Record<string, string> = {
      zero_drift: "All packages match the target baseline versions.",
      data_unavailable:
        "Version change data is not available for this snapshot.",
    };
    const title =
      emptyReason && copyMap[emptyReason]
        ? copyMap[emptyReason]
        : "No Version Changes data in this snapshot";
    return <EmptyState titleText={title} icon={CubesIcon} headingLevel="h3" />;
  }

  const downgrades = entries.filter((e) => e.direction === "downgrade");
  const upgrades = entries.filter((e) => e.direction === "upgrade");

  const renderGroup = (
    label: string,
    items: VersionChangeEntry[],
    variant: "danger" | "success",
  ) => {
    if (items.length === 0) return null;
    const arrow = variant === "danger" ? "▼" : "▲";
    return (
      <Tbody>
        <Tr
          aria-label={`${items.length} ${label.toLowerCase()}`}
          className={`inspectah-vc-group-header inspectah-vc-group-header--${variant}`}
        >
          <Td colSpan={3} className="inspectah-vc-group-header__cell">
            {arrow} {label} ({items.length})
          </Td>
        </Tr>
        {items.map((vc) => {
          const id = `${vc.name}.${vc.arch}`;
          const [baseFmt, hostFmt] = formatEvrPair(
            vc.base_epoch,
            vc.base_version,
            vc.host_epoch,
            vc.host_version,
          );
          return (
            <Tr
              key={id}
              data-testid={`context-item-${id}`}
              tabIndex={-1}
              className={
                revealItemId === id
                  ? "inspectah-vc-row--revealed"
                  : undefined
              }
            >
              <Td dataLabel="Package">
                {vc.name}.{vc.arch}
              </Td>
              <Td dataLabel="Host Version" className="inspectah-vc-version">
                {hostFmt}
              </Td>
              <Td dataLabel="Target Version" className="inspectah-vc-version">
                {baseFmt}
              </Td>
            </Tr>
          );
        })}
      </Tbody>
    );
  };

  return (
    <Table variant="compact" aria-label="Version changes">
      <Thead>
        <Tr>
          <Th>Package</Th>
          <Th>Host Version</Th>
          <Th>Target Version</Th>
        </Tr>
      </Thead>
      {renderGroup("Downgrades", downgrades, "danger")}
      {renderGroup("Upgrades", upgrades, "success")}
    </Table>
  );
}
