import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { RpmUploadModal } from "../RpmUploadModal";
import { RpmBatchUploadModal } from "../RpmBatchUploadModal";

describe("RpmUploadModal", () => {
  const defaultProps = {
    isOpen: true,
    packageName: "custom-agent",
    packageArch: "x86_64",
    onUpload: vi.fn(),
    onClose: vi.fn(),
    triggerRef: { current: null } as React.RefObject<HTMLElement | null>,
  };

  it("renders modal with package name in title", () => {
    render(<RpmUploadModal {...defaultProps} />);
    expect(screen.getByText(/Upload RPM for custom-agent/)).toBeInTheDocument();
  });

  it("shows expected NEVRA pattern", () => {
    render(<RpmUploadModal {...defaultProps} />);
    expect(screen.getByText(/custom-agent.*x86_64\.rpm/)).toBeInTheDocument();
  });

  it("confirm button is disabled when no file is selected", () => {
    render(<RpmUploadModal {...defaultProps} />);
    const confirmBtn = screen.getByRole("button", { name: /confirm|upload/i });
    expect(confirmBtn).toBeDisabled();
  });

  it("does not render when isOpen is false", () => {
    render(<RpmUploadModal {...defaultProps} isOpen={false} />);
    expect(screen.queryByText(/Upload RPM/)).not.toBeInTheDocument();
  });

  it("has accessible modal label", () => {
    render(<RpmUploadModal {...defaultProps} />);
    expect(
      screen.getByRole("dialog", { name: /Upload RPM for custom-agent/ }),
    ).toBeInTheDocument();
  });

  it("validates RPM filename against bare package name (not canonical key)", () => {
    // Simulate what App.tsx does: split canonical "nginx.x86_64" into
    // packageName="nginx" and packageArch="x86_64"
    const onUpload = vi.fn();
    render(
      <RpmUploadModal
        {...defaultProps}
        packageName="nginx"
        packageArch="x86_64"
        onUpload={onUpload}
      />,
    );
    // The modal should show the bare name, not the canonical key
    expect(screen.getByText(/Upload RPM for nginx/)).toBeInTheDocument();
    expect(screen.getByText(/nginx-\*-\*\.x86_64\.rpm/)).toBeInTheDocument();
  });

  it("shows matched feedback after successful upload", async () => {
    const user = userEvent.setup();
    const onUpload = vi.fn().mockResolvedValue({
      uploaded: 1,
      matched: "custom-agent.x86_64",
      status: "matched",
    });
    render(<RpmUploadModal {...defaultProps} onUpload={onUpload} />);
    // Simulate file selection by finding the file input
    const input = document.querySelector('input[type="file"]')!;
    const file = new File(
      ["rpm"],
      "custom-agent-1.0-1.el9.x86_64.rpm",
      { type: "application/x-rpm" },
    );
    fireEvent.change(input, { target: { files: [file] } });
    // Click upload using userEvent for proper async handling
    const uploadBtn = screen.getByRole("button", { name: /confirm|upload/i });
    await user.click(uploadBtn);
    await waitFor(() => {
      expect(
        screen.getByTestId("upload-match-result"),
      ).toBeInTheDocument();
      expect(
        screen.getByText(/Matched to custom-agent\.x86_64/),
      ).toBeInTheDocument();
    });
  });

  it("shows warning for unmatched upload", async () => {
    const user = userEvent.setup();
    const onUpload = vi.fn().mockResolvedValue({
      uploaded: 1,
      matched: null,
      status: "unmatched",
    });
    render(<RpmUploadModal {...defaultProps} onUpload={onUpload} />);
    const input = document.querySelector('input[type="file"]')!;
    const file = new File(
      ["rpm"],
      "custom-agent-1.0-1.el9.x86_64.rpm",
      { type: "application/x-rpm" },
    );
    fireEvent.change(input, { target: { files: [file] } });
    const uploadBtn = screen.getByRole("button", { name: /confirm|upload/i });
    await user.click(uploadBtn);
    await waitFor(() => {
      expect(
        screen.getByTestId("upload-unmatched-warning"),
      ).toBeInTheDocument();
      expect(
        screen.getByText(/no matching package found/),
      ).toBeInTheDocument();
    });
  });
});

describe("RpmBatchUploadModal", () => {
  const defaultBatchProps = {
    isOpen: true,
    needsUploadPackages: [
      "nginx.x86_64",
      "custom-agent.x86_64",
      "my-tool.x86_64",
    ],
    onBatchUpload: vi.fn(),
    onClose: vi.fn(),
  };

  it("renders modal with package count", () => {
    render(<RpmBatchUploadModal {...defaultBatchProps} />);
    expect(screen.getByText(/Upload RPMs.*3 packages/i)).toBeInTheDocument();
  });

  it("confirm button is disabled when no files are dropped", () => {
    render(<RpmBatchUploadModal {...defaultBatchProps} />);
    const confirmBtn = screen.getByRole("button", { name: /confirm|upload/i });
    expect(confirmBtn).toBeDisabled();
  });

  it("does not render when isOpen is false", () => {
    render(<RpmBatchUploadModal {...defaultBatchProps} isOpen={false} />);
    expect(screen.queryByText(/Upload RPMs/)).not.toBeInTheDocument();
  });

  it("has accessible modal label", () => {
    render(<RpmBatchUploadModal {...defaultBatchProps} />);
    expect(
      screen.getByRole("dialog", { name: /Upload RPMs/i }),
    ).toBeInTheDocument();
  });

  it("matches dropped RPM files against canonical name.arch keys", async () => {
    const onBatchUpload = vi.fn();
    render(
      <RpmBatchUploadModal
        isOpen={true}
        needsUploadPackages={["nginx.x86_64", "custom-agent.x86_64"]}
        onBatchUpload={onBatchUpload}
        onClose={vi.fn()}
      />,
    );
    // Use the file input instead of drag-drop
    const fileInput = screen.getByLabelText(/browse/i).closest("label")?.htmlFor
      ? document.getElementById(
          screen.getByLabelText(/browse/i).closest("label")!.htmlFor,
        )
      : null;
    const input = fileInput ?? document.querySelector('input[type="file"]')!;
    const files = [
      new File(["rpm1"], "nginx-1.24-1.el9.x86_64.rpm", {
        type: "application/x-rpm",
      }),
      new File(["rpm2"], "custom-agent-2.0-1.el9.x86_64.rpm", {
        type: "application/x-rpm",
      }),
    ];
    fireEvent.change(input, { target: { files } });
    // Should show 2 matched
    await waitFor(() => {
      expect(screen.getByText(/2 of 2 RPMs matched/)).toBeInTheDocument();
    });
  });

  it("disambiguates multilib packages with same bare name but different arch", async () => {
    const onBatchUpload = vi.fn();
    render(
      <RpmBatchUploadModal
        isOpen={true}
        needsUploadPackages={["glibc.x86_64", "glibc.i686"]}
        onBatchUpload={onBatchUpload}
        onClose={vi.fn()}
      />,
    );
    const input = document.querySelector('input[type="file"]')!;
    const files = [
      new File(["rpm1"], "glibc-2.34-1.el9.x86_64.rpm", {
        type: "application/x-rpm",
      }),
      new File(["rpm2"], "glibc-2.34-1.el9.i686.rpm", {
        type: "application/x-rpm",
      }),
    ];
    fireEvent.change(input, { target: { files } });
    // Both should match without conflicts
    await waitFor(() => {
      expect(screen.getByText(/2 of 2 RPMs matched/)).toBeInTheDocument();
    });
    // No conflict alerts
    expect(screen.queryByText(/Conflicting uploads/)).not.toBeInTheDocument();
  });
});
