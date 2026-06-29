import type {
  AggregateItem,
  LanguagePackageMetadata,
  UnmanagedFileMetadata,
} from "../../api/types";
import { itemDisplayName } from "./AggregateItemRow";

export interface ItemDetailPaneProps {
  item: AggregateItem;
  /** Section ID from the parent AggregateSection — drives section-specific detail content. */
  sectionId?: string;
}

/** Format epoch seconds as a locale-appropriate date string. */
function formatTimestamp(epochSeconds: number): string {
  return new Date(epochSeconds * 1000).toLocaleString();
}

function LanguagePackageDetail({ item }: { item: AggregateItem }) {
  const meta = item.section_metadata as unknown as LanguagePackageMetadata;
  if (!meta) return null;

  return (
    <div className="item-detail-pane__section-detail">
      <div className="item-detail-pane__field">
        <dt>Confidence</dt>
        <dd data-testid="detail-confidence">{meta.confidence}</dd>
      </div>
      {meta.manifest_basis != null && (
        <div className="item-detail-pane__field" data-testid="detail-manifest-basis">
          <dt>Source</dt>
          <dd>from {meta.manifest_basis}</dd>
        </div>
      )}
      {meta.packages.length > 0 && (
        <table
          className="item-detail-pane__pkg-table"
          data-testid="detail-package-table"
        >
          <thead>
            <tr>
              <th>Package</th>
              <th>Version</th>
            </tr>
          </thead>
          <tbody>
            {meta.packages.map((pkg) => (
              <tr key={`${pkg.name}-${pkg.version}`}>
                <td className="item-detail-pane__mono">{pkg.name}</td>
                <td className="item-detail-pane__mono">{pkg.version}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}

/** Format bytes as a human-readable size string. */
function formatFileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024)
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
}

/** Format a snake_case file type as a human-readable label. */
function formatFileType(fileType: string): string {
  return fileType
    .split("_")
    .map((w) => w.charAt(0).toUpperCase() + w.slice(1))
    .join(" ");
}

function UnmanagedFileDetail({ item }: { item: AggregateItem }) {
  const meta = item.section_metadata as unknown as UnmanagedFileMetadata;
  if (!meta?.provenance) return null;

  const prov = meta.provenance;

  return (
    <div
      className="item-detail-pane__section-detail"
      data-testid="detail-provenance"
    >
      <div className="item-detail-pane__field">
        <dt>File type</dt>
        <dd data-testid="detail-file-type">{formatFileType(meta.file_type)}</dd>
      </div>
      <div className="item-detail-pane__field">
        <dt>Size</dt>
        <dd data-testid="detail-file-size">{formatFileSize(meta.size)}</dd>
      </div>
      {meta.under_var && (
        <div
          className="item-detail-pane__field item-detail-pane__warning"
          data-testid="detail-var-warning"
        >
          <dt>Warning</dt>
          <dd>
            File is under /var — contents may not persist across image updates
          </dd>
        </div>
      )}
      <div className="item-detail-pane__field">
        <dt>Last modified</dt>
        <dd data-testid="detail-last-modified">
          {formatTimestamp(prov.last_modified)}
        </dd>
      </div>
      <div className="item-detail-pane__field">
        <dt>Permissions</dt>
        <dd className="item-detail-pane__mono">{prov.permissions}</dd>
      </div>
      <div className="item-detail-pane__field">
        <dt>UID</dt>
        <dd>{prov.uid}</dd>
      </div>
      <div className="item-detail-pane__field">
        <dt>GID</dt>
        <dd>{prov.gid}</dd>
      </div>
      <div className="item-detail-pane__field">
        <dt>Writable mount</dt>
        <dd>{prov.writable_mount ? "Yes" : "No"}</dd>
      </div>
      <div className="item-detail-pane__field">
        <dt>Mutable</dt>
        <dd>{prov.mutability ? "Yes" : "No"}</dd>
      </div>
      <div className="item-detail-pane__field">
        <dt>Service working dir</dt>
        <dd>{prov.service_working_dir ? "Yes" : "No"}</dd>
      </div>
    </div>
  );
}

export function ItemDetailPane({ item, sectionId }: ItemDetailPaneProps) {
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
        {sectionId === "language_packages" && item.section_metadata && (
          <LanguagePackageDetail item={item} />
        )}
        {sectionId === "unmanaged_files" && item.section_metadata && (
          <UnmanagedFileDetail item={item} />
        )}
      </dl>
    </div>
  );
}
