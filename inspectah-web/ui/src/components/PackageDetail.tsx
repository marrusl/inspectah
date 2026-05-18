import { useState } from "react";
import { Content, DescriptionList, DescriptionListGroup, DescriptionListTerm, DescriptionListDescription, Label, Button } from "@patternfly/react-core";
import type { RefinedPackage, AttentionTag, VersionChangeEntry } from "../api/types";
import { attentionLabelColor, formatReasonText } from "./attentionUtils";
import { DependencyModal } from "./DependencyModal";

export interface PackageDetailProps {
  pkg: RefinedPackage;
  leafDepTree?: Record<string, string[]>;
  versionChange?: VersionChangeEntry | null;
}

function formatNevra(pkg: RefinedPackage): string {
  const { name, epoch, version, release, arch } = pkg.entry;
  const epochPart = epoch && epoch !== "0" ? `${epoch}:` : "";
  return `${name}-${epochPart}${version}-${release}.${arch}`;
}

function formatState(state: string): string {
  return state
    .split("_")
    .map((w) => w.charAt(0).toUpperCase() + w.slice(1))
    .join(" ");
}

export function PackageDetail({ pkg, leafDepTree }: PackageDetailProps) {
  const [depModalOpen, setDepModalOpen] = useState(false);
  const canonicalId = `${pkg.entry.name}.${pkg.entry.arch}`;
  const deps = leafDepTree?.[canonicalId];
  const hasDeps = deps && deps.length > 0;

  return (
    <div data-testid="package-detail" style={{ padding: "var(--pf-t--global--spacer--sm) 0" }}>
      <DescriptionList isHorizontal isCompact>
        <DescriptionListGroup>
          <DescriptionListTerm>NEVRA</DescriptionListTerm>
          <DescriptionListDescription>
            <Content component="small">
              <code>{formatNevra(pkg)}</code>
            </Content>
          </DescriptionListDescription>
        </DescriptionListGroup>
        <DescriptionListGroup>
          <DescriptionListTerm>State</DescriptionListTerm>
          <DescriptionListDescription>
            {formatState(pkg.entry.state)}
          </DescriptionListDescription>
        </DescriptionListGroup>
        <DescriptionListGroup>
          <DescriptionListTerm>Repository</DescriptionListTerm>
          <DescriptionListDescription>
            {pkg.entry.source_repo || "Unknown"}
          </DescriptionListDescription>
        </DescriptionListGroup>
        {pkg.attention.length > 0 && (
          <DescriptionListGroup>
            <DescriptionListTerm>Attention</DescriptionListTerm>
            <DescriptionListDescription>
              {pkg.attention.map((tag: AttentionTag, i: number) => (
                <span key={i} style={{ marginRight: "var(--pf-t--global--spacer--sm)" }}>
                  <Label color={attentionLabelColor(tag.level)}>
                    {formatReasonText(tag.reason, tag.detail)}
                  </Label>
                  {tag.detail && (
                    <Content component="small" style={{ marginLeft: "var(--pf-t--global--spacer--xs)" }}>
                      {tag.detail}
                    </Content>
                  )}
                </span>
              ))}
            </DescriptionListDescription>
          </DescriptionListGroup>
        )}
        {pkg.entry.fleet && (
          <DescriptionListGroup>
            <DescriptionListTerm>Fleet</DescriptionListTerm>
            <DescriptionListDescription>
              {pkg.entry.fleet.count} of {pkg.entry.fleet.total} hosts
            </DescriptionListDescription>
          </DescriptionListGroup>
        )}
      </DescriptionList>
      {hasDeps && (
        <>
          <Button
            variant="link"
            onClick={() => setDepModalOpen(true)}
            style={{ marginTop: "var(--pf-t--global--spacer--sm)" }}
          >
            View Dependencies ({deps.length})
          </Button>
          <DependencyModal
            packageId={canonicalId}
            dependencies={deps}
            isOpen={depModalOpen}
            onClose={() => setDepModalOpen(false)}
          />
        </>
      )}
    </div>
  );
}
