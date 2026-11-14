import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import { DecisionItem } from "../DecisionItem";
import type { RefinedConfig, TriageTag, WithheldView } from "../../api/types";

// A config row whose file the export does not write says why, with the
// server's text as given (spec § 11.1).

const TRIAGE: TriageTag = {
  triage: { mode: "single_host", site: null },
  primary_reason: "config_unowned",
  annotations: [],
};

function config(path: string, withheld?: WithheldView): RefinedConfig {
  return {
    entry: {
      path,
      kind: "unowned",
      category: "other",
      content: "a=1\n",
      rpm_va_flags: null,
      package: null,
      diff_against_rpm: null,
      disposition: { kind: "actionable", include: true },
      tie: false,
      tie_winner: false,
      aggregate: null,
    },
    triage: TRIAGE,
    withheld,
  };
}

function renderRow(cfg: RefinedConfig) {
  render(
    <DecisionItem
      item={{ type: "config", data: cfg }}
      triageTag={TRIAGE}
      rowIndex={1}
      isViewed={false}
      isPending={false}
      onToggleInclude={vi.fn()}
      onMarkViewed={vi.fn()}
    />,
  );
}

const CONFLICT =
  "/etc/app/conf/x is also staged, and one of the two paths is a directory of the other; exclude one of the two";

describe("DecisionItem: a withheld config row", () => {
  it("renders the withheld reason on the row, tied to it", () => {
    renderRow(config("/etc/app/conf", { outcome: "withheld", why: "Conflict", text: CONFLICT }));
    const reason = screen.getByTestId("withheld-configs:/etc/app/conf");
    expect(reason.textContent).toBe(CONFLICT);
    const row = screen.getByTestId("decision-item-configs:/etc/app/conf");
    const ids = (row.getAttribute("aria-describedby") ?? "").split(/\s+/);
    expect(ids).toContain(reason.id);
  });

  it("renders a refused offer's text the same way", () => {
    renderRow(
      config("/etc/app/../x", {
        outcome: "refused",
        rule: "Traversal",
        text: "path traversal detected",
      }),
    );
    expect(screen.getByTestId("withheld-configs:/etc/app/../x").textContent).toBe(
      "path traversal detected",
    );
  });

  it("renders nothing for a row the export writes", () => {
    renderRow(config("/etc/app/written.conf"));
    expect(screen.getByTestId("decision-item-configs:/etc/app/written.conf")).not.toHaveAttribute(
      "aria-describedby",
    );
    expect(
      screen.queryByTestId("withheld-configs:/etc/app/written.conf"),
    ).not.toBeInTheDocument();
  });

  it("shows markup characters in the reason literally", () => {
    const hostile = "<b>bold</b> & co";
    renderRow(
      config("/etc/app/conf", { outcome: "withheld", why: "Conflict", text: hostile }),
    );
    const reason = screen.getByTestId("withheld-configs:/etc/app/conf");
    expect(reason.textContent).toBe(hostile);
    expect(reason.querySelector("b")).toBeNull();
  });
});
