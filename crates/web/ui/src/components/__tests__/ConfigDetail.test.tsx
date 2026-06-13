import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { ConfigDetail } from "../ConfigDetail";
import type { RefinedConfig } from "../../api/types";

describe("ConfigDetail", () => {
  const createMockConfig = (content: string): RefinedConfig => {
    return {
      entry: {
        path: "/etc/test.conf",
        kind: "rpm_owned_modified",
        category: "other",
        content,
        rpm_va_flags: null,
        package: "test-package",
        diff_against_rpm: null,
        include: true,
        tie: false,
        tie_winner: false,
        fleet: null,
      },
      triage: {
        triage: { mode: "single_host", baseline: null },
        primary_reason: "config_modified",
        annotations: [],
      },
    };
  };

  it("renders full content for long files (>500 chars)", () => {
    const longContent = "a".repeat(600);
    const config = createMockConfig(longContent);

    const { container } = render(<ConfigDetail config={config} />);

    const preElement = container.querySelector("pre");
    expect(preElement).toBeInTheDocument();
    expect(preElement?.textContent).toBe(longContent);
    expect(preElement?.textContent).not.toContain("...");
  });

  it("renders full content for short files (<500 chars)", () => {
    const shortContent = "b".repeat(200);
    const config = createMockConfig(shortContent);

    const { container } = render(<ConfigDetail config={config} />);

    const preElement = container.querySelector("pre");
    expect(preElement).toBeInTheDocument();
    expect(preElement?.textContent).toBe(shortContent);
  });

  it("renders config without content field", () => {
    const config: RefinedConfig = {
      entry: {
        path: "/etc/empty.conf",
        kind: "rpm_owned_modified",
        category: "other",
        content: "",
        rpm_va_flags: null,
        package: "test-package",
        diff_against_rpm: null,
        include: true,
        tie: false,
        tie_winner: false,
        fleet: null,
      },
      triage: {
        triage: { mode: "single_host", baseline: null },
        primary_reason: "config_modified",
        annotations: [],
      },
    };

    render(<ConfigDetail config={config} />);

    // Should show path but no content section for empty content
    expect(screen.getByText("/etc/empty.conf")).toBeInTheDocument();
  });
});
