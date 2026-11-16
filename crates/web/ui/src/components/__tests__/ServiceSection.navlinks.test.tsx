/**
 * Links out of a unit row (spec § 6.6 items 5 and 7): `Also=` members to
 * their own unit row, `required_config` paths to their config row. U15
 * (component half) and U16.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ServiceSection } from "../ServiceSection";
import type { PendingGuard } from "../../hooks/usePendingGuard";
import type { UnitFields } from "../unitOutcome";
import { loadUnitFields } from "../../__tests__/fixtures/systemd-units/load";

const stubGuard: PendingGuard = {
  pendingKeys: new Set<string>(),
  runGuarded: vi.fn(),
} as unknown as PendingGuard;

beforeEach(() => {
  vi.stubGlobal("fetch", vi.fn());
  window.location.hash = "";
});
afterEach(() => {
  vi.restoreAllMocks();
});

const kitchenSink = loadUnitFields("kitchen-sink");
const planted = loadUnitFields("planted");

function renderUnits(
  fields: UnitFields,
  extra: {
    configPaths?: ReadonlySet<string>;
    onNavigateToItem?: (sectionId: string, itemId: string) => void;
    onNavigateToConfig?: (path: string) => void;
  } = {},
) {
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
      pendingGuard={stubGuard}
      {...extra}
    />,
  );
}

/** The `also-member` element of `unit`'s Also= line named `member`. */
function alsoMember(unit: string, member: string): HTMLElement {
  return within(screen.getByTestId(`service-item-${unit}`))
    .getAllByTestId("also-member")
    .find((m) => within(m).getByTestId("also-member-unit").textContent === member)!;
}

describe("U15: Also= members link to their own row (§ 6.6 item 5)", () => {
  it("links a member with a unit row and leaves one without as code", () => {
    renderUnits(kitchenSink, { onNavigateToItem: vi.fn() });
    const socket = within(alsoMember("cups.service", "cups.socket")).getByTestId("also-member-unit");
    expect(socket.tagName).toBe("A");
    expect(socket).toHaveAttribute("href", "#units:cups.socket");
    const path = within(alsoMember("cups.service", "cups.path")).getByTestId("also-member-unit");
    expect(path.tagName).toBe("CODE");
    expect(kitchenSink.unit_states.some((r) => r.unit === "cups.path")).toBe(false);
  });

  it("keeps the links word beside each member", () => {
    renderUnits(kitchenSink, { onNavigateToItem: vi.fn() });
    const cups = kitchenSink.unit_states.find((r) => r.unit === "cups.service")!;
    for (const m of cups.also!) {
      expect(within(alsoMember("cups.service", m.unit)).getByTestId("also-member-links").textContent).toBe(
        m.links,
      );
    }
  });

  it("calls onNavigateToItem with the member and does not follow the hash", async () => {
    const onNavigateToItem = vi.fn();
    renderUnits(kitchenSink, { onNavigateToItem });
    await userEvent.click(within(alsoMember("cups.service", "cups.socket")).getByRole("link"));
    expect(onNavigateToItem).toHaveBeenCalledWith("units", "cups.socket");
    expect(onNavigateToItem).toHaveBeenCalledTimes(1);
    expect(window.location.hash).toBe("");
  });
});

describe("U16: required_config links to its config row (§ 6.6 item 7)", () => {
  const DEMO_A = "/etc/wireguard/demo-a.conf";
  const DEMO_B = "/etc/wireguard/demo-b.conf";
  const DATA = "/var/lib/demo-a";
  const fields: UnitFields = {
    ...planted,
    unit_states: planted.unit_states.map((r) =>
      r.unit === "wg-quick@demo-a.service" ? { ...r, data_paths: [DATA] } : r,
    ),
  };

  it("links a path config_files holds, and leaves the other path and data_paths as code", async () => {
    const onNavigateToConfig = vi.fn();
    // The data path is in configPaths too: data_paths stay plain regardless.
    renderUnits(fields, { configPaths: new Set([DEMO_A, DATA]), onNavigateToConfig });
    const linked = within(screen.getByTestId("required-config-wg-quick@demo-a.service")).getByTestId(
      "required-config-path",
    );
    expect(linked.tagName).toBe("A");
    expect(linked).toHaveAttribute("href", `#configs:${DEMO_A}`);
    expect(linked.textContent).toBe(DEMO_A);

    const plain = within(screen.getByTestId("required-config-wg-quick@demo-b.service")).getByTestId(
      "required-config-path",
    );
    expect(plain.tagName).toBe("CODE");
    expect(plain.textContent).toBe(DEMO_B);

    const data = within(screen.getByTestId("data-paths-wg-quick@demo-a.service")).getByTestId("data-path");
    expect(data.tagName).toBe("CODE");
    expect(data.textContent).toBe(DATA);

    await userEvent.click(linked);
    expect(onNavigateToConfig).toHaveBeenCalledWith(DEMO_A);
    expect(window.location.hash).toBe("");
  });
});
