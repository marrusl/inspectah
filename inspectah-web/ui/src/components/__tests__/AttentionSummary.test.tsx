import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { AttentionSummary } from "../AttentionSummary";

describe("AttentionSummary", () => {
  it("shows review count and repo count when needs_review > 0", () => {
    render(
      <AttentionSummary needsReviewCount={3} needsReviewRepoCount={2} infoCount={5} infoRepoCount={3} />,
    );
    expect(screen.getByText("3 packages need review across 2 repos")).toBeInTheDocument();
  });

  it("shows singular 'package' and 'repo' when counts are 1", () => {
    render(
      <AttentionSummary needsReviewCount={1} needsReviewRepoCount={1} infoCount={0} infoRepoCount={0} />,
    );
    expect(screen.getByText("1 package needs review across 1 repo")).toBeInTheDocument();
  });

  it("shows informational fallback when needs_review is 0 but informational exists", () => {
    render(
      <AttentionSummary needsReviewCount={0} needsReviewRepoCount={0} infoCount={12} infoRepoCount={3} />,
    );
    const el = screen.getByTestId("attention-summary");
    expect(el.textContent).toContain("No packages flagged for review");
    expect(el.textContent).toContain("12 informational across 3 repos");
  });

  it("shows all-clear when both are 0", () => {
    render(
      <AttentionSummary needsReviewCount={0} needsReviewRepoCount={0} infoCount={0} infoRepoCount={0} />,
    );
    expect(screen.getByText("All actionable items reviewed")).toBeInTheDocument();
  });

  it("has correct data-testid", () => {
    render(
      <AttentionSummary needsReviewCount={0} needsReviewRepoCount={0} infoCount={0} infoRepoCount={0} />,
    );
    expect(screen.getByTestId("attention-summary")).toBeInTheDocument();
  });
});
