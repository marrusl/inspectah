import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ContextItem } from "../ContextItem";
import type { ContextItem as ContextItemType } from "../../api/types";

describe("ContextItem", () => {
  const mockItem: ContextItemType = {
    id: "svc-1",
    title: "httpd.service",
    subtitle: "Apache HTTP Server",
    detail:
      "Loaded: loaded (/usr/lib/systemd/system/httpd.service; enabled)\nActive: active (running)",
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
    const expandButton = screen.queryByRole("button", {
      name: /expand detail/i,
    });
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

    const collapseButton = screen.getByRole("button", {
      name: /collapse detail/i,
    });
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
  describe("scheduled timer row", () => {
    const timerRow: ContextItemType = {
      id: "backup",
      title: "backup",
      subtitle: "daily",
      detail: "/usr/local/bin/backup.sh",
      searchable_text: "backup daily timer",
      unit: "backup.timer",
      unit_row: true,
    };

    it("links to the unit row and offers no include control", async () => {
      const user = userEvent.setup();
      const onNavigateToUnit = vi.fn();
      render(
        <ContextItem item={timerRow} onNavigateToUnit={onNavigateToUnit} />,
      );

      const link = screen.getByRole("link", { name: "backup.timer" });
      await user.click(link);
      expect(onNavigateToUnit).toHaveBeenCalledExactlyOnceWith("backup.timer");
      // The link does not also open the row's detail.
      expect(
        screen.queryByText("/usr/local/bin/backup.sh"),
      ).not.toBeInTheDocument();
      expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();
      expect(screen.queryByRole("switch")).not.toBeInTheDocument();
    });

    it("names the unit as text when the unit has no row", () => {
      const onNavigateToUnit = vi.fn();
      render(
        <ContextItem
          item={{ ...timerRow, unit: "cleanup.timer", unit_row: false }}
          onNavigateToUnit={onNavigateToUnit}
        />,
      );
      expect(screen.getByText("cleanup.timer")).toBeInTheDocument();
      expect(screen.queryByRole("link")).not.toBeInTheDocument();
    });

    it("names no unit on a row that is not a timer", () => {
      render(<ContextItem item={mockItem} onNavigateToUnit={vi.fn()} />);
      expect(screen.queryByRole("link")).not.toBeInTheDocument();
      expect(
        screen
          .getByTestId("context-item-svc-1")
          .querySelector("[data-testid^='context-item-unit-']"),
      ).toBeNull();
    });
  });
});
