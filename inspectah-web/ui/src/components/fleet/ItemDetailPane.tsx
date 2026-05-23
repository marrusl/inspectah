import type { FleetItem } from "../../api/types";
import { itemDisplayName, attentionDisplayLabel } from "./FleetItemRow";

export interface ItemDetailPaneProps {
  item: FleetItem;
}

export function ItemDetailPane({ item }: ItemDetailPaneProps) {
  const name = itemDisplayName(item.item_id);
  const { count, total } = item.prevalence;
  const kind = item.item_id.kind;

  const isPackage = kind === "Package" || kind === "VersionLock" || kind === "NonRpm";

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
          <dd>{count}/{total} hosts</dd>
        </div>
        {item.attention.level !== "none" && (
          <div className="item-detail-pane__field">
            <dt>Attention</dt>
            <dd>{attentionDisplayLabel(item.attention.level)}</dd>
          </div>
        )}
        {item.attention.reason && (
          <div className="item-detail-pane__field">
            <dt>Reason</dt>
            <dd>{item.attention.reason.replace(/_/g, " ")}</dd>
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
