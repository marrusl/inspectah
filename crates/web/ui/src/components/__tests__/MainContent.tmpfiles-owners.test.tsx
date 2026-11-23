/**
 * The tmpfiles.d owner notice in Configuration Files: the carried lines
 * whose user or group the image lacks, as `tmpfiles_missing_owners`
 * sends them, with the remedy the audit reports print.
 */
import { describe, it, expect, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import { MainContent } from "../MainContent";
import { TMPFILES_OWNER_REMEDY } from "../TmpfilesOwnerNotice";
import type { TmpfilesMissingOwner, ViewResponse } from "../../api/types";
import { emptyView } from "../../test-utils/emptyView";

vi.mock("../../api/client", () => ({
  ungroupGroup: vi.fn(),
  applyOp: vi.fn(),
  fetchViewed: vi.fn().mockResolvedValue({ ids: [] }),
}));

const BOTH: TmpfilesMissingOwner = {
  file: "/etc/tmpfiles.d/appone.conf",
  path: "/var/lib/appone/cache",
  user: "appuser",
  group: "appgroup",
};
const GROUP_ONLY: TmpfilesMissingOwner = {
  file: "/etc/tmpfiles.d/apptwo.conf",
  path: "/var/lib/app two",
  user: null,
  group: "appgroup",
};

function renderConfig(view: ViewResponse) {
  render(
    <MainContent
      activeSection="config"
      loading={false}
      viewData={view}
      sections={[]}
      onViewUpdate={vi.fn()}
      onMutationError={vi.fn()}
      sectionSearchOpen={false}
      onSectionSearchClose={vi.fn()}
    />,
  );
}

describe("tmpfiles.d owner notice", () => {
  it("names each line's path and missing accounts, with the remedy", () => {
    renderConfig({
      ...emptyView(),
      tmpfiles_missing_owners: [BOTH, GROUP_ONLY],
    });
    const notice = screen.getByTestId("tmpfiles-owner-notice");
    expect(notice).toHaveTextContent(
      "tmpfiles.d lines naming accounts the image lacks (2)",
    );
    const rows = within(notice).getAllByTestId("tmpfiles-owner-row");
    expect(rows.map((r) => r.textContent)).toEqual([
      "/etc/tmpfiles.d/appone.conf: /var/lib/appone/cache needs user appuser, group appgroup",
      "/etc/tmpfiles.d/apptwo.conf: /var/lib/app two needs group appgroup",
    ]);
    expect(notice).toHaveTextContent(TMPFILES_OWNER_REMEDY);
  });

  it("renders nothing when the field is absent", () => {
    renderConfig(emptyView());
    expect(screen.queryByTestId("tmpfiles-owner-notice")).toBeNull();
  });

  it("renders nothing when the list is empty", () => {
    renderConfig({ ...emptyView(), tmpfiles_missing_owners: [] });
    expect(screen.queryByTestId("tmpfiles-owner-notice")).toBeNull();
  });
});
