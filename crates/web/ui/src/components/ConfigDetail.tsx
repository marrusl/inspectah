import { useState } from "react";
import {
  Content,
  DescriptionList,
  DescriptionListGroup,
  DescriptionListTerm,
  DescriptionListDescription,
  Label,
} from "@patternfly/react-core";
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
  const [showDiff, setShowDiff] = useState(false);
  const hasDiff =
    config.entry.diff_against_rpm != null &&
    config.entry.diff_against_rpm.length > 0;

  return (
    <div
      data-testid="config-detail"
      style={{ padding: "var(--pf-t--global--spacer--sm) 0" }}
    >
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
        {(config.attention ?? []).length > 0 && (
          <DescriptionListGroup>
            <DescriptionListTerm>Attention</DescriptionListTerm>
            <DescriptionListDescription>
              {(config.attention ?? []).map((tag: AttentionTag, i: number) => (
                <span
                  key={i}
                  style={{ marginRight: "var(--pf-t--global--spacer--sm)" }}
                >
                  <Label color={attentionLabelColor(tag.level)}>
                    {formatReasonText(tag.reason, tag.detail)}
                  </Label>
                  {tag.detail && (
                    <Content
                      component="small"
                      style={{ marginLeft: "var(--pf-t--global--spacer--xs)" }}
                    >
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
                <pre
                  style={{
                    maxHeight: "200px",
                    overflow: "auto",
                    whiteSpace: "pre-wrap",
                  }}
                >
                  {config.entry.content}
                </pre>
              </Content>
            </DescriptionListDescription>
          </DescriptionListGroup>
        )}
        {config.entry.aggregate && (
          <DescriptionListGroup>
            <DescriptionListTerm>Aggregate</DescriptionListTerm>
            <DescriptionListDescription>
              {config.entry.aggregate.count} of {config.entry.aggregate.total}{" "}
              hosts
            </DescriptionListDescription>
          </DescriptionListGroup>
        )}
      </DescriptionList>
      {hasDiff && (
        <div style={{ marginTop: "var(--pf-t--global--spacer--sm)" }}>
          <button
            type="button"
            onClick={() => setShowDiff((prev) => !prev)}
            style={{
              background: "none",
              border: "none",
              cursor: "pointer",
              padding: 0,
              color: "var(--pf-t--global--link--color--default)",
              fontSize: "var(--pf-t--global--font--size--body--sm)",
              textDecoration: "underline",
            }}
          >
            {showDiff ? "Hide diff" : "View diff"}
          </button>
          {showDiff && (
            <pre
              data-testid="config-diff"
              style={{
                marginTop: "var(--pf-t--global--spacer--xs)",
                padding: "var(--pf-t--global--spacer--sm)",
                background:
                  "var(--pf-t--global--background--color--secondary--default)",
                borderRadius: "var(--pf-t--global--border--radius--small)",
                fontSize: "var(--pf-t--global--font--size--body--sm)",
                overflow: "auto",
                maxHeight: "300px",
                whiteSpace: "pre-wrap",
              }}
            >
              {config.entry.diff_against_rpm}
            </pre>
          )}
        </div>
      )}
    </div>
  );
}
