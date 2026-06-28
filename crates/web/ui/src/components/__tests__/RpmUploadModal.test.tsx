import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
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
});

describe("RpmBatchUploadModal", () => {
  const defaultBatchProps = {
    isOpen: true,
    needsUploadPackages: ["nginx", "custom-agent", "my-tool"],
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
});
