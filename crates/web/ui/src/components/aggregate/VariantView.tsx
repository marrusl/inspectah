import { useState, useRef } from "react";
import { Button } from "@patternfly/react-core";
import type {
  AggregateItem,
  ItemId,
  LanguagePackageVariantPayload,
  VariantPackageList,
  UnmanagedFileVariantPayload,
} from "../../api/types";
import type { UseVariantAckResult } from "../../hooks/useVariantAck";
import type { UseAggregateDiffResult } from "../../hooks/useAggregateDiff";
import { DiffDrawer } from "./DiffDrawer";

export interface VariantViewProps {
  item: AggregateItem;
  ack: UseVariantAckResult;
  onSelectVariant: (itemId: ItemId, hash: string) => void;
  diffHook: UseAggregateDiffResult;
  /** Section ID from the parent AggregateSection — drives section-specific variant content. */
  sectionId?: string;
}

// ---------------------------------------------------------------------------
// Section-specific variant comparison sub-components
// ---------------------------------------------------------------------------

/** Compute a unified package diff across variant package lists. */
interface PackageDiffRow {
  name: string;
  /** Version per variant, keyed by content_hash. Missing = not present. */
  versions: Map<string, string>;
  /** "common" | "changed" | "added" | "removed" — relative to the selected variant. */
  status: "common" | "changed" | "added" | "removed";
}

function buildPackageDiff(variants: VariantPackageList[]): PackageDiffRow[] {
  // Map: package name -> Map<content_hash, version>
  const pkgMap = new Map<string, Map<string, string>>();
  const variantHashes = variants.map((v) => v.content_hash);

  for (const variant of variants) {
    for (const pkg of variant.packages) {
      let hashMap = pkgMap.get(pkg.name);
      if (!hashMap) {
        hashMap = new Map();
        pkgMap.set(pkg.name, hashMap);
      }
      hashMap.set(variant.content_hash, pkg.version);
    }
  }

  const rows: PackageDiffRow[] = [];
  for (const [name, versions] of pkgMap) {
    const presentIn = variantHashes.filter((h) => versions.has(h));
    const uniqueVersions = new Set(versions.values());

    let status: PackageDiffRow["status"];
    if (presentIn.length === variantHashes.length) {
      status = uniqueVersions.size === 1 ? "common" : "changed";
    } else {
      // Present in some but not all — check if in selected variant
      const selectedVariant = variants.find((v) => v.selected);
      if (selectedVariant && versions.has(selectedVariant.content_hash)) {
        status = "removed"; // In selected but missing from another
      } else {
        status = "added"; // Not in selected, added in another
      }
    }

    rows.push({ name, versions, status });
  }

  // Sort: changed first, then added, removed, common
  const ORDER: Record<string, number> = { changed: 0, added: 1, removed: 2, common: 3 };
  rows.sort((a, b) => (ORDER[a.status] ?? 4) - (ORDER[b.status] ?? 4) || a.name.localeCompare(b.name));

  return rows;
}

function LanguagePackageComparison({
  payload,
}: {
  payload: LanguagePackageVariantPayload;
}) {
  const { variant_packages } = payload;
  if (variant_packages.length < 2) return null;

  const rows = buildPackageDiff(variant_packages);

  return (
    <div
      className="variant-view__comparison"
      data-testid="variant-package-comparison"
    >
      <h4 className="variant-view__comparison-heading">Package comparison</h4>
      <table className="variant-view__comparison-table">
        <thead>
          <tr>
            <th>Package</th>
            {variant_packages.map((v) => (
              <th key={v.content_hash}>
                <span className="variant-view__comparison-hash">
                  {v.content_hash.substring(0, 8)}
                </span>
                <span className="variant-view__comparison-hosts">
                  {v.hosts.join(", ")}
                </span>
              </th>
            ))}
            <th>Status</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => (
            <tr key={row.name} className={`variant-view__pkg-row--${row.status}`}>
              <td className="variant-view__comparison-mono">{row.name}</td>
              {variant_packages.map((v) => (
                <td key={v.content_hash}>
                  {row.versions.get(v.content_hash) ?? "—"}
                </td>
              ))}
              <td>
                <span className={`variant-view__status-badge--${row.status}`}>
                  {row.status}
                </span>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

/** Format bytes to a human-readable size string. */
function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024)
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
}

/** Format epoch seconds as a locale-appropriate date string. */
function formatTimestamp(epochSeconds: number): string {
  return new Date(epochSeconds * 1000).toLocaleString();
}

function UnmanagedFileComparison({
  payload,
}: {
  payload: UnmanagedFileVariantPayload;
}) {
  const { variant_metadata } = payload;
  if (variant_metadata.length < 2) return null;

  const contentDiffers = new Set(variant_metadata.map((v) => v.content_hash)).size > 1;

  return (
    <div
      className="variant-view__comparison"
      data-testid="variant-metadata-comparison"
    >
      <h4 className="variant-view__comparison-heading">File metadata comparison</h4>
      {contentDiffers && (
        <span
          className="variant-view__content-differs"
          data-testid="content-differs-indicator"
        >
          Content differs between variants
        </span>
      )}
      <table className="variant-view__comparison-table">
        <thead>
          <tr>
            <th>Variant</th>
            <th>Hosts</th>
            <th>Size</th>
            <th>Last modified</th>
          </tr>
        </thead>
        <tbody>
          {variant_metadata.map((v) => (
            <tr key={v.content_hash}>
              <td className="variant-view__comparison-mono">
                {v.content_hash.substring(0, 8)}
                {v.selected && (
                  <span className="variant-view__comparison-selected">
                    {" "}
                    (selected)
                  </span>
                )}
              </td>
              <td>{v.hosts.join(", ")}</td>
              <td>{formatSize(v.size)}</td>
              <td>{formatTimestamp(v.last_modified)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

export function VariantView({
  item,
  ack,
  onSelectVariant,
  diffHook,
  sectionId,
}: VariantViewProps) {
  const [showDiff, setShowDiff] = useState(false);
  const [diffTargetHash, setDiffTargetHash] = useState<string | null>(null);
  const viewRef = useRef<HTMLDivElement>(null);

  // Guard: return null for items without variants (after hooks)
  if (!item.variants) {
    return null;
  }

  const variants = item.variants;
  const selectedHash = variants.selected;

  const handleSelect = (hash: string) => {
    if (hash !== selectedHash) {
      onSelectVariant(item.item_id, hash);
    }
  };

  const isReviewed = ack.isAcked(item.item_id);

  const handleConfirm = () => {
    if (isReviewed) {
      ack.unconfirm(item.item_id);
    } else {
      ack.confirm(item.item_id);
    }
  };

  const handleDiffVsSelected = (targetHash: string) => {
    setDiffTargetHash(targetHash);
    diffHook.fetchDiff(item.item_id, selectedHash, targetHash);
    setShowDiff(true);
  };

  const handleCloseDiff = () => {
    setShowDiff(false);
    setDiffTargetHash(null);
    diffHook.clearDiff();
  };

  const handleRetry = () => {
    if (diffTargetHash) {
      diffHook.fetchDiff(item.item_id, selectedHash, diffTargetHash);
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    // Don't handle keys when focus is in a text input
    const tag = (e.target as HTMLElement).tagName?.toLowerCase();
    if (tag === "input" || tag === "textarea" || tag === "select") return;

    if (e.key === "Escape" && showDiff) {
      e.preventDefault();
      handleCloseDiff();
      return;
    }
  };

  // Build operand descriptions for the diff drawer header
  const selectedOption = variants.options.find((o) => o.hash === selectedHash);
  const targetOption = diffTargetHash
    ? variants.options.find((o) => o.hash === diffTargetHash)
    : null;

  return (
    <div
      ref={viewRef}
      className="variant-view"
      data-testid="variant-view"
      onKeyDown={handleKeyDown}
      tabIndex={-1}
    >
      <div
        className="variant-view__options"
        role="radiogroup"
        aria-label="Variant options"
      >
        {variants.options.map((option) => {
          const hostLabel =
            option.host_count === 1 ? "1 host" : `${option.host_count} hosts`;
          const isSelected = option.hash === selectedHash;

          return (
            <label key={option.hash} className="variant-view__option">
              <input
                type="radio"
                name={`variant-${JSON.stringify(item.item_id)}`}
                value={option.hash}
                checked={isSelected}
                onChange={() => handleSelect(option.hash)}
              />
              <span className="variant-view__option-info">
                <span className="variant-view__option-hash">
                  {option.hash.substring(0, 8)}
                </span>
                <span className="variant-view__option-hosts">{hostLabel}:</span>
                <span className="variant-view__option-hostnames">
                  {option.hosts.join(", ")}
                </span>
              </span>
              {isSelected && (
                <span
                  className="variant-view__selected-indicator"
                  data-testid="variant-selected-indicator"
                >
                  Selected
                </span>
              )}
              {!isSelected && variants.options.length >= 2 && (
                <Button
                  variant="link"
                  isInline
                  className="variant-view__diff-link"
                  onClick={(e) => {
                    e.preventDefault();
                    handleDiffVsSelected(option.hash);
                  }}
                >
                  Diff vs selected
                </Button>
              )}
            </label>
          );
        })}
      </div>

      {sectionId === "language_packages" &&
        item.variant_payload &&
        "variant_packages" in item.variant_payload && (
          <LanguagePackageComparison
            payload={item.variant_payload as unknown as LanguagePackageVariantPayload}
          />
        )}

      {sectionId === "unmanaged_files" &&
        item.variant_payload &&
        "variant_metadata" in item.variant_payload && (
          <UnmanagedFileComparison
            payload={item.variant_payload as unknown as UnmanagedFileVariantPayload}
          />
        )}

      <div className="variant-view__actions">
        {isReviewed ? (
          <button
            type="button"
            className="variant-view__reviewed-indicator"
            onClick={handleConfirm}
            data-testid="variant-reviewed-indicator"
            aria-label="Undo review"
          >
            Reviewed
          </button>
        ) : (
          <Button variant="primary" onClick={handleConfirm}>
            Mark as reviewed
          </Button>
        )}
      </div>

      {showDiff && (
        <DiffDrawer
          diff={diffHook.diff}
          isLoading={diffHook.isLoading}
          error={diffHook.error}
          onRetry={handleRetry}
          onClose={handleCloseDiff}
          targetLabel={
            targetOption
              ? `${targetOption.hash.substring(0, 8)} (${targetOption.hosts.join(", ")})`
              : undefined
          }
          baseLabel={
            selectedOption
              ? `${selectedOption.hash.substring(0, 8)} (${selectedOption.hosts.join(", ")}) [selected]`
              : undefined
          }
        />
      )}
    </div>
  );
}
