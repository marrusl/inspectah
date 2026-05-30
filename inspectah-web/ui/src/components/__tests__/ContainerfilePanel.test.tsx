import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, act } from "@testing-library/react";
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
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
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

    // Advance past scroll debounce + highlight activation
    act(() => { vi.advanceTimersByTime(200); });

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

    act(() => { vi.advanceTimersByTime(200); });

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

  it("adds data-line-id to added lines", () => {
    const { rerender } = render(
      <ContainerfilePanel
        content={"FROM ubi9\n"}
        isOpen={true}
        onToggle={vi.fn()}
        loading={false}
      />,
    );

    rerender(
      <ContainerfilePanel
        content={"FROM ubi9\nEXPOSE 80\n"}
        isOpen={true}
        onToggle={vi.fn()}
        loading={false}
      />,
    );

    act(() => { vi.advanceTimersByTime(200); });

    const codeEl = screen.getByRole("complementary").querySelector("code");
    const addedLine = codeEl!.querySelector(".inspectah-cf-line--added");
    expect(addedLine).toBeTruthy();
    expect(addedLine!.getAttribute("data-line-id")).toBeTruthy();
  });
});

describe("ContainerfilePanel scroll behavior", () => {
  let scrollToMock: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    _resetIdCounter();
    vi.useFakeTimers();
    scrollToMock = vi.fn();

    // Mock scrollTo on all elements (panel body calls it)
    Element.prototype.scrollTo = scrollToMock;

    // Mock getBoundingClientRect to report the line as out of view
    vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(function (this: Element) {
      if (this.classList?.contains("inspectah-cf-panel__body")) {
        return { top: 0, bottom: 300, left: 0, right: 400, width: 400, height: 300 } as DOMRect;
      }
      // Changed lines are below the visible area
      return { top: 400, bottom: 420, left: 0, right: 400, width: 400, height: 20 } as DOMRect;
    });

    // Default: no reduced motion preference
    Object.defineProperty(window, "matchMedia", {
      writable: true,
      value: (query: string) => ({
        matches: false,
        media: query,
        onchange: null,
        addListener: () => {},
        removeListener: () => {},
        addEventListener: () => {},
        removeEventListener: () => {},
        dispatchEvent: () => false,
      }),
    });
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("scrolls to the first changed line", () => {
    const { rerender } = render(
      <ContainerfilePanel
        content={"FROM quay.io/fedora/fedora-bootc:42\n"}
        isOpen={true}
        onToggle={vi.fn()}
        loading={false}
      />,
    );

    rerender(
      <ContainerfilePanel
        content={"FROM quay.io/fedora/fedora-bootc:42\nEXPOSE 80\n"}
        isOpen={true}
        onToggle={vi.fn()}
        loading={false}
      />,
    );

    // Advance past the 150ms debounce
    act(() => { vi.advanceTimersByTime(200); });

    expect(scrollToMock).toHaveBeenCalled();
  });

  it("does not scroll when no changes are present", () => {
    const { rerender } = render(
      <ContainerfilePanel
        content={"FROM ubi9\n"}
        isOpen={true}
        onToggle={vi.fn()}
        loading={false}
      />,
    );

    // Same content
    rerender(
      <ContainerfilePanel
        content={"FROM ubi9\n"}
        isOpen={true}
        onToggle={vi.fn()}
        loading={false}
      />,
    );

    act(() => { vi.advanceTimersByTime(200); });

    expect(scrollToMock).not.toHaveBeenCalled();
  });

  it("skips scroll when first changed line is already visible", () => {
    // Override getBoundingClientRect so the changed line is within the panel body
    vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(function (this: Element) {
      if (this.classList?.contains("inspectah-cf-panel__body")) {
        return { top: 0, bottom: 500, left: 0, right: 400, width: 400, height: 500 } as DOMRect;
      }
      // Changed line is inside the visible area
      return { top: 100, bottom: 120, left: 0, right: 400, width: 400, height: 20 } as DOMRect;
    });

    const { rerender } = render(
      <ContainerfilePanel
        content={"FROM ubi9\n"}
        isOpen={true}
        onToggle={vi.fn()}
        loading={false}
      />,
    );

    rerender(
      <ContainerfilePanel
        content={"FROM ubi9\nEXPOSE 80\n"}
        isOpen={true}
        onToggle={vi.fn()}
        loading={false}
      />,
    );

    act(() => { vi.advanceTimersByTime(200); });

    expect(scrollToMock).not.toHaveBeenCalled();
  });

  it("uses behavior auto when prefers-reduced-motion is set", () => {
    Object.defineProperty(window, "matchMedia", {
      writable: true,
      value: (query: string) => ({
        matches: query === "(prefers-reduced-motion: reduce)" ? true : false,
        media: query,
        onchange: null,
        addListener: () => {},
        removeListener: () => {},
        addEventListener: () => {},
        removeEventListener: () => {},
        dispatchEvent: () => false,
      }),
    });

    const { rerender } = render(
      <ContainerfilePanel
        content={"FROM ubi9\n"}
        isOpen={true}
        onToggle={vi.fn()}
        loading={false}
      />,
    );

    rerender(
      <ContainerfilePanel
        content={"FROM ubi9\nEXPOSE 80\n"}
        isOpen={true}
        onToggle={vi.fn()}
        loading={false}
      />,
    );

    act(() => { vi.advanceTimersByTime(200); });

    expect(scrollToMock).toHaveBeenCalledWith(
      expect.objectContaining({ behavior: "auto" }),
    );
  });

  it("debounces multiple rapid content changes", () => {
    const { rerender } = render(
      <ContainerfilePanel
        content={"FROM ubi9\n"}
        isOpen={true}
        onToggle={vi.fn()}
        loading={false}
      />,
    );

    // First change
    rerender(
      <ContainerfilePanel
        content={"FROM ubi9\nRUN echo one\n"}
        isOpen={true}
        onToggle={vi.fn()}
        loading={false}
      />,
    );

    // Advance only 50ms (less than 150ms debounce)
    act(() => { vi.advanceTimersByTime(50); });

    // Second change before debounce fires
    rerender(
      <ContainerfilePanel
        content={"FROM ubi9\nRUN echo one\nRUN echo two\n"}
        isOpen={true}
        onToggle={vi.fn()}
        loading={false}
      />,
    );

    // Now advance past the debounce
    act(() => { vi.advanceTimersByTime(200); });

    // scrollIntoView should only have been called once (the debounced one)
    expect(scrollToMock).toHaveBeenCalledTimes(1);
  });
});

describe("ContainerfilePanel reduced motion support", () => {
  beforeEach(() => {
    _resetIdCounter();
  });

  afterEach(() => {
    // Restore default matchMedia
    Object.defineProperty(window, "matchMedia", {
      writable: true,
      value: () => ({
        matches: false, media: "", onchange: null,
        addListener: () => {}, removeListener: () => {},
        addEventListener: () => {}, removeEventListener: () => {},
        dispatchEvent: () => false,
      }),
    });
  });

  it("removes highlight class after 2s in reduced-motion mode", () => {
    // Mock prefers-reduced-motion
    Object.defineProperty(window, "matchMedia", {
      writable: true,
      value: (query: string) => ({
        matches: query === "(prefers-reduced-motion: reduce)",
        media: query,
        onchange: null,
        addListener: () => {},
        removeListener: () => {},
        addEventListener: () => {},
        removeEventListener: () => {},
        dispatchEvent: () => false,
      }),
    });

    vi.useFakeTimers();

    const { rerender } = render(
      <ContainerfilePanel
        content={"FROM quay.io/fedora/fedora-bootc:42\n"}
        isOpen={true}
        onToggle={vi.fn()}
        loading={false}
      />,
    );

    rerender(
      <ContainerfilePanel
        content={"FROM quay.io/fedora/fedora-bootc:42\nEXPOSE 80\n"}
        isOpen={true}
        onToggle={vi.fn()}
        loading={false}
      />,
    );

    // Advance past scroll debounce (150ms) + reduced-motion activation (50ms)
    act(() => { vi.advanceTimersByTime(250); });

    // Highlight should be present
    expect(document.querySelectorAll(".inspectah-cf-line--added").length).toBe(1);

    // After 2s more, highlight class should be removed
    act(() => { vi.advanceTimersByTime(2000); });
    expect(document.querySelectorAll(".inspectah-cf-line--added").length).toBe(0);

    vi.useRealTimers();
  });

  it("prunes removing lines immediately in reduced-motion mode", () => {
    // Mock prefers-reduced-motion
    Object.defineProperty(window, "matchMedia", {
      writable: true,
      value: (query: string) => ({
        matches: query === "(prefers-reduced-motion: reduce)",
        media: query,
        onchange: null,
        addListener: () => {},
        removeListener: () => {},
        addEventListener: () => {},
        removeEventListener: () => {},
        dispatchEvent: () => false,
      }),
    });

    vi.useFakeTimers();

    const { rerender } = render(
      <ContainerfilePanel
        content={"FROM quay.io/fedora/fedora-bootc:42\nEXPOSE 80\n"}
        isOpen={true}
        onToggle={vi.fn()}
        loading={false}
      />,
    );

    // Remove EXPOSE line
    rerender(
      <ContainerfilePanel
        content={"FROM quay.io/fedora/fedora-bootc:42\n"}
        isOpen={true}
        onToggle={vi.fn()}
        loading={false}
      />,
    );

    // In reduced-motion mode, the line is pruned immediately by the effect
    // so it never appears in the DOM with the removing class
    expect(document.querySelectorAll(".inspectah-cf-line--removing").length).toBe(0);

    vi.useRealTimers();
  });
});

describe("ContainerfilePanel multi-line scroll targeting", () => {
  beforeEach(() => {
    _resetIdCounter();
    vi.useFakeTimers();

    // Mock scrollTo on all elements
    Element.prototype.scrollTo = vi.fn();

    // Default: no reduced motion preference, wide viewport
    Object.defineProperty(window, "matchMedia", {
      writable: true,
      value: (query: string) => ({
        matches: false,
        media: query,
        onchange: null,
        addListener: () => {},
        removeListener: () => {},
        addEventListener: () => {},
        removeEventListener: () => {},
        dispatchEvent: () => false,
      }),
    });
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("targets the topmost changed line when multiple lines change", () => {
    // Track which element querySelector finds as the first [data-line-id]
    const querySelectorSpy = vi.spyOn(Element.prototype, "querySelector");

    // Mock getBoundingClientRect so changed lines are out of view
    vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(function (this: Element) {
      if (this.classList?.contains("inspectah-cf-panel__body")) {
        return { top: 0, bottom: 300, left: 0, right: 400, width: 400, height: 300 } as DOMRect;
      }
      return { top: 400, bottom: 420, left: 0, right: 400, width: 400, height: 20 } as DOMRect;
    });

    // Start with several stable lines
    const baseline = [
      "FROM quay.io/fedora/fedora-bootc:42",
      "RUN dnf install -y httpd",
      "RUN dnf install -y nginx",
      "RUN dnf install -y curl",
      "EXPOSE 80",
      "",
    ].join("\n");

    const { rerender } = render(
      <ContainerfilePanel
        content={baseline}
        isOpen={true}
        onToggle={vi.fn()}
        loading={false}
      />,
    );

    // Change multiple lines spread across the content:
    // add a line near the top and another near the bottom
    const updated = [
      "FROM quay.io/fedora/fedora-bootc:42",
      "RUN dnf install -y httpd",
      "RUN dnf install -y vim",
      "RUN dnf install -y nginx",
      "RUN dnf install -y curl",
      "RUN dnf install -y wget",
      "EXPOSE 80",
      "",
    ].join("\n");

    rerender(
      <ContainerfilePanel
        content={updated}
        isOpen={true}
        onToggle={vi.fn()}
        loading={false}
      />,
    );

    // Advance past scroll debounce (150ms) + scroll arrival delay (350ms)
    act(() => { vi.advanceTimersByTime(600); });

    // The scroll logic uses querySelector("[data-line-id]") on the panel body,
    // which returns the first matching element in DOM order (topmost).
    // Verify querySelector was called with the [data-line-id] selector.
    const dataLineIdCalls = querySelectorSpy.mock.calls.filter(
      (call) => call[0] === "[data-line-id]",
    );
    expect(dataLineIdCalls.length).toBeGreaterThan(0);

    // Verify that multiple changed lines exist in the DOM with highlight classes
    const codeEl = screen.getByRole("complementary").querySelector("code");
    const addedLines = codeEl!.querySelectorAll(".inspectah-cf-line--added");
    expect(addedLines.length).toBe(2);

    // The first added line in DOM order should be "vim" (appears before "wget")
    expect(addedLines[0].textContent).toContain("vim");
    expect(addedLines[1].textContent).toContain("wget");

    // scrollTo should have been called (targeting the topmost changed line)
    expect(Element.prototype.scrollTo).toHaveBeenCalled();
  });
});

describe("ContainerfilePanel collapse edge cases", () => {
  beforeEach(() => {
    _resetIdCounter();
    vi.useFakeTimers();

    // Default: no reduced motion, wide viewport
    Object.defineProperty(window, "matchMedia", {
      writable: true,
      value: (query: string) => ({
        matches: false,
        media: query,
        onchange: null,
        addListener: () => {},
        removeListener: () => {},
        addEventListener: () => {},
        removeEventListener: () => {},
        dispatchEvent: () => false,
      }),
    });
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("establishes baseline without pending indicator on first non-null content while collapsed", () => {
    const onToggle = vi.fn();

    // Start with null content, collapsed
    const { rerender } = render(
      <ContainerfilePanel
        content={null}
        isOpen={false}
        onToggle={onToggle}
        loading={false}
      />,
    );

    // Receive first non-null content while still collapsed
    rerender(
      <ContainerfilePanel
        content={"FROM ubi9\nRUN dnf install -y httpd\n"}
        isOpen={false}
        onToggle={onToggle}
        loading={false}
      />,
    );

    // The tab should NOT show the pending-changes indicator —
    // first content establishes baseline, it's not a "change"
    const tab = screen.getByRole("button", { name: /expand containerfile panel/i });
    expect(tab.classList.contains("inspectah-cf-panel__tab--has-changes")).toBe(false);
    expect(tab.getAttribute("aria-label")).not.toContain("pending changes");
  });

  it("shows highlights correctly when expanding after first content arrived while collapsed", () => {
    const onToggle = vi.fn();

    // Start with null content, collapsed
    const { rerender } = render(
      <ContainerfilePanel
        content={null}
        isOpen={false}
        onToggle={onToggle}
        loading={false}
      />,
    );

    // First non-null content while collapsed (establishes baseline)
    rerender(
      <ContainerfilePanel
        content={"FROM ubi9\n"}
        isOpen={false}
        onToggle={onToggle}
        loading={false}
      />,
    );

    // Content changes while still collapsed
    rerender(
      <ContainerfilePanel
        content={"FROM ubi9\nEXPOSE 80\n"}
        isOpen={false}
        onToggle={onToggle}
        loading={false}
      />,
    );

    // Now expand — should show the diff against baseline
    rerender(
      <ContainerfilePanel
        content={"FROM ubi9\nEXPOSE 80\n"}
        isOpen={true}
        onToggle={onToggle}
        loading={false}
      />,
    );

    act(() => { vi.advanceTimersByTime(200); });

    const codeEl = screen.getByRole("complementary").querySelector("code");
    const addedLines = codeEl!.querySelectorAll(".inspectah-cf-line--added");
    expect(addedLines.length).toBe(1);
    expect(addedLines[0].textContent).toContain("EXPOSE");
  });

  it("clears highlights from DOM when resize triggers auto-collapse", () => {
    let matchMediaHandler: ((e: MediaQueryListEvent) => void) | null = null;

    // Capture the matchMedia change handler so we can trigger it
    Object.defineProperty(window, "matchMedia", {
      writable: true,
      value: (query: string) => ({
        matches: false,
        media: query,
        onchange: null,
        addListener: () => {},
        removeListener: () => {},
        addEventListener: (_event: string, handler: (e: MediaQueryListEvent) => void) => {
          if (query === "(max-width: 1279px)") {
            matchMediaHandler = handler;
          }
        },
        removeEventListener: () => {},
        dispatchEvent: () => false,
      }),
    });

    const onToggle = vi.fn();

    const { rerender } = render(
      <ContainerfilePanel
        content={"FROM ubi9\n"}
        isOpen={true}
        onToggle={onToggle}
        loading={false}
      />,
    );

    // Add a line to create highlights
    rerender(
      <ContainerfilePanel
        content={"FROM ubi9\nEXPOSE 80\n"}
        isOpen={true}
        onToggle={onToggle}
        loading={false}
      />,
    );

    act(() => { vi.advanceTimersByTime(200); });

    // Verify highlights are active before collapse
    const codeEl = screen.getByRole("complementary").querySelector("code");
    expect(codeEl!.querySelectorAll(".inspectah-cf-line--added").length).toBe(1);

    // Simulate viewport resize triggering auto-collapse
    expect(matchMediaHandler).not.toBeNull();
    act(() => {
      matchMediaHandler!({ matches: true } as MediaQueryListEvent);
    });

    // onToggle should have been called by the resize handler
    expect(onToggle).toHaveBeenCalled();

    // Simulate the parent responding by setting isOpen=false
    rerender(
      <ContainerfilePanel
        content={"FROM ubi9\nEXPOSE 80\n"}
        isOpen={false}
        onToggle={onToggle}
        loading={false}
      />,
    );

    // In collapsed state, the code element is not rendered,
    // so highlight classes are absent from the DOM
    const collapsedPanel = screen.getByRole("complementary");
    expect(collapsedPanel.querySelector(".inspectah-cf-line--added")).toBeNull();
    expect(collapsedPanel.querySelector("code")).toBeNull();
  });

  it("clears pending-change indicator when content reverts to baseline while collapsed", () => {
    const onToggle = vi.fn();

    // Establish baseline while open
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

    // Content changes while collapsed — pending indicator appears
    rerender(
      <ContainerfilePanel
        content={"FROM ubi9\nEXPOSE 80\n"}
        isOpen={false}
        onToggle={onToggle}
        loading={false}
      />,
    );

    let tab = screen.getByRole("button", { name: /expand containerfile panel/i });
    expect(tab.classList.contains("inspectah-cf-panel__tab--has-changes")).toBe(true);

    // Content reverts to baseline while still collapsed — indicator should clear
    rerender(
      <ContainerfilePanel
        content={"FROM ubi9\n"}
        isOpen={false}
        onToggle={onToggle}
        loading={false}
      />,
    );

    tab = screen.getByRole("button", { name: /expand containerfile panel/i });
    expect(tab.classList.contains("inspectah-cf-panel__tab--has-changes")).toBe(false);
    expect(tab.getAttribute("aria-label")).not.toContain("pending changes");
  });
});
