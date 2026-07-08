import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import { MainContent } from "../MainContent";
import type { ViewResponse, ReferenceSection } from "../../api/types";

// Mock the API client
vi.mock("../../api/client", () => ({
  ungroupGroup: vi.fn(),
  applyOp: vi.fn(),
}));

/** Minimal ViewResponse that renders without crashing. */
function makeViewData(): Partial<ViewResponse> {
  return {
    packages: [],
    config_files: [],
    repo_groups: [],
    package_groups: [],
    generation: 1,
    stats: {
      sections: [],
      needs_review_count: 0,
      ops_applied: 0,
      can_undo: false,
      can_redo: false,
      baseline_available: false,
    },
  };
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
        viewData={viewData as ViewResponse}
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
        viewData={viewData as ViewResponse}
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
        viewData={viewData as ViewResponse}
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
