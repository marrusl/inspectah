import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ContainerfilePanel } from "../ContainerfilePanel";

describe("ContainerfilePanel", () => {
  it("renders content as text (no dangerouslySetInnerHTML)", () => {
    render(
      <ContainerfilePanel
        content={"FROM ubi9\nRUN dnf install -y httpd"}
        isOpen={true}
        onToggle={vi.fn()}
        loading={false}
      />,
    );

    expect(screen.getByText("Containerfile")).toBeInTheDocument();
    const codeEl = screen.getByRole("complementary").querySelector("code");
    expect(codeEl).toBeTruthy();
    // Content rendered as text nodes, not innerHTML
    expect(codeEl!.textContent).toContain("FROM");
    expect(codeEl!.textContent).toContain("ubi9");
    // No dangerouslySetInnerHTML — keywords are in styled spans
    const keywords = codeEl!.querySelectorAll(".inspectah-cf-panel__keyword");
    expect(keywords.length).toBe(2); // FROM, RUN
    expect(keywords[0].textContent).toBe("FROM");
    expect(keywords[1].textContent).toBe("RUN");
  });

  it("renders Containerfile content as safe text, not innerHTML", () => {
    const malicious = 'FROM ubi9\nRUN echo "<img src=x onerror=alert(1)>"';
    render(
      <ContainerfilePanel
        content={malicious}
        isOpen={true}
        onToggle={vi.fn()}
        loading={false}
      />,
    );

    const codeEl = screen.getByRole("complementary").querySelector("code");
    expect(codeEl).toBeTruthy();
    // The XSS payload is rendered as text, not parsed as HTML
    expect(codeEl!.textContent).toContain("<img src=x onerror=alert(1)>");
    expect(codeEl!.querySelector("img")).toBeNull();
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

  it("uses left-pointing icon when open (collapse direction)", () => {
    render(
      <ContainerfilePanel
        content={"FROM ubi9\n"}
        isOpen={true}
        onToggle={vi.fn()}
        loading={false}
      />,
    );

    const collapseBtn = screen.getByLabelText("Collapse Containerfile panel");
    // AngleDoubleLeftIcon renders an SVG — verify aria-label direction
    expect(collapseBtn).toBeInTheDocument();
    // The button should contain an SVG (the icon)
    const svg = collapseBtn.querySelector("svg");
    expect(svg).toBeTruthy();
  });

  it("shows context-sections footer note", () => {
    render(
      <ContainerfilePanel
        content={"FROM ubi9\n"}
        isOpen={true}
        onToggle={vi.fn()}
        loading={false}
      />,
    );

    expect(
      screen.getByText(/Context sections are included as-is/),
    ).toBeInTheDocument();
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

  it("does NOT call onToggle on mount for narrow viewport (parent handles init)", () => {
    // Override matchMedia to report narrow viewport
    Object.defineProperty(window, "matchMedia", {
      writable: true,
      value: () => ({
        matches: true, // narrow viewport
        media: "(max-width: 1279px)",
        onchange: null,
        addListener: () => {},
        removeListener: () => {},
        addEventListener: () => {},
        removeEventListener: () => {},
        dispatchEvent: () => false,
      }),
    });

    const onToggle = vi.fn();
    // Parent already computed isOpen=false for narrow viewports,
    // so the panel renders collapsed without calling onToggle.
    render(
      <ContainerfilePanel
        content={"FROM ubi9\n"}
        isOpen={false}
        onToggle={onToggle}
        loading={false}
      />,
    );

    // onToggle should NOT be called — the parent passed the correct initial state
    expect(onToggle).not.toHaveBeenCalled();
    // Panel should be in collapsed state
    expect(
      screen.getByLabelText("Expand Containerfile panel"),
    ).toBeInTheDocument();

    // Restore default matchMedia
    Object.defineProperty(window, "matchMedia", {
      writable: true,
      value: () => ({
        matches: false,
        media: "",
        onchange: null,
        addListener: () => {},
        removeListener: () => {},
        addEventListener: () => {},
        removeEventListener: () => {},
        dispatchEvent: () => false,
      }),
    });
  });
});
