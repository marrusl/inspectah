import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { ContextList } from "../ContextList";
import type { ContextSection } from "../../api/types";

describe("ContextList", () => {
  const mockSection: ContextSection = {
    id: "services",
    display_name: "Services",
    items: [
      {
        id: "svc-1",
        title: "httpd.service",
        subtitle: "Apache HTTP Server",
        detail: "Loaded: loaded\nActive: active (running)",
        searchable_text: "httpd apache",
      },
      {
        id: "svc-2",
        title: "sshd.service",
        subtitle: "OpenSSH Server",
        detail: null,
        searchable_text: "sshd ssh",
      },
    ],
  };

  it("renders section items as DataList", () => {
    render(<ContextList section={mockSection} />);
    expect(screen.getByText("httpd.service")).toBeInTheDocument();
    expect(screen.getByText("sshd.service")).toBeInTheDocument();
  });

  it("renders DataList without selection capability", () => {
    const { container } = render(<ContextList section={mockSection} />);
    // DataList should not have selectable items
    const checkboxes = container.querySelectorAll('input[type="checkbox"]');
    expect(checkboxes).toHaveLength(0);
  });

  it("applies muted left border styling", () => {
    const { container } = render(<ContextList section={mockSection} />);
    const dataList = container.querySelector('[role="list"]');
    expect(dataList).toHaveStyle({
      borderLeft: expect.stringContaining("var(--pf-t--global--border--color--default)"),
    });
  });

  it("renders EmptyState when section has no items", () => {
    const emptySection: ContextSection = {
      id: "services",
      display_name: "Services",
      items: [],
    };
    render(<ContextList section={emptySection} />);
    expect(screen.getByText(/No Services data in this snapshot/i)).toBeInTheDocument();
  });

  it("shows correct empty state message for each section", () => {
    const sections = [
      { id: "containers", display_name: "Containers" },
      { id: "users_groups", display_name: "Users & Groups" },
      { id: "network", display_name: "Network" },
    ];

    sections.forEach((section) => {
      const emptySection: ContextSection = { ...section, items: [] };
      const { unmount } = render(<ContextList section={emptySection} />);
      expect(
        screen.getByText(new RegExp(`No ${section.display_name} data`, "i"))
      ).toBeInTheDocument();
      unmount();
    });
  });

  it("renders all items in order", () => {
    render(<ContextList section={mockSection} />);
    const items = screen.getAllByRole("listitem");
    expect(items).toHaveLength(2);
  });
});
