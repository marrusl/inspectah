import { Label, Switch } from "@patternfly/react-core";
import type {
  AggregateItem,
  ItemId,
  LanguagePackageMetadata,
  UnmanagedFileMetadata,
} from "../../api/types";
import type { UseVariantAckResult } from "../../hooks/useVariantAck";
import { PrevalenceBadge } from "../PrevalenceBadge";

export interface AggregateItemRowProps {
  item: AggregateItem;
  isDecisionSection: boolean;
  onToggle: (itemId: ItemId, include: boolean) => void;
  ack: UseVariantAckResult;
  onExpandVariant?: (itemId: ItemId) => void;
  /** Whether this row's inline variant view is expanded. */
  isExpanded?: boolean;
  /** Section ID from the parent AggregateSection — drives metadata rendering. */
  sectionId?: string;
}

/* ---- Section metadata helpers ---- */

const FILE_TYPE_LABELS: Record<string, string> = {
  elf_binary: "ELF Binary",
  shell_script: "Shell Script",
  data: "Data",
  text: "Text",
  symlink: "Symlink",
  directory: "Directory",
};

/** Map a backend file_type slug to a human-readable label. */
export function formatFileType(fileType: string): string {
  if (FILE_TYPE_LABELS[fileType]) return FILE_TYPE_LABELS[fileType];
  // Title-case unknown types: "python_script" → "Python Script"
  return fileType
    .split("_")
    .map((w) => w.charAt(0).toUpperCase() + w.slice(1))
    .join(" ");
}

/** Format byte count to a human-readable size string. */
export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  if (bytes < 1024 * 1024 * 1024)
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
}

export function attentionDisplayLabel(level: string): string {
  switch (level) {
    case "needs_review":
      return "Needs review";
    case "informational":
      return "Info";
    case "routine":
      return "Routine";
    default:
      return level.replace(/_/g, " ");
  }
}

export function itemDisplayName(itemId: ItemId): string {
  switch (itemId.kind) {
    case "Package":
      return `${itemId.key.name}.${itemId.key.arch}`;
    case "Config":
      return itemId.key.path;
    case "Repo":
      return itemId.key.path;
    case "ModuleStream":
      return itemId.key.module_stream;
    case "VersionLock":
      return itemId.key.name_arch;
    case "Service":
      return itemId.key.unit;
    case "DropIn":
      return itemId.key.path;
    case "Quadlet":
      return itemId.key.path;
    case "Compose":
      return itemId.key.path;
    case "Flatpak": {
      const parts = [itemId.key.app_id];
      if (itemId.key.remote) parts.push(itemId.key.remote);
      if (itemId.key.branch) parts.push(itemId.key.branch);
      return parts.join(" / ");
    }
    case "NMConnection":
      return itemId.key.path;
    case "FirewallZone":
      return itemId.key.path;
    case "KernelModule":
      return itemId.key.name;
    case "Sysctl":
      return itemId.key.key;
    case "TunedSelection":
      return itemId.key.profile;
    case "CronJob":
      return itemId.key.path;
    case "SystemdTimer":
      return itemId.key.name;
    case "AtJob":
      return itemId.key.file;
    case "GeneratedTimer":
      return itemId.key.name;
    case "SelinuxPort":
      return itemId.key.protocol_port;
    case "Fstab":
      return itemId.key.mount_point;
    case "NonRpm":
      return itemId.key.name;
    case "Group":
      return itemId.key.name;
    case "LanguageEnv":
      return `${itemId.key.ecosystem}:${itemId.key.path}`;
    case "UnmanagedFile":
      return itemId.key.path;
  }
}

export function AggregateItemRow({
  item,
  isDecisionSection,
  onToggle,
  ack: _ack,
  onExpandVariant,
  isExpanded = false,
  sectionId,
}: AggregateItemRowProps) {
  const name = itemDisplayName(item.item_id);
  const { count, total } = item.prevalence;
  const hasVariants = item.variants != null && item.variants.count > 1;
  const locked = item.locked === true;

  const handleToggle = () => {
    if (locked) return;
    onToggle(item.item_id, !item.include);
  };

  const handleVariantClick = (e: React.MouseEvent) => {
    e.stopPropagation();
    if (isDecisionSection) onExpandVariant?.(item.item_id);
  };

  const handleRowClick = () => {
    if (isDecisionSection) onExpandVariant?.(item.item_id);
  };

  /* Section-specific metadata fragments */
  let sectionMeta: React.ReactNode = null;

  if (sectionId === "language_packages" && item.section_metadata) {
    const meta = item.section_metadata as unknown as LanguagePackageMetadata;
    sectionMeta = (
      <span className="aggregate-item-row__section-meta">
        <Label
          color="blue"
          isCompact
          data-testid="section-meta-ecosystem"
          className="aggregate-item-row__ecosystem"
        >
          {meta.ecosystem}
        </Label>
        <span
          className={`aggregate-item-row__confidence aggregate-item-row__confidence--${meta.confidence}`}
          data-testid="section-meta-confidence"
        >
          {meta.confidence}
        </span>
        <Label
          color="grey"
          isCompact
          data-testid="section-meta-pkg-count"
          className="aggregate-item-row__pkg-count"
        >
          {meta.package_count} packages
        </Label>
        {meta.manifest_basis != null && (
          <span
            className="aggregate-item-row__manifest-basis"
            data-testid="section-meta-manifest-basis"
          >
            {meta.manifest_basis}
          </span>
        )}
      </span>
    );
  }

  if (sectionId === "unmanaged_files" && item.section_metadata) {
    const meta = item.section_metadata as unknown as UnmanagedFileMetadata;
    sectionMeta = (
      <span className="aggregate-item-row__section-meta">
        <Label
          color="blue"
          isCompact
          data-testid="section-meta-file-type"
          className="aggregate-item-row__file-type"
        >
          {formatFileType(meta.file_type)}
        </Label>
        <Label
          color="grey"
          isCompact
          data-testid="section-meta-size"
          className="aggregate-item-row__file-size"
        >
          {formatSize(meta.size)}
        </Label>
        {meta.under_var && (
          <span
            className="aggregate-item-row__var-warning"
            data-testid="section-meta-var-warning"
            title="File is under /var — may be ephemeral or runtime-generated"
            aria-label="/var warning"
          >
            &#9888;
          </span>
        )}
      </span>
    );
  }

  return (
    <div
      className={`aggregate-item-row${locked ? " aggregate-item-row--locked" : ""}`}
      data-testid="aggregate-item-row"
      data-item-id={JSON.stringify(item.item_id)}
      data-locked={locked ? "true" : undefined}
      onClick={handleRowClick}
      role="row"
      tabIndex={0}
    >
      {isDecisionSection && (
        <div
          className="aggregate-item-row__toggle"
          onClick={(e) => e.stopPropagation()}
        >
          <Switch
            id={`aggregate-switch-${name}`}
            isChecked={item.include}
            onChange={handleToggle}
            isDisabled={locked}
            aria-label={locked ? `${name} (locked)` : `Toggle ${name}`}
          />
        </div>
      )}

      <div className="aggregate-item-row__name">{name}</div>

      {sectionMeta}

      {locked && (
        <Label
          color="grey"
          isCompact
          data-testid={`locked-badge-aggregate-${name}`}
          className="aggregate-item-row__locked-badge"
        >
          {item.attention_reason ?? "LOCKED"}
        </Label>
      )}

      <PrevalenceBadge count={count} total={total} suffix="hosts" />

      {hasVariants && isDecisionSection && (
        <button
          className="aggregate-item-row__variants"
          onClick={handleVariantClick}
          type="button"
          aria-expanded={isExpanded}
        >
          {item.variants!.count} variants{" "}
          <span
            className={`aggregate-item-row__variants-chevron${isExpanded ? " aggregate-item-row__variants-chevron--expanded" : ""}`}
            aria-hidden="true"
          >
            &#9656;
          </span>
        </button>
      )}
      {hasVariants && !isDecisionSection && (
        <span className="aggregate-item-row__variants aggregate-item-row__variants--readonly">
          {item.variants!.count} variants
        </span>
      )}
    </div>
  );
}
