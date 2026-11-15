import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ExportDialog } from "../ExportDialog";
import type { NotCarriedRow, ViewResponse } from "../../api/types";
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
  unit_states: [],
  unit_dropins: [],
  mount_findings: [],
  quadlets: [],
  flatpaks: [],
  declined_generated: [],
  generated_timers: [],
  sysctl: { state: "unavailable", reason: "not collected" },
  tuned: [],
  users_groups_decisions: [],
  package_groups: [],
  session_is_sensitive: false,
  not_carried_packages: [],
  not_carried_notice: null,
};

function notCarried(name: string, units: string[]): NotCarriedRow {
  return {
    item_id: { kind: "Package", key: { name, arch: "x86_64" } },
    display_name: name,
    source_repo: "baseos",
    enabled_units: units,
    owns_enabled_unit: units.length > 0,
  };
}

const NOT_CARRIED_ROWS: NotCarriedRow[] = [
  notCarried("firewalld", ["firewalld.service"]),
  notCarried("rhc", ["rhcd.service"]),
  notCarried("python3-firewall", []),
];

/** What the server sends as `not_carried_notice` for NOT_CARRIED_ROWS. */
const VIEW_NOTICE = {
  kind: "notice" as const,
  text: "2 packages that own a unit enabled on the source host are not carried: firewalld, rhc. See the Not carried table in audit-report.md and the # Not carried block in the Containerfile.",
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

    expect(screen.getByText("Reference sections")).toBeInTheDocument();
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
      headers: new Headers({ "x-export-warnings": "[]" }),
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
      headers: new Headers({ "x-export-warnings": "[]" }),
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

  describe("after download", () => {
    let createObjectURL: ReturnType<typeof vi.fn>;
    let revokeObjectURL: ReturnType<typeof vi.fn>;
    let clickSpy: ReturnType<typeof vi.fn>;

    beforeEach(() => {
      createObjectURL = vi.fn(() => "blob:http://localhost/fake-url");
      revokeObjectURL = vi.fn();
      vi.stubGlobal("URL", { ...URL, createObjectURL, revokeObjectURL });
      clickSpy = vi.fn();
      const origCreateElement = document.createElement.bind(document);
      vi.spyOn(document, "createElement").mockImplementation((tag: string) => {
        const el = origCreateElement(tag);
        if (tag === "a") {
          el.click = clickSpy;
        }
        return el;
      });
    });

    function mockTarball(headers: Record<string, string>) {
      mockFetch.mockResolvedValueOnce({
        ok: true,
        headers: new Headers(headers),
        blob: () => Promise.resolve(new Blob(["fake tarball"])),
      });
    }

    function renderOpen(onClose = vi.fn()) {
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
      return onClose;
    }

    const NOTICE =
      "2 packages that own a unit enabled on the source host are not carried: firewalld, rhc. See the Not carried table in audit-report.md and the # Not carried block in the Containerfile.";
    const FAILURE =
      "epel-release.noarch: staged RPM not verified, not exported";

    it("stays open listing the warnings, a notice as info and a failure as warning", async () => {
      const user = userEvent.setup();
      mockTarball({
        "x-export-warnings": JSON.stringify([
          { kind: "notice", text: NOTICE },
          { kind: "failure", text: FAILURE },
        ]),
      });
      const onClose = renderOpen();

      await user.click(screen.getByRole("button", { name: "Export" }));

      await screen.findByText("Downloaded, with warnings");
      expect(clickSpy).toHaveBeenCalled();
      expect(onClose).not.toHaveBeenCalled();

      const notice = screen.getByTestId("export-warning-0");
      const failure = screen.getByTestId("export-warning-1");
      expect(notice).toHaveTextContent(NOTICE);
      expect(notice).toHaveClass("pf-m-info");
      expect(failure).toHaveTextContent(FAILURE);
      expect(failure).toHaveClass("pf-m-warning");

      // Not individually dismissable: the only way out is closing the dialog.
      expect(
        screen.queryByRole("button", { name: /close .*alert/i }),
      ).not.toBeInTheDocument();
      expect(
        screen.queryByRole("button", { name: "Export" }),
      ).not.toBeInTheDocument();

      // The footer button; the modal's X is also labelled "Close".
      const footerClose = screen
        .getAllByRole("button", { name: "Close" })
        .find((b) => b.textContent === "Close");
      expect(footerClose).toBeDefined();
      await user.click(footerClose!);
      expect(onClose).toHaveBeenCalledTimes(1);
    });

    it("names the dialog after its state", async () => {
      const user = userEvent.setup();
      mockTarball({
        "x-export-warnings": JSON.stringify([{ kind: "failure", text: FAILURE }]),
      });
      renderOpen();
      expect(
        screen.getByRole("dialog", { name: "Export tarball" }),
      ).toBeInTheDocument();

      await user.click(screen.getByRole("button", { name: "Export" }));

      expect(
        await screen.findByRole("dialog", { name: "Export warnings" }),
      ).toBeInTheDocument();
    });

    it("starts fresh when reopened after a download with warnings", async () => {
      const user = userEvent.setup();
      const onClose = vi.fn();
      const props = {
        onClose,
        stats: MOCK_STATS,
        generation: 7,
        sessionIsSensitive: false,
        onViewUpdate: vi.fn(),
      };
      const { rerender } = render(<ExportDialog isOpen={true} {...props} />);

      mockTarball({
        "x-export-warnings": JSON.stringify([{ kind: "failure", text: FAILURE }]),
      });
      await user.click(screen.getByRole("button", { name: "Export" }));
      await screen.findByText("Downloaded, with warnings");
      const footerClose = screen
        .getAllByRole("button", { name: "Close" })
        .find((b) => b.textContent === "Close");
      await user.click(footerClose!);
      expect(onClose).toHaveBeenCalledTimes(1);

      rerender(<ExportDialog isOpen={false} {...props} />);
      rerender(<ExportDialog isOpen={true} {...props} />);

      expect(
        screen.queryByText("Downloaded, with warnings"),
      ).not.toBeInTheDocument();
      mockTarball({ "x-export-warnings": "[]" });
      await user.click(screen.getByRole("button", { name: "Export" }));

      await waitFor(() => expect(onClose).toHaveBeenCalledTimes(2));
      expect(
        screen.queryByText("Downloaded, with warnings"),
      ).not.toBeInTheDocument();
    });

    it("closes on success when the header is []", async () => {
      const user = userEvent.setup();
      mockTarball({ "x-export-warnings": "[]" });
      const onClose = renderOpen();

      await user.click(screen.getByRole("button", { name: "Export" }));

      await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
      expect(clickSpy).toHaveBeenCalled();
      expect(
        screen.queryByText("Downloaded, with warnings"),
      ).not.toBeInTheDocument();
    });

    it("closes on success when the header is missing", async () => {
      const user = userEvent.setup();
      mockTarball({});
      const onClose = renderOpen();

      await user.click(screen.getByRole("button", { name: "Export" }));

      await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
      expect(clickSpy).toHaveBeenCalled();
    });

    it("still downloads and says so when the header is malformed", async () => {
      const user = userEvent.setup();
      mockTarball({ "x-export-warnings": "[{not json" });
      const onClose = renderOpen();

      await user.click(screen.getByRole("button", { name: "Export" }));

      await screen.findByText("Downloaded, with warnings");
      expect(clickSpy).toHaveBeenCalled();
      expect(onClose).not.toHaveBeenCalled();
      expect(screen.getByTestId("export-warning-0")).toHaveClass(
        "pf-m-warning",
      );
      expect(screen.queryByText("Export failed")).not.toBeInTheDocument();
    });

    it("renders warning text as plain text, not markup", async () => {
      const user = userEvent.setup();
      mockTarball({
        "x-export-warnings": JSON.stringify([
          { kind: "failure", text: "<b>bold</b>.noarch: not exported" },
        ]),
      });
      renderOpen();

      await user.click(screen.getByRole("button", { name: "Export" }));

      const entry = await screen.findByTestId("export-warning-0");
      expect(entry).toHaveTextContent("<b>bold</b>.noarch: not exported");
      expect(entry.querySelector("b")).toBeNull();
    });

    describe("with the not-carried notice shown before download", () => {
      function renderWithNotice(onClose = vi.fn()) {
        render(
          <ExportDialog
            isOpen={true}
            onClose={onClose}
            stats={MOCK_STATS}
            generation={7}
            sessionIsSensitive={false}
            onViewUpdate={vi.fn()}
            notCarriedPackages={NOT_CARRIED_ROWS}
            notCarriedNotice={VIEW_NOTICE}
            onReviewNotCarried={vi.fn()}
          />,
        );
        return onClose;
      }

      it("skips the header entry equal to the view's notice", async () => {
        const user = userEvent.setup();
        mockTarball({
          "x-export-warnings": JSON.stringify([
            VIEW_NOTICE,
            { kind: "failure", text: FAILURE },
          ]),
        });
        renderWithNotice();

        await user.click(screen.getByRole("button", { name: "Export" }));

        await screen.findByText("Downloaded, with warnings");
        expect(screen.getByTestId("export-warning-0")).toHaveTextContent(
          FAILURE,
        );
        expect(screen.queryByTestId("export-warning-1")).toBeNull();
        expect(screen.queryByText(VIEW_NOTICE.text)).toBeNull();
      });

      it("closes when the notice was the only entry", async () => {
        const user = userEvent.setup();
        mockTarball({ "x-export-warnings": JSON.stringify([VIEW_NOTICE]) });
        const onClose = renderWithNotice();

        await user.click(screen.getByRole("button", { name: "Export" }));

        await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
        expect(clickSpy).toHaveBeenCalledTimes(1);
        expect(
          screen.queryByText("Downloaded, with warnings"),
        ).not.toBeInTheDocument();
      });

      // After firewalld is included: one owner left, and the notice says so.
      const ROWS_AFTER_INCLUDE = NOT_CARRIED_ROWS.filter(
        (r) => r.display_name !== "firewalld",
      );
      const NOTICE_AFTER_INCLUDE = {
        kind: "notice" as const,
        text: "1 package that owns a unit enabled on the source host is not carried: rhc. See the Not carried table in audit-report.md and the # Not carried block in the Containerfile.",
      };

      it("drops the notice shown at Export, not one that arrives mid-download", async () => {
        const user = userEvent.setup();
        let respond: (headers: Record<string, string>) => void = () => {};
        mockFetch.mockReturnValueOnce(
          new Promise((resolve) => {
            respond = (headers) =>
              resolve({
                ok: true,
                headers: new Headers(headers),
                blob: () => Promise.resolve(new Blob(["fake tarball"])),
              });
          }),
        );
        const props = {
          isOpen: true,
          onClose: vi.fn(),
          stats: MOCK_STATS,
          generation: 7,
          sessionIsSensitive: false,
          onViewUpdate: vi.fn(),
          onReviewNotCarried: vi.fn(),
        };
        const { rerender } = render(
          <ExportDialog
            {...props}
            notCarriedPackages={NOT_CARRIED_ROWS}
            notCarriedNotice={VIEW_NOTICE}
          />,
        );

        await user.click(screen.getByRole("button", { name: "Export" }));
        // The view changes while the tarball is still on its way.
        rerender(
          <ExportDialog
            {...props}
            notCarriedPackages={ROWS_AFTER_INCLUDE}
            notCarriedNotice={NOTICE_AFTER_INCLUDE}
          />,
        );
        respond({
          "x-export-warnings": JSON.stringify([
            VIEW_NOTICE,
            NOTICE_AFTER_INCLUDE,
          ]),
        });

        // The operator saw VIEW_NOTICE when pressing Export; the later one
        // was never on screen before download, so it is shown.
        await screen.findByText("Downloaded, with warnings");
        expect(screen.getByTestId("export-warning-0")).toHaveTextContent(
          NOTICE_AFTER_INCLUDE.text,
        );
        expect(screen.queryByTestId("export-warning-1")).toBeNull();
        expect(screen.queryByText(VIEW_NOTICE.text)).toBeNull();
      });

      it("shows the current notice when reopened after the view changed", async () => {
        const user = userEvent.setup();
        const onClose = vi.fn();
        const props = {
          onClose,
          stats: MOCK_STATS,
          generation: 7,
          sessionIsSensitive: false,
          onViewUpdate: vi.fn(),
          onReviewNotCarried: vi.fn(),
        };
        const { rerender } = render(
          <ExportDialog
            isOpen={true}
            {...props}
            notCarriedPackages={NOT_CARRIED_ROWS}
            notCarriedNotice={VIEW_NOTICE}
          />,
        );
        mockTarball({
          "x-export-warnings": JSON.stringify([
            VIEW_NOTICE,
            { kind: "failure", text: FAILURE },
          ]),
        });
        await user.click(screen.getByRole("button", { name: "Export" }));
        await screen.findByText("Downloaded, with warnings");
        const footerClose = screen
          .getAllByRole("button", { name: "Close" })
          .find((b) => b.textContent === "Close");
        await user.click(footerClose!);

        // An include between the two opens changes the rows and the notice.
        rerender(
          <ExportDialog
            isOpen={false}
            {...props}
            notCarriedPackages={ROWS_AFTER_INCLUDE}
            notCarriedNotice={NOTICE_AFTER_INCLUDE}
          />,
        );
        rerender(
          <ExportDialog
            isOpen={true}
            {...props}
            notCarriedPackages={ROWS_AFTER_INCLUDE}
            notCarriedNotice={NOTICE_AFTER_INCLUDE}
          />,
        );

        expect(
          screen.queryByText("Downloaded, with warnings"),
        ).not.toBeInTheDocument();
        const notice = screen.getByTestId("not-carried-notice");
        expect(notice).toHaveTextContent(
          "1 package that owns an enabled unit will not be carried",
        );
        await user.click(
          within(notice).getByRole("button", {
            name: "Show not-carried details",
          }),
        );
        expect(within(notice).getByText("rhcd.service")).toBeInTheDocument();
        expect(within(notice).queryByText("firewalld.service")).toBeNull();

        // And the download dedupes against the notice now on screen.
        mockTarball({
          "x-export-warnings": JSON.stringify([NOTICE_AFTER_INCLUDE]),
        });
        await user.click(screen.getByRole("button", { name: "Export" }));

        await waitFor(() => expect(onClose).toHaveBeenCalledTimes(2));
        expect(
          screen.queryByText("Downloaded, with warnings"),
        ).not.toBeInTheDocument();
      });

      it("shows a header notice that differs from the view's", async () => {
        const user = userEvent.setup();
        const drifted = {
          kind: "notice",
          text: VIEW_NOTICE.text.replace("2 packages", "3 packages"),
        };
        mockTarball({ "x-export-warnings": JSON.stringify([drifted]) });
        renderWithNotice();

        await user.click(screen.getByRole("button", { name: "Export" }));

        expect(await screen.findByTestId("export-warning-0")).toHaveTextContent(
          drifted.text,
        );
      });
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

  it("shows unmatched upload warning and gates export when unmatched uploads exist", async () => {
    const user = userEvent.setup();
    render(
      <ExportDialog
        isOpen={true}
        onClose={vi.fn()}
        stats={MOCK_STATS}
        generation={7}
        sessionIsSensitive={false}
        onViewUpdate={vi.fn()}
        unmatchedUploadCount={2}
      />,
    );

    // Warning should be visible
    expect(
      screen.getByTestId("unmatched-upload-warning"),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/2 uploaded RPMs have no matching package/),
    ).toBeInTheDocument();

    // Export button should be disabled until acknowledged
    const exportBtn = screen.getByRole("button", { name: "Export" });
    expect(exportBtn).toBeDisabled();

    // Check the acknowledgment box
    const checkbox = screen.getByTestId("unmatched-ack-checkbox");
    await user.click(checkbox);

    // Export button should now be enabled
    expect(exportBtn).not.toBeDisabled();
  });

  it("does not show unmatched warning when unmatchedUploadCount is zero", () => {
    render(
      <ExportDialog
        isOpen={true}
        onClose={vi.fn()}
        stats={MOCK_STATS}
        generation={7}
        sessionIsSensitive={false}
        onViewUpdate={vi.fn()}
        unmatchedUploadCount={0}
      />,
    );

    expect(
      screen.queryByTestId("unmatched-upload-warning"),
    ).not.toBeInTheDocument();
    // Export should be enabled (no gates)
    expect(screen.getByRole("button", { name: "Export" })).not.toBeDisabled();
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

  // The dialog's job is to say what export will actually do. Advisories
  // were counted among the exclusions, so it credited the user with
  // decisions the tool made. They are reported in the audit report but
  // their files are skipped by `write_config_tree`, so both halves have to
  // be said -- reporting only the exclusion count now undercounts what is
  // absent from the image.
  it("reports advisory findings apart from the exclusion counts", () => {
    render(
      <ExportDialog
        isOpen={true}
        onClose={vi.fn()}
        stats={mockStats({
          sections: [
            { kind: "package", total: 10, included: 8, excluded: 2, advisory: 0 },
            { kind: "config", total: 3, included: 1, excluded: 0, advisory: 2 },
          ],
        })}
        generation={7}
        sessionIsSensitive={false}
        onViewUpdate={vi.fn()}
      />,
    );

    expect(
      screen.getByText(/2 packages excluded, 0 configs excluded/),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        /Advisory findings \(2\) are reported in the audit report; their files are not copied into the image\./,
      ),
    ).toBeInTheDocument();
  });

  it("omits the advisory note when there are no advisories", () => {
    render(
      <ExportDialog
        isOpen={true}
        onClose={vi.fn()}
        stats={mockStats({
          sections: [
            { kind: "package", total: 10, included: 8, excluded: 2, advisory: 0 },
            { kind: "config", total: 4, included: 3, excluded: 1, advisory: 0 },
          ],
        })}
        generation={7}
        sessionIsSensitive={false}
        onViewUpdate={vi.fn()}
      />,
    );

    expect(screen.queryByText(/Advisory findings/)).not.toBeInTheDocument();
  });

  describe("not-carried notice before download", () => {
    function renderNotice(
      props: Partial<React.ComponentProps<typeof ExportDialog>> = {},
    ) {
      const handlers = { onClose: vi.fn(), onReviewNotCarried: vi.fn() };
      render(
        <ExportDialog
          isOpen={true}
          stats={MOCK_STATS}
          generation={7}
          sessionIsSensitive={false}
          onViewUpdate={vi.fn()}
          notCarriedPackages={NOT_CARRIED_ROWS}
          notCarriedNotice={VIEW_NOTICE}
          {...handlers}
          {...props}
        />,
      );
      return handlers;
    }

    it("shows nothing when the view has no notice", () => {
      renderNotice({ notCarriedNotice: null });
      expect(screen.queryByTestId("not-carried-notice")).toBeNull();
    });

    it("is an info alert counting the rows that own an enabled unit", () => {
      renderNotice();
      const alert = screen.getByTestId("not-carried-notice");
      expect(alert).toHaveClass("pf-m-info");
      expect(alert).toHaveTextContent(
        "2 packages that own enabled units will not be carried",
      );
    });

    it("uses the singular for one owner", () => {
      renderNotice({
        notCarriedPackages: [
          notCarried("cronie", ["crond.service"]),
          notCarried("python3-firewall", []),
        ],
      });
      expect(screen.getByTestId("not-carried-notice")).toHaveTextContent(
        "1 package that owns an enabled unit will not be carried",
      );
    });

    it("keeps the names collapsed until expanded", async () => {
      const user = userEvent.setup();
      renderNotice();
      const alert = screen.getByTestId("not-carried-notice");
      expect(within(alert).queryByText("rhc")).toBeNull();

      await user.click(
        within(alert).getByRole("button", { name: "Show not-carried details" }),
      );

      expect(within(alert).getByText("firewalld")).toBeInTheDocument();
      expect(within(alert).getByText("rhcd.service")).toBeInTheDocument();
      // Only the owners: the notice is about units, not every listed package.
      expect(within(alert).queryByText("python3-firewall")).toBeNull();
    });

    it("links to the group, closing the dialog", async () => {
      const user = userEvent.setup();
      const { onClose, onReviewNotCarried } = renderNotice();

      await user.click(
        screen.getByRole("button", { name: "Review not-carried packages" }),
      );

      expect(onClose).toHaveBeenCalledTimes(1);
      expect(onReviewNotCarried).toHaveBeenCalledTimes(1);
    });

    it("offers no link when nothing can show the group", () => {
      renderNotice({ onReviewNotCarried: undefined });
      expect(
        screen.queryByRole("button", { name: "Review not-carried packages" }),
      ).toBeNull();
      expect(screen.getByTestId("not-carried-notice")).toBeInTheDocument();
    });
  });
});
