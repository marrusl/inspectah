import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
// Rendered with a real pending guard, as its owner supplies one in the app.
import { ServiceSectionWithGuard as ServiceSection } from "../../test-utils/withPendingGuard";
import type {
  DefaultTargetView,
  DropInDecisionDto,
  ManualLinkView,
  MountFindingDto,
  ServiceDecisionDto,
  TriageTag,
  UndeterminedInstallView,
} from "../../api/types";
import { emptyView } from "../../test-utils/emptyView";

// Hand-made links, `UndeterminedInstall` records, `Also=` members and the
// default target in the units list (spec § 11.1). Every advisory and
// omission line is the server's: the list shows it as given.

const mockFetch = vi.fn();
beforeEach(() => {
  mockFetch.mockReset();
  vi.stubGlobal("fetch", mockFetch);
});
afterEach(() => {
  vi.restoreAllMocks();
});

const TRIAGE: TriageTag = {
  triage: { mode: "single_host", site: null },
  primary_reason: "service_non_default_state",
  annotations: [],
};

function makeUnit(overrides: Partial<ServiceDecisionDto> = {}): ServiceDecisionDto {
  return {
    unit: "httpd.service",
    kind: "service",
    fragment: "vendor",
    triage: TRIAGE,
    include: true,
    locked: false,
    current_state: "enabled",
    required_config: [],
    data_paths: [],
    ...overrides,
  };
}

function makeDropin(overrides: Partial<DropInDecisionDto> = {}): DropInDecisionDto {
  return {
    unit: "app.service",
    path: "/etc/systemd/system/app.service.d/10-install.conf",
    triage: TRIAGE,
    include: true,
    locked: false,
    kind: "service",
    ...overrides,
  };
}

const STATIC_LINES = [
  "# Advisory: multi-user.target.wants/backup.service is a link on the source host that backup.service's [Install] section there does not make (it names no link: none, only Also=, or a list a drop-in reset to empty). Not carried.",
  "#   To carry it, add after the configuration COPY: RUN systemctl add-wants -- multi-user.target backup.service",
];

const ALSO_LINES = [
  "# Advisory: app.service's package unit names helper.service in Also=, and its files under /etc/systemd/system do not. The package install in the image acted on the package's list.",
  "#   Check helper.service with systemctl is-enabled in the image.",
];

const ADD = "/etc/systemd/system/app.service.d/20-add.conf";
const OMITTED_LINE = `# Omitted: app.service links (excluded from the export: ${ADD}; the [Install] the image reads is not the one recorded, so the source host's links for app.service are not reproduced, and links its package install made may remain in the image. Check them with systemctl is-enabled in the image.)`;

function makeLink(overrides: Partial<ManualLinkView> = {}): ManualLinkView {
  return {
    relation: "wants",
    parent: "multi-user.target",
    unit: "backup.service",
    cause: "static_unit",
    advisories: STATIC_LINES,
    ...overrides,
  };
}

const NEED_DROP_IN = "/etc/systemd/system/need.service.d/10-reset.conf";
const UNDETERMINED_LINES = [
  `# Advisory: need.service is not carried: its [Install] section on the source host cannot be relied on (UpheldBy=, which systemctl enable on systemd 254 or later turns into an .upholds link), and these files under /etc/systemd/system change it: ${NEED_DROP_IN}.`,
  "#   The image's links for need.service can differ from the source host's, including links its package install made before these files are copied. Check them with systemctl is-enabled in the image.",
];

function makeUndetermined(
  overrides: Partial<UndeterminedInstallView> = {},
): UndeterminedInstallView {
  return {
    unit: "need.service",
    reason: "upheld_by",
    install_files: [NEED_DROP_IN],
    advisories: UNDETERMINED_LINES,
    ...overrides,
  };
}

function makeTarget(overrides: Partial<DefaultTargetView> = {}): DefaultTargetView {
  return {
    target: "graphical.target",
    include: true,
    locked: false,
    declined: null,
    ...overrides,
  };
}

const baseProps = {
  services: [] as ServiceDecisionDto[],
  dropins: [] as DropInDecisionDto[],
  mountFindings: [] as MountFindingDto[],
  manualLinks: [] as ManualLinkView[],
  undeterminedInstalls: [] as UndeterminedInstallView[],
  defaultTarget: undefined as DefaultTargetView | undefined,
  onViewUpdate: vi.fn(),
  onMutationError: vi.fn(),
};

/** The ids an element's `aria-describedby` names, each resolved to text. */
function describedBy(el: HTMLElement): string[] {
  const ids = (el.getAttribute("aria-describedby") ?? "").split(/\s+/).filter(Boolean);
  return ids.map((id) => {
    const target = document.getElementById(id);
    expect(target, `aria-describedby names a missing id ${id}`).not.toBeNull();
    return target!.textContent ?? "";
  });
}

function linkRow(link: ManualLinkView): HTMLElement {
  const identity = JSON.stringify([link.relation, link.parent, link.unit]);
  return screen.getByTestId(`manual-link-${identity}`);
}

describe("ServiceSection: hand-made links", () => {
  it("renders a link as a row with its advisories and no toggle", () => {
    const link = makeLink();
    render(
      <ServiceSection {...baseProps} services={[makeUnit()]} manualLinks={[link]} />,
    );
    const row = linkRow(link);
    // Presence first: the unit row has a toggle, the link row none.
    expect(
      within(screen.getByTestId("service-item-httpd.service")).getByRole("checkbox"),
    ).toBeInTheDocument();
    expect(within(row).queryByRole("checkbox")).not.toBeInTheDocument();
    expect(row).toHaveAttribute("tabindex", "-1");
    expect(row).toHaveAttribute("data-cause", "static_unit");
    expect(within(row).getByTestId("manual-link-relation")).toHaveTextContent(/^wanted by$/);
    expect(within(row).getByTestId("manual-link-parent")).toHaveTextContent(
      /^multi-user\.target$/,
    );
    const lines = within(row)
      .getAllByTestId("manual-link-advisory")
      .map((l) => l.textContent);
    expect(lines).toEqual(STATIC_LINES);
    expect(describedBy(row)).toEqual([STATIC_LINES.join("")]);
    expect(within(row).queryByTestId("manual-link-not-applied")).not.toBeInTheDocument();
  });

  it("renders the also relation and the two install-difference causes", () => {
    const also = makeLink({
      relation: "also",
      parent: "app.service",
      unit: "helper.service",
      cause: "vendor_install_only",
      advisories: ALSO_LINES,
    });
    const hostOnly = makeLink({
      parent: "graphical.target",
      unit: "app.service",
      cause: "host_install_only",
      advisories: ["# Advisory: graphical.target.wants/app.service is on the source host"],
    });
    const vendorOnly = makeLink({
      unit: "app.service",
      cause: "vendor_install_only",
      advisories: ["# Advisory: multi-user.target.wants/app.service is not on the source host"],
    });
    render(<ServiceSection {...baseProps} manualLinks={[also, hostOnly, vendorOnly]} />);

    const alsoRow = linkRow(also);
    expect(alsoRow).toHaveAttribute("data-cause", "vendor_install_only");
    expect(within(alsoRow).getByTestId("manual-link-relation")).toHaveTextContent(
      /^named in Also= by$/,
    );
    expect(within(alsoRow).getByTestId("manual-link-parent")).toHaveTextContent(
      /^app\.service$/,
    );
    expect(
      within(alsoRow)
        .getAllByTestId("manual-link-advisory")
        .map((l) => l.textContent),
    ).toEqual(ALSO_LINES);
    expect(linkRow(hostOnly)).toHaveAttribute("data-cause", "host_install_only");
    expect(linkRow(vendorOnly)).toHaveAttribute("data-cause", "vendor_install_only");
    for (const row of [alsoRow, linkRow(hostOnly), linkRow(vendorOnly)]) {
      expect(within(row).queryByRole("checkbox")).not.toBeInTheDocument();
    }
    // Control: the section is not shown as empty.
    expect(screen.queryByText(/no service state changes/i)).not.toBeInTheDocument();
  });

  it("reads the undetermined cause, which is served as an object", () => {
    const link = makeLink({
      unit: "need.service",
      cause: { undetermined: { reason: "upheld_by" } },
    });
    render(<ServiceSection {...baseProps} manualLinks={[link]} />);
    const row = linkRow(link);
    expect(row).toHaveAttribute("data-cause", "undetermined");
    expect(within(row).getAllByTestId("manual-link-advisory")).toHaveLength(2);
  });

  it("shows a not_applied row's excluded paths and the omission line", () => {
    const link = makeLink({
      parent: "graphical.target",
      unit: "app.service",
      cause: "host_install_only",
      advisories: [OMITTED_LINE],
      not_applied: [ADD],
    });
    render(<ServiceSection {...baseProps} manualLinks={[link]} />);
    const row = linkRow(link);
    const notApplied = within(row).getByTestId("manual-link-not-applied");
    expect(
      within(notApplied)
        .getAllByTestId("not-applied-path")
        .map((p) => p.textContent),
    ).toEqual([ADD]);
    expect(
      within(row)
        .getAllByTestId("manual-link-advisory")
        .map((l) => l.textContent),
    ).toEqual([OMITTED_LINE]);
    const described = describedBy(row);
    expect(described[0]).toBe(OMITTED_LINE);
    expect(described[1]).toContain(ADD);
  });

  it("gives two links a test id each when their names hold dashes", () => {
    // Joined with "-", both triples read wants-a.service-b.service-c.service.
    const first = makeLink({ parent: "a.service", unit: "b.service-c.service" });
    const second = makeLink({ parent: "a.service-b.service", unit: "c.service" });
    render(<ServiceSection {...baseProps} manualLinks={[first, second]} />);
    const rows = screen
      .getByTestId("service-section")
      .querySelectorAll(".inspectah-manual-link");
    expect(rows).toHaveLength(2);
    const ids = [...rows].map((r) => r.getAttribute("data-testid"));
    expect(new Set(ids).size).toBe(2);
    expect(linkRow(first)).not.toBe(linkRow(second));
  });

  it("does not toggle a link from the keyboard", async () => {
    const link = makeLink();
    render(<ServiceSection {...baseProps} manualLinks={[link]} />);
    linkRow(link).focus();
    await userEvent.keyboard(" x");
    expect(mockFetch).not.toHaveBeenCalled();
  });
});

describe("ServiceSection: undetermined installs", () => {
  it("renders the record with its reason, files and block, and no toggle", () => {
    render(
      <ServiceSection
        {...baseProps}
        undeterminedInstalls={[makeUndetermined()]}
      />,
    );
    const row = screen.getByTestId("undetermined-install-need.service");
    expect(within(row).queryByRole("checkbox")).not.toBeInTheDocument();
    // The only row shown holds the section's one tab stop (§ 6.13).
    expect(row).toHaveAttribute("tabindex", "0");
    expect(row).toHaveAttribute("data-reason", "upheld_by");
    expect(within(row).getByTestId("undetermined-install-reason")).toHaveTextContent(
      /^upheld by$/,
    );
    expect(
      within(row)
        .getAllByTestId("install-file")
        .map((f) => f.textContent),
    ).toEqual([NEED_DROP_IN]);
    expect(
      within(row)
        .getAllByTestId("undetermined-install-advisory")
        .map((l) => l.textContent),
    ).toEqual(UNDETERMINED_LINES);
    expect(
      within(row).queryByTestId("undetermined-install-not-applied"),
    ).not.toBeInTheDocument();
    expect(screen.queryByText(/no service state changes/i)).not.toBeInTheDocument();
  });

  it("renders no file list for a record with no install files", () => {
    render(
      <ServiceSection
        {...baseProps}
        undeterminedInstalls={[makeUndetermined({ install_files: [] })]}
      />,
    );
    const row = screen.getByTestId("undetermined-install-need.service");
    expect(within(row).queryByTestId("install-file")).not.toBeInTheDocument();
    expect(within(row).getAllByTestId("undetermined-install-advisory")).toHaveLength(2);
  });

  it("shows a not_applied record's excluded paths and its omission line", () => {
    const line = `# Omitted: need.service links (excluded from the export: ${NEED_DROP_IN})`;
    render(
      <ServiceSection
        {...baseProps}
        undeterminedInstalls={[
          makeUndetermined({ advisories: [line], not_applied: [NEED_DROP_IN] }),
        ]}
      />,
    );
    const row = screen.getByTestId("undetermined-install-need.service");
    const notApplied = within(row).getByTestId("undetermined-install-not-applied");
    expect(
      within(notApplied)
        .getAllByTestId("not-applied-path")
        .map((p) => p.textContent),
    ).toEqual([NEED_DROP_IN]);
    expect(
      within(row)
        .getAllByTestId("undetermined-install-advisory")
        .map((l) => l.textContent),
    ).toEqual([line]);
  });
});

describe("ServiceSection: the default target", () => {
  it("renders the target with a toggle that sends DefaultTarget", async () => {
    const onViewUpdate = vi.fn();
    const next = emptyView({
      default_target: makeTarget({ include: false }),
      containerfile_preview: "FROM base\n",
    });
    mockFetch.mockResolvedValueOnce({ ok: true, json: () => Promise.resolve(next) });
    render(
      <ServiceSection
        {...baseProps}
        defaultTarget={makeTarget()}
        onViewUpdate={onViewUpdate}
      />,
    );
    const row = screen.getByTestId("default-target");
    expect(within(row).getByTestId("default-target-name")).toHaveTextContent(
      /^graphical\.target$/,
    );
    const box = within(row).getByRole("checkbox", { name: "Toggle graphical.target" });
    expect(box).toBeChecked();
    expect(box).toBeEnabled();
    expect(within(row).queryByTestId("declined-default-target")).not.toBeInTheDocument();

    await userEvent.click(box);
    expect(mockFetch).toHaveBeenCalledTimes(1);
    const body = JSON.parse(mockFetch.mock.calls[0][1].body);
    expect(body.op).toBe("SetInclude");
    expect(body.target.item_id).toEqual({ kind: "DefaultTarget" });
    expect(body.target.include).toBe(false);
    // The view the server returns, with its new Containerfile, goes to the
    // owner, which shows it.
    await vi.waitFor(() => expect(onViewUpdate).toHaveBeenCalledWith(next));
  });

  it("toggles the target from the keyboard", async () => {
    mockFetch.mockResolvedValueOnce({ ok: true, json: () => Promise.resolve(emptyView()) });
    render(
      <ServiceSection {...baseProps} defaultTarget={makeTarget({ include: false })} />,
    );
    screen.getByTestId("default-target").focus();
    await userEvent.keyboard(" ");
    expect(mockFetch).toHaveBeenCalledTimes(1);
    const body = JSON.parse(mockFetch.mock.calls[0][1].body);
    expect(body.target).toEqual({ item_id: { kind: "DefaultTarget" }, include: true });
  });

  it("shows the declined text, tied to the toggle", () => {
    const text = "custom.target (its unit file is not in the export)";
    render(
      <ServiceSection
        {...baseProps}
        defaultTarget={makeTarget({
          target: "custom.target",
          declined: { renders: "omitted", reason: "UnitFileNotStaged", text },
        })}
      />,
    );
    const note = screen.getByTestId("declined-default-target");
    expect(note.textContent).toBe(text);
    expect(note).toHaveClass("inspectah-unit-declined--omitted");
    expect(
      describedBy(screen.getByRole("checkbox", { name: "Toggle custom.target" })),
    ).toEqual([text]);
  });

  it("disables a locked target's toggle", async () => {
    render(<ServiceSection {...baseProps} defaultTarget={makeTarget({ locked: true })} />);
    const row = screen.getByTestId("default-target");
    expect(within(row).getByRole("checkbox")).toBeDisabled();
    row.focus();
    await userEvent.keyboard(" x");
    expect(mockFetch).not.toHaveBeenCalled();
  });

  it("renders no row when the view has no default target", () => {
    render(<ServiceSection {...baseProps} services={[makeUnit()]} />);
    expect(screen.getByTestId("service-item-httpd.service")).toBeInTheDocument();
    expect(screen.queryByTestId("default-target")).not.toBeInTheDocument();
  });
});

describe("ServiceSection: Also= members and install drop-ins", () => {
  it("lists a unit row's members with what their links say", () => {
    render(
      <ServiceSection
        {...baseProps}
        services={[
          makeUnit({
            unit: "bundle.service",
            also: [
              { unit: "a.service", links: "installed" },
              { unit: "c.service", links: "unlinked" },
            ],
          }),
          // Control: a row with no members lists none.
          makeUnit(),
        ]}
      />,
    );
    const members = within(screen.getByTestId("service-item-bundle.service")).getAllByTestId(
      "also-member",
    );
    expect(
      members.map((m) => [
        within(m).getByTestId("also-member-unit").textContent,
        within(m).getByTestId("also-member-links").textContent,
      ]),
    ).toEqual([
      ["a.service", "installed"],
      ["c.service", "unlinked"],
    ]);
    expect(
      within(screen.getByTestId("service-item-httpd.service")).queryByTestId("also-member"),
    ).not.toBeInTheDocument();
  });

  it("marks the drop-ins that are the unit's install drop-ins", () => {
    const install = "/etc/systemd/system/app.service.d/10-install.conf";
    const other = "/etc/systemd/system/app.service.d/20-nice.conf";
    render(
      <ServiceSection
        {...baseProps}
        services={[makeUnit({ unit: "app.service", install_drop_ins: [install] })]}
        dropins={[makeDropin({ path: install }), makeDropin({ path: other })]}
      />,
    );
    expect(
      within(screen.getByTestId(`dropin-item-${install}`)).getByTestId("install-dropin-badge"),
    ).toBeInTheDocument();
    expect(
      within(screen.getByTestId(`dropin-item-${other}`)).queryByTestId("install-dropin-badge"),
    ).not.toBeInTheDocument();
  });
});

describe("ServiceSection: link and record text is text", () => {
  it("shows markup characters literally", () => {
    const hostile = "pkg <b>bold</b> & co";
    render(
      <ServiceSection
        {...baseProps}
        manualLinks={[makeLink({ advisories: [hostile], not_applied: [hostile] })]}
        undeterminedInstalls={[makeUndetermined({ advisories: [hostile] })]}
        defaultTarget={makeTarget({
          declined: { renders: "omitted", reason: "UnitFileNotStaged", text: hostile },
        })}
      />,
    );
    expect(screen.getByTestId("service-section").querySelector("b")).toBeNull();
    expect(screen.getByTestId("manual-link-advisory").textContent).toBe(hostile);
    expect(screen.getByTestId("undetermined-install-advisory").textContent).toBe(hostile);
    expect(screen.getByTestId("declined-default-target").textContent).toBe(hostile);
  });
});
