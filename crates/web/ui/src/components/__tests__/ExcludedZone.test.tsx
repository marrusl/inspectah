import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { ExcludedZone } from "../ExcludedZone";

describe("ExcludedZone", () => {
  it("renders nothing when never toggled", () => {
    const { container } = render(
      <ExcludedZone packages={[]} hasEverToggled={false} onToggle={vi.fn()} />,
    );
    expect(container.firstChild).toBeNull();
  });

  it("shows empty state with header and message after toggle and re-enable", () => {
    render(
      <ExcludedZone packages={[]} hasEverToggled={true} onToggle={vi.fn()} />,
    );
    expect(
      screen.getByText(/from excluded repos · 0 packages/i),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/no packages from excluded repos/i),
    ).toBeInTheDocument();
  });

  it("shows excluded packages with strikethrough", () => {
    const pkgs = [
      { name: "nginx", repo: "epel", include: false },
      { name: "jq", repo: "epel", include: false },
    ];
    render(
      <ExcludedZone packages={pkgs} hasEverToggled={true} onToggle={vi.fn()} />,
    );
    expect(screen.getByText("nginx")).toBeInTheDocument();
    expect(screen.getByText("jq")).toBeInTheDocument();
    expect(screen.getByText("nginx")).toHaveStyle({
      textDecoration: "line-through",
    });
    expect(screen.getByText(/from excluded repos · 2 packages/i)).toBeInTheDocument();
  });

  it("renders a checkbox per package reflecting its own include state", () => {
    const pkgs = [
      { name: "nginx", repo: "epel", include: false },
      { name: "jq", repo: "epel", include: true },
    ];
    render(
      <ExcludedZone packages={pkgs} hasEverToggled={true} onToggle={vi.fn()} />,
    );
    expect(screen.getByRole("checkbox", { name: "nginx" })).not.toBeChecked();
    expect(screen.getByRole("checkbox", { name: "jq" })).toBeChecked();
  });

  it("drops the strikethrough for an individually re-included package", () => {
    const pkgs = [{ name: "jq", repo: "epel", include: true }];
    render(
      <ExcludedZone packages={pkgs} hasEverToggled={true} onToggle={vi.fn()} />,
    );
    expect(screen.getByText("jq")).toHaveStyle({ textDecoration: "none" });
  });

  it("calls onToggle with the package name when its checkbox is clicked", () => {
    const onToggle = vi.fn();
    const pkgs = [{ name: "nginx", repo: "epel", include: false }];
    render(
      <ExcludedZone packages={pkgs} hasEverToggled={true} onToggle={onToggle} />,
    );
    fireEvent.click(screen.getByRole("checkbox", { name: "nginx" }));
    expect(onToggle).toHaveBeenCalledWith("nginx");
  });

  it("replaces the checkbox with an Upload RPM button when the package needs an upload", () => {
    const pkgs = [{ name: "nginx", repo: "epel", include: false }];
    render(
      <ExcludedZone
        packages={pkgs}
        hasEverToggled={true}
        onToggle={vi.fn()}
        rpmRowStates={{ nginx: "needs_upload" }}
      />,
    );
    expect(
      screen.queryByRole("checkbox", { name: "nginx" }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Upload RPM for nginx" }),
    ).toBeInTheDocument();
  });

  it("calls onUploadClick with the package name when the Upload RPM button is clicked", () => {
    const onUploadClick = vi.fn();
    const pkgs = [{ name: "nginx", repo: "epel", include: false }];
    render(
      <ExcludedZone
        packages={pkgs}
        hasEverToggled={true}
        onToggle={vi.fn()}
        rpmRowStates={{ nginx: "needs_upload" }}
        onUploadClick={onUploadClick}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Upload RPM for nginx" }));
    expect(onUploadClick).toHaveBeenCalledWith("nginx");
  });

  it("excluded count header has aria-live for dynamic announcements", () => {
    const pkgs = [{ name: "nginx", repo: "epel", include: false }];
    render(
      <ExcludedZone packages={pkgs} hasEverToggled={true} onToggle={vi.fn()} />,
    );
    const header = screen.getByText(/from excluded repos · 1 package\b/i);
    expect(header).toHaveAttribute("aria-live", "polite");
  });

  it("singular count reads '1 package', not '1 packages'", () => {
    const pkgs = [{ name: "nginx", repo: "epel", include: false }];
    render(
      <ExcludedZone packages={pkgs} hasEverToggled={true} onToggle={vi.fn()} />,
    );
    expect(
      screen.getByText(/from excluded repos · 1 package$/i),
    ).toBeInTheDocument();
  });

  it("collapses when 50+ packages with expander", () => {
    const pkgs = Array.from({ length: 55 }, (_, i) => ({
      name: `pkg-${i}`,
      repo: "epel",
      include: false,
    }));
    render(
      <ExcludedZone packages={pkgs} hasEverToggled={true} onToggle={vi.fn()} />,
    );
    expect(screen.getByText(/show 55 excluded/i)).toBeInTheDocument();
  });

  it("expander button has aria-controls pointing to content region", () => {
    const pkgs = Array.from({ length: 55 }, (_, i) => ({
      name: `pkg-${i}`,
      repo: "epel",
      include: false,
    }));
    render(
      <ExcludedZone packages={pkgs} hasEverToggled={true} onToggle={vi.fn()} />,
    );
    const expander = screen.getByRole("button", { name: /show 55 excluded/i });
    expect(expander).toHaveAttribute("aria-expanded", "false");
    expect(expander).toHaveAttribute("aria-controls", "excluded-zone-content");
  });

  it("content region is visible after expanding", () => {
    const pkgs = Array.from({ length: 55 }, (_, i) => ({
      name: `pkg-${i}`,
      repo: "epel",
      include: false,
    }));
    render(
      <ExcludedZone packages={pkgs} hasEverToggled={true} onToggle={vi.fn()} />,
    );
    const contentRegion = document.getElementById("excluded-zone-content");
    expect(contentRegion).toBeInTheDocument();
    expect(contentRegion).toHaveAttribute("hidden");

    fireEvent.click(screen.getByRole("button", { name: /show 55 excluded/i }));

    expect(contentRegion).not.toHaveAttribute("hidden");
    expect(screen.getByText("pkg-0")).toBeInTheDocument();
  });

  it("expander stays mounted after expand with aria-expanded='true'", () => {
    const pkgs = Array.from({ length: 55 }, (_, i) => ({
      name: `pkg-${i}`,
      repo: "epel",
      include: false,
    }));
    render(
      <ExcludedZone packages={pkgs} hasEverToggled={true} onToggle={vi.fn()} />,
    );

    fireEvent.click(screen.getByRole("button", { name: /show 55 excluded/i }));

    const expander = screen.getByRole("button", {
      name: /hide 55 excluded/i,
    });
    expect(expander).toBeInTheDocument();
    expect(expander).toHaveAttribute("aria-expanded", "true");
    expect(expander).toHaveAttribute("aria-controls", "excluded-zone-content");
  });

  it("focus remains on expander after toggling", () => {
    const pkgs = Array.from({ length: 55 }, (_, i) => ({
      name: `pkg-${i}`,
      repo: "epel",
      include: false,
    }));
    render(
      <ExcludedZone packages={pkgs} hasEverToggled={true} onToggle={vi.fn()} />,
    );

    const expandBtn = screen.getByRole("button", { name: /show 55 excluded/i });
    expandBtn.focus();
    fireEvent.click(expandBtn);

    // After expand, the button is still mounted (now "Hide") and retains focus
    const collapseBtn = screen.getByRole("button", {
      name: /hide 55 excluded/i,
    });
    expect(collapseBtn).toHaveFocus();
  });

  it("empty state renders excluded-zone testid for spatial consistency", () => {
    render(
      <ExcludedZone packages={[]} hasEverToggled={true} onToggle={vi.fn()} />,
    );
    expect(screen.getByTestId("excluded-zone")).toBeInTheDocument();
  });
});
