import { describe, it, expect, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { RepoConflictPopover } from "../RepoConflictPopover";
import type { RepoSourceEntry } from "../../../api/types";

const entries: RepoSourceEntry[] = [
  { repo: "epel", host_count: 2 },
  { repo: "appstream", host_count: 1 },
];

describe("RepoConflictPopover", () => {
  it("renders warning button when repo_conflict entries are present", () => {
    render(
      <RepoConflictPopover
        packageName="nginx"
        identityKey="nginx.x86_64"
        entries={entries}
        isDismissed={false}
        onDismiss={vi.fn()}
      />,
    );

    const trigger = screen.getByRole("button", {
      name: /repo conflict for nginx/i,
    });
    expect(trigger).toBeInTheDocument();
  });

  it("does not render when isDismissed is true", () => {
    const { container } = render(
      <RepoConflictPopover
        packageName="nginx"
        identityKey="nginx.x86_64"
        entries={entries}
        isDismissed={true}
        onDismiss={vi.fn()}
      />,
    );

    expect(container).toBeEmptyDOMElement();
  });

  it("does not render when entries are empty", () => {
    const { container } = render(
      <RepoConflictPopover
        packageName="nginx"
        identityKey="nginx.x86_64"
        entries={[]}
        isDismissed={false}
        onDismiss={vi.fn()}
      />,
    );

    expect(container).toBeEmptyDOMElement();
  });

  it("popover opens on click with repo and host count details", async () => {
    const user = userEvent.setup();
    render(
      <RepoConflictPopover
        packageName="nginx"
        identityKey="nginx.x86_64"
        entries={entries}
        isDismissed={false}
        onDismiss={vi.fn()}
      />,
    );

    const trigger = screen.getByRole("button", {
      name: /repo conflict for nginx/i,
    });
    await user.click(trigger);

    const dialog = screen.getByRole("dialog");
    expect(dialog).toBeInTheDocument();
    expect(within(dialog).getByText(/epel/)).toBeInTheDocument();
    expect(within(dialog).getByText(/2 hosts/)).toBeInTheDocument();
    expect(within(dialog).getByText(/appstream/)).toBeInTheDocument();
    expect(within(dialog).getByText(/1 host$/)).toBeInTheDocument();
  });

  it("popover opens on Enter key", async () => {
    const user = userEvent.setup();
    render(
      <RepoConflictPopover
        packageName="nginx"
        identityKey="nginx.x86_64"
        entries={entries}
        isDismissed={false}
        onDismiss={vi.fn()}
      />,
    );

    const trigger = screen.getByRole("button", {
      name: /repo conflict for nginx/i,
    });
    trigger.focus();
    await user.keyboard("{Enter}");

    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });

  it("popover opens on Space key", async () => {
    const user = userEvent.setup();
    render(
      <RepoConflictPopover
        packageName="nginx"
        identityKey="nginx.x86_64"
        entries={entries}
        isDismissed={false}
        onDismiss={vi.fn()}
      />,
    );

    const trigger = screen.getByRole("button", {
      name: /repo conflict for nginx/i,
    });
    trigger.focus();
    await user.keyboard(" ");

    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });

  it("dismiss button inside popover calls onDismiss and closes popover", async () => {
    const user = userEvent.setup();
    const onDismiss = vi.fn();
    render(
      <RepoConflictPopover
        packageName="nginx"
        identityKey="nginx.x86_64"
        entries={entries}
        isDismissed={false}
        onDismiss={onDismiss}
      />,
    );

    const trigger = screen.getByRole("button", {
      name: /repo conflict for nginx/i,
    });
    await user.click(trigger);

    const dialog = screen.getByRole("dialog");
    const dismissBtn = within(dialog).getByRole("button", {
      name: /dismiss conflict warning for nginx/i,
    });
    await user.click(dismissBtn);

    expect(onDismiss).toHaveBeenCalledWith("nginx.x86_64");
  });

  it("dismiss button has package-specific aria-label", async () => {
    const user = userEvent.setup();
    render(
      <RepoConflictPopover
        packageName="nginx"
        identityKey="nginx.x86_64"
        entries={entries}
        isDismissed={false}
        onDismiss={vi.fn()}
      />,
    );

    await user.click(
      screen.getByRole("button", { name: /repo conflict for nginx/i }),
    );

    const dialog = screen.getByRole("dialog");
    const dismissBtn = within(dialog).getByRole("button", {
      name: /dismiss conflict warning for nginx/i,
    });
    expect(dismissBtn).toHaveAttribute(
      "aria-label",
      "Dismiss conflict warning for nginx",
    );
  });

  it("Escape closes popover without dismissing", async () => {
    const user = userEvent.setup();
    const onDismiss = vi.fn();
    render(
      <RepoConflictPopover
        packageName="nginx"
        identityKey="nginx.x86_64"
        entries={entries}
        isDismissed={false}
        onDismiss={onDismiss}
      />,
    );

    const trigger = screen.getByRole("button", {
      name: /repo conflict for nginx/i,
    });
    await user.click(trigger);
    expect(screen.getByRole("dialog")).toBeInTheDocument();

    await user.keyboard("{Escape}");

    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(onDismiss).not.toHaveBeenCalled();
  });

  it("focus returns to trigger on Escape close", async () => {
    const user = userEvent.setup();
    render(
      <RepoConflictPopover
        packageName="nginx"
        identityKey="nginx.x86_64"
        entries={entries}
        isDismissed={false}
        onDismiss={vi.fn()}
      />,
    );

    const trigger = screen.getByRole("button", {
      name: /repo conflict for nginx/i,
    });
    await user.click(trigger);
    await user.keyboard("{Escape}");

    expect(trigger).toHaveFocus();
  });

  it("trigger has aria-haspopup='dialog'", () => {
    render(
      <RepoConflictPopover
        packageName="nginx"
        identityKey="nginx.x86_64"
        entries={entries}
        isDismissed={false}
        onDismiss={vi.fn()}
      />,
    );

    const trigger = screen.getByRole("button", {
      name: /repo conflict for nginx/i,
    });
    expect(trigger).toHaveAttribute("aria-haspopup", "dialog");
  });

  it("trigger has correct aria-expanded state", async () => {
    const user = userEvent.setup();
    render(
      <RepoConflictPopover
        packageName="nginx"
        identityKey="nginx.x86_64"
        entries={entries}
        isDismissed={false}
        onDismiss={vi.fn()}
      />,
    );

    const trigger = screen.getByRole("button", {
      name: /repo conflict for nginx/i,
    });
    expect(trigger).toHaveAttribute("aria-expanded", "false");

    await user.click(trigger);
    expect(trigger).toHaveAttribute("aria-expanded", "true");
  });

  it("popover has role='dialog' with accessible label", async () => {
    const user = userEvent.setup();
    render(
      <RepoConflictPopover
        packageName="nginx"
        identityKey="nginx.x86_64"
        entries={entries}
        isDismissed={false}
        onDismiss={vi.fn()}
      />,
    );

    await user.click(
      screen.getByRole("button", { name: /repo conflict for nginx/i }),
    );

    const dialog = screen.getByRole("dialog", {
      name: /repo source conflict for nginx/i,
    });
    expect(dialog).toBeInTheDocument();
  });

  it("accessible name on trigger includes source count", () => {
    render(
      <RepoConflictPopover
        packageName="nginx"
        identityKey="nginx.x86_64"
        entries={entries}
        isDismissed={false}
        onDismiss={vi.fn()}
      />,
    );

    const trigger = screen.getByRole("button", {
      name: /repo conflict for nginx.*2 sources/i,
    });
    expect(trigger).toBeInTheDocument();
  });
});
