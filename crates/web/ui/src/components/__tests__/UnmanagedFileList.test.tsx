import { describe, it, expect, vi } from "vitest";
import { render, screen, within, fireEvent, act } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { UnmanagedFileList } from "../UnmanagedFileList";
import type {
  UnmanagedFileGroup,
  UnmanagedFileItem,
  ProvenanceSignals,
} from "../../api/types";

// --- Test data factories ---

const DEFAULT_PROVENANCE: ProvenanceSignals = {
  file_type: "elf_binary",
  last_modified: 1700000000,
  uid: 0,
  gid: 0,
  permissions: "0755",
  mutability: false,
  writable_mount: false,
  service_working_dir: false,
};

function makeFile(
  path: string,
  overrides?: Partial<UnmanagedFileItem>,
): UnmanagedFileItem {
  return {
    path,
    size: 1024 * 100,
    is_var_path: path.startsWith("/var/"),
    include: true,
    provenance: { ...DEFAULT_PROVENANCE },
    ...overrides,
  };
}

const groups: UnmanagedFileGroup[] = [
  {
    directory: "/opt/splunk",
    items: [
      makeFile("/opt/splunk/bin/splunkd", {
        size: 50 * 1024 * 1024,
        provenance: { ...DEFAULT_PROVENANCE, mutability: true },
      }),
      makeFile("/opt/splunk/etc/system.conf", {
        size: 2048,
        provenance: { ...DEFAULT_PROVENANCE, file_type: "config" },
      }),
      makeFile("/opt/splunk/lib/libcrypto.so", { size: 5 * 1024 * 1024 }),
    ],
  },
  {
    directory: "/srv/myapp",
    items: [
      makeFile("/srv/myapp/app.jar", {
        size: 120 * 1024 * 1024,
        provenance: {
          ...DEFAULT_PROVENANCE,
          file_type: "jar",
          service_working_dir: true,
        },
      }),
      makeFile("/srv/myapp/start.sh", {
        size: 512,
        provenance: { ...DEFAULT_PROVENANCE, file_type: "script" },
      }),
    ],
  },
  {
    directory: "/var/lib/custom",
    items: [
      makeFile("/var/lib/custom/data.db", {
        size: 200 * 1024 * 1024,
        provenance: {
          ...DEFAULT_PROVENANCE,
          file_type: "data_file",
          writable_mount: true,
          mutability: true,
        },
      }),
    ],
  },
];

/** Two-group fixture for keyboard navigation tests. */
const twoGroupFixture: UnmanagedFileGroup[] = [
  {
    directory: "/opt/splunk",
    items: [
      makeFile("/opt/splunk/bin/splunkd", { size: 50 * 1024 * 1024 }),
      makeFile("/opt/splunk/etc/config.yml", { size: 2048 }),
    ],
  },
  {
    directory: "/opt/datadog",
    items: [makeFile("/opt/datadog/agent", { size: 10 * 1024 * 1024 })],
  },
];

describe("UnmanagedFileList", () => {
  it("renders directory group headers", () => {
    render(
      <UnmanagedFileList
        groups={groups}
        onToggleItem={vi.fn()}
        onToggleGroup={vi.fn()}
        isPending={false}
      />,
    );
    expect(screen.getByText("/opt/splunk")).toBeInTheDocument();
    expect(screen.getByText("/srv/myapp")).toBeInTheDocument();
    expect(screen.getByText("/var/lib/custom")).toBeInTheDocument();
  });

  it("renders item count per group", () => {
    render(
      <UnmanagedFileList
        groups={groups}
        onToggleItem={vi.fn()}
        onToggleGroup={vi.fn()}
        isPending={false}
      />,
    );
    expect(screen.getByText("3 items")).toBeInTheDocument();
    expect(screen.getByText("2 items")).toBeInTheDocument();
    expect(screen.getByText("1 item")).toBeInTheDocument();
  });

  it("shows /var warning for items under /var", () => {
    render(
      <UnmanagedFileList
        groups={groups}
        onToggleItem={vi.fn()}
        onToggleGroup={vi.fn()}
        isPending={false}
      />,
    );
    expect(screen.getByText(/persistent, mutable/)).toBeInTheDocument();
  });

  it("renders provenance signals on file rows", () => {
    render(
      <UnmanagedFileList
        groups={groups}
        onToggleItem={vi.fn()}
        onToggleGroup={vi.fn()}
        isPending={false}
      />,
    );
    // /srv/myapp/app.jar has service_working_dir: true
    expect(screen.getByText(/service workdir/i)).toBeInTheDocument();
    // /var/lib/custom/data.db has writable_mount: true
    expect(screen.getByText(/writable mount/i)).toBeInTheDocument();
    // /opt/splunk/bin/splunkd and /var/lib/custom/data.db both have mutability: true
    expect(
      screen.getAllByText(/modified since install/i).length,
    ).toBeGreaterThanOrEqual(1);
  });

  it("calls onToggleGroup when group checkbox is clicked", async () => {
    const onToggleGroup = vi.fn();
    render(
      <UnmanagedFileList
        groups={groups}
        onToggleItem={vi.fn()}
        onToggleGroup={onToggleGroup}
        isPending={false}
      />,
    );
    const user = userEvent.setup();
    const groupCb = screen.getByLabelText("Toggle all files in /opt/splunk");
    await user.click(groupCb);
    expect(onToggleGroup).toHaveBeenCalledWith(
      "/opt/splunk",
      expect.any(Boolean),
    );
  });

  it("shows running size rollup in header", () => {
    render(
      <UnmanagedFileList
        groups={groups}
        onToggleItem={vi.fn()}
        onToggleGroup={vi.fn()}
        isPending={false}
      />,
    );
    const rollup = screen.getByTestId("unmanaged-rollup");
    expect(rollup.textContent).toMatch(/6 of 6 items included/);
  });

  it("calls onToggleItem with file path when individual file checkbox is clicked", async () => {
    const onToggleItem = vi.fn();
    render(
      <UnmanagedFileList
        groups={groups}
        onToggleItem={onToggleItem}
        onToggleGroup={vi.fn()}
        isPending={false}
      />,
    );
    const user = userEvent.setup();
    const fileCb = screen.getByLabelText("Toggle /opt/splunk/bin/splunkd");
    await user.click(fileCb);
    expect(onToggleItem).toHaveBeenCalledWith("/opt/splunk/bin/splunkd");
  });

  it("Include None button calls onIncludeNone", async () => {
    const onIncludeNone = vi.fn();
    render(
      <UnmanagedFileList
        groups={groups}
        onToggleItem={vi.fn()}
        onToggleGroup={vi.fn()}
        isPending={false}
        onIncludeNone={onIncludeNone}
        onResetAll={vi.fn()}
      />,
    );
    const user = userEvent.setup();
    await user.click(screen.getByText("Include None"));
    expect(onIncludeNone).toHaveBeenCalled();
  });

  // --- Grouped accessibility ---

  it("group header has role='button' and aria-expanded", () => {
    render(
      <UnmanagedFileList
        groups={groups}
        onToggleItem={vi.fn()}
        onToggleGroup={vi.fn()}
        isPending={false}
      />,
    );
    const groupHeader = screen
      .getByLabelText("/opt/splunk file group")
      .querySelector("[role='button']")!;
    expect(groupHeader).toHaveAttribute("aria-expanded", "true");
  });

  it("group rollup has aria-live='polite' for debounced size announcements", () => {
    render(
      <UnmanagedFileList
        groups={groups}
        onToggleItem={vi.fn()}
        onToggleGroup={vi.fn()}
        isPending={false}
      />,
    );
    const rollup = screen.getByTestId("unmanaged-rollup");
    expect(rollup).toHaveAttribute("aria-live", "polite");
  });

  it("ArrowRight on collapsed group expands it", async () => {
    render(
      <UnmanagedFileList
        groups={groups}
        onToggleItem={vi.fn()}
        onToggleGroup={vi.fn()}
        isPending={false}
      />,
    );
    const user = userEvent.setup();
    // First collapse a group by clicking the header
    const groupHeader = screen
      .getByLabelText("/opt/splunk file group")
      .querySelector("[role='button']")! as HTMLElement;
    await user.click(groupHeader);
    expect(groupHeader).toHaveAttribute("aria-expanded", "false");
    // ArrowRight should expand it
    groupHeader.focus();
    await user.keyboard("{ArrowRight}");
    expect(groupHeader).toHaveAttribute("aria-expanded", "true");
  });

  it("ArrowLeft on expanded group collapses it", async () => {
    render(
      <UnmanagedFileList
        groups={groups}
        onToggleItem={vi.fn()}
        onToggleGroup={vi.fn()}
        isPending={false}
      />,
    );
    const user = userEvent.setup();
    const groupHeader = screen
      .getByLabelText("/opt/splunk file group")
      .querySelector("[role='button']")! as HTMLElement;
    expect(groupHeader).toHaveAttribute("aria-expanded", "true");
    groupHeader.focus();
    await user.keyboard("{ArrowLeft}");
    expect(groupHeader).toHaveAttribute("aria-expanded", "false");
  });

  // --- Arrow-key navigation between groups and items ---

  it("ArrowDown from group header moves focus to first item in group", async () => {
    render(
      <UnmanagedFileList
        groups={groups}
        onToggleItem={vi.fn()}
        onToggleGroup={vi.fn()}
        isPending={false}
      />,
    );
    const user = userEvent.setup();
    const groupHeader = screen
      .getByLabelText("/opt/splunk file group")
      .querySelector("[role='button']")! as HTMLElement;
    groupHeader.focus();
    await user.keyboard("{ArrowDown}");
    expect(document.activeElement).toBe(
      screen.getByTestId("unmanaged-item-/opt/splunk/bin/splunkd"),
    );
  });

  it("ArrowDown from last item in group moves focus to next group header", async () => {
    render(
      <UnmanagedFileList
        groups={twoGroupFixture}
        onToggleItem={vi.fn()}
        onToggleGroup={vi.fn()}
        isPending={false}
      />,
    );
    const user = userEvent.setup();
    const lastItem = screen.getByTestId(
      "unmanaged-item-/opt/splunk/etc/config.yml",
    );
    lastItem.focus();
    await user.keyboard("{ArrowDown}");
    const nextGroupHeader = screen
      .getByLabelText("/opt/datadog file group")
      .querySelector("[role='button']")! as HTMLElement;
    expect(document.activeElement).toBe(nextGroupHeader);
  });

  it("ArrowUp from first item in group moves focus back to group header", async () => {
    render(
      <UnmanagedFileList
        groups={groups}
        onToggleItem={vi.fn()}
        onToggleGroup={vi.fn()}
        isPending={false}
      />,
    );
    const user = userEvent.setup();
    const firstItem = screen.getByTestId(
      "unmanaged-item-/opt/splunk/bin/splunkd",
    );
    firstItem.focus();
    await user.keyboard("{ArrowUp}");
    const groupHeader = screen
      .getByLabelText("/opt/splunk file group")
      .querySelector("[role='button']")! as HTMLElement;
    expect(document.activeElement).toBe(groupHeader);
  });

  // --- Polite announcements for group and item toggles ---

  it("announces group toggle via aria-live", async () => {
    const onToggleGroup = vi.fn();
    render(
      <UnmanagedFileList
        groups={groups}
        onToggleItem={vi.fn()}
        onToggleGroup={onToggleGroup}
        isPending={false}
      />,
    );
    const user = userEvent.setup();
    const groupCb = screen.getByLabelText("Toggle all files in /opt/splunk");
    await user.click(groupCb);
    const liveRegion = screen.getByTestId(
      "unmanaged-group-announce-/opt/splunk",
    );
    expect(liveRegion).toHaveAttribute("aria-live", "polite");
    expect(liveRegion.textContent).toMatch(
      /Excluded \d+ files in \/opt\/splunk/,
    );
  });

  it("announces item toggle via aria-live", async () => {
    const onToggleItem = vi.fn();
    render(
      <UnmanagedFileList
        groups={groups}
        onToggleItem={onToggleItem}
        onToggleGroup={vi.fn()}
        isPending={false}
      />,
    );
    const user = userEvent.setup();
    const fileCb = screen.getByLabelText("Toggle /opt/splunk/bin/splunkd");
    await user.click(fileCb);
    const liveRegion = screen.getByTestId(
      "unmanaged-item-announce-/opt/splunk/bin/splunkd",
    );
    expect(liveRegion).toHaveAttribute("aria-live", "polite");
    expect(liveRegion.textContent).toMatch(
      /Excluded \/opt\/splunk\/bin\/splunkd/,
    );
  });

  // --- Debounced size-rollup announcement ---

  it("debounces size-rollup announcement after rapid toggles", () => {
    vi.useFakeTimers();
    render(
      <UnmanagedFileList
        groups={groups}
        onToggleItem={vi.fn()}
        onToggleGroup={vi.fn()}
        isPending={false}
      />,
    );
    const rollupAnnounce = screen.getByTestId("unmanaged-rollup-announce");

    // Toggle two items rapidly via fireEvent (avoids userEvent timer conflicts)
    fireEvent.click(screen.getByLabelText("Toggle /opt/splunk/bin/splunkd"));
    fireEvent.click(
      screen.getByLabelText("Toggle /opt/splunk/etc/system.conf"),
    );

    // Before debounce fires, announcement should not have updated
    expect(rollupAnnounce.textContent).toBe("");

    // After 500ms debounce, announcement should fire once
    act(() => {
      vi.advanceTimersByTime(500);
    });
    expect(rollupAnnounce.textContent).toMatch(
      /\d+ of \d+ items included, ~[\d.]+ [KMGT]?B/,
    );

    vi.useRealTimers();
  });
});
