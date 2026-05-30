import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { RepoGroup } from "../RepoGroup";
import type { RepoGroupInfo } from "../../api/types";

const baseRepo: RepoGroupInfo = {
  section_id: "epel",
  provenance: "verified",
  is_distro: false,
  tier: "third_party",
  package_count: 5,
  enabled: true,
};

describe("RepoGroup", () => {
  it("renders header with repo name", () => {
    render(
      <RepoGroup repo={baseRepo} defaultExpanded={true}>
        <div data-testid="child">content</div>
      </RepoGroup>,
    );
    expect(screen.getByText("epel")).toBeInTheDocument();
  });

  it("shows children when defaultExpanded is true", () => {
    render(
      <RepoGroup repo={baseRepo} defaultExpanded={true}>
        <div data-testid="child">content</div>
      </RepoGroup>,
    );
    expect(screen.getByTestId("child")).toBeVisible();
  });

  it("hides children when defaultExpanded is false", () => {
    render(
      <RepoGroup repo={baseRepo} defaultExpanded={false}>
        <div data-testid="child">content</div>
      </RepoGroup>,
    );
    expect(screen.queryByTestId("child")).not.toBeInTheDocument();
  });

  it("toggles expansion on chevron click", async () => {
    render(
      <RepoGroup repo={baseRepo} defaultExpanded={false}>
        <div data-testid="child">content</div>
      </RepoGroup>,
    );
    expect(screen.queryByTestId("child")).not.toBeInTheDocument();

    const chevron = screen
      .getByTestId("repo-group-epel")
      .querySelector(".inspectah-repo-group-header__chevron")!;
    await userEvent.click(chevron as HTMLElement);
    expect(screen.getByTestId("child")).toBeVisible();
  });

  it("toggles expansion on Enter key", async () => {
    render(
      <RepoGroup repo={baseRepo} defaultExpanded={false}>
        <div data-testid="child">content</div>
      </RepoGroup>,
    );
    const header = screen.getByTestId("repo-group-epel");
    header.focus();
    await userEvent.keyboard("{Enter}");
    expect(screen.getByTestId("child")).toBeVisible();
  });

  it("does NOT toggle expansion on Space key", async () => {
    render(
      <RepoGroup repo={baseRepo} defaultExpanded={false}>
        <div data-testid="child">content</div>
      </RepoGroup>,
    );
    const header = screen.getByTestId("repo-group-epel");
    header.focus();
    await userEvent.keyboard(" ");
    expect(screen.queryByTestId("child")).not.toBeInTheDocument();
  });

  it("force-expands when forceExpanded is true regardless of user toggle", async () => {
    const { rerender } = render(
      <RepoGroup repo={baseRepo} defaultExpanded={false}>
        <div data-testid="child">content</div>
      </RepoGroup>,
    );
    expect(screen.queryByTestId("child")).not.toBeInTheDocument();

    rerender(
      <RepoGroup repo={baseRepo} defaultExpanded={false} forceExpanded={true}>
        <div data-testid="child">content</div>
      </RepoGroup>,
    );
    expect(screen.getByTestId("child")).toBeVisible();
  });

  it("calls onRepoToggle when switch is clicked", async () => {
    const onRepoToggle = vi.fn();
    render(
      <RepoGroup
        repo={baseRepo}
        defaultExpanded={true}
        onRepoToggle={onRepoToggle}
      >
        <div>content</div>
      </RepoGroup>,
    );
    const toggle = screen.getByRole("switch", { name: /toggle epel repo/i });
    await userEvent.click(toggle);
    expect(onRepoToggle).toHaveBeenCalledWith("epel", false);
  });

  it("does not show toggle for distro repos", () => {
    const distroRepo: RepoGroupInfo = {
      ...baseRepo,
      section_id: "baseos",
      is_distro: true,
    };
    render(
      <RepoGroup repo={distroRepo} defaultExpanded={true}>
        <div>content</div>
      </RepoGroup>,
    );
    expect(screen.queryByRole("switch")).not.toBeInTheDocument();
  });

  it("renders with disabled styling when repo is disabled", () => {
    const disabledRepo: RepoGroupInfo = { ...baseRepo, enabled: false };
    render(
      <RepoGroup repo={disabledRepo} defaultExpanded={false}>
        <div data-testid="child">content</div>
      </RepoGroup>,
    );
    const label = screen.getByText("epel");
    expect(label.style.textDecoration).toBe("line-through");
  });

  it("shows infoCount in collapsed header when provided", () => {
    render(
      <RepoGroup repo={baseRepo} defaultExpanded={false} infoCount={3}>
        <div>content</div>
      </RepoGroup>,
    );
    expect(screen.getByText("3 informational")).toBeInTheDocument();
  });

  it("shows summaryText in collapsed header when provided", () => {
    render(
      <RepoGroup
        repo={baseRepo}
        defaultExpanded={false}
        summaryText="No action needed"
      >
        <div>content</div>
      </RepoGroup>,
    );
    expect(screen.getByText("No action needed")).toBeInTheDocument();
  });

  it("wraps children in a role='rowgroup' container with matching id", () => {
    render(
      <RepoGroup repo={baseRepo} defaultExpanded={true}>
        <div data-testid="child">content</div>
      </RepoGroup>,
    );
    const group = document.getElementById("repo-group-content-epel");
    expect(group).toBeTruthy();
    expect(group).toHaveAttribute("role", "rowgroup");
    expect(group).toHaveAttribute("aria-label", "epel packages");
  });

  it("focus stays on repo header after expand/collapse cycle", async () => {
    render(
      <RepoGroup repo={baseRepo} defaultExpanded={false}>
        <div data-testid="child">content</div>
      </RepoGroup>,
    );
    const header = screen.getByTestId("repo-group-epel");
    header.focus();
    expect(document.activeElement).toBe(header);
    await userEvent.keyboard("{Enter}");
    expect(document.activeElement).toBe(header);
    await userEvent.keyboard("{Enter}");
    expect(document.activeElement).toBe(header);
  });
});
