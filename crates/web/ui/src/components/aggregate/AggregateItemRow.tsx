import { Button, Label, Switch } from "@patternfly/react-core";
import type {
  AggregateItem,
  ConfigShadow,
  ItemId,
  LanguagePackageMetadata,
  UnmanagedFileMetadata,
} from "../../api/types";
import type { UseVariantAckResult } from "../../hooks/useVariantAck";
import { PrevalenceBadge } from "../PrevalenceBadge";
import {
  CONTROLLED_IN_SYSTEMD_UNITS,
  DECIDED_UNDER_REPOSITORIES,
} from "../shadowOwner";

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
  /**
   * The repo whose switch decides each repo-owned path: the first owner,
   * by section id, that has a switch. A path with none gets no link.
   */
  repoOwners?: ReadonlyMap<string, string>;
  /** Reveal a repo's row under Repositories, in Packages. */
  onNavigateToRepo?: (sectionId: string) => void;
}

const SHADOW_LABELS: Record<ConfigShadow, string> = {
  repositories: DECIDED_UNDER_REPOSITORIES,
  units: CONTROLLED_IN_SYSTEMD_UNITS,
};

/**
 * Whether this row decides its own item. A row another section decides
 * (`shadowed_by`) is inventory: the server refuses its include, variant
 * and edit ops, so it offers none of them.
 */
export function decidesHere(
  item: AggregateItem,
  isDecisionSection: boolean,
): boolean {
  return isDecisionSection && item.shadowed_by === undefined;
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
    case "Unit":
      return itemId.key.unit;
    case "DropIn":
      return itemId.key.path;
    case "DefaultTarget":
      return "default.target";
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
    case "TunedSelection":
      return itemId.key.profile;
    case "CronJob":
      return itemId.key.path;
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
  repoOwners,
  onNavigateToRepo,
}: AggregateItemRowProps) {
  const name = itemDisplayName(item.item_id);
  const { count, total } = item.prevalence;
  const hasVariants = item.variants != null && item.variants.count > 1;
  const locked = item.locked === true;
  const decides = decidesHere(item, isDecisionSection);
  const shadow = item.shadowed_by;
  const owningRepo =
    shadow === "repositories" && item.item_id.kind === "Config"
      ? (repoOwners?.get(item.item_id.key.path) ?? null)
      : null;

  const handleToggle = () => {
    if (locked) return;
    onToggle(item.item_id, !item.include);
  };

  const handleVariantClick = (e: React.MouseEvent) => {
    e.stopPropagation();
    if (decides) onExpandVariant?.(item.item_id);
  };

  const handleRowClick = () => {
    if (decides) onExpandVariant?.(item.item_id);
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      if (decides) onExpandVariant?.(item.item_id);
    } else if (e.key === "Escape" && isExpanded) {
      e.preventDefault();
      onExpandVariant?.(item.item_id); // toggle close
    }
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
      className={`aggregate-item-row${locked ? " aggregate-item-row--locked" : ""}${shadow ? " aggregate-item-row--readonly" : ""}`}
      data-testid="aggregate-item-row"
      data-item-id={JSON.stringify(item.item_id)}
      data-locked={locked ? "true" : undefined}
      data-shadowed-by={shadow}
      onClick={handleRowClick}
      onKeyDown={handleKeyDown}
      role="row"
      tabIndex={0}
    >
      {decides && (
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

      {shadow && (
        <Label
          color="purple"
          isCompact
          data-testid="shadow-owner-badge"
          className="aggregate-item-row__shadow-owner"
        >
          {SHADOW_LABELS[shadow]}
        </Label>
      )}

      {owningRepo !== null && onNavigateToRepo && (
        <Button
          variant="link"
          isInline
          data-testid="shadow-owner-link"
          onClick={(e) => {
            e.stopPropagation();
            onNavigateToRepo(owningRepo);
          }}
          // The row's own Enter and Space handler would cancel the
          // button's activation.
          onKeyDown={(e) => e.stopPropagation()}
        >
          Go to repo {owningRepo}
        </Button>
      )}

      <PrevalenceBadge count={count} total={total} suffix="hosts" />

      {hasVariants && decides && (
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
      {hasVariants && !decides && (
        <span className="aggregate-item-row__variants aggregate-item-row__variants--readonly">
          {item.variants!.count} variants
        </span>
      )}
    </div>
  );
}
