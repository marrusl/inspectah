import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen } from "@testing-library/react";
import { MainContent } from "../MainContent";
import { emptyView } from "../../test-utils/emptyView";

// The units section reads the view's links, records and default target
// (spec § 11.1) and hands them to the list.

const props = {
  activeSection: "units",
  loading: false,
  sections: null,
  onViewUpdate: vi.fn(),
  onMutationError: vi.fn(),
  sectionSearchOpen: false,
  onSectionSearchClose: vi.fn(),
};

describe("MainContent: units section links, records and default target", () => {
  beforeEach(() => {
    vi.stubGlobal(
      "fetch",
      vi.fn(() => Promise.resolve({ ok: true, json: () => Promise.resolve({ ids: [] }) })),
    );
  });
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("renders the view's manual links, undetermined installs and default target", () => {
    render(
      <MainContent
        {...props}
        viewData={emptyView({
          manual_links: [
            {
              relation: "wants",
              parent: "multi-user.target",
              unit: "backup.service",
              cause: "static_unit",
              advisories: ["# Advisory: a link"],
            },
          ],
          undetermined_installs: [
            {
              unit: "need.service",
              reason: "upheld_by",
              install_files: [],
              advisories: ["# Advisory: a record"],
            },
          ],
          default_target: {
            target: "graphical.target",
            include: true,
            locked: false,
            declined: null,
          },
        })}
      />,
    );
    expect(
      screen.getByTestId(
        `manual-link-${JSON.stringify(["wants", "multi-user.target", "backup.service"])}`,
      ),
    ).toBeInTheDocument();
    expect(screen.getByTestId("undetermined-install-need.service")).toBeInTheDocument();
    expect(screen.getByTestId("default-target")).toBeInTheDocument();
  });

  it("renders no default target row for a view without one", () => {
    render(<MainContent {...props} viewData={emptyView()} />);
    expect(screen.getByTestId("service-section")).toBeInTheDocument();
    expect(screen.queryByTestId("default-target")).not.toBeInTheDocument();
  });
});
