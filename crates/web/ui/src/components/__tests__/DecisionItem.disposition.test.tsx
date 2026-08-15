import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { DecisionItem } from "../DecisionItem";
import type { DecisionItemKind } from "../DecisionItem";
import { isIncluded, isToggleable } from "../../api/disposition";
import type {
  Disposition,
  RefinedConfig,
  RefinedPackage,
  TriageTag,
} from "../../api/types";

const TRIAGE: TriageTag = {
  triage: { mode: "single_host", site: null },
  primary_reason: "config_unowned",
  annotations: [],
};

const MODERNIZATION_RATIONALE = "sysvinit script — port to a systemd unit";
const ADVISORY_PATH = "/etc/init.d/legacy-app";

function config(disposition: Disposition, path = ADVISORY_PATH): RefinedConfig {
  return {
    entry: {
      path,
      kind: "unowned",
      category: "other",
      content: "",
      rpm_va_flags: null,
      package: null,
      diff_against_rpm: null,
      disposition,
      tie: false,
      tie_winner: false,
      aggregate: null,
    },
    triage: TRIAGE,
  };
}

function pkg(disposition: Disposition): RefinedPackage {
  return {
    entry: {
      name: "httpd",
      epoch: "0",
      version: "2.4.57",
      release: "1.el9",
      arch: "x86_64",
      state: "added",
      disposition,
      source_repo: "appstream",
      aggregate: null,
    },
    triage: TRIAGE,
  };
}

const ADVISORY: Disposition = {
  kind: "advisory",
  advisory_type: "modernization",
  rationale: MODERNIZATION_RATIONALE,
};
const INVENTORY: Disposition = { kind: "inventory" };
const INCLUDED: Disposition = { kind: "actionable", include: true };
const EXCLUDED: Disposition = { kind: "actionable", include: false };

function renderItem(item: DecisionItemKind, onToggleInclude = vi.fn()) {
  render(
    <DecisionItem
      item={item}
      triageTag={TRIAGE}
      rowIndex={1}
      isViewed={false}
      isPending={false}
      onToggleInclude={onToggleInclude}
      onMarkViewed={vi.fn()}
    />,
  );
  return onToggleInclude;
}

describe("disposition predicates", () => {
  // These mirror FindingKind::is_included() in inspectah-core. An advisory or
  // inventory finding carries no `include` key at all, and the old `?? true`
  // read turned that absence into "the user chose to bake this in".
  it("counts only an actionable include as included", () => {
    expect(isIncluded(INCLUDED)).toBe(true);
    expect(isIncluded(EXCLUDED)).toBe(false);
    expect(isIncluded(ADVISORY)).toBe(false);
    expect(isIncluded(INVENTORY)).toBe(false);
  });

  it("counts only actionable findings as toggleable", () => {
    expect(isToggleable(INCLUDED)).toBe(true);
    expect(isToggleable(EXCLUDED)).toBe(true);
    expect(isToggleable(ADVISORY)).toBe(false);
    expect(isToggleable(INVENTORY)).toBe(false);
  });
});

describe("DecisionItem renders the disposition it was given", () => {
  beforeEach(() => {
    vi.stubGlobal("fetch", vi.fn());
  });
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("gives an actionable finding a checkbox reflecting its include flag", () => {
    renderItem({ type: "config", data: config(INCLUDED) });
    const box = screen.getByRole("checkbox") as HTMLInputElement;
    expect(box.checked).toBe(true);
  });

  it("offers no toggle for an advisory finding", () => {
    renderItem({ type: "config", data: config(ADVISORY) });
    // The server refuses SetInclude on an advisory, so a checkbox here can
    // only ever be a control that lies about what it does.
    expect(screen.queryByRole("checkbox")).toBeNull();
  });

  it("names the advisory type and shows its rationale", () => {
    renderItem({ type: "config", data: config(ADVISORY) });
    const row = screen.getByTestId(`decision-item-configs:${ADVISORY_PATH}`);
    expect(row.getAttribute("data-disposition")).toBe("advisory");
    expect(screen.getByTestId("advisory-badge")).toHaveTextContent(
      /modernization/i,
    );
    expect(screen.getByText(MODERNIZATION_RATIONALE)).toBeInTheDocument();
  });

  it("offers no toggle for an inventory finding", () => {
    renderItem({ type: "config", data: config(INVENTORY) });
    expect(screen.queryByRole("checkbox")).toBeNull();
    const row = screen.getByTestId(`decision-item-configs:${ADVISORY_PATH}`);
    expect(row.getAttribute("data-disposition")).toBe("inventory");
  });

  it("does not render an advisory as an included item", () => {
    // The bug this replaces: `disposition?.include ?? true` made every
    // advisory look like a box the user had ticked.
    renderItem({ type: "config", data: config(ADVISORY) });
    expect(screen.queryByRole("checkbox")).toBeNull();
    expect(screen.queryByTestId("advisory-badge")).not.toBeNull();
  });

  it("ignores the toggle keys on a non-toggleable finding", async () => {
    const user = userEvent.setup();
    const onToggle = renderItem({ type: "config", data: config(ADVISORY) });
    const row = screen.getByTestId(`decision-item-configs:${ADVISORY_PATH}`);
    row.focus();
    await user.keyboard(" ");
    await user.keyboard("x");
    expect(onToggle).not.toHaveBeenCalled();
  });

  it("still toggles an actionable finding from the keyboard", async () => {
    const user = userEvent.setup();
    const onToggle = renderItem({ type: "package", data: pkg(INCLUDED) });
    const row = screen.getByTestId("decision-item-packages:httpd.x86_64");
    row.focus();
    await user.keyboard(" ");
    expect(onToggle).toHaveBeenCalledWith({
      op: "SetInclude",
      target: {
        item_id: { kind: "Package", key: { name: "httpd", arch: "x86_64" } },
        include: false,
      },
    });
  });
});
