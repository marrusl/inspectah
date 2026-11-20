import { describe, it, expect, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { PackageList } from "../PackageList";
import type { GroupInfo, RepoGroupInfo } from "../../api/types";

const distroRepo: RepoGroupInfo = {
  section_id: "baseos",
  provenance: "verified",
  is_distro: true,
  tier: "distro",
  package_count: 3,
  effective_member_count: 3,
  effective_included: 0,
  effective_unlisted: 0,
  enabled: true,
};

const editorsGroup: GroupInfo = {
  name: "editors",
  member_count: 1,
  added_count: 1,
  locked_count: 0,
  optional_spillover_count: 0,
  effective_member_count: 1,
  effective_included: 1,
  render_state: "renderable",
  degradation_reason: null,
  members: [
    { name: "vim", locked: false, overlap_groups: [], in_base_image: false },
  ],
};

const PACKAGES = [
  { name: "aide.aarch64", source_repo: "baseos", include: true },
  { name: "bat.aarch64", source_repo: "baseos", include: true },
  { name: "curl.aarch64", source_repo: "baseos", include: false },
];

function renderList(
  props: Partial<React.ComponentProps<typeof PackageList>> = {},
) {
  const onToggle = vi.fn();
  render(
    <PackageList
      mode="single"
      packages={PACKAGES}
      repoGroups={[distroRepo]}
      onToggle={onToggle}
      onRepoToggle={vi.fn()}
      {...props}
    />,
  );
  return { onToggle };
}

const row = (name: string) => screen.getByTestId(`package-row-${name}`);

/** The two toggle keys, as `user.keyboard` and `KeyboardEvent.key` name them. */
const TOGGLE_KEYS = [
  { label: "x", typed: "x", key: "x" },
  { label: "Space", typed: " ", key: " " },
] as const;
const MODIFIERS = ["ctrlKey", "metaKey", "altKey"] as const;
const MODIFIED_TOGGLES = MODIFIERS.flatMap((modifier) =>
  TOGGLE_KEYS.map(({ label, key }) => ({ modifier, label, key })),
);

describe("PackageList row shortcuts", () => {
  it("j and k move between package rows", async () => {
    const user = userEvent.setup();
    renderList();

    await user.click(screen.getByText("aide.aarch64"));
    expect(row("aide.aarch64")).toHaveFocus();

    await user.keyboard("j");
    expect(row("bat.aarch64")).toHaveFocus();
    await user.keyboard("{ArrowDown}");
    expect(row("curl.aarch64")).toHaveFocus();
    await user.keyboard("k");
    expect(row("bat.aarch64")).toHaveFocus();
    await user.keyboard("{ArrowUp}");
    expect(row("aide.aarch64")).toHaveFocus();
  });

  it("G goes to the last row and g to the first", async () => {
    const user = userEvent.setup();
    renderList();

    await user.click(screen.getByText("bat.aarch64"));
    await user.keyboard("G");
    expect(row("curl.aarch64")).toHaveFocus();
    await user.keyboard("g");
    expect(row("aide.aarch64")).toHaveFocus();
  });

  it("x and Space toggle the focused package", async () => {
    const user = userEvent.setup();
    const { onToggle } = renderList();

    await user.click(screen.getByText("bat.aarch64"));
    await user.keyboard("x");
    expect(onToggle).toHaveBeenCalledTimes(1);
    expect(onToggle).toHaveBeenLastCalledWith("bat.aarch64");

    await user.keyboard(" ");
    expect(onToggle).toHaveBeenCalledTimes(2);
  });

  it.each(TOGGLE_KEYS)(
    "$label does nothing while the package's request is in flight",
    async ({ typed }) => {
      const user = userEvent.setup();
      const { onToggle } = renderList({
        pendingKeys: new Set(["bat.aarch64"]),
      });

      await user.click(screen.getByText("bat.aarch64"));
      await user.keyboard(typed);
      expect(onToggle).not.toHaveBeenCalled();
    },
  );

  it.each(TOGGLE_KEYS)(
    "$label does nothing on a row waiting for an RPM upload",
    async ({ typed }) => {
      const user = userEvent.setup();
      const onUploadClick = vi.fn();
      const { onToggle } = renderList({
        rpmRowStates: { "bat.aarch64": "needs_upload" },
        onUploadClick,
      });

      row("bat.aarch64").focus();
      await user.keyboard(typed);
      expect(onToggle).not.toHaveBeenCalled();
      expect(onUploadClick).not.toHaveBeenCalled();
      expect(row("bat.aarch64")).toHaveFocus();
    },
  );

  it.each(MODIFIED_TOGGLES)(
    "$label with $modifier held leaves the package alone",
    ({ modifier, key }) => {
      const { onToggle } = renderList();

      row("bat.aarch64").focus();
      // fireEvent returns false when a handler called preventDefault.
      const notPrevented = fireEvent.keyDown(row("bat.aarch64"), {
        key,
        [modifier]: true,
      });
      expect(onToggle).not.toHaveBeenCalled();
      expect(notPrevented).toBe(true);
    },
  );

  it("Space on the checkbox toggles once, not twice", async () => {
    const user = userEvent.setup();
    const { onToggle } = renderList();

    screen.getByRole("checkbox", { name: "bat.aarch64" }).focus();
    await user.keyboard(" ");
    expect(onToggle).toHaveBeenCalledTimes(1);
  });

  it("j moves from a group row through its shown members into the packages", async () => {
    const user = userEvent.setup();
    renderList({ packageGroups: [editorsGroup] });

    const groupRow = screen.getByTestId("group-row-editors");
    groupRow.focus();
    await user.keyboard("{Enter}");
    expect(screen.getByTestId("group-member-vim")).toBeInTheDocument();

    await user.keyboard("j");
    expect(screen.getByTestId("group-member-vim")).toHaveFocus();
    await user.keyboard("j");
    expect(row("aide.aarch64")).toHaveFocus();
  });

  it("Ctrl+K is left to the global search", async () => {
    const user = userEvent.setup();
    renderList();

    await user.click(screen.getByText("bat.aarch64"));
    await user.keyboard("{Control>}k{/Control}");
    expect(row("bat.aarch64")).toHaveFocus();
  });
});
