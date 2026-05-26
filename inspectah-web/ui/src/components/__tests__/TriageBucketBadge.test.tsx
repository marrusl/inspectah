import { render, screen } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { DecisionItem } from "../DecisionItem";
import type { DecisionItemKind } from "../DecisionItem";
import type { RefinedPackage, RefinedConfig, TriageTag } from "../../api/types";

// --- Test data factories ---

function makeTriageTag(bucket: string, reason: TriageTag["primary_reason"] = "package_user_added"): TriageTag {
  return {
    triage: { mode: "single_host" as const, [bucket]: null },
    primary_reason: reason,
    annotations: [],
  };
}

function makePkgItem(
  name: string,
  triage: TriageTag,
): DecisionItemKind {
  const pkg: RefinedPackage = {
    entry: {
      name,
      epoch: "0",
      version: "1.0",
      release: "1.el9",
      arch: "x86_64",
      state: "added",
      include: true,
      source_repo: "baseos",
      fleet: null,
    },
    triage,
  };
  return { type: "package", data: pkg };
}

function makeConfigItem(
  path: string,
  triage: TriageTag,
): DecisionItemKind {
  const config: RefinedConfig = {
    entry: {
      path,
      kind: "rpm_owned_modified",
      category: "other",
      content: "",
      rpm_va_flags: null,
      package: "httpd",
      diff_against_rpm: null,
      include: true,
      tie: false,
      tie_winner: false,
      fleet: null,
    },
    triage,
  };
  return { type: "config", data: config };
}

const noop = vi.fn();

describe("Triage bucket badge on package rows", () => {
  it("shows Investigate badge for package with investigate triage", () => {
    const item = makePkgItem("nginx", makeTriageTag("investigate"));
    render(
      <DecisionItem
        item={item}
        triageTag={item.data.triage}
        rowIndex={1}
        isViewed={false}
        isPending={false}
        onMarkViewed={noop}
      />,
    );
    const badge = screen.getByTestId("triage-bucket-badge");
    expect(badge).toBeInTheDocument();
    expect(badge).toHaveTextContent("Investigate");
  });

  it("shows Site badge for package with site triage", () => {
    const item = makePkgItem("curl", makeTriageTag("site", "package_version_changed"));
    render(
      <DecisionItem
        item={item}
        triageTag={item.data.triage}
        rowIndex={1}
        isViewed={false}
        isPending={false}
        onMarkViewed={noop}
      />,
    );
    const badge = screen.getByTestId("triage-bucket-badge");
    expect(badge).toBeInTheDocument();
    expect(badge).toHaveTextContent("Site");
  });

  it("does not show bucket badge for package with baseline triage", () => {
    const item = makePkgItem("bash", makeTriageTag("baseline", "package_baseline_match"));
    render(
      <DecisionItem
        item={item}
        triageTag={item.data.triage}
        rowIndex={1}
        isViewed={false}
        isPending={false}
        onMarkViewed={noop}
      />,
    );
    expect(screen.queryByTestId("triage-bucket-badge")).not.toBeInTheDocument();
  });

  it("does not show bucket badge for config items", () => {
    const item = makeConfigItem("/etc/httpd/conf/httpd.conf", makeTriageTag("investigate", "config_modified"));
    render(
      <DecisionItem
        item={item}
        triageTag={item.data.triage}
        rowIndex={1}
        isViewed={false}
        isPending={false}
        onMarkViewed={noop}
      />,
    );
    expect(screen.queryByTestId("triage-bucket-badge")).not.toBeInTheDocument();
  });

  it("does not show bucket badge when triageTag is not provided", () => {
    const item = makePkgItem("wget", makeTriageTag("investigate"));
    render(
      <DecisionItem
        item={item}
        rowIndex={1}
        isViewed={false}
        isPending={false}
        onMarkViewed={noop}
      />,
    );
    expect(screen.queryByTestId("triage-bucket-badge")).not.toBeInTheDocument();
  });

  it("shows Divergent badge for package with divergent triage", () => {
    const item = makePkgItem("openssl", makeTriageTag("divergent", "package_version_changed"));
    render(
      <DecisionItem
        item={item}
        triageTag={item.data.triage}
        rowIndex={1}
        isViewed={false}
        isPending={false}
        onMarkViewed={noop}
      />,
    );
    const badge = screen.getByTestId("triage-bucket-badge");
    expect(badge).toBeInTheDocument();
    expect(badge).toHaveTextContent("Divergent");
  });

  it("does not show bucket badge for universal triage", () => {
    const item = makePkgItem("glibc", makeTriageTag("universal", "package_baseline_match"));
    render(
      <DecisionItem
        item={item}
        triageTag={item.data.triage}
        rowIndex={1}
        isViewed={false}
        isPending={false}
        onMarkViewed={noop}
      />,
    );
    expect(screen.queryByTestId("triage-bucket-badge")).not.toBeInTheDocument();
  });
});
