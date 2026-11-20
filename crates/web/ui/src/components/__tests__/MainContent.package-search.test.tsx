import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MainContent } from "../MainContent";
import type { GroupInfo, ViewResponse } from "../../api/types";
import { emptyView } from "../../test-utils/emptyView";

vi.mock("../../api/client", () => ({
  ungroupGroup: vi.fn(),
  applyOp: vi.fn(),
}));

const containerGroup: GroupInfo = {
  name: "Container Management",
  member_count: 2,
  added_count: 2,
  locked_count: 0,
  optional_spillover_count: 0,
  effective_member_count: 2,
  effective_included: 2,
  render_state: "renderable",
  degradation_reason: null,
  members: [
    { name: "buildah", locked: false, overlap_groups: [], in_base_image: false },
    { name: "podman", locked: false, overlap_groups: [], in_base_image: false },
  ],
};

function viewWithPackages(names: string[]): ViewResponse {
  const view = emptyView({ package_groups: [containerGroup] });
  view.packages = names.map((nameArch) => {
    const dot = nameArch.lastIndexOf(".");
    return {
      entry: {
        name: nameArch.slice(0, dot),
        arch: nameArch.slice(dot + 1),
        disposition: { kind: "actionable", include: true },
        source_repo: "baseos",
      },
      triage: {},
    } as unknown as ViewResponse["packages"][number];
  });
  return view;
}

function renderPackagesWithSearch() {
  const view = viewWithPackages([
    "bat.x86_64",
    "buildah.x86_64",
    "podman.x86_64",
    "gdb.x86_64",
  ]);
  function Harness() {
    return (
      <MainContent
        activeSection="packages"
        loading={false}
        viewData={view}
        sections={null}
        onViewUpdate={vi.fn()}
        onMutationError={vi.fn()}
        sectionSearchOpen={true}
        onSectionSearchClose={vi.fn()}
      />
    );
  }
  return render(<Harness />);
}

describe("Packages section search", () => {
  it("keeps the whole word and the focus in the search box while typing", async () => {
    const user = userEvent.setup();
    renderPackagesWithSearch();

    const input = screen.getByLabelText("Filter section items");
    expect(input).toHaveFocus();

    await user.type(input, "buildah");

    expect(input).toHaveValue("buildah");
    expect(input).toHaveFocus();
    // The filter applied the whole word, not its first letter.
    expect(screen.queryByTestId("package-row-gdb.x86_64")).toBeNull();
    expect(screen.getByTestId("group-member-buildah")).toBeInTheDocument();
  });

  it("ArrowDown from the search box moves to the first shown row", async () => {
    const user = userEvent.setup();
    renderPackagesWithSearch();

    const input = screen.getByLabelText("Filter section items");
    await user.type(input, "gdb");
    await user.keyboard("{ArrowDown}");

    expect(screen.getByTestId("package-row-gdb.x86_64")).toHaveFocus();
  });
});
