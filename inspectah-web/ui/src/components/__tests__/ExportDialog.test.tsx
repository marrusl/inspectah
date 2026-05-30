import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ExportDialog } from "../ExportDialog";
import type { ViewResponse } from "../../api/types";
import { mockStats } from "../../test-utils/mockStats";

// --- Mock fetch globally ---
const mockFetch = vi.fn();
beforeEach(() => {
  mockFetch.mockReset();
  vi.stubGlobal("fetch", mockFetch);
});
afterEach(() => {
  vi.restoreAllMocks();
});

const MOCK_STATS = mockStats({
  sections: [
    { kind: "package", total: 10, included: 8, excluded: 2 },
    { kind: "config", total: 5, included: 3, excluded: 2 },
  ],
  needs_review_count: 3,
  ops_applied: 1,
  can_undo: true,
  can_redo: false,
  baseline_available: true,
});

const MOCK_VIEW: ViewResponse = {
  packages: [],
  config_files: [],
  containerfile_preview: "",
  stats: MOCK_STATS,
  generation: 7,
  repo_groups: [],
  version_changes: [],
  service_states: [],
  service_dropins: [],
  quadlets: [],
  flatpaks: [],
  sysctls: [],
  tuned: [],
  users_groups_decisions: [],
  session_is_sensitive: false,
};

describe("ExportDialog", () => {
  it("does not render content when closed", () => {
    render(
      <ExportDialog
        isOpen={false}
        onClose={vi.fn()}
        stats={MOCK_STATS}
        generation={7}
        sessionIsSensitive={false}
        onViewUpdate={vi.fn()}
      />,
    );

    expect(screen.queryByText("Export Tarball")).not.toBeInTheDocument();
  });

  it("renders modal with exclusion summary when open", () => {
    render(
      <ExportDialog
        isOpen={true}
        onClose={vi.fn()}
        stats={MOCK_STATS}
        generation={7}
        sessionIsSensitive={false}
        onViewUpdate={vi.fn()}
      />,
    );

    expect(screen.getByText("Export Tarball")).toBeInTheDocument();
    expect(
      screen.getByText(/2 packages excluded, 2 configs excluded/),
    ).toBeInTheDocument();
    expect(screen.getByText(/7/)).toBeInTheDocument();
  });

  it("shows context-sections info warning when open", () => {
    render(
      <ExportDialog
        isOpen={true}
        onClose={vi.fn()}
        stats={MOCK_STATS}
        generation={7}
        sessionIsSensitive={false}
        onViewUpdate={vi.fn()}
      />,
    );

    expect(screen.getByText("Context sections")).toBeInTheDocument();
    expect(screen.getByText(/cannot be toggled/)).toBeInTheDocument();
  });

  it("renders Export and Cancel buttons", () => {
    render(
      <ExportDialog
        isOpen={true}
        onClose={vi.fn()}
        stats={MOCK_STATS}
        generation={7}
        sessionIsSensitive={false}
        onViewUpdate={vi.fn()}
      />,
    );

    expect(screen.getByRole("button", { name: "Export" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Cancel" })).toBeInTheDocument();
  });

  it("calls onClose when Cancel is clicked", async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();

    render(
      <ExportDialog
        isOpen={true}
        onClose={onClose}
        stats={MOCK_STATS}
        generation={7}
        sessionIsSensitive={false}
        onViewUpdate={vi.fn()}
      />,
    );

    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("triggers POST /api/tarball with correct generation on Export click", async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();

    // Mock successful tarball response
    const mockBlob = new Blob(["fake tarball"], {
      type: "application/gzip",
    });
    mockFetch.mockResolvedValueOnce({
      ok: true,
      blob: () => Promise.resolve(mockBlob),
    });

    // Mock URL.createObjectURL/revokeObjectURL for jsdom
    const mockUrl = "blob:http://localhost/fake-url";
    const origCreateObjectURL = URL.createObjectURL;
    const origRevokeObjectURL = URL.revokeObjectURL;
    URL.createObjectURL = vi.fn(() => mockUrl);
    URL.revokeObjectURL = vi.fn();

    render(
      <ExportDialog
        isOpen={true}
        onClose={onClose}
        stats={MOCK_STATS}
        generation={7}
        sessionIsSensitive={false}
        onViewUpdate={vi.fn()}
      />,
    );

    await user.click(screen.getByRole("button", { name: "Export" }));

    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalledWith("/api/tarball", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ generation: 7 }),
      });
    });

    // Should close dialog on success
    await waitFor(() => {
      expect(onClose).toHaveBeenCalled();
    });

    // Restore
    URL.createObjectURL = origCreateObjectURL;
    URL.revokeObjectURL = origRevokeObjectURL;
  });

  it("triggers browser download on successful export", async () => {
    const user = userEvent.setup();
    const mockBlob = new Blob(["fake tarball"], {
      type: "application/gzip",
    });
    mockFetch.mockResolvedValueOnce({
      ok: true,
      blob: () => Promise.resolve(mockBlob),
    });

    // Spy on URL.createObjectURL and revokeObjectURL
    const mockUrl = "blob:http://localhost/fake-url";
    const createObjectURL = vi.fn(() => mockUrl);
    const revokeObjectURL = vi.fn();
    vi.stubGlobal("URL", {
      ...URL,
      createObjectURL,
      revokeObjectURL,
    });

    // Spy on createElement to capture the anchor click
    const clickSpy = vi.fn();
    const origCreateElement = document.createElement.bind(document);
    vi.spyOn(document, "createElement").mockImplementation((tag: string) => {
      const el = origCreateElement(tag);
      if (tag === "a") {
        el.click = clickSpy;
      }
      return el;
    });

    render(
      <ExportDialog
        isOpen={true}
        onClose={vi.fn()}
        stats={MOCK_STATS}
        generation={7}
        sessionIsSensitive={false}
        onViewUpdate={vi.fn()}
      />,
    );

    await user.click(screen.getByRole("button", { name: "Export" }));

    await waitFor(() => {
      expect(createObjectURL).toHaveBeenCalledWith(mockBlob);
      expect(clickSpy).toHaveBeenCalled();
      expect(revokeObjectURL).toHaveBeenCalledWith(mockUrl);
    });
  });

  it("handles 409 stale generation: re-fetches view and closes", async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();
    const onViewUpdate = vi.fn();

    // First call: POST /api/tarball returns 409
    // Second call: GET /api/view returns fresh view
    mockFetch
      .mockResolvedValueOnce({
        ok: false,
        status: 409,
        json: () => Promise.resolve({ error: "generation mismatch" }),
      })
      .mockResolvedValueOnce({
        ok: true,
        json: () => Promise.resolve({ ...MOCK_VIEW, generation: 42 }),
      });

    render(
      <ExportDialog
        isOpen={true}
        onClose={onClose}
        stats={MOCK_STATS}
        generation={7}
        sessionIsSensitive={false}
        onViewUpdate={onViewUpdate}
      />,
    );

    await user.click(screen.getByRole("button", { name: "Export" }));

    await waitFor(() => {
      // Should have called fetchView to re-fetch
      expect(mockFetch).toHaveBeenCalledTimes(2);
      expect(onViewUpdate).toHaveBeenCalled();
      expect(onClose).toHaveBeenCalled();
    });
  });

  it("shows error alert on non-409 failure", async () => {
    const user = userEvent.setup();

    mockFetch.mockResolvedValueOnce({
      ok: false,
      status: 500,
      json: () => Promise.resolve({ error: "internal server error" }),
    });

    render(
      <ExportDialog
        isOpen={true}
        onClose={vi.fn()}
        stats={MOCK_STATS}
        generation={7}
        sessionIsSensitive={false}
        onViewUpdate={vi.fn()}
      />,
    );

    await user.click(screen.getByRole("button", { name: "Export" }));

    await waitFor(() => {
      expect(screen.getByText("Export failed")).toBeInTheDocument();
      expect(screen.getByText("internal server error")).toBeInTheDocument();
    });
  });

  it("shows zero exclusions when stats have no exclusions", () => {
    const zeroStats = mockStats({
      sections: [
        { kind: "package", total: 10, included: 10, excluded: 0 },
        { kind: "config", total: 5, included: 5, excluded: 0 },
      ],
      needs_review_count: 3,
      ops_applied: 1,
      can_undo: true,
      can_redo: false,
      baseline_available: true,
    });

    render(
      <ExportDialog
        isOpen={true}
        onClose={vi.fn()}
        stats={zeroStats}
        generation={1}
        sessionIsSensitive={false}
        onViewUpdate={vi.fn()}
      />,
    );

    expect(
      screen.getByText(/0 packages excluded, 0 configs excluded/),
    ).toBeInTheDocument();
  });
});
