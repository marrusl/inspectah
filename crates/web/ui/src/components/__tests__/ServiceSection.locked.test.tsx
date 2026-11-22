/**
 * A locked units row names its lock in words, never in a slug (spec § 9.3,
 * § 6.6 item 13, § 7 string 24). U28.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, within } from "@testing-library/react";
import { ServiceSectionWithGuard as ServiceSection } from "../../test-utils/withPendingGuard";
import type { DropInDecisionDto, ServiceDecisionDto, TriageTag } from "../../api/types";
import type { UnitFields } from "../unitOutcome";
import { loadUnitFields } from "../../__tests__/fixtures/systemd-units/load";

beforeEach(() => {
  vi.stubGlobal("fetch", vi.fn());
});
afterEach(() => {
  vi.restoreAllMocks();
});

const SLUG = "service-image-mode-incompatible";
const LABEL = "Incompatible with image mode";

const TRIAGE: TriageTag = {
  triage: { mode: "single_host", site: null },
  primary_reason: "service_non_default_state",
  annotations: [],
};

function makeUnit(overrides: Partial<ServiceDecisionDto> = {}): ServiceDecisionDto {
  return {
    unit: "held.service",
    kind: "service",
    fragment: "vendor",
    triage: TRIAGE,
    include: false,
    locked: true,
    current_state: "enabled",
    required_config: [],
    data_paths: [],
    ...overrides,
  };
}

function makeDropin(overrides: Partial<DropInDecisionDto> = {}): DropInDecisionDto {
  return {
    unit: "httpd.service",
    path: "/etc/systemd/system/httpd.service.d/held.conf",
    triage: TRIAGE,
    include: true,
    locked: true,
    kind: "service",
    config_paths: [overrides.path ?? "/etc/systemd/system/httpd.service.d/held.conf"],
    ...overrides,
  };
}

const EMPTY: UnitFields = {
  unit_states: [],
  unit_dropins: [],
  default_target: undefined,
  mount_findings: [],
  manual_links: [],
  undetermined_installs: [],
};

function renderUnits(fields: UnitFields) {
  return render(
    <ServiceSection
      services={fields.unit_states}
      dropins={fields.unit_dropins}
      mountFindings={fields.mount_findings}
      manualLinks={fields.manual_links}
      undeterminedInstalls={fields.undetermined_installs}
      defaultTarget={fields.default_target}
      onViewUpdate={vi.fn()}
      onMutationError={vi.fn()}
    />,
  );
}

describe("U28: a locked row's label (§ 9.3)", () => {
  it("names the image-mode lock in words on packagekit.service, and never prints the slug", () => {
    const planted = loadUnitFields("planted");
    const packagekit = planted.unit_states.find((r) => r.unit === "packagekit.service");
    // Presence first: the planted row is locked with the slug as its reason.
    expect(packagekit?.locked).toBe(true);
    expect(packagekit?.attention_reason).toBe(SLUG);

    const { container } = renderUnits(planted);
    expect(screen.getByTestId("locked-badge-service-packagekit.service")).toHaveTextContent(
      new RegExp(`^${LABEL}$`),
    );
    const checkbox = within(screen.getByTestId("service-item-packagekit.service")).getByRole("checkbox");
    expect(checkbox).toHaveAccessibleName(`packagekit.service (locked: ${LABEL})`);
    expect(container.innerHTML).not.toContain(SLUG);
  });

  it("shows LOCKED on a locked row with no reason", () => {
    renderUnits({ ...EMPTY, unit_states: [makeUnit({ attention_reason: null })] });
    expect(screen.getByTestId("locked-badge-service-held.service")).toHaveTextContent(/^LOCKED$/);
  });

  it("shows any other reason on a locked row as given", () => {
    renderUnits({ ...EMPTY, unit_states: [makeUnit({ attention_reason: "held by the base image" })] });
    expect(screen.getByTestId("locked-badge-service-held.service")).toHaveTextContent(
      /^held by the base image$/,
    );
    expect(within(screen.getByTestId("service-item-held.service")).getByRole("checkbox")).toHaveAccessibleName(
      "held.service (locked: held by the base image)",
    );
  });

  it("leaves a locked drop-in's prose reason unchanged", () => {
    const dropin = makeDropin({ attention_reason: "held by the base image" });
    renderUnits({ ...EMPTY, unit_dropins: [dropin] });
    expect(screen.getByTestId(`locked-badge-dropin-${dropin.path}`)).toHaveTextContent(
      /^held by the base image$/,
    );
    expect(within(screen.getByTestId(`dropin-item-${dropin.path}`)).getByRole("checkbox")).toHaveAccessibleName(
      `${dropin.path} (locked: held by the base image)`,
    );
  });
});

/**
 * Dimmed rows keep a full-strength focus outline (WCAG 1.4.11; rows are
 * keyboard stops, § 6.13): the row element carries no opacity, and the
 * dimming sits on its toggle and name cells only, so the outline and the
 * status badges stay at full strength.
 */
describe("a dimmed row keeps its outline and badges at full strength", () => {
  const DIMMED = "inspectah-unit-dimmed";

  function expectDimmedCells(row: HTMLElement) {
    expect(row.style.opacity).toBe("");
    expect(row).not.toHaveClass(DIMMED);
    const main = row.querySelector(":scope > .inspectah-decision-row__main")!;
    expect(main.querySelector(":scope > .inspectah-decision-row__toggle")).toHaveClass(DIMMED);
    expect(main.querySelector(":scope > .inspectah-decision-row__name")).toHaveClass(DIMMED);
    for (const badge of Array.from(main.querySelectorAll(":scope > .inspectah-decision-row__badge"))) {
      expect(badge).not.toHaveClass(DIMMED);
    }
    expect(row.querySelectorAll(`.${DIMMED}`)).toHaveLength(2);
  }

  it("dims a locked unit row's toggle and name cells, not the row", () => {
    const planted = loadUnitFields("planted");
    renderUnits(planted);
    expectDimmedCells(screen.getByTestId("service-item-packagekit.service"));
  });

  it("dims the toggle and name cells of a drop-in under an excluded parent, not the row", () => {
    const parent = makeUnit({ unit: "httpd.service", locked: false, include: false });
    const dropin = makeDropin({ locked: false });
    renderUnits({ ...EMPTY, unit_states: [parent], unit_dropins: [dropin] });
    const row = screen.getByTestId(`dropin-item-${dropin.path}`);
    expect(within(row).getByText("Unit excluded")).toBeInTheDocument();
    expectDimmedCells(row);
  });

  it("dims a locked drop-in and a locked default target the same way", () => {
    const dropin = makeDropin({ attention_reason: "held by the base image" });
    renderUnits({
      ...EMPTY,
      unit_dropins: [dropin],
      default_target: { target: "graphical.target", include: true, locked: true, declined: null },
    });
    expectDimmedCells(screen.getByTestId(`dropin-item-${dropin.path}`));
    expectDimmedCells(screen.getByTestId("default-target"));
  });

  it("dims nothing on a row that is neither locked nor under an excluded parent", () => {
    const parent = makeUnit({ unit: "httpd.service", locked: false, include: true });
    const dropin = makeDropin({ locked: false });
    renderUnits({ ...EMPTY, unit_states: [parent], unit_dropins: [dropin] });
    for (const id of ["service-item-httpd.service", `dropin-item-${dropin.path}`]) {
      const row = screen.getByTestId(id);
      expect(row.style.opacity).toBe("");
      expect(row.querySelectorAll(".inspectah-unit-dimmed")).toHaveLength(0);
    }
  });
});
