import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { DiffDrawer } from "../DiffDrawer";
import type { FleetDiffResponse } from "../../../api/types";

const sampleDiff: FleetDiffResponse = {
  base_hash: "aaa111",
  target_hash: "bbb222",
  base_hosts: ["host-a"],
  target_hosts: ["host-b", "host-c"],
  hunks: [
    {
      base_range: { start: 1, count: 3 },
      target_range: { start: 1, count: 4 },
      changes: [
        { kind: "equal", content: "listen_addresses = '*'" },
        { kind: "delete", content: "max_connections = 100" },
        { kind: "insert", content: "max_connections = 200" },
        { kind: "insert", content: "shared_buffers = 256MB" },
        { kind: "equal", content: "port = 5432" },
      ],
    },
  ],
  stats: {
    total_changes: 3,
    insertions: 2,
    deletions: 1,
  },
};

describe("DiffDrawer", () => {
  it("shows loading state", () => {
    render(
      <DiffDrawer
        diff={null}
        isLoading={true}
        error={null}
        onRetry={vi.fn()}
        onClose={vi.fn()}
      />,
    );

    expect(screen.getByRole("status")).toBeInTheDocument();
  });

  it("shows error with retry", async () => {
    const user = userEvent.setup();
    const onRetry = vi.fn();

    render(
      <DiffDrawer
        diff={null}
        isLoading={false}
        error="Network error"
        onRetry={onRetry}
        onClose={vi.fn()}
      />,
    );

    expect(screen.getByText(/network error/i)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /retry/i }));
    expect(onRetry).toHaveBeenCalled();
  });

  it("renders unified diff with additions and deletions", () => {
    render(
      <DiffDrawer
        diff={sampleDiff}
        isLoading={false}
        error={null}
        onRetry={vi.fn()}
        onClose={vi.fn()}
      />,
    );

    // Equal lines
    expect(screen.getByText("listen_addresses = '*'")).toBeInTheDocument();
    // Deleted line
    expect(screen.getByText("max_connections = 100")).toBeInTheDocument();
    // Inserted lines
    expect(screen.getByText("max_connections = 200")).toBeInTheDocument();
    expect(screen.getByText("shared_buffers = 256MB")).toBeInTheDocument();
  });

  it("shows diff stats", () => {
    render(
      <DiffDrawer
        diff={sampleDiff}
        isLoading={false}
        error={null}
        onRetry={vi.fn()}
        onClose={vi.fn()}
      />,
    );

    expect(screen.getByText(/\+2 insertions/)).toBeInTheDocument();
    expect(screen.getByText(/-1 deletion/)).toBeInTheDocument();
  });

  it("calls onClose when close button clicked", async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();

    render(
      <DiffDrawer
        diff={sampleDiff}
        isLoading={false}
        error={null}
        onRetry={vi.fn()}
        onClose={onClose}
      />,
    );

    await user.click(screen.getByRole("button", { name: /close/i }));
    expect(onClose).toHaveBeenCalled();
  });

  it("shows empty state when diff is null and not loading", () => {
    render(
      <DiffDrawer
        diff={null}
        isLoading={false}
        error={null}
        onRetry={vi.fn()}
        onClose={vi.fn()}
      />,
    );

    // Should still render close button but no diff content
    expect(screen.getByRole("button", { name: /close/i })).toBeInTheDocument();
    expect(screen.queryByText(/insertions/)).not.toBeInTheDocument();
  });
});
