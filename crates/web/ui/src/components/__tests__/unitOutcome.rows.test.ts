/**
 * What each units row shows, decided on the model before anything renders
 * it (spec § 6.5, § 6.6 items 2 to 7, 9, 10 and 12, § 6.7, § 6.9 rows 1
 * and 2, § 9.3).
 */
import { describe, it, expect } from "vitest";
import type {
  DeclinedView,
  DropInDecisionDto,
  ManualLinkView,
  MountFindingDto,
  ServiceDecisionDto,
  TriageTag,
  UndeterminedInstallView,
} from "../../api/types";
import {
  GROUP_DESCRIPTIONS,
  QUIET_HOST_SENTENCES,
  causeName,
  defaultTargetPresentation,
  dropinRowPresentation,
  keyFields,
  outcomeLabel,
  reportedRowLabels,
  unitRowKeySet,
  unitRowPresentation,
  type UnitRowContext,
} from "../unitOutcome";
import { loadUnitFields } from "../../__tests__/fixtures/systemd-units/load";

const kitchenSink = loadUnitFields("kitchen-sink");
const planted = loadUnitFields("planted");

const httpd = kitchenSink.unit_states.find((r) => r.unit === "httpd.service")!;
const httpdDropin = kitchenSink.unit_dropins.find(
  (d) => d.path === "/etc/systemd/system/httpd.service.d/override.conf",
)!;

/** A triage tag in a bucket, with a primary reason. */
function triage(bucket: string, primary_reason: string): TriageTag {
  return {
    triage: { mode: "single_host", [bucket]: null },
    primary_reason,
    annotations: [],
  } as unknown as TriageTag;
}

const UNKNOWN_ORIGIN = triage("investigate", "service_unknown_origin");
const NON_DEFAULT_STATE = triage("investigate", "service_non_default_state");
const ROUTINE = triage("baseline", "service_unknown_origin");

function unit(over: Partial<ServiceDecisionDto>): ServiceDecisionDto {
  return { ...httpd, ...over };
}

function dropin(over: Partial<DropInDecisionDto>): DropInDecisionDto {
  return { ...httpdDropin, ...over };
}

const NO_KEYS: ReadonlySet<string> = new Set();
const NO_PATHS: ReadonlySet<string> = new Set();

function ctx(over: Partial<UnitRowContext> = {}): UnitRowContext {
  return { groupId: "services", unitRowKeys: NO_KEYS, configPaths: NO_PATHS, ...over };
}

const DECLINED: Record<DeclinedView["renders"], DeclinedView> = {
  fixme: { renders: "fixme", reason: "Test", text: "a fixme reason" },
  advisory: { renders: "advisory", reason: "Test", text: "an advisory reason" },
  omitted: { renders: "omitted", reason: "Test", text: "an omitted reason" },
};

describe("the outcome label (§ 6.6 item 2, string 15)", () => {
  it.each<[DeclinedView["renders"], string, string]>([
    ["fixme", "FIXME", "orange"],
    ["advisory", "Advisory", "blue"],
    ["omitted", "Omitted", "grey"],
  ])("labels %s %s in %s", (renders, text, color) => {
    expect(outcomeLabel(DECLINED[renders])).toEqual({ text, color });
  });

  it("gives a unit row and the default target the label of their declined", () => {
    expect(unitRowPresentation(unit({ declined: DECLINED.fixme }), ctx()).outcome).toEqual({
      text: "FIXME",
      color: "orange",
    });
    expect(unitRowPresentation(unit({ declined: undefined }), ctx()).outcome).toBeNull();
    expect(defaultTargetPresentation(planted.default_target!).outcome).toEqual({
      text: "Omitted",
      color: "grey",
    });
    expect(defaultTargetPresentation(kitchenSink.default_target!).outcome).toBeNull();
  });
});

describe("a unit row's triage badge (§ 6.6 item 3)", () => {
  it("shows no badge for the non-default state reason", () => {
    expect(unitRowPresentation(unit({ triage: NON_DEFAULT_STATE }), ctx()).triageBadge).toBeNull();
  });

  it("shows any other reason as before, coloured by its level", () => {
    expect(unitRowPresentation(unit({ triage: UNKNOWN_ORIGIN }), ctx()).triageBadge).toEqual({
      text: "Unknown Origin",
      color: "red",
    });
  });

  it("shows no badge for a routine reason, nor on a locked row", () => {
    expect(unitRowPresentation(unit({ triage: ROUTINE }), ctx()).triageBadge).toBeNull();
    expect(
      unitRowPresentation(unit({ triage: UNKNOWN_ORIGIN, locked: true }), ctx()).triageBadge,
    ).toBeNull();
  });
});

describe("one reason per row (§ 6.6 item 4)", () => {
  it("shows the declined text and not the attention reason on a declined row", () => {
    const p = unitRowPresentation(unit({ declined: DECLINED.advisory, attention_reason: "attention" }), ctx());
    expect(p.attentionNote).toBeNull();
  });

  it("shows an unlocked, undeclined row's attention reason", () => {
    expect(unitRowPresentation(unit({ attention_reason: "attention" }), ctx()).attentionNote).toBe(
      "attention",
    );
    expect(unitRowPresentation(unit({ attention_reason: null }), ctx()).attentionNote).toBeNull();
  });

  it("keeps a locked row's reason in its badge, not under the row", () => {
    const p = unitRowPresentation(unit({ locked: true, attention_reason: "a prose reason" }), ctx());
    expect(p.attentionNote).toBeNull();
    expect(p.lockedLabel).toBe("a prose reason");
  });
});

describe("a locked unit row's label (§ 9.3, string 24)", () => {
  it("maps the slug to its words, in the badge and the checkbox name", () => {
    const packagekit = planted.unit_states.find((r) => r.unit === "packagekit.service")!;
    const p = unitRowPresentation(packagekit, ctx());
    expect(p.locked).toBe(true);
    expect(p.toggleable).toBe(false);
    expect(p.lockedLabel).toBe("Incompatible with image mode");
    expect(p.toggleLabel).toBe("packagekit.service (locked: Incompatible with image mode)");
  });

  it("shows LOCKED in the badge and cannot toggle in the name with no reason", () => {
    const p = unitRowPresentation(unit({ locked: true, attention_reason: null }), ctx());
    expect(p.lockedLabel).toBe("LOCKED");
    expect(p.toggleLabel).toBe("httpd.service (locked: cannot toggle)");
  });

  it("names an unlocked row Toggle <unit>, with no locked label", () => {
    const p = unitRowPresentation(unit({ locked: false }), ctx());
    expect(p.locked).toBe(false);
    expect(p.toggleable).toBe(true);
    expect(p.lockedLabel).toBeNull();
    expect(p.toggleLabel).toBe("Toggle httpd.service");
  });
});

describe("Also= members link to their own row (§ 6.6 item 5)", () => {
  it("links a member with a unit row and leaves one without as code", () => {
    const cups = kitchenSink.unit_states.find((r) => r.unit === "cups.service")!;
    const p = unitRowPresentation(cups, ctx({ unitRowKeys: unitRowKeySet(keyFields(kitchenSink)) }));
    expect(p.alsoMembers).toEqual([
      { unit: "cups.socket", links: cups.also![0].links, linked: true },
      { unit: "cups.path", links: cups.also![1].links, linked: false },
    ]);
  });

  it("holds every unit row's key, and no other row's", () => {
    const keyed = keyFields(planted);
    const keys = unitRowKeySet(keyed);
    expect([...keys].sort()).toEqual([...new Set(keyed.unitStates.map((k) => k.rowKey))].sort());
  });
});

describe("required_config paths link to their config row (§ 6.6 item 7)", () => {
  it("links a path config_files holds, leaves the rest as code, and data paths plain", () => {
    const p = unitRowPresentation(
      unit({ required_config: ["/etc/a.conf", "/etc/b.conf"], data_paths: ["/etc/a.conf", "/var/lib/x"] }),
      ctx({ configPaths: new Set(["/etc/a.conf"]) }),
    );
    expect(p.requiredConfig).toEqual([
      { path: "/etc/a.conf", linked: true },
      { path: "/etc/b.conf", linked: false },
    ]);
    expect(p.dataPaths).toEqual([
      { path: "/etc/a.conf", linked: false },
      { path: "/var/lib/x", linked: false },
    ]);
  });
});

describe("the package word (§ 6.6 item 9)", () => {
  it("shows the package, else the fragment word", () => {
    expect(unitRowPresentation(unit({ owning_package: "httpd" }), ctx()).packageWord).toBe("httpd");
    expect(
      unitRowPresentation(unit({ owning_package: null, fragment: "shadowed" }), ctx()).packageWord,
    ).toBe("shadowed");
  });
});

describe("the kind chip (§ 6.6 item 10)", () => {
  it("shows the kind in Other units only", () => {
    const slice = unit({ unit: "limits-demo.slice", kind: "slice" });
    expect(unitRowPresentation(slice, ctx({ groupId: "other_units" })).kindChip).toBe("slice");
    for (const groupId of ["services", "sockets", "timers", "paths"] as const) {
      expect(unitRowPresentation(slice, ctx({ groupId })).kindChip, groupId).toBeNull();
    }
  });
});

describe("a drop-in row (§ 6.6 items 3 and 6, string 23)", () => {
  const parent = unit({ include: true, locked: false });

  it("shows Unit excluded while its parent is excluded, with no triage badge, and cannot toggle", () => {
    const p = dropinRowPresentation(dropin({ triage: UNKNOWN_ORIGIN }), unit({ include: false }));
    expect(p.parentExcluded).toBe(true);
    expect(p.unitExcluded).toBe(true);
    expect(p.triageBadge).toBeNull();
    expect(p.toggleable).toBe(false);
  });

  it("keeps its triage badge under an included parent, even the non-default state reason", () => {
    const p = dropinRowPresentation(dropin({ triage: UNKNOWN_ORIGIN }), parent);
    expect(p.unitExcluded).toBe(false);
    expect(p.triageBadge).toEqual({ text: "Unknown Origin", color: "red" });
    expect(dropinRowPresentation(dropin({ triage: ROUTINE }), parent).triageBadge).toBeNull();
    expect(dropinRowPresentation(dropin({ triage: NON_DEFAULT_STATE }), parent).triageBadge).not.toBeNull();
  });

  it("is locked by its own lock or its parent's, showing its reason as given", () => {
    const own = dropinRowPresentation(dropin({ locked: true, attention_reason: "a prose reason" }), parent);
    expect(own.locked).toBe(true);
    expect(own.toggleable).toBe(false);
    expect(own.lockedLabel).toBe("a prose reason");
    expect(own.toggleLabel).toBe(`${httpdDropin.path} (locked: a prose reason)`);
    const byParent = dropinRowPresentation(dropin({ locked: false, attention_reason: null }), unit({ locked: true }));
    expect(byParent.locked).toBe(true);
    expect(byParent.lockedLabel).toBe("LOCKED");
    expect(byParent.toggleLabel).toBe(`${httpdDropin.path} (locked: cannot toggle)`);
  });

  it("shows no Unit excluded and no triage badge on a locked drop-in under an excluded parent", () => {
    const p = dropinRowPresentation(dropin({ locked: true, triage: UNKNOWN_ORIGIN }), unit({ include: false }));
    expect(p.unitExcluded).toBe(false);
    expect(p.triageBadge).toBeNull();
  });

  it("names an unlocked drop-in Toggle <path>", () => {
    const p = dropinRowPresentation(dropin({}), parent);
    expect(p.toggleable).toBe(true);
    expect(p.lockedLabel).toBeNull();
    expect(p.toggleLabel).toBe(`Toggle ${httpdDropin.path}`);
  });

  it("marks one of its parent's install drop-ins, and the full shadow with its rationale", () => {
    const path = "/etc/systemd/system/install-dropin-demo.service.d/install.conf";
    const installParent = planted.unit_states.find((r) => r.unit === "install-dropin-demo.service")!;
    expect(dropinRowPresentation(dropin({ path }), installParent).installDropIn).toBe(true);
    expect(dropinRowPresentation(dropin({}), installParent).installDropIn).toBe(false);
    const shadow = kitchenSink.unit_dropins.find((d) => d.shadow_type === "full_shadow")!;
    const p = dropinRowPresentation(shadow, null);
    expect(p.fullShadow).toBe(true);
    expect(p.shadowRationale).toBe(shadow.shadow_rationale);
    expect(dropinRowPresentation(dropin({ shadow_type: "full_shadow", shadow_rationale: null }), parent))
      .toMatchObject({ fullShadow: true, shadowRationale: null });
    expect(dropinRowPresentation(dropin({}), parent)).toMatchObject({ fullShadow: false, shadowRationale: null });
  });

  it("treats an orphan drop-in as under no excluded or locked parent", () => {
    const p = dropinRowPresentation(dropin({ triage: UNKNOWN_ORIGIN }), null);
    expect(p).toMatchObject({
      parentExcluded: false,
      unitExcluded: false,
      locked: false,
      toggleable: true,
      installDropIn: false,
      triageBadge: { text: "Unknown Origin", color: "red" },
    });
  });
});

describe("the default target (§ 6.5)", () => {
  it("names its checkbox Toggle <target>, or <target> (locked) when locked", () => {
    const target = kitchenSink.default_target!;
    expect(defaultTargetPresentation(target)).toMatchObject({
      toggleLabel: "Toggle graphical.target",
      toggleable: true,
      locked: false,
    });
    expect(defaultTargetPresentation({ ...target, locked: true })).toMatchObject({
      toggleLabel: "graphical.target (locked)",
      toggleable: false,
      locked: true,
    });
  });
});

describe("a reported row's words (§ 6.6 item 12, § 6.7, strings 17 and 18)", () => {
  const link: ManualLinkView = {
    unit: "a.service",
    relation: "upholds",
    parent: "b.target",
    cause: "static_unit",
    advisories: [],
  };
  const undetermined: UndeterminedInstallView = {
    unit: "c.service",
    reason: "upheld_by",
    install_files: [],
    advisories: [],
  } as UndeterminedInstallView;
  const mount = planted.mount_findings[0] as MountFindingDto;

  it("gives a link its relation phrase and its cause label", () => {
    expect(reportedRowLabels({ kind: "manual_link", data: link })).toEqual({
      detail: "upheld by",
      cause: "Link its [Install] does not make",
    });
  });

  it("gives an undetermined install its reason words and the undetermined label", () => {
    expect(reportedRowLabels({ kind: "undetermined_install", data: undetermined })).toEqual({
      detail: "upheld by",
      cause: "[Install] cannot be relied on",
    });
  });

  it("gives a mount finding its kind and no cause", () => {
    expect(reportedRowLabels({ kind: "mount_finding", data: mount })).toEqual({
      detail: mount.kind,
      cause: null,
    });
  });

  it("names a link's cause by its wire word, undetermined for the object form", () => {
    expect(causeName("static_unit")).toBe("static_unit");
    expect(causeName({ undetermined: { reason: "upheld_by" } } as never)).toBe("undetermined");
  });
});

describe("group descriptions and quiet-host sentences (§ 6.7, § 6.9 rows 1 and 2, strings 6, 12, 13)", () => {
  it("describes Reported, not carried, and no other group", () => {
    expect(GROUP_DESCRIPTIONS).toEqual({
      reported:
        "inspectah reports these and does not carry them. No toggle: the lines under each row are the Containerfile's comments for it.",
    });
  });

  it("gives each quiet host its sentence", () => {
    expect(QUIET_HOST_SENTENCES).toEqual({
      with_target: "No other unit differs from its preset, and no drop-in was found.",
      without_target: "No unit differs from its preset, and no drop-in was found.",
    });
  });
});
