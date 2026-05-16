import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { SectionSearch } from "../SectionSearch";

describe("SectionSearch", () => {
  it("renders a search input", () => {
    render(
      <SectionSearch
        value=""
        onChange={vi.fn()}
        onClose={vi.fn()}
        onArrowDown={vi.fn()}
        resultCount={0}
      />,
    );

    expect(screen.getByTestId("section-search")).toBeInTheDocument();
    expect(screen.getByTestId("section-search-input")).toBeInTheDocument();
  });

  it("displays the current filter value", () => {
    render(
      <SectionSearch
        value="httpd"
        onChange={vi.fn()}
        onClose={vi.fn()}
        onArrowDown={vi.fn()}
        resultCount={3}
      />,
    );

    const input = screen.getByLabelText("Filter section items");
    expect(input).toHaveValue("httpd");
  });

  it("calls onChange when user types", async () => {
    const onChange = vi.fn();
    render(
      <SectionSearch
        value=""
        onChange={onChange}
        onClose={vi.fn()}
        onArrowDown={vi.fn()}
        resultCount={0}
      />,
    );

    const input = screen.getByLabelText("Filter section items");
    await userEvent.type(input, "a");
    expect(onChange).toHaveBeenCalledWith("a");
  });

  it("calls onClose on Escape", () => {
    const onClose = vi.fn();
    render(
      <SectionSearch
        value=""
        onChange={vi.fn()}
        onClose={onClose}
        onArrowDown={vi.fn()}
        resultCount={0}
      />,
    );

    const input = screen.getByLabelText("Filter section items");
    fireEvent.keyDown(input, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("calls onArrowDown on ArrowDown", () => {
    const onArrowDown = vi.fn();
    render(
      <SectionSearch
        value="test"
        onChange={vi.fn()}
        onClose={vi.fn()}
        onArrowDown={onArrowDown}
        resultCount={5}
      />,
    );

    const input = screen.getByLabelText("Filter section items");
    fireEvent.keyDown(input, { key: "ArrowDown" });
    expect(onArrowDown).toHaveBeenCalledTimes(1);
  });

  it("shows result count when matches exist", () => {
    render(
      <SectionSearch
        value="httpd"
        onChange={vi.fn()}
        onClose={vi.fn()}
        onArrowDown={vi.fn()}
        resultCount={5}
      />,
    );

    expect(screen.getByText("5 matches")).toBeInTheDocument();
  });

  it("shows singular match text for 1 result", () => {
    render(
      <SectionSearch
        value="httpd"
        onChange={vi.fn()}
        onClose={vi.fn()}
        onArrowDown={vi.fn()}
        resultCount={1}
      />,
    );

    expect(screen.getByText("1 match")).toBeInTheDocument();
  });

  it("auto-focuses the input on mount", () => {
    render(
      <SectionSearch
        value=""
        onChange={vi.fn()}
        onClose={vi.fn()}
        onArrowDown={vi.fn()}
        resultCount={0}
      />,
    );

    const input = screen.getByLabelText("Filter section items");
    expect(document.activeElement).toBe(input);
  });
});
