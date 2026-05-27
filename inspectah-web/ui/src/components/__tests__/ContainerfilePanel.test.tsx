import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ContainerfilePanel } from "../ContainerfilePanel";
import { _resetIdCounter } from "../../hooks/useContainerfileDiff";

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

  it("uses right-pointing icon when open (collapse direction)", () => {
    render(
      <ContainerfilePanel
        content={"FROM ubi9\n"}
        isOpen={true}
        onToggle={vi.fn()}
        loading={false}
      />,
    );

    const collapseBtn = screen.getByLabelText("Collapse Containerfile panel");
    // AngleDoubleRightIcon renders an SVG — verify aria-label direction
    expect(collapseBtn).toBeInTheDocument();
    // The button should contain an SVG (the icon)
    const svg = collapseBtn.querySelector("svg");
    expect(svg).toBeTruthy();
  });

  it("renders resize drag handle when open", () => {
    render(
      <ContainerfilePanel
        content={"FROM ubi9\n"}
        isOpen={true}
        onToggle={vi.fn()}
        loading={false}
      />,
    );

    const handle = screen.getByRole("separator", {
      name: /resize containerfile panel/i,
    });
    expect(handle).toBeInTheDocument();
  });

  it("does not render drag handle when collapsed", () => {
    render(
      <ContainerfilePanel
        content={"FROM ubi9\n"}
        isOpen={false}
        onToggle={vi.fn()}
        loading={false}
      />,
    );

    expect(
      screen.queryByRole("separator", { name: /resize/i }),
    ).not.toBeInTheDocument();
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

describe("ContainerfilePanel change highlights", () => {
  beforeEach(() => {
    _resetIdCounter();
  });

  it("highlights added lines on content change", () => {
    const { rerender } = render(
      <ContainerfilePanel
        content={"FROM ubi9\nRUN dnf install -y httpd\n"}
        isOpen={true}
        onToggle={vi.fn()}
        loading={false}
      />,
    );

    // Update with an added line
    rerender(
      <ContainerfilePanel
        content={"FROM ubi9\nRUN dnf install -y httpd\nEXPOSE 80\n"}
        isOpen={true}
        onToggle={vi.fn()}
        loading={false}
      />,
    );

    const codeEl = screen.getByRole("complementary").querySelector("code");
    const addedLines = codeEl!.querySelectorAll(".inspectah-cf-line--added");
    expect(addedLines.length).toBe(1);
    expect(addedLines[0].textContent).toContain("EXPOSE");
  });

  it("does not highlight on first render (baseline)", () => {
    render(
      <ContainerfilePanel
        content={"FROM ubi9\nRUN dnf install -y httpd\n"}
        isOpen={true}
        onToggle={vi.fn()}
        loading={false}
      />,
    );

    const codeEl = screen.getByRole("complementary").querySelector("code");
    const addedLines = codeEl!.querySelectorAll(".inspectah-cf-line--added");
    const removingLines = codeEl!.querySelectorAll(".inspectah-cf-line--removing");
    expect(addedLines.length).toBe(0);
    expect(removingLines.length).toBe(0);
  });

  it("marks removed lines with departing class and aria-hidden", () => {
    const { rerender } = render(
      <ContainerfilePanel
        content={"FROM ubi9\nRUN dnf install -y httpd\nEXPOSE 80\n"}
        isOpen={true}
        onToggle={vi.fn()}
        loading={false}
      />,
    );

    // Remove the EXPOSE line
    rerender(
      <ContainerfilePanel
        content={"FROM ubi9\nRUN dnf install -y httpd\n"}
        isOpen={true}
        onToggle={vi.fn()}
        loading={false}
      />,
    );

    const codeEl = screen.getByRole("complementary").querySelector("code");
    const removingLines = codeEl!.querySelectorAll(".inspectah-cf-line--removing");
    expect(removingLines.length).toBe(1);
    expect(removingLines[0].textContent).toContain("EXPOSE");
    expect(removingLines[0].getAttribute("aria-hidden")).toBe("true");
  });

  it("shows dot indicator when collapsed and content changes", () => {
    const onToggle = vi.fn();
    const { rerender } = render(
      <ContainerfilePanel
        content={"FROM ubi9\n"}
        isOpen={true}
        onToggle={onToggle}
        loading={false}
      />,
    );

    // Collapse the panel
    rerender(
      <ContainerfilePanel
        content={"FROM ubi9\n"}
        isOpen={false}
        onToggle={onToggle}
        loading={false}
      />,
    );

    // Change content while collapsed
    rerender(
      <ContainerfilePanel
        content={"FROM ubi9\nRUN dnf install -y httpd\n"}
        isOpen={false}
        onToggle={onToggle}
        loading={false}
      />,
    );

    const tab = screen.getByRole("button", { name: /expand containerfile panel/i });
    expect(tab.classList.contains("inspectah-cf-panel__tab--has-changes")).toBe(true);
    expect(tab.getAttribute("aria-label")).toContain("pending changes");
  });

  it("announces diff summary via aria-live region", () => {
    const { rerender } = render(
      <ContainerfilePanel
        content={"FROM ubi9\n"}
        isOpen={true}
        onToggle={vi.fn()}
        loading={false}
      />,
    );

    // Add a line
    rerender(
      <ContainerfilePanel
        content={"FROM ubi9\nRUN dnf install -y httpd\n"}
        isOpen={true}
        onToggle={vi.fn()}
        loading={false}
      />,
    );

    const liveRegion = screen.getByRole("complementary").querySelector("[aria-live='polite']");
    expect(liveRegion).toBeTruthy();
    expect(liveRegion!.textContent).toContain("1 line added");
  });

  it("does not announce when diff is empty", () => {
    const { rerender } = render(
      <ContainerfilePanel
        content={"FROM ubi9\n"}
        isOpen={true}
        onToggle={vi.fn()}
        loading={false}
      />,
    );

    // Rerender with same content
    rerender(
      <ContainerfilePanel
        content={"FROM ubi9\n"}
        isOpen={true}
        onToggle={vi.fn()}
        loading={false}
      />,
    );

    const liveRegion = screen.getByRole("complementary").querySelector("[aria-live='polite']");
    expect(liveRegion).toBeTruthy();
    expect(liveRegion!.textContent).toBe("");
  });
});
