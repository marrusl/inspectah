import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ContextItem } from "../ContextItem";
import type { ContextItem as ContextItemType } from "../../api/types";

describe("ContextItem", () => {
  const mockItem: ContextItemType = {
    id: "svc-1",
    title: "httpd.service",
    subtitle: "Apache HTTP Server",
    detail: "Loaded: loaded (/usr/lib/systemd/system/httpd.service; enabled)\nActive: active (running)",
    searchable_text: "httpd apache web server",
  };

  const mockItemNoDetail: ContextItemType = {
    id: "svc-2",
    title: "sshd.service",
    subtitle: "OpenSSH Server",
    detail: null,
    searchable_text: "sshd ssh server",
  };

  it("renders title and subtitle", () => {
    render(<ContextItem item={mockItem} />);
    expect(screen.getByText("httpd.service")).toBeInTheDocument();
    expect(screen.getByText("Apache HTTP Server")).toBeInTheDocument();
  });

  it("renders without subtitle when null", () => {
    const itemNoSubtitle: ContextItemType = { ...mockItem, subtitle: null };
    render(<ContextItem item={itemNoSubtitle} />);
    expect(screen.getByText("httpd.service")).toBeInTheDocument();
    expect(screen.queryByText("Apache HTTP Server")).not.toBeInTheDocument();
  });

  it("shows expand control when detail is present", () => {
    render(<ContextItem item={mockItem} />);
    const expandButton = screen.getByRole("button", { name: /expand detail/i });
    expect(expandButton).toBeInTheDocument();
  });

  it("does not show expand control when detail is null", () => {
    render(<ContextItem item={mockItemNoDetail} />);
    const expandButton = screen.queryByRole("button", { name: /expand detail/i });
    expect(expandButton).not.toBeInTheDocument();
  });

  it("expands to show detail when clicked", async () => {
    const user = userEvent.setup();
    render(<ContextItem item={mockItem} />);

    // Detail should not be visible initially
    expect(screen.queryByText(/Loaded: loaded/)).not.toBeInTheDocument();

    const expandButton = screen.getByRole("button", { name: /expand detail/i });
    await user.click(expandButton);

    // Detail should be visible after click
    expect(screen.getByText(/Loaded: loaded/)).toBeInTheDocument();
    expect(screen.getByText(/Active: active/)).toBeInTheDocument();
  });

  it("collapses when clicked again", async () => {
    const user = userEvent.setup();
    render(<ContextItem item={mockItem} />);

    const expandButton = screen.getByRole("button", { name: /expand detail/i });
    await user.click(expandButton);
    expect(screen.getByText(/Loaded: loaded/)).toBeInTheDocument();

    const collapseButton = screen.getByRole("button", { name: /collapse detail/i });
    await user.click(collapseButton);
    expect(screen.queryByText(/Loaded: loaded/)).not.toBeInTheDocument();
  });

  it("updates aria-expanded attribute", async () => {
    const user = userEvent.setup();
    render(<ContextItem item={mockItem} />);

    const expandButton = screen.getByRole("button");
    expect(expandButton).toHaveAttribute("aria-expanded", "false");

    await user.click(expandButton);
    expect(expandButton).toHaveAttribute("aria-expanded", "true");

    await user.click(expandButton);
    expect(expandButton).toHaveAttribute("aria-expanded", "false");
  });

  it("has data-testid and tabIndex for focus targeting", () => {
    render(<ContextItem item={mockItem} />);
    const el = screen.getByTestId("context-item-svc-1");
    expect(el).toBeInTheDocument();
    expect(el).toHaveAttribute("tabindex", "-1");
  });
});
