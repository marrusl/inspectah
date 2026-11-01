import { describe, it, expect, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { GlobalSearch } from "../GlobalSearch";
import type { DropInDecisionDto, TriageTag } from "../../api/types";

const TRIAGE: TriageTag = {
  triage: { mode: "single_host", site: null },
  primary_reason: "service_drop_in_present",
  annotations: [],
};

const DROPIN_PATH = "/etc/systemd/system/sshd.service.d/override.conf";

const DROPIN: DropInDecisionDto = {
  unit: "sshd.service",
  path: DROPIN_PATH,
  triage: TRIAGE,
  include: true,
};

function renderSearch(onNavigate = vi.fn()) {
  render(
    <GlobalSearch
      packageItems={[]}
      configItems={[]}
      contextSections={null}
      serviceDropins={[DROPIN]}
      onNavigate={onNavigate}
    />,
  );
  return onNavigate;
}

// Searching the path of a shadowed config has to surface the drop-in that
// owns its decision. Before this, the index covered configItems and the
// reference-projection sections only, so the owner was unfindable.
describe("GlobalSearch service drop-in indexing", () => {
  it("finds a drop-in by its path", async () => {
    renderSearch();
    await userEvent.type(
      screen.getByLabelText("Search all sections"),
      "override.conf",
    );
    const results = screen.getByTestId("global-search-results");
    expect(within(results).getByText(DROPIN_PATH)).toBeInTheDocument();
  });

  it("finds a drop-in by its unit name", async () => {
    renderSearch();
    await userEvent.type(screen.getByLabelText("Search all sections"), "sshd");
    const results = screen.getByTestId("global-search-results");
    expect(within(results).getByText(DROPIN_PATH)).toBeInTheDocument();
  });

  it("navigates to the drop-in row in the services section", async () => {
    const onNavigate = renderSearch();
    await userEvent.type(
      screen.getByLabelText("Search all sections"),
      "override.conf",
    );
    await userEvent.click(screen.getByTestId("global-search-result-0"));
    expect(onNavigate).toHaveBeenCalledWith("services", DROPIN_PATH);
  });
});
