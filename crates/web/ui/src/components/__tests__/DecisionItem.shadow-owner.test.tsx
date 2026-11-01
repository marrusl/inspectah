import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { DecisionItem } from "../DecisionItem";
import type { ShadowOwner } from "../shadowOwner";
import type { RefinedConfig, TriageTag } from "../../api/types";

const TRIAGE: TriageTag = {
  triage: { mode: "single_host", site: null },
  primary_reason: "service_drop_in_present",
  annotations: [],
};

const PATH = "/etc/systemd/system/sshd.service.d/override.conf";

const OWNER: ShadowOwner = {
  label: "Controlled in Service Drop-ins",
  state: { text: "Owner included", included: true },
  explanation: "The Service Drop-ins row for this same path carries the file.",
  target: { sectionId: "services", itemId: PATH },
};

function shadowedConfig(): RefinedConfig {
  return {
    entry: {
      path: PATH,
      kind: "unowned",
      category: "other",
      content: "",
      rpm_va_flags: null,
      package: null,
      diff_against_rpm: null,
      disposition: { kind: "inventory" },
      tie: false,
      tie_winner: false,
      aggregate: null,
      shadowed_by: "services",
    },
    triage: TRIAGE,
  };
}

function renderShadowed(
  owner: ShadowOwner | null = OWNER,
  onNavigateToOwner = vi.fn(),
) {
  render(
    <DecisionItem
      item={{ type: "config", data: shadowedConfig() }}
      triageTag={TRIAGE}
      rowIndex={1}
      isViewed={false}
      isPending={false}
      shadowOwner={owner ?? undefined}
      onNavigateToOwner={onNavigateToOwner}
      onToggleInclude={vi.fn()}
      onMarkViewed={vi.fn()}
    />,
  );
  return onNavigateToOwner;
}

describe("DecisionItem shadowed config row", () => {
  it("replaces the bare Inventory badge with the owner's name", () => {
    renderShadowed();
    expect(screen.queryByTestId("inventory-badge")).toBeNull();
    expect(screen.getByTestId("shadow-owner-badge")).toHaveTextContent(
      "Controlled in Service Drop-ins",
    );
  });

  it("shows whether the owner is currently included", () => {
    renderShadowed();
    expect(screen.getByTestId("shadow-owner-state")).toHaveTextContent(
      "Owner included",
    );
  });

  it("explains where the decision went below the row", () => {
    renderShadowed();
    expect(
      screen.getByTestId(`shadow-owner-explanation-configs:${PATH}`),
    ).toHaveTextContent("Service Drop-ins row for this same path");
  });

  it("routes to the owning row when the operator clicks through", async () => {
    const onNavigateToOwner = renderShadowed();
    await userEvent.click(screen.getByTestId("shadow-owner-link"));
    expect(onNavigateToOwner).toHaveBeenCalledWith("services", PATH);
  });

  it("offers no route and no state when the owning row is absent", () => {
    renderShadowed({ ...OWNER, state: null, target: null });
    expect(screen.getByTestId("shadow-owner-badge")).toBeInTheDocument();
    expect(screen.queryByTestId("shadow-owner-state")).toBeNull();
    expect(screen.queryByTestId("shadow-owner-link")).toBeNull();
  });

  it("keeps the bare Inventory badge when no owner was resolved", () => {
    renderShadowed(null);
    expect(screen.getByTestId("inventory-badge")).toBeInTheDocument();
    expect(screen.queryByTestId("shadow-owner-badge")).toBeNull();
  });

  it("offers no include checkbox — the owner holds the decision", () => {
    renderShadowed();
    expect(screen.queryByRole("checkbox")).toBeNull();
  });
});
