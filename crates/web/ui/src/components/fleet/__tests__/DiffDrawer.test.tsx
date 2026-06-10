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

  it("renders diff lines with correct prefixes and CSS classes", () => {
    const { container } = render(
      <DiffDrawer
        diff={sampleDiff}
        isLoading={false}
        error={null}
        onRetry={vi.fn()}
        onClose={vi.fn()}
      />,
    );

    // Equal line (space prefix, no modifier class)
    const equalLines = container.querySelectorAll(
      ".diff-drawer__line:not(.diff-drawer__line--insert):not(.diff-drawer__line--delete)",
    );
    expect(equalLines.length).toBeGreaterThan(0);
    const firstEqualLine = equalLines[0];
    expect(
      firstEqualLine.querySelector(".diff-drawer__line-prefix")?.textContent,
    ).toBe(" ");

    // Delete line (- prefix, delete class)
    const deleteLines = container.querySelectorAll(
      ".diff-drawer__line--delete",
    );
    expect(deleteLines).toHaveLength(1);
    expect(
      deleteLines[0].querySelector(".diff-drawer__line-prefix")?.textContent,
    ).toBe("-");

    // Insert lines (+ prefix, insert class)
    const insertLines = container.querySelectorAll(
      ".diff-drawer__line--insert",
    );
    expect(insertLines).toHaveLength(2);
    insertLines.forEach((line) => {
      expect(line.querySelector(".diff-drawer__line-prefix")?.textContent).toBe(
        "+",
      );
    });
  });

  it("renders hunk range header", () => {
    render(
      <DiffDrawer
        diff={sampleDiff}
        isLoading={false}
        error={null}
        onRetry={vi.fn()}
        onClose={vi.fn()}
      />,
    );

    // Range header format: @@ -start,count +start,count @@
    expect(screen.getByText("@@ -1,3 +1,4 @@")).toBeInTheDocument();
  });

  it("shows descriptive title with operand labels", () => {
    render(
      <DiffDrawer
        diff={sampleDiff}
        isLoading={false}
        error={null}
        onRetry={vi.fn()}
        onClose={vi.fn()}
        targetLabel="e5f6g7h8 (web-4, web-5)"
        baseLabel="a1b2c3d4 (web-1, web-2) [selected]"
      />,
    );

    const title = screen.getByTestId("diff-drawer-title");
    expect(title.textContent).toBe(
      "Diff: e5f6g7h8 (web-4, web-5) vs a1b2c3d4 (web-1, web-2) [selected]",
    );
  });

  it("shows generic title when no operand labels provided", () => {
    render(
      <DiffDrawer
        diff={sampleDiff}
        isLoading={false}
        error={null}
        onRetry={vi.fn()}
        onClose={vi.fn()}
      />,
    );

    const title = screen.getByTestId("diff-drawer-title");
    expect(title.textContent).toBe("Diff");
  });
});
