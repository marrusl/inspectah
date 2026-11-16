/**
 * UnitGroup: a collapsible units group (spec § 6.3, § 6.13), U32's
 * structure half.
 */
import { describe, it, expect } from "vitest";
import { useState } from "react";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { UnitGroup } from "../UnitGroup";

function Harness({
  count,
  followUpText,
  description,
}: {
  count?: number;
  followUpText?: string | null;
  description?: string;
}) {
  const [expanded, setExpanded] = useState(true);
  return (
    <UnitGroup
      groupId="services"
      title="Services"
      count={count}
      followUpText={followUpText}
      description={description}
      expanded={expanded}
      onToggle={() => setExpanded((e) => !e)}
    >
      <div role="row" data-testid="row-a">
        a.service
      </div>
    </UnitGroup>
  );
}

function grid(): HTMLElement {
  return screen.getByRole("grid", { hidden: true });
}

describe("UnitGroup structure (U32)", () => {
  it("names its grid in aria-controls", () => {
    render(<Harness count={11} />);
    const button = screen.getByRole("button");
    expect(button.getAttribute("aria-controls")).toBe("unit-group-grid-services");
    expect(grid().id).toBe("unit-group-grid-services");
  });

  it("labels the grid by the title alone, with no count or flag", () => {
    render(<Harness count={11} followUpText="5 need follow-up" />);
    const labelId = grid().getAttribute("aria-labelledby");
    expect(labelId).toBe("unit-group-title-services");
    expect(document.getElementById(labelId!)!.textContent).toBe("Services");
    expect(screen.getByRole("grid", { name: "Services" })).toBe(grid());
  });

  it("puts each row in a rowgroup inside the grid", () => {
    render(<Harness count={1} />);
    const row = screen.getByTestId("row-a");
    const rowgroup = row.closest('[role="rowgroup"]');
    expect(rowgroup).not.toBeNull();
    expect(rowgroup!.closest('[role="grid"]')).toBe(grid());
  });

  it("toggles aria-expanded and the grid's hidden on Enter and Space", async () => {
    const user = userEvent.setup();
    render(<Harness count={1} />);
    const button = screen.getByRole("button");
    expect(button.getAttribute("aria-expanded")).toBe("true");
    expect(grid().hidden).toBe(false);

    button.focus();
    await user.keyboard("{Enter}");
    expect(button.getAttribute("aria-expanded")).toBe("false");
    expect(grid().hidden).toBe(true);

    await user.keyboard(" ");
    expect(button.getAttribute("aria-expanded")).toBe("true");
    expect(grid().hidden).toBe(false);
  });

  it("keeps collapsed rows in the DOM", async () => {
    const user = userEvent.setup();
    render(<Harness count={1} />);
    await user.click(screen.getByRole("button"));
    expect(grid().hidden).toBe(true);
    expect(screen.getByTestId("row-a")).toBeInTheDocument();
  });

  it("names the button by title, count and flag", () => {
    render(<Harness count={11} followUpText="5 need follow-up" />);
    expect(screen.getByRole("button", { name: "Services 11, 5 need follow-up" })).toBeInTheDocument();
  });

  it("names the button by title and count when there is no flag", () => {
    render(<Harness count={11} followUpText={null} />);
    const button = screen.getByRole("button", { name: "Services 11" });
    expect(button.querySelector(".pf-v6-c-badge")).toHaveTextContent("11");
  });

  it("shows no count badge when count is undefined", () => {
    render(<Harness />);
    const button = screen.getByRole("button", { name: "Services" });
    expect(button.querySelector(".pf-v6-c-badge")).toBeNull();
  });

  it("shows the description between the header and the grid", () => {
    render(<Harness count={3} description="A description." />);
    const desc = screen.getByText("A description.");
    expect(desc.closest('[role="grid"]')).toBeNull();
    expect(screen.getByRole("button").contains(desc)).toBe(false);
  });
});
