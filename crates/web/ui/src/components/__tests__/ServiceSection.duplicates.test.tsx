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

  /**
   * Each row's references in document order, each named by a piece of the
   * class or text of the note it names. The badge is named by its class:
   * its text is not this test's concern.
   */
  it.each<[string, string, string[]]>([
    [
      "unit row, declined, with instance paths",
      "service-item-wg-quick@wg0.service",
      [
        "inspectah-unit-declined",
        "Instance of wg-quick@.service",
        "Needs configuration: /etc/wireguard/wg0.conf",
      ],
    ],
    ["unit row, locked", "service-item-packagekit.service", ["inspectah-decision-row__badge"]],
    ["unit row, declined, with attention", "service-item-tmp.mount", ["inspectah-unit-declined"]],
    ["drop-in row, withheld", `dropin-item-${LIMITS_DROPIN}`, ["inspectah-unit-note--withheld"]],
    [
      "link row",
      'manual-link-["wants","multi-user.target","dup-link-demo.service"]',
      ["# Advisory: multi-user.target.wants/dup-link-demo.service"],
    ],
    [
      "mount finding row",
      `mount-finding-${planted.mount_findings[0].unit}`,
      [`# Advisory: ${planted.mount_findings[0].unit}`],
    ],
  ])("each duplicated %s resolves its own references inside itself", (_name, testId, expected) => {
    renderUnits(withNotedDups);
    const rows = screen.getAllByTestId(testId);
    expect(rows).toHaveLength(2);
    rows.forEach((row, occurrence) => {
      // Checked per occurrence: the second must name its notes as fully as the first.
      const named: string[] = [];
      const referrers = [row, ...Array.from(row.querySelectorAll<HTMLElement>("*"))];
      for (const el of referrers) {
        for (const attr of REFERENCE_ATTRS) {
          const value = el.getAttribute(attr);
          if (!value) continue;
          for (const id of value.split(/\s+/).filter(Boolean)) {
            const target = document.getElementById(id);
            expect(target, `occurrence ${occurrence}: ${attr} names missing id ${id}`).not.toBeNull();
            expect(row.contains(target), `occurrence ${occurrence}: ${attr} id ${id} lies outside its row`).toBe(
              true,
            );
            named.push(`${target!.className} ${target!.textContent ?? ""}`);
          }
        }
      }
      expect(named, `occurrence ${occurrence}`).toHaveLength(expected.length);
      expected.forEach((piece, i) => expect(named[i], `occurrence ${occurrence}, note ${i}`).toContain(piece));
    });
  });
});
