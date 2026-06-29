import { describe, it, expect, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { RpmBatchUploadModal } from "../RpmBatchUploadModal";

const PACKAGES = ["httpd.x86_64", "mod_ssl.x86_64", "apr.x86_64"];

function renderModal(
  props: Partial<React.ComponentProps<typeof RpmBatchUploadModal>> = {},
) {
  return render(
    <RpmBatchUploadModal
      isOpen={true}
      needsUploadPackages={PACKAGES}
      onBatchUpload={vi.fn()}
      onClose={vi.fn()}
      {...props}
    />,
  );
}

describe("RpmBatchUploadModal — package checklist", () => {
  it("renders all package names in the checklist", () => {
    renderModal();
    for (const pkg of PACKAGES) {
      expect(screen.getByText(pkg)).toBeInTheDocument();
    }
  });

  it("shows the section header with package count", () => {
    renderModal();
    expect(
      screen.getByText(`Packages needing RPMs (${PACKAGES.length})`),
    ).toBeInTheDocument();
  });

  it("shows match progress summary", () => {
    renderModal();
    expect(
      screen.getByText(`0 of ${PACKAGES.length} packages matched`),
    ).toBeInTheDocument();
  });

  it("shows green check labels for matched packages via file input", async () => {
    const user = userEvent.setup();
    renderModal();

    // Use file input instead of drag-and-drop — jsdom doesn't support
    // DataTransfer well enough for React's synthetic event system.
    const file = new File(["rpm-content"], "httpd-2.4.57-1.el9.x86_64.rpm", {
      type: "application/x-rpm",
    });
    const input = document.getElementById(
      "rpm-batch-file-input",
    ) as HTMLInputElement;
    await user.upload(input, file);

    // After upload, httpd.x86_64 should be matched and the summary should update
    expect(
      screen.getByText(`1 of ${PACKAGES.length} packages matched`),
    ).toBeInTheDocument();
  });

  it("renders all package labels in the checklist container", () => {
    renderModal();
    const checklist = screen.getByLabelText("Package checklist");
    expect(checklist).toBeInTheDocument();
    const items = within(checklist).getAllByRole("listitem");
    expect(items).toHaveLength(PACKAGES.length);
  });

  it("toggles the expandable section", async () => {
    const user = userEvent.setup();
    renderModal();

    // Checklist starts expanded
    const summary = screen.getByText(
      `0 of ${PACKAGES.length} packages matched`,
    );
    expect(summary).toBeVisible();

    // PF6 ExpandableSection toggle button has accessible name from toggleContent
    const toggle = screen.getByRole("button", {
      name: /packages needing rpms/i,
    });
    await user.click(toggle);
    expect(summary).not.toBeVisible();

    // Click again to expand
    await user.click(toggle);
    expect(summary).toBeVisible();
  });

  it("renders nothing when modal is not open", () => {
    const { container } = renderModal({ isOpen: false });
    expect(container).toBeEmptyDOMElement();
  });
});
