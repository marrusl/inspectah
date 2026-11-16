/**
 * A declined generated unit is shown on the row that asked for it (spec
 * § 11.1, T21 refine half): a generated cron timer on its Scheduled Tasks
 * row, the flatpak provisioning service on the flatpak list. The text is
 * the server's, shown as given.
 */
import { describe, it, expect, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import { MainContent } from "../MainContent";
import type {
  DeclinedGeneratedView,
  FlatpakDecisionDto,
  ReferenceSection,
  ViewResponse,
} from "../../api/types";
import { emptyView } from "../../test-utils/emptyView";

vi.mock("../../api/client", () => ({
  ungroupGroup: vi.fn(),
  applyOp: vi.fn(),
}));

/** The text the server sends for a name collision, as the plan writes it. */
function collisionText(unit: string): string {
  return (
    `${unit} (inspectah generates this unit, and a unit collected from the ` +
    `host, ${unit}, has the name of one of its files, so the generated ` +
    "files are not written and the unit is not enabled here; to carry the " +
    "generated unit, exclude the collected unit and its file)"
  );
}

const TIMER_DECLINE: DeclinedGeneratedView = {
  row: { row: "scheduled", name: "cron-backup" },
  unit: "cron-backup.timer",
  declined: {
    renders: "fixme",
    reason: "GeneratedNameCollision",
    text: collisionText("cron-backup.timer"),
  },
};

const FLATPAK_DECLINE: DeclinedGeneratedView = {
  row: { row: "flatpak" },
  unit: "flatpak-provision.service",
  declined: {
    renders: "fixme",
    reason: "GeneratedNameCollision",
    text: collisionText("flatpak-provision.service"),
  },
};

/**
 * The Scheduled Tasks section as the server sends it: the collected
 * `cron-backup` timer, the generated `cron-backup` row of the same name
 * (each id is its kind and source), and another generated row.
 */
const SCHEDULED: ReferenceSection = {
  id: "scheduled_tasks",
  display_name: "Scheduled Tasks",
  items: [
    {
      id: "timer:cron-backup",
      title: "cron-backup",
      subtitle: "daily",
      detail: null,
      searchable_text: "cron-backup daily timer",
      unit: "cron-backup.timer",
      unit_row: true,
    },
    {
      id: "generated:0:cron-backup",
      title: "cron-backup",
      subtitle: "0 2 * * *",
      detail: null,
      searchable_text: "cron-backup 0 2 * * * generated-timer",
      generated_timer: "cron-backup",
    },
    {
      id: "generated:1:cron-cleanup",
      title: "cron-cleanup",
      subtitle: "0 3 * * *",
      detail: null,
      searchable_text: "cron-cleanup 0 3 * * * generated-timer",
      generated_timer: "cron-cleanup",
    },
  ],
};

const FLATPAK: FlatpakDecisionDto = {
  app_id: "org.gimp.GIMP",
  remote: "flathub",
  branch: "stable",
  triage: {
    triage: { mode: "single_host", site: null },
    primary_reason: "flatpak_provisioned_on_first_boot",
    annotations: [],
  },
  include: true,
  locked: false,
  lifecycle: "first_boot",
};

function renderSection(
  activeSection: string,
  viewData: ViewResponse,
  onNavigateToItem?: (sectionId: string, itemId: string) => void,
) {
  return render(
    <MainContent
      activeSection={activeSection}
      loading={false}
      viewData={viewData}
      sections={[SCHEDULED]}
      onViewUpdate={vi.fn()}
      onMutationError={vi.fn()}
      sectionSearchOpen={false}
      onSectionSearchClose={vi.fn()}
      onNavigateToItem={onNavigateToItem}
    />,
  );
}

describe("declined generated unit on its scheduled row", () => {
  it("shows the server's text on the generated row only", () => {
    renderSection(
      "scheduled_tasks",
      emptyView({ declined_generated: [TIMER_DECLINE] }),
      vi.fn(),
    );
    const generated = screen.getByTestId(
      "context-item-generated:0:cron-backup",
    );
    const collected = screen.getByTestId("context-item-timer:cron-backup");
    const notes = screen.getAllByText(collisionText("cron-backup.timer"));
    expect(notes).toHaveLength(1);
    expect(notes[0]).toHaveClass("inspectah-unit-declined--fixme");
    // The note is under the generated row, not the collected timer of the
    // same name.
    expect(generated).toContainElement(notes[0]);
    expect(collected).not.toContainElement(notes[0]);
    // Control: another generated row in the same section has no note.
    expect(
      screen.getByTestId("context-item-generated:1:cron-cleanup").textContent,
    ).not.toContain("inspectah generates this unit");
  });

  it("shows no note when nothing is declined", () => {
    renderSection("scheduled_tasks", emptyView(), vi.fn());
    expect(
      screen.getByTestId("context-item-generated:0:cron-backup"),
    ).toBeInTheDocument();
    expect(screen.queryByText(/inspectah generates this unit/)).toBeNull();
  });
});

describe("declined flatpak provisioning service", () => {
  it("shows the server's text on the flatpak list", () => {
    renderSection(
      "containers",
      emptyView({ flatpaks: [FLATPAK], declined_generated: [FLATPAK_DECLINE] }),
    );
    const note = screen.getByTestId("declined-generated-flatpak");
    expect(note).toHaveTextContent(collisionText("flatpak-provision.service"));
    expect(note).toHaveClass("inspectah-unit-declined--fixme");
    expect(screen.getByTestId("flatpak-item-org.gimp.GIMP")).toHaveAttribute(
      "aria-describedby",
      note.id,
    );
  });

  it("shows no note when the service is not declined", () => {
    renderSection("containers", emptyView({ flatpaks: [FLATPAK] }));
    expect(
      screen.getByTestId("flatpak-item-org.gimp.GIMP"),
    ).toBeInTheDocument();
    expect(screen.queryByTestId("declined-generated-flatpak")).toBeNull();
    expect(
      screen.getByTestId("flatpak-item-org.gimp.GIMP"),
    ).not.toHaveAttribute("aria-describedby");
  });
});

describe("scheduled timer unit link without navigation", () => {
  it("names the unit as text when the view cannot navigate", () => {
    renderSection("scheduled_tasks", emptyView());
    const row = screen.getByTestId("context-item-timer:cron-backup");
    expect(within(row).getByText("cron-backup.timer")).toBeInTheDocument();
    expect(within(row).queryByRole("link")).toBeNull();
  });

  it("links the unit when the view can navigate", () => {
    renderSection("scheduled_tasks", emptyView(), vi.fn());
    const row = screen.getByTestId("context-item-timer:cron-backup");
    expect(
      within(row).getByRole("link", { name: "cron-backup.timer" }),
    ).toBeInTheDocument();
  });
});
