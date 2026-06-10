import type { FleetItem } from "../../api/types";
import { itemDisplayName } from "./FleetItemRow";

export interface ItemDetailPaneProps {
  item: FleetItem;
}

export function ItemDetailPane({ item }: ItemDetailPaneProps) {
  const name = itemDisplayName(item.item_id);
  const { count, total } = item.prevalence;
  const kind = item.item_id.kind;

  const isPackage =
    kind === "Package" || kind === "VersionLock" || kind === "NonRpm";

  return (
    <div className="item-detail-pane" data-testid="item-detail-pane">
      <dl className="item-detail-pane__fields">
        <div className="item-detail-pane__field">
          <dt>{isPackage ? "Package" : "Path"}</dt>
          <dd className="item-detail-pane__mono">{name}</dd>
        </div>
        <div className="item-detail-pane__field">
          <dt>Kind</dt>
          <dd>{kind}</dd>
        </div>
        <div className="item-detail-pane__field">
          <dt>Prevalence</dt>
          <dd>
            {count}/{total} hosts
          </dd>
        </div>
        {item.triage && item.triage.bucket !== "universal" && (
          <div className="item-detail-pane__field">
            <dt>Triage</dt>
            <dd>
              {item.triage.bucket.charAt(0).toUpperCase() +
                item.triage.bucket.slice(1)}
            </dd>
          </div>
        )}
        {item.variants && item.variants.count === 1 && (
          <div className="item-detail-pane__field">
            <dt>Content hash</dt>
            <dd className="item-detail-pane__mono">
              {item.variants.options[0]?.hash.substring(0, 12)}
            </dd>
          </div>
        )}
      </dl>
    </div>
  );
}
