import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import { ExcludedZone } from "../ExcludedZone";

describe("ExcludedZone", () => {
  it("renders nothing when never toggled", () => {
    const { container } = render(
      <ExcludedZone packages={[]} hasEverToggled={false} />
    );
    expect(container.firstChild).toBeNull();
  });

  it("shows empty state after toggle and re-enable", () => {
    render(<ExcludedZone packages={[]} hasEverToggled={true} />);
    expect(screen.getByText(/no excluded packages/i)).toBeInTheDocument();
  });

  it("shows excluded packages with strikethrough", () => {
    const pkgs = [
      { name: "nginx", repo: "epel" },
      { name: "jq", repo: "epel" },
    ];
    render(<ExcludedZone packages={pkgs} hasEverToggled={true} />);
    expect(screen.getByText("nginx")).toBeInTheDocument();
    expect(screen.getByText("jq")).toBeInTheDocument();
    expect(screen.getByText(/excluded · 2 packages/i)).toBeInTheDocument();
  });

  it("excluded count header has aria-live for dynamic announcements", () => {
    const pkgs = [
      { name: "nginx", repo: "epel" },
    ];
    render(<ExcludedZone packages={pkgs} hasEverToggled={true} />);
    const header = screen.getByText(/excluded · 1 packages/i);
    expect(header).toHaveAttribute("aria-live", "polite");
  });

  it("collapses when 50+ packages with expander", () => {
    const pkgs = Array.from({ length: 55 }, (_, i) => ({
      name: `pkg-${i}`,
      repo: "epel",
    }));
    render(<ExcludedZone packages={pkgs} hasEverToggled={true} />);
    expect(screen.getByText(/show 55 excluded/i)).toBeInTheDocument();
  });

  it("expander button has aria-controls pointing to content region", () => {
    const pkgs = Array.from({ length: 55 }, (_, i) => ({
      name: `pkg-${i}`,
      repo: "epel",
    }));
    render(<ExcludedZone packages={pkgs} hasEverToggled={true} />);
    const expander = screen.getByRole("button", { name: /show 55 excluded/i });
    expect(expander).toHaveAttribute("aria-expanded", "false");
    expect(expander).toHaveAttribute("aria-controls", "excluded-zone-content");
  });

  it("content region is visible after expanding", () => {
    const pkgs = Array.from({ length: 55 }, (_, i) => ({
      name: `pkg-${i}`,
      repo: "epel",
    }));
    render(<ExcludedZone packages={pkgs} hasEverToggled={true} />);
    const contentRegion = document.getElementById("excluded-zone-content");
    expect(contentRegion).toBeInTheDocument();
    expect(contentRegion).toHaveAttribute("hidden");

    fireEvent.click(screen.getByRole("button", { name: /show 55 excluded/i }));

    expect(contentRegion).not.toHaveAttribute("hidden");
    expect(screen.getByText("pkg-0")).toBeInTheDocument();
  });
});
