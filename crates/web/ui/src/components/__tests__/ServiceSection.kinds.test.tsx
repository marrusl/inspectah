import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { getDefaultNormalizer, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
// Rendered with a real pending guard, as its owner supplies one in the app.
import { ServiceSectionWithGuard as ServiceSection } from "../../test-utils/withPendingGuard";
import type {
  DropInDecisionDto,
  MountFindingDto,
  ServiceDecisionDto,
  TriageTag,
} from "../../api/types";

// Every unit kind and the reasons the server gives for it (spec § 11.1).
// The reason texts are the server's: the list shows them as given.

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
    current_state: "enabled",
    ...overrides,
  };
}

function makeDropin(overrides: Partial<DropInDecisionDto> = {}): DropInDecisionDto {
  return {
    unit: "httpd.service",
    path: "/etc/systemd/system/httpd.service.d/override.conf",
    triage: TRIAGE,
    include: true,
    kind: "service",
    ...overrides,
  };
}

const ADVISORY_LINES = [
  "# Advisory: srv.mount is enabled on the source host. inspectah does not enable, disable or mask a mount, automount or swap unit the administrator wrote.",
  "#   Its unit file is copied into the image.",
  "#   To carry it, add after the configuration COPY: RUN systemctl enable -- 'srv.mount'",
];

function makeFinding(overrides: Partial<MountFindingDto> = {}): MountFindingDto {
  return {
    unit: "srv.mount",
    kind: "mount",
    state: "enabled",
    mount_point: "/srv",
    fragment: "local",
    unit_file_path: "/etc/systemd/system/srv.mount",
    advisories: ADVISORY_LINES,
    ...overrides,
  };
}

const DECLINED_TEXT =
  "kdump.service (the export also carries a unit file at /etc/systemd/system/kdump.service; exclude one of the two)";

const baseProps = {
  services: [] as ServiceDecisionDto[],
  dropins: [] as DropInDecisionDto[],
  mountFindings: [] as MountFindingDto[],
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

describe("ServiceSection: every unit kind", () => {
  it("renders a socket row with its kind as served", () => {
    render(
      <ServiceSection
        {...baseProps}
        services={[
          makeUnit({ unit: "cockpit.socket", kind: "socket" }),
          makeUnit({ unit: "httpd.service", kind: "service" }),
        ]}
      />,
    );
    expect(screen.getByTestId("service-kind-cockpit.socket")).toHaveTextContent(
      /^socket$/,
    );
    expect(screen.getByTestId("service-kind-httpd.service")).toHaveTextContent(
      /^service$/,
    );
  });

  it("renders a declined row's text with its renders class, tied to the row", () => {
    render(
      <ServiceSection
        {...baseProps}
        services={[
          makeUnit({
            unit: "kdump.service",
            current_state: "masked",
            declined: { renders: "omitted", reason: "MaskPathOccupied", text: DECLINED_TEXT },
          }),
          makeUnit({
            unit: "ostree-remount.service",
            current_state: "masked",
            declined: {
              renders: "fixme",
              reason: "TargetNeedsUnit",
              text: "ostree-remount.service is masked on the source host. Not carried: the image needs this unit.",
            },
          }),
          // Control: a row the export writes has no declined text.
          makeUnit({ unit: "httpd.service" }),
        ]}
      />,
    );
    const omitted = screen.getByTestId("declined-service-kdump.service");
    expect(omitted).toHaveTextContent(DECLINED_TEXT, { normalizeWhitespace: false });
    expect(omitted.textContent).toBe(DECLINED_TEXT);
    expect(omitted).toHaveClass("inspectah-unit-declined--omitted");
    const fixme = screen.getByTestId("declined-service-ostree-remount.service");
    expect(fixme).toHaveClass("inspectah-unit-declined--fixme");
    expect(
      describedBy(screen.getByRole("checkbox", { name: "Toggle kdump.service" })),
    ).toContain(DECLINED_TEXT);

    expect(screen.queryByTestId("declined-service-httpd.service")).not.toBeInTheDocument();
    expect(
      screen.getByRole("checkbox", { name: "Toggle httpd.service" }),
    ).not.toHaveAttribute("aria-describedby");
  });

  it("shows declined on a row whose attention_reason is null", () => {
    render(
      <ServiceSection
        {...baseProps}
        services={[
          makeUnit({
            unit: "kdump.service",
            attention_reason: null,
            declined: { renders: "omitted", reason: "MaskPathOccupied", text: DECLINED_TEXT },
          }),
        ]}
      />,
    );
    expect(screen.getByTestId("declined-service-kdump.service").textContent).toBe(
      DECLINED_TEXT,
    );
    expect(screen.queryByTestId("attention-service-kdump.service")).not.toBeInTheDocument();
  });

  it("shows both attention_reason and declined when a row carries both", () => {
    const sentence = "kdump.service is masked on the source host. Not carried: the image needs this unit.";
    render(
      <ServiceSection
        {...baseProps}
        services={[
          makeUnit({
            unit: "kdump.service",
            attention_reason: sentence,
            declined: { renders: "omitted", reason: "MaskPathOccupied", text: DECLINED_TEXT },
          }),
        ]}
      />,
    );
    expect(screen.getByTestId("attention-service-kdump.service").textContent).toBe(sentence);
    expect(screen.getByTestId("declined-service-kdump.service").textContent).toBe(
      DECLINED_TEXT,
    );
    expect(
      describedBy(screen.getByRole("checkbox", { name: "Toggle kdump.service" })),
    ).toEqual([sentence, DECLINED_TEXT]);
  });

  it("shows the reason once when attention_reason and declined.text are the same", () => {
    // A scanned protected unit: the collector's sentence and the plan's
    // decline text come from one server function. The test above, with
    // two different texts, is the control.
    const sentence =
      "ostree-remount.service is masked on the source host. Not carried: the image needs this unit.";
    render(
      <ServiceSection
        {...baseProps}
        services={[
          makeUnit({
            unit: "ostree-remount.service",
            current_state: "masked",
            attention_reason: sentence,
            declined: { renders: "fixme", reason: "TargetNeedsUnit", text: sentence },
          }),
        ]}
      />,
    );
    expect(
      screen.queryByTestId("attention-service-ostree-remount.service"),
    ).not.toBeInTheDocument();
    expect(
      screen.getByTestId("declined-service-ostree-remount.service").textContent,
    ).toBe(sentence);
    const box = screen.getByRole("checkbox", {
      name: "Toggle ostree-remount.service",
    });
    expect(box.getAttribute("aria-describedby")).toBe(
      "declined-service-ostree-remount.service",
    );
    expect(describedBy(box)).toEqual([sentence]);
  });

  it("keeps a runtime updater row (excluded, with a reason, no declined) toggleable", async () => {
    const reason = "dnf-automatic.timer updates packages at runtime.";
    mockFetch.mockResolvedValueOnce({ ok: true, json: () => Promise.resolve({}) });
    render(
      <ServiceSection
        {...baseProps}
        services={[
          makeUnit({
            unit: "dnf-automatic.timer",
            kind: "timer",
            include: false,
            attention_reason: reason,
          }),
        ]}
      />,
    );
    const box = screen.getByRole("checkbox", { name: "Toggle dnf-automatic.timer" });
    expect(box).not.toBeChecked();
    expect(box).toBeEnabled();
    expect(screen.getByTestId("attention-service-dnf-automatic.timer").textContent).toBe(
      reason,
    );
    expect(
      screen.queryByTestId("declined-service-dnf-automatic.timer"),
    ).not.toBeInTheDocument();
    await userEvent.click(box);
    expect(mockFetch).toHaveBeenCalledTimes(1);
    const body = JSON.parse(mockFetch.mock.calls[0][1].body);
    expect(body.target.item_id).toEqual({
      kind: "Unit",
      key: { unit: "dnf-automatic.timer" },
    });
    expect(body.target.include).toBe(true);
  });

  it("renders a drop-in's withheld text, tied to its toggle", () => {
    const text = "a zero-length file at a unit path is a mask; the export carries a mask only as a systemctl mask command";
    render(
      <ServiceSection
        {...baseProps}
        services={[makeUnit()]}
        dropins={[
          makeDropin({ withheld: { outcome: "withheld", why: "ZeroLengthUnitFile", text } }),
          // Control: a drop-in the export writes.
          makeDropin({ path: "/etc/systemd/system/httpd.service.d/b.conf" }),
        ]}
      />,
    );
    const path = "/etc/systemd/system/httpd.service.d/override.conf";
    expect(screen.getByTestId(`withheld-dropin-${path}`).textContent).toBe(text);
    expect(describedBy(screen.getByRole("checkbox", { name: `Toggle ${path}` }))).toEqual([
      text,
    ]);
    expect(
      screen.queryByTestId("withheld-dropin-/etc/systemd/system/httpd.service.d/b.conf"),
    ).not.toBeInTheDocument();
  });
});

// `aria-describedby` is a list of ids separated by whitespace, so an id
// built from a path holding a space or a tab names ids that do not exist.
describe("ServiceSection: a drop-in's notes at a path with whitespace", () => {
  /** Matches a test id holding a tab or a newline as written. */
  const EXACT = { normalizer: getDefaultNormalizer({ trim: false, collapseWhitespace: false }) };
  const text = "a zero-length file at a unit path is a mask; the export carries a mask only as a systemctl mask command";

  it.each([
    ["a space", "/etc/systemd/system/httpd.service.d/my override.conf"],
    ["a tab", "/etc/systemd/system/httpd.service.d/a\tb.conf"],
    ["a newline", "/etc/systemd/system/httpd.service.d/a\nb.conf"],
  ])("ties the withheld text to the toggle when the path holds %s", (_label, path) => {
    render(
      <ServiceSection
        {...baseProps}
        services={[makeUnit()]}
        dropins={[makeDropin({ path, withheld: { outcome: "refused", rule: "Traversal", text } })]}
      />,
    );
    const row = screen.getByTestId(`dropin-item-${path}`, EXACT);
    expect(describedBy(within(row).getByRole("checkbox"))).toEqual([text]);
  });

  it("ties the shadow rationale and the withheld text to the toggle", () => {
    const path = "/etc/systemd/system/my app.service";
    render(
      <ServiceSection
        {...baseProps}
        dropins={[
          makeDropin({
            path,
            shadow_type: "full_shadow",
            shadow_rationale: "replaces the vendor unit",
            withheld: { outcome: "withheld", why: "Conflict", text },
          }),
        ]}
      />,
    );
    const row = screen.getByTestId(`dropin-item-${path}`);
    expect(describedBy(within(row).getByRole("checkbox"))).toEqual([
      "replaces the vendor unit",
      text,
    ]);
  });

  it("ties a locked drop-in row to its reason", () => {
    const path = "/etc/systemd/system/httpd.service.d/my override.conf";
    render(
      <ServiceSection
        {...baseProps}
        dropins={[makeDropin({ path, locked: true, attention_reason: "held by the base image" })]}
      />,
    );
    expect(describedBy(screen.getByTestId(`dropin-item-${path}`))).toEqual([
      "held by the base image",
    ]);
  });
});

describe("ServiceSection: mount findings", () => {
  it("renders findings as rows with no toggle, their advisories line by line", () => {
    render(
      <ServiceSection
        {...baseProps}
        services={[makeUnit()]}
        mountFindings={[makeFinding()]}
      />,
    );
    const row = screen.getByTestId("mount-finding-srv.mount");
    // Presence first: the unit row has a toggle, the finding row none.
    expect(
      within(screen.getByTestId("service-item-httpd.service")).getByRole("checkbox"),
    ).toBeInTheDocument();
    expect(within(row).queryByRole("checkbox")).not.toBeInTheDocument();
    expect(row).toHaveAttribute("tabindex", "-1");
    expect(within(row).getByTestId("mount-finding-kind-srv.mount")).toHaveTextContent(
      /^mount$/,
    );
    const lines = within(row)
      .getAllByTestId("mount-finding-advisory")
      .map((l) => l.textContent);
    expect(lines).toEqual(ADVISORY_LINES);
    expect(describedBy(row)).toEqual([ADVISORY_LINES.join("")]);
  });

  it("does not toggle a finding from the keyboard", async () => {
    render(<ServiceSection {...baseProps} mountFindings={[makeFinding()]} />);
    const row = screen.getByTestId("mount-finding-srv.mount");
    row.focus();
    await userEvent.keyboard(" x");
    expect(mockFetch).not.toHaveBeenCalled();
  });

  it("links a finding to the config row that holds its unit file", async () => {
    const onNavigateToConfig = vi.fn();
    render(
      <ServiceSection
        {...baseProps}
        mountFindings={[makeFinding()]}
        onNavigateToConfig={onNavigateToConfig}
      />,
    );
    await userEvent.click(
      screen.getByRole("link", { name: "/etc/systemd/system/srv.mount" }),
    );
    expect(onNavigateToConfig).toHaveBeenCalledWith("/etc/systemd/system/srv.mount");
  });

  it("renders nothing for a null state, mount point or unit file", () => {
    render(
      <ServiceSection
        {...baseProps}
        mountFindings={[
          makeFinding({
            unit: "ghost.swap",
            kind: "swap",
            state: null,
            mount_point: null,
            fragment: "absent",
            unit_file_path: null,
            advisories: [],
          }),
        ]}
      />,
    );
    const row = screen.getByTestId("mount-finding-ghost.swap");
    expect(within(row).queryByTestId("mount-finding-state-ghost.swap")).not.toBeInTheDocument();
    expect(within(row).queryByTestId("mount-finding-where-ghost.swap")).not.toBeInTheDocument();
    expect(within(row).queryByRole("link")).not.toBeInTheDocument();
    expect(row).not.toHaveAttribute("aria-describedby");
    // Control: the section is not shown as empty.
    expect(screen.queryByText(/no service state changes/i)).not.toBeInTheDocument();
  });
});

describe("ServiceSection: server text is text", () => {
  it("shows markup characters in reasons and advisories literally", () => {
    const hostile = "pkg <b>bold</b> & co";
    // A different string, so the attention note is not folded into the
    // declined one.
    const hostileAttention = "unit <b>x</b> & y";
    render(
      <ServiceSection
        {...baseProps}
        services={[
          makeUnit({
            unit: "kdump.service",
            attention_reason: hostileAttention,
            declined: { renders: "omitted", reason: "PackageExcluded", text: hostile },
          }),
        ]}
        dropins={[
          makeDropin({
            unit: "kdump.service",
            withheld: { outcome: "refused", rule: "Traversal", text: hostile },
          }),
        ]}
        mountFindings={[makeFinding({ advisories: [hostile] })]}
      />,
    );
    const section = screen.getByTestId("service-section");
    expect(section.querySelector("b")).toBeNull();
    expect(screen.getByTestId("attention-service-kdump.service").textContent).toBe(
      hostileAttention,
    );
    for (const id of [
      "declined-service-kdump.service",
      "withheld-dropin-/etc/systemd/system/httpd.service.d/override.conf",
    ]) {
      expect(screen.getByTestId(id).textContent).toBe(hostile);
    }
    expect(screen.getByTestId("mount-finding-advisory").textContent).toBe(hostile);
  });
});
