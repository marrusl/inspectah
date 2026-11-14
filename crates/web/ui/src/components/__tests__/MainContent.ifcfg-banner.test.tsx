import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import { MainContent } from "../MainContent";
import type { ViewResponse, ReferenceSection } from "../../api/types";
import { emptyView } from "../../test-utils/emptyView";

// Mock the API client
vi.mock("../../api/client", () => ({
  ungroupGroup: vi.fn(),
  applyOp: vi.fn(),
}));

/** A view with nothing in it. */
function makeViewData(): ViewResponse {
  return emptyView();
}

/** Minimal ReferenceSection for network. */
function makeNetworkSection(
  hasIfcfg: boolean,
  ifcfgNote?: string,
): ReferenceSection {
  return {
    id: "network",
    display_name: "Network",
    items: [],
    has_ifcfg: hasIfcfg,
    ifcfg_note: ifcfgNote,
  };
}

describe("MainContent ifcfg deprecation banner", () => {
  it("shows ifcfg deprecation banner when has_ifcfg is true", () => {
    const viewData = makeViewData();
    const sections = [
      makeNetworkSection(
        true,
        "ifcfg network configuration files are deprecated in NetworkManager.",
      ),
    ];

    render(
      <MainContent
        activeSection="network"
        loading={false}
        viewData={viewData}
        sections={sections}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
        sectionSearchOpen={false}
        onSectionSearchClose={vi.fn()}
      />,
    );

    expect(screen.getByText("ifcfg Deprecation")).toBeInTheDocument();
    expect(
      screen.getByText(
        /ifcfg network configuration files are deprecated in NetworkManager/,
      ),
    ).toBeInTheDocument();
  });

  it("hides banner when has_ifcfg is false", () => {
    const viewData = makeViewData();
    const sections = [makeNetworkSection(false)];

    render(
      <MainContent
        activeSection="network"
        loading={false}
        viewData={viewData}
        sections={sections}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
        sectionSearchOpen={false}
        onSectionSearchClose={vi.fn()}
      />,
    );

    expect(screen.queryByText("ifcfg Deprecation")).not.toBeInTheDocument();
  });

  it("hides banner when ifcfg_note is missing even if has_ifcfg is true", () => {
    const viewData = makeViewData();
    const sections = [makeNetworkSection(true, undefined)];

    render(
      <MainContent
        activeSection="network"
        loading={false}
        viewData={viewData}
        sections={sections}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
        sectionSearchOpen={false}
        onSectionSearchClose={vi.fn()}
      />,
    );

    expect(screen.queryByText("ifcfg Deprecation")).not.toBeInTheDocument();
  });
});
