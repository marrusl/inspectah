import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { SortHeader } from "../SortHeader";

describe("SortHeader", () => {
  it("renders two column headers", () => {
    render(
      <SortHeader
        leftLabel="Packages"
        rightLabel="Repo"
        activeColumn="left"
        direction="asc"
        onSort={vi.fn()}
      />
    );
    expect(screen.getByRole("columnheader", { name: /packages/i })).toBeInTheDocument();
    expect(screen.getByRole("columnheader", { name: /repo/i })).toBeInTheDocument();
  });

  it("shows chevron on active column only", () => {
    render(
      <SortHeader
        leftLabel="Packages"
        rightLabel="Prevalence"
        activeColumn="left"
        direction="asc"
        onSort={vi.fn()}
      />
    );
    const left = screen.getByRole("columnheader", { name: /packages/i });
    expect(left.textContent).toContain("▲");
    const right = screen.getByRole("columnheader", { name: /prevalence/i });
    expect(right.textContent).not.toContain("▲");
    expect(right.textContent).not.toContain("▼");
  });

  it("calls onSort when clicked", () => {
    const onSort = vi.fn();
    render(
      <SortHeader
        leftLabel="Packages"
        rightLabel="Repo"
        activeColumn="left"
        direction="asc"
        onSort={onSort}
      />
    );
    fireEvent.click(screen.getByRole("columnheader", { name: /repo/i }));
    expect(onSort).toHaveBeenCalledWith("right");
  });

  it("has correct aria-sort attributes", () => {
    render(
      <SortHeader
        leftLabel="Packages"
        rightLabel="Repo"
        activeColumn="left"
        direction="asc"
        onSort={vi.fn()}
      />
    );
    expect(screen.getByRole("columnheader", { name: /packages/i })).toHaveAttribute("aria-sort", "ascending");
    expect(screen.getByRole("columnheader", { name: /repo/i })).toHaveAttribute("aria-sort", "none");
  });
});
