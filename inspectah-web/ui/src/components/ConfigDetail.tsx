import { Content, DescriptionList, DescriptionListGroup, DescriptionListTerm, DescriptionListDescription, Label } from "@patternfly/react-core";
import type { RefinedConfig, AttentionTag } from "../api/types";
import { attentionLabelColor, formatReasonText } from "./attentionUtils";

export interface ConfigDetailProps {
  config: RefinedConfig;
}

function formatKind(kind: string): string {
  return kind
    .split("_")
    .map((w) => w.charAt(0).toUpperCase() + w.slice(1))
    .join(" ");
}

export function ConfigDetail({ config }: ConfigDetailProps) {
  return (
    <div data-testid="config-detail" style={{ padding: "var(--pf-t--global--spacer--sm) 0" }}>
      <DescriptionList isHorizontal isCompact>
        <DescriptionListGroup>
          <DescriptionListTerm>Path</DescriptionListTerm>
          <DescriptionListDescription>
            <Content component="small">
              <code>{config.entry.path}</code>
            </Content>
          </DescriptionListDescription>
        </DescriptionListGroup>
        <DescriptionListGroup>
          <DescriptionListTerm>Kind</DescriptionListTerm>
          <DescriptionListDescription>
            {formatKind(config.entry.kind)}
          </DescriptionListDescription>
        </DescriptionListGroup>
        {config.entry.package && (
          <DescriptionListGroup>
            <DescriptionListTerm>Owner Package</DescriptionListTerm>
            <DescriptionListDescription>
              {config.entry.package}
            </DescriptionListDescription>
          </DescriptionListGroup>
        )}
        {config.attention.length > 0 && (
          <DescriptionListGroup>
            <DescriptionListTerm>Attention</DescriptionListTerm>
            <DescriptionListDescription>
              {config.attention.map((tag: AttentionTag, i: number) => (
                <span key={i} style={{ marginRight: "var(--pf-t--global--spacer--sm)" }}>
                  <Label color={attentionLabelColor(tag.level)}>
                    {formatReasonText(tag.reason)}
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
        {config.entry.content && (
          <DescriptionListGroup>
            <DescriptionListTerm>Content Preview</DescriptionListTerm>
            <DescriptionListDescription>
              <Content component="small">
                <pre style={{ maxHeight: "200px", overflow: "auto", whiteSpace: "pre-wrap" }}>
                  {config.entry.content.length > 500
                    ? config.entry.content.slice(0, 500) + "..."
                    : config.entry.content}
                </pre>
              </Content>
            </DescriptionListDescription>
          </DescriptionListGroup>
        )}
        {config.entry.fleet && (
          <DescriptionListGroup>
            <DescriptionListTerm>Fleet</DescriptionListTerm>
            <DescriptionListDescription>
              {config.entry.fleet.count} of {config.entry.fleet.total} hosts
            </DescriptionListDescription>
          </DescriptionListGroup>
        )}
      </DescriptionList>
    </div>
  );
}
