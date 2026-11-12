import { describe, it, expect, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { AggregateSidebar } from "../AggregateSidebar";
import type { AggregateSection } from "../../../api/types";

const SECTIONS: AggregateSection[] = [
  { id: "packages", display_name: "Packages", is_decision_section: true, items: [] },
  { id: "system_tuning", display_name: "System Tuning", is_decision_section: true, items: [] },
  { id: "network", display_name: "Network", is_decision_section: false, items: [] },
];

describe("AggregateSidebar", () => {
  it("gives every section entry link semantics and keyboard activation", async () => {
    const onSelect = vi.fn();
    render(
      <AggregateSidebar
        sections={SECTIONS}
        activeSection="packages"
        onSelect={onSelect}
        ackState={{} as never}
      />,
    );
    const nav = screen.getByRole("navigation", { name: "Aggregate sections" });
    for (const section of SECTIONS) {
      const link = within(nav).getByRole("link", {
        name: new RegExp(`^${section.display_name} `),
      });
      link.focus();
      expect(link).toHaveFocus();
      await userEvent.keyboard("{Enter}");
      expect(onSelect).toHaveBeenLastCalledWith(section.id);
    }
    expect(onSelect).toHaveBeenCalledTimes(SECTIONS.length);
    expect(window.location.hash).toBe("");
  });
});
