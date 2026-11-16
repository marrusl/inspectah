/// <reference types="vite/client" />
/**
 * The units section's Vitest fixtures (spec § 10.1), and where they came
 * from. Every test reads them through this file.
 *
 * kitchen-sink.units.json: recorded, not written. Source:
 * inspectah-rhel10-kitchen-sink-e6591669.tar.gz, a driftify kitchen-sink
 * scan of RHEL 10.2 taken by inspectah e6591669 (schema 28), served by
 * refine built at inspectah 91a1438d with `--open false --fresh`, on a
 * local port other than the default (the port is immaterial to the
 * bodies). Requests sent, without an Origin header: GET /api/health,
 * GET /api/view, GET /api/snapshot/sections, then POST /api/op with
 * {"kind":"Op","op":"SetInclude","target":{"item_id":{"kind":"Unit","key":{"unit":"tmp.mount"}},"include":false}}.
 * The file is the six unit fields of the GET /api/view body (unit_states,
 * unit_dropins, default_target, mount_findings, manual_links,
 * undetermined_installs), copied verbatim by a script kept outside the
 * repository. It holds 19 unit rows, 4 drop-ins, 1 default target, 2
 * manual links, 1 undetermined install and 0 mount findings. The bodies
 * were searched for host names, account names, addresses and credential
 * markers before they were committed; the "redacted" text in them is the
 * scan's own.
 *
 * stock10.units.json and stock9.units.json: hand-built from the stock host
 * records of run R4 in process-docs/skills/real-host-validation-runs.md
 * (multi-user.target on both; RHEL 9 adds
 * NetworkManager-wait-online.service, enabled against a disable preset,
 * carried). The record does not carry fragment or triage, so stock9's row
 * takes them from a served kitchen-sink row of the same kind (a vendor
 * service at a non-default state).
 *
 * planted.units.json: every kitchen-sink record unchanged, plus the
 * hand-made records planted.test.ts names, and in place of the recorded
 * default target a declined one. `u8_after_include` holds the same six
 * fields with the [Install] drop-in of install-dropin-demo.service
 * included and that row's `declined` cleared, the view U8's mocked applyOp
 * returns. Built by a `python3 -I` script kept outside the repository that
 * reads the kitchen-sink file and the Rust source. Planted unit names are
 * synthetic (`*-demo`) except the source's own list entries. Where each
 * string came from:
 * - Field layout and triage of the planted records: the kitchen-sink rows
 *   of the same class (httpd.service for a vendor row,
 *   driftify-also-member.service for a local one, the nginx.service
 *   override.conf drop-in, wg-quick@wg0.service for an instance,
 *   getty@tty2.service for getty@.service, whose masked state and enable
 *   preset are bluetooth.service's).
 * - packagekit.service: the unit from INCOMPATIBLE_SERVICES
 *   (crates/core/src/baseline.rs), its package PackageKit the one
 *   crates/cli/tests/units_scan_test.rs gives packagekit-offline-update.service,
 *   the attention reason the slug
 *   normalize_incompatible_services sets (crates/refine/src/normalize.rs).
 * - dnf-automatic.timer: RUNTIME_UPDATERS and RUNTIME_UPDATER_REASON
 *   (crates/core/src/baseline.rs), its package dnf-automatic from
 *   crates/cli/tests/units_scan_test.rs.
 * - wg-quick@demo-a.service, wg-quick@demo-b.service: required_config from
 *   INSTANCE_CONFIG_TABLE (crates/collect/src/inspectors/units/instance.rs).
 * - install-dropin-demo.service's declined text: OmissionReason
 *   InstallEvidenceChanged's describe() inside omitted_line's format
 *   (crates/pipeline/src/render/unit_plan.rs, § 10.2 rule 5).
 * - The declined default target: DefaultTargetFileNotStaged's describe()
 *   through default_target_line (unit_plan.rs).
 * - The withheld drop-in's text: WithheldWhy::Conflict's describe() for a
 *   conflict at its own path (crates/pipeline/src/render/configtree.rs).
 * - The orphan drop-in's parent, orphan-demo: not a unit name, so the
 *   projection serves no `kind` (crates/refine/src/projection/decisions.rs).
 * - data-demo.mount's mount finding: mount_advisory_lines with
 *   MOUNT_ADVISORY_FACT and MOUNT_FILE_STAGED (unit_plan.rs).
 * - Link advisories: manual_link_lines with LINK_STATIC_UNIT,
 *   LINK_COMMAND_PREFIX, LINK_HOST_INSTALL_ONLY(_REST), ALIAS_FILES_COPIED,
 *   LINK_VENDOR_INSTALL_ONLY(_REST) and LINK_REMOVE_PREFIX (unit_plan.rs).
 *   The script's builder reproduces the kitchen-sink's driftify-static.service
 *   link exactly before it builds the planted ones.
 * - Undetermined installs: undetermined_install_lines with
 *   install_unknown_words and UNDETERMINED_CHECK (unit_plan.rs); the
 *   builder reproduces the kitchen-sink's driftify-upheld.service record
 *   exactly first.
 */
import type {
  DefaultTargetView,
  DropInDecisionDto,
  ManualLinkView,
  MountFindingDto,
  ServiceDecisionDto,
  UndeterminedInstallView,
} from "../../../api/types";

/** The six unit fields of the view response. Task 2.1 moves this type. */
export interface UnitFields {
  unit_states: ServiceDecisionDto[];
  unit_dropins: DropInDecisionDto[];
  default_target?: DefaultTargetView;
  mount_findings: MountFindingDto[];
  manual_links: ManualLinkView[];
  undetermined_installs: UndeterminedInstallView[];
}

export type UnitFixtureName = "kitchen-sink" | "planted" | "stock10" | "stock9";

/** planted.units.json's extra key: the U8 view after the include. */
const AFTER_INCLUDE_KEY = "u8_after_include";

const FIXTURES = import.meta.glob<{ default: unknown }>("./*.units.json", { eager: true });

/** A fresh copy of the fixture's top level, so a test may change it freely. */
function raw(name: UnitFixtureName): Record<string, unknown> {
  const entry = FIXTURES[`./${name}.units.json`];
  if (!entry) throw new Error(`no units fixture ${name}.units.json`);
  return JSON.parse(JSON.stringify(entry.default)) as Record<string, unknown>;
}

/** The six unit fields of a fixture (`planted` without its U8 after view). */
export function loadUnitFields(name: UnitFixtureName): UnitFields {
  const fields = raw(name);
  delete fields[AFTER_INCLUDE_KEY];
  return fields as unknown as UnitFields;
}

/** The six unit fields U8's mocked applyOp returns: planted, drop-in included. */
export function loadPlantedAfterInclude(): UnitFields {
  const after = raw("planted")[AFTER_INCLUDE_KEY];
  if (!after) throw new Error(`planted.units.json has no ${AFTER_INCLUDE_KEY}`);
  return after as UnitFields;
}
