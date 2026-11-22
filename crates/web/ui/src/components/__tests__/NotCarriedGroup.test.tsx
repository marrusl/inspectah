import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, within, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { NotCarriedGroup } from "../NotCarriedGroup";
import type { NotCarriedRow } from "../../api/types";

function row(
  name: string,
  arch: string,
  units: string[],
  display_name = name,
): NotCarriedRow {
  return {
    item_id: { kind: "Package", key: { name, arch } },
    display_name,
    source_repo: "baseos",
    enabled_units: units,
    owns_enabled_unit: units.length > 0,
  };
}

// Server order: unit owners first, then the rest. "rhc" sorts after
// "firewalld" but before "python3-firewall"; "atop" would sort first if
// the client re-sorted by name.
const ROWS: NotCarriedRow[] = [
  row("firewalld", "noarch", ["firewalld.service"]),
  row("rhc", "x86_64", ["rhcd.service"]),
  row("python3-firewall", "noarch", []),
  row("atop", "x86_64", []),
];

async function renderExpanded(rows: NotCarriedRow[], onInclude = vi.fn()) {
  const user = userEvent.setup();
  render(<NotCarriedGroup rows={rows} onInclude={onInclude} />);
  await user.click(screen.getByRole("button", { name: /not carried/i }));
  return { user, onInclude };
}

describe("NotCarriedGroup", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("renders nothing when the list is empty", () => {
    const { container } = render(
      <NotCarriedGroup rows={[]} onInclude={vi.fn()} />,
    );
    expect(container).toBeEmptyDOMElement();
    expect(screen.queryByTestId("not-carried-group")).toBeNull();
  });

  it("counts the rows and the unit owners in the toggle", () => {
    render(<NotCarriedGroup rows={ROWS} onInclude={vi.fn()} />);
    const toggle = screen.getByRole("button", { name: /not carried/i });
    expect(toggle).toHaveTextContent("Not carried (4)");
    expect(toggle).toHaveTextContent("2 own an enabled unit");
  });

  it("starts collapsed and expands on the toggle", async () => {
    const user = userEvent.setup();
    render(<NotCarriedGroup rows={ROWS} onInclude={vi.fn()} />);
    const toggle = screen.getByRole("button", { name: /not carried/i });
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(
      screen.queryByRole("button", { name: "Include firewalld" }),
    ).toBeNull();
    await user.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    expect(
      screen.getByRole("button", { name: "Include firewalld" }),
    ).toBeVisible();
  });

  it("renders the rows in array order without re-sorting", async () => {
    await renderExpanded(ROWS);
    const rendered = screen
      .getAllByTestId(/^not-carried-row-/)
      .map((el) => el.getAttribute("data-testid"));
    expect(rendered).toEqual([
      "not-carried-row-firewalld.noarch",
      "not-carried-row-rhc.x86_64",
      "not-carried-row-python3-firewall.noarch",
      "not-carried-row-atop.x86_64",
    ]);
  });

  it("shows the display name and source repo on every row", async () => {
    await renderExpanded([
      row("glibc", "i686", [], "glibc.i686"),
      row("glibc", "x86_64", [], "glibc.x86_64"),
    ]);
    const i686 = screen.getByTestId("not-carried-row-glibc.i686");
    expect(within(i686).getByText("glibc.i686")).toBeInTheDocument();
    expect(within(i686).getByText("baseos")).toBeInTheDocument();
  });

  it("marks unit owners and lists their units, and only them", async () => {
    await renderExpanded(ROWS);
    const owner = screen.getByTestId("not-carried-row-firewalld.noarch");
    const plain = screen.getByTestId("not-carried-row-python3-firewall.noarch");

    expect(owner).toHaveClass("inspectah-not-carried__row--owner");
    expect(within(owner).getByText("firewalld.service")).toBeInTheDocument();
    expect(
      within(owner).getByTestId("not-carried-owner-label"),
    ).toBeInTheDocument();

    expect(plain).not.toHaveClass("inspectah-not-carried__row--owner");
    expect(within(plain).queryByTestId("not-carried-owner-label")).toBeNull();
  });

  it("reads ownership from the flag, not from the unit list", async () => {
    // The server's flag is the definition; a client must not re-derive it.
    const flagged: NotCarriedRow = {
      ...row("cronie", "x86_64", []),
      owns_enabled_unit: true,
    };
    await renderExpanded([flagged]);
    expect(screen.getByTestId("not-carried-row-cronie.x86_64")).toHaveClass(
      "inspectah-not-carried__row--owner",
    );
  });

  it("includes a row with one click, passing the row back", async () => {
    const { user, onInclude } = await renderExpanded(ROWS);
    await user.click(screen.getByRole("button", { name: "Include rhc" }));
    expect(onInclude).toHaveBeenCalledTimes(1);
    expect(onInclude).toHaveBeenCalledWith(ROWS[1]);
  });

  it("disables Include while a change is pending", async () => {
    const user = userEvent.setup();
    render(<NotCarriedGroup rows={ROWS} onInclude={vi.fn()} isPending />);
    await user.click(screen.getByRole("button", { name: /not carried/i }));
    expect(
      screen.getByRole("button", { name: "Include firewalld" }),
    ).toBeDisabled();
  });

  // Since beta.7 an enabled-unit owner is carried by default and only an
  // excluded or held one is listed; including a package also takes the
  // packages it needs off the list. The help must not describe the old rules.
  it("explains which unit owners are listed and what including does", async () => {
    await renderExpanded(ROWS);
    const help = screen.getByTestId("not-carried-help");
    expect(help).toHaveTextContent(/enabled unit .* carried by default/i);
    expect(help).toHaveTextContent(/repo disabled on the host/i);
    expect(help).toHaveTextContent(/gives each held package's reason/i);
    expect(help).toHaveTextContent(
      /adds it to the install list, lets dnf resolve its dependencies/i,
    );
    // Including a held package does not clear its hold: a prerequisite only a
    // disabled repo provides still fails to resolve, so the help says so.
    expect(help).toHaveTextContent(
      /for held packages, resolve the reason shown in the Containerfile first/i,
    );
    expect(help).not.toHaveTextContent(/packages it needs to the image/i);
    expect(screen.queryByText(/stay listed/i)).toBeNull();
  });

  it("expands on reveal and reports it was revealed", async () => {
    const scroll = vi.spyOn(Element.prototype, "scrollIntoView");
    const onRevealed = vi.fn();
    render(
      <NotCarriedGroup
        rows={ROWS}
        onInclude={vi.fn()}
        reveal
        onRevealed={onRevealed}
      />,
    );
    expect(
      screen.getByRole("button", { name: /not carried/i }),
    ).toHaveAttribute("aria-expanded", "true");
    expect(onRevealed).toHaveBeenCalledTimes(1);

    // The reveal scrolls the group into view and focuses its toggle two
    // frames later; wait for it so it runs while mounted, not after teardown.
    const toggle = screen.getByRole("button", { name: /not carried/i });
    await waitFor(() => expect(toggle).toHaveFocus());
    expect(scroll).toHaveBeenCalledTimes(1);
  });
});
