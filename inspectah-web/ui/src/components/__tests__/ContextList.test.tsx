import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { ContextList } from "../ContextList";
import type { ReferenceSection } from "../../api/types";

describe("ContextList", () => {
  const mockSection: ReferenceSection = {
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
    const emptySection: ReferenceSection = {
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
      const emptySection: ReferenceSection = { ...section, items: [] };
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

  it("renders subsections after main items", () => {
    const section: ReferenceSection = {
      id: "services",
      display_name: "Services",
      items: [
        {
          id: "firewalld.service",
          title: "firewalld.service",
          subtitle: "enabled (diverges from preset: disable)",
          detail: null,
          searchable_text: "firewalld",
        },
      ],
      subsections: [
        {
          id: "service_advisories",
          display_name: "Service Advisories",
          items: [
            {
              id: "custom-app.service",
              title: "custom-app.service",
              subtitle: "package excluded - may still be present as a dependency",
              detail: null,
              searchable_text: "custom-app",
            },
          ],
        },
      ],
    };

    render(<ContextList section={section} />);

    expect(screen.getByText("firewalld.service")).toBeInTheDocument();
    expect(screen.getByText("Service Advisories")).toBeInTheDocument();
    expect(screen.getByText("custom-app.service")).toBeInTheDocument();
  });

  it("does not show empty state when only subsections exist", () => {
    const section: ReferenceSection = {
      id: "services",
      display_name: "Services",
      items: [],
      subsections: [
        {
          id: "service_warnings",
          display_name: "Service Warnings",
          items: [
            {
              id: "linked.service",
              title: "linked.service",
              subtitle: "linked (warning)",
              detail: "unit linked.service has state 'linked' - linked unit requires manual handling",
              searchable_text: "linked warning",
            },
          ],
        },
      ],
    };

    render(<ContextList section={section} />);

    expect(screen.queryByText(/No Services data in this snapshot/i)).not.toBeInTheDocument();
    expect(screen.getByText("Service Warnings")).toBeInTheDocument();
    expect(screen.getByText("linked.service")).toBeInTheDocument();
  });
});
