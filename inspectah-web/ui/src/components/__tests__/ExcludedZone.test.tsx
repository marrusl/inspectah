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

  it("collapses when 50+ packages with expander", () => {
    const pkgs = Array.from({ length: 55 }, (_, i) => ({
      name: `pkg-${i}`,
      repo: "epel",
    }));
    render(<ExcludedZone packages={pkgs} hasEverToggled={true} />);
    expect(screen.getByText(/show 55 excluded/i)).toBeInTheDocument();
  });
});
