import { render, screen, fireEvent, within } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { SortHeader } from "../SortHeader";

describe("SortHeader", () => {
  it("renders a grid with two column headers", () => {
    render(
      <SortHeader
        leftLabel="Packages"
        rightLabel="Repo"
        activeColumn="left"
        direction="asc"
        onSort={vi.fn()}
      />,
    );
    expect(
      screen.getByRole("grid", { name: /sort controls/i }),
    ).toBeInTheDocument();
    const headers = screen.getAllByRole("columnheader");
    expect(headers).toHaveLength(2);
  });

  it("shows chevron on active column only", () => {
    render(
      <SortHeader
        leftLabel="Packages"
        rightLabel="Prevalence"
        activeColumn="left"
        direction="asc"
        onSort={vi.fn()}
      />,
    );
    const headers = screen.getAllByRole("columnheader");
    expect(headers[0].textContent).toContain("▲");
    expect(headers[1].textContent).not.toContain("▲");
    expect(headers[1].textContent).not.toContain("▼");
  });

  it("calls onSort when button is clicked", () => {
    const onSort = vi.fn();
    render(
      <SortHeader
        leftLabel="Packages"
        rightLabel="Repo"
        activeColumn="left"
        direction="asc"
        onSort={onSort}
      />,
    );
    const headers = screen.getAllByRole("columnheader");
    const repoBtn = within(headers[1]).getByRole("button");
    fireEvent.click(repoBtn);
    expect(onSort).toHaveBeenCalledWith("right");
  });

  it("has correct aria-sort attributes on columnheader wrappers", () => {
    render(
      <SortHeader
        leftLabel="Packages"
        rightLabel="Repo"
        activeColumn="left"
        direction="asc"
        onSort={vi.fn()}
      />,
    );
    const headers = screen.getAllByRole("columnheader");
    expect(headers[0]).toHaveAttribute("aria-sort", "ascending");
    expect(headers[1]).toHaveAttribute("aria-sort", "none");
  });

  it("buttons have descriptive aria-labels with sort direction", () => {
    render(
      <SortHeader
        leftLabel="Packages"
        rightLabel="Repo"
        activeColumn="left"
        direction="asc"
        onSort={vi.fn()}
      />,
    );
    expect(
      screen.getByRole("button", {
        name: /sort by packages, currently ascending/i,
      }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /sort by repo$/i }),
    ).toBeInTheDocument();
  });

  it("grid wraps row with proper role structure", () => {
    render(
      <SortHeader
        leftLabel="Packages"
        rightLabel="Repo"
        activeColumn="left"
        direction="asc"
        onSort={vi.fn()}
      />,
    );
    const grid = screen.getByRole("grid");
    const row = within(grid).getByRole("row");
    expect(row).toBeInTheDocument();
    expect(within(row).getAllByRole("columnheader")).toHaveLength(2);
  });
});
