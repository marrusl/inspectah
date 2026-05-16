import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ContainerfilePanel } from "../ContainerfilePanel";

describe("ContainerfilePanel", () => {
  it("renders syntax-highlighted content when open", () => {
    render(
      <ContainerfilePanel
        content={"FROM ubi9\nRUN dnf install -y httpd"}
        isOpen={true}
        onToggle={vi.fn()}
        loading={false}
      />,
    );

    expect(screen.getByText("Containerfile")).toBeInTheDocument();
    // highlight.js wraps keywords in spans
    const codeEl = screen.getByRole("complementary").querySelector("code");
    expect(codeEl).toBeTruthy();
    expect(codeEl!.innerHTML).toContain("FROM");
  });

  it("shows line count in footer", () => {
    render(
      <ContainerfilePanel
        content={"FROM ubi9\nRUN dnf install -y httpd\nEXPOSE 80"}
        isOpen={true}
        onToggle={vi.fn()}
        loading={false}
      />,
    );

    expect(screen.getByText("3 lines")).toBeInTheDocument();
  });

  it("renders collapsed state with vertical label", () => {
    render(
      <ContainerfilePanel
        content={"FROM ubi9\n"}
        isOpen={false}
        onToggle={vi.fn()}
        loading={false}
      />,
    );

    expect(
      screen.getByLabelText("Expand Containerfile panel"),
    ).toBeInTheDocument();
    expect(screen.getByText("Containerfile")).toBeInTheDocument();
  });

  it("calls onToggle when collapse button is clicked", async () => {
    const onToggle = vi.fn();
    render(
      <ContainerfilePanel
        content={"FROM ubi9\n"}
        isOpen={true}
        onToggle={onToggle}
        loading={false}
      />,
    );

    await userEvent.click(
      screen.getByLabelText("Collapse Containerfile panel"),
    );
    expect(onToggle).toHaveBeenCalled();
  });

  it("calls onToggle when collapsed tab is clicked", async () => {
    const onToggle = vi.fn();
    render(
      <ContainerfilePanel
        content={"FROM ubi9\n"}
        isOpen={false}
        onToggle={onToggle}
        loading={false}
      />,
    );

    await userEvent.click(
      screen.getByLabelText("Expand Containerfile panel"),
    );
    expect(onToggle).toHaveBeenCalled();
  });

  it("toggles on Ctrl+E", () => {
    const onToggle = vi.fn();
    render(
      <ContainerfilePanel
        content={"FROM ubi9\n"}
        isOpen={true}
        onToggle={onToggle}
        loading={false}
      />,
    );

    fireEvent.keyDown(document, { key: "e", ctrlKey: true });
    expect(onToggle).toHaveBeenCalled();
  });

  it("shows skeletons when loading", () => {
    render(
      <ContainerfilePanel
        content={null}
        isOpen={true}
        onToggle={vi.fn()}
        loading={true}
      />,
    );

    // Skeletons render as spans with role="progressbar" in PatternFly
    const skeletons = screen.getByRole("complementary").querySelectorAll(".pf-v6-c-skeleton");
    expect(skeletons.length).toBeGreaterThan(0);
  });
});
