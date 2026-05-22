import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ZoneGroup } from "../ZoneGroup";

describe("ZoneGroup", () => {
  it("renders zone header with label and count", () => {
    render(
      <ZoneGroup zone="consensus" count={5} defaultExpanded={true}>
        <div>child content</div>
      </ZoneGroup>,
    );
    expect(screen.getByText("Consensus")).toBeInTheDocument();
    expect(screen.getByText("5")).toBeInTheDocument();
  });

  it("renders expanded when defaultExpanded is true", () => {
    render(
      <ZoneGroup zone="near_consensus" count={3} defaultExpanded={true}>
        <div>visible content</div>
      </ZoneGroup>,
    );
    expect(screen.getByText("visible content")).toBeVisible();
  });

  it("renders collapsed when defaultExpanded is false", () => {
    render(
      <ZoneGroup zone="consensus" count={7} defaultExpanded={false}>
        <div>hidden content</div>
      </ZoneGroup>,
    );
    // PF6 ExpandableSection hides content via hidden attribute when collapsed
    expect(screen.getByText("hidden content")).not.toBeVisible();
  });

  it("toggles collapsed state on header click", async () => {
    const user = userEvent.setup();
    render(
      <ZoneGroup zone="divergent" count={2} defaultExpanded={true}>
        <div>toggle me</div>
      </ZoneGroup>,
    );
    expect(screen.getByText("toggle me")).toBeVisible();

    // PF6 ExpandableSection toggle button has accessible name from toggleContent
    const toggle = screen.getByRole("button", { name: /divergent/i });
    await user.click(toggle);
    expect(screen.getByText("toggle me")).not.toBeVisible();
  });

  it("shows children when expanded", () => {
    render(
      <ZoneGroup zone="near_consensus" count={4} defaultExpanded={true}>
        <div>child 1</div>
        <div>child 2</div>
      </ZoneGroup>,
    );
    expect(screen.getByText("child 1")).toBeVisible();
    expect(screen.getByText("child 2")).toBeVisible();
  });

  it("hides children when collapsed", () => {
    render(
      <ZoneGroup zone="consensus" count={10} defaultExpanded={false}>
        <div>child 1</div>
        <div>child 2</div>
      </ZoneGroup>,
    );
    expect(screen.getByText("child 1")).not.toBeVisible();
    expect(screen.getByText("child 2")).not.toBeVisible();
  });

  it("displays correct human-readable zone labels", () => {
    const { rerender } = render(
      <ZoneGroup zone="consensus" count={1} defaultExpanded={true}>
        <div />
      </ZoneGroup>,
    );
    expect(screen.getByText("Consensus")).toBeInTheDocument();

    rerender(
      <ZoneGroup zone="near_consensus" count={1} defaultExpanded={true}>
        <div />
      </ZoneGroup>,
    );
    expect(screen.getByText("Near Consensus")).toBeInTheDocument();

    rerender(
      <ZoneGroup zone="divergent" count={1} defaultExpanded={true}>
        <div />
      </ZoneGroup>,
    );
    expect(screen.getByText("Divergent")).toBeInTheDocument();
  });
});
