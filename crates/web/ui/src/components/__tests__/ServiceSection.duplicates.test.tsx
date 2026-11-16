/**
 * True duplicates (§ 6.12) each render as their own row, and each row's
 * notes stay linked to that row through `aria-describedby` (§ 6.6).
 */
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen } from "@testing-library/react";
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
});
afterEach(() => {
  vi.restoreAllMocks();
});

const planted = loadUnitFields("planted");

const LIMITS_DROPIN = "/etc/systemd/system/limits-demo.slice.d/override.conf";
/** Planted rows that carry notes, each given a second time. */
const NOTED_UNITS = ["wg-quick@wg0.service", "packagekit.service", "tmp.mount"];

/**
 * Planted, plus a second copy of rows whose notes `aria-describedby`
 * names: three unit rows (declined, instance paths, locked, attention),
 * the withheld drop-in and the first mount finding.
 */
const withNotedDups: UnitFields = {
  ...planted,
  unit_states: [
    ...planted.unit_states,
    ...planted.unit_states.filter((r) => NOTED_UNITS.includes(r.unit)),
  ],
  unit_dropins: [
    ...planted.unit_dropins,
    ...planted.unit_dropins.filter((d) => d.path === LIMITS_DROPIN),
  ],
  mount_findings: [...planted.mount_findings, planted.mount_findings[0]],
};

function renderUnits(fields: UnitFields) {
  render(
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
    />,
  );
  return screen.getByTestId("service-section");
}

const REFERENCE_ATTRS = ["aria-describedby", "aria-labelledby", "for"] as const;

describe("duplicated rows keep their own ids (§ 6.6, § 6.12)", () => {
  it.each<[string, UnitFields]>([
    ["planted", planted],
    ["planted with noted rows duplicated", withNotedDups],
  ])("%s: no two elements share an id", (_name, fields) => {
    const section = renderUnits(fields);
    const ids = Array.from(section.querySelectorAll("[id]"))
      .map((el) => el.id)
      .filter((id) => id !== "");
    const repeated = ids.filter((id, i) => ids.indexOf(id) !== i);
    expect(repeated).toEqual([]);
  });

  it.each<[string, string]>([
    ["unit row, declined, with instance paths", "service-item-wg-quick@wg0.service"],
    ["unit row, locked", "service-item-packagekit.service"],
    ["unit row, declined, with attention", "service-item-tmp.mount"],
    ["drop-in row, withheld", `dropin-item-${LIMITS_DROPIN}`],
    ["link row", 'manual-link-["wants","multi-user.target","dup-link-demo.service"]'],
    ["mount finding row", `mount-finding-${planted.mount_findings[0].unit}`],
  ])("each duplicated %s resolves its references inside itself", (_name, testId) => {
    renderUnits(withNotedDups);
    const rows = screen.getAllByTestId(testId);
    expect(rows).toHaveLength(2);
    let checked = 0;
    for (const row of rows) {
      const referrers = [row, ...Array.from(row.querySelectorAll<HTMLElement>("*"))];
      for (const el of referrers) {
        for (const attr of REFERENCE_ATTRS) {
          const value = el.getAttribute(attr);
          if (!value) continue;
          for (const id of value.split(/\s+/).filter(Boolean)) {
            const target = document.getElementById(id);
            expect(target, `${attr} names missing id ${id}`).not.toBeNull();
            expect(row.contains(target), `${attr} id ${id} lies outside its row`).toBe(true);
            checked += 1;
          }
        }
      }
    }
    // Each duplicate names at least one note, or the test proves nothing.
    expect(checked).toBeGreaterThanOrEqual(rows.length);
  });
});
