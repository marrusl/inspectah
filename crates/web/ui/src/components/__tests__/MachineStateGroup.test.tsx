/// <reference types="vite/client" />
/**
 * The machine-state block (spec § 8, Refine presentation), against the
 * rows the real handlers return: `machine-state.view.json` is recorded by
 * `crates/web/tests/system_tuning_fixtures_test.rs` from a scan of a host
 * with one class file per override level and notice kind.
 */
import { describe, it, expect, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MachineStateGroup } from "../MachineStateGroup";
import type { MachineStateRowDto, ViewResponse } from "../../api/types";

const FIXTURES = import.meta.glob<{ default: ViewResponse }>(
  "../../__tests__/fixtures/system-tuning/machine-state*.view.json",
  { eager: true },
);

function recorded(name: string): ViewResponse {
  const entry =
    FIXTURES[`../../__tests__/fixtures/system-tuning/${name}.view.json`];
  if (!entry) throw new Error(`no recorded fixture ${name}.view.json`);
  return entry.default;
}

const VIEW = recorded("machine-state");
const INCLUDED = recorded("machine-state-included");

function row(rows: MachineStateRowDto[], path: string): MachineStateRowDto {
  const found = rows.find((r) => r.path === path);
  if (!found) throw new Error(`no row ${path} in the fixture`);
  return found;
}

const HOSTNAME = "/etc/hostname";
const IQN = "/etc/iscsi/initiatorname.iscsi";
const KEYTAB = "/etc/krb5.keytab";
const SSH_KEY = "/etc/ssh/ssh_host_ed25519_key";
const INSIGHTS = "/etc/insights-client/machine-id";
const TUNED = "/etc/grub.d/00_tuned";
const NVME_HOSTNQN = "/etc/nvme/hostnqn";

function rowEl(path: string): HTMLElement {
  return screen.getByTestId(`machine-state-row-${path}`);
}

async function renderExpanded(
  rows: MachineStateRowDto[] = VIEW.machine_state,
  onSetInclude = vi.fn(),
) {
  const user = userEvent.setup();
  const utils = render(
    <MachineStateGroup
      rows={rows}
      stepCount={VIEW.machine_state_steps.length}
      onSetInclude={onSetInclude}
    />,
  );
  await user.click(screen.getByRole("button", { name: /^Machine state/ }));
  return { user, onSetInclude, ...utils };
}

describe("MachineStateGroup", () => {
  it("renders nothing when there are no rows", () => {
    const { container } = render(
      <MachineStateGroup rows={[]} stepCount={0} onSetInclude={vi.fn()} />,
    );
    expect(container).toBeEmptyDOMElement();
  });

  it("states the counts in the collapsed toggle", () => {
    render(
      <MachineStateGroup
        rows={VIEW.machine_state}
        stepCount={VIEW.machine_state_steps.length}
        onSetInclude={vi.fn()}
      />,
    );
    const toggle = screen.getByRole("button", { name: /^Machine state/ });
    expect(toggle).toHaveTextContent(
      "Machine state (6) · 2 host credentials · 2 provisioning steps",
    );
    expect(toggle).toHaveAttribute("aria-expanded", "false");
  });

  it("adds the operator's overrides to the toggle", () => {
    render(
      <MachineStateGroup
        rows={INCLUDED.machine_state}
        stepCount={INCLUDED.machine_state_steps.length}
        onSetInclude={vi.fn()}
      />,
    );
    expect(
      screen.getByRole("button", { name: /^Machine state/ }),
    ).toHaveTextContent("· 2 included by you");
  });

  it("groups rows into sub-lists in risk order, server order within", async () => {
    await renderExpanded();
    const headings = screen
      .getAllByRole("heading", { level: 3 })
      .map((h) => h.textContent);
    expect(headings).toEqual(["Host credentials", "Host identity", "Generated"]);
    const identity = screen.getByTestId("machine-state-sublist-host_identity");
    expect(
      within(identity)
        .getAllByRole("listitem")
        .map((li) => li.querySelector("code")?.textContent),
    ).toEqual([HOSTNAME, INSIGHTS, IQN]);
    const credentials = screen.getByTestId(
      "machine-state-sublist-host_credential",
    );
    expect(within(credentials).getAllByText("Host credential")).toHaveLength(2);
  });

  it("takes each row's control from its override level", async () => {
    await renderExpanded();
    expect(
      within(rowEl(HOSTNAME)).getByRole("button", {
        name: `Include ${HOSTNAME}`,
      }),
    ).toHaveTextContent(/^Include$/);
    expect(
      within(rowEl(IQN)).getByRole("button", { name: `Include ${IQN}…` }),
    ).toHaveTextContent("Include…");
    for (const path of [KEYTAB, SSH_KEY, INSIGHTS]) {
      const li = rowEl(path);
      expect(within(li).queryByRole("button")).toBeNull();
      expect(within(li).queryByRole("checkbox")).toBeNull();
      expect(li).toHaveTextContent("Not carried");
      // The row itself is a reveal target, not a tab stop.
      expect(li).toHaveAttribute("tabindex", "-1");
    }
  });

  it("says on every row what replaces the file", async () => {
    await renderExpanded();
    expect(rowEl(KEYTAB)).toHaveTextContent(
      "At provisioning: ipa-client-install, realm join or adcli join",
    );
    expect(rowEl(SSH_KEY)).toHaveTextContent("Recreated at first boot");
    expect(rowEl(HOSTNAME)).toHaveTextContent(
      "Set at provisioning by the installer, cloud-init or DHCP",
    );
    expect(rowEl(TUNED)).toHaveTextContent("Not needed in the image");
  });

  it("shows a member's own notice in place of the generic line", async () => {
    // The recorded scan has no NVMe files; the member shares the iSCSI
    // name's class and first-boot notice, so derive its row from that one.
    const nvme: MachineStateRowDto = {
      ...row(VIEW.machine_state, IQN),
      path: NVME_HOSTNQN,
      member: "nvme_host_id",
      member_notice: "the member's own notice, from the server",
    };
    expect(nvme.notice).toEqual({ kind: "automatic", by: "first_boot" });
    await renderExpanded([...VIEW.machine_state, nvme]);
    expect(rowEl(NVME_HOSTNQN)).not.toHaveTextContent("first boot");
    expect(rowEl(NVME_HOSTNQN)).toHaveTextContent(
      "The member's own notice, from the server",
    );
    expect(rowEl(SSH_KEY)).toHaveTextContent("Recreated at first boot");
  });

  it("shows the generic line when no member notice is sent", async () => {
    // No member id is special in the client: without `member_notice`,
    // the NVMe row reads as its notice says.
    const nvme: MachineStateRowDto = {
      ...row(VIEW.machine_state, IQN),
      path: NVME_HOSTNQN,
      member: "nvme_host_id",
    };
    await renderExpanded([...VIEW.machine_state, nvme]);
    expect(rowEl(NVME_HOSTNQN)).toHaveTextContent("Recreated at first boot");
    expect(rowEl(SSH_KEY)).toHaveTextContent("Recreated at first boot");
  });

  it("shows what was not captured, and unknown for a failed stat", async () => {
    const keytab = row(VIEW.machine_state, KEYTAB);
    const rows = VIEW.machine_state.map((r) =>
      r.path === KEYTAB ? { ...keytab, mode: null, size: null } : r,
    );
    await renderExpanded(rows);
    expect(rowEl(KEYTAB)).toHaveTextContent(
      "mode unknown · size unknown · contents not captured",
    );
    expect(rowEl(SSH_KEY)).toHaveTextContent(
      "mode 0644 · 15 bytes · contents not captured",
    );
    expect(rowEl(HOSTNAME)).not.toHaveTextContent("contents not captured");
  });

  it("shows the audit report pointer for a notice it does not know", async () => {
    const rows = VIEW.machine_state.map((r) =>
      r.path === TUNED
        ? ({ ...r, notice: { kind: "reissued" } } as unknown as MachineStateRowDto)
        : r.path === SSH_KEY
          ? ({
              ...r,
              notice: { kind: "automatic", by: "orchestrator" },
            } as unknown as MachineStateRowDto)
          : r,
    );
    await renderExpanded(rows);
    expect(rowEl(TUNED)).toHaveTextContent("See the audit report");
    expect(rowEl(SSH_KEY)).toHaveTextContent("See the audit report");
  });

  it("sends a plain include at once, with the row", async () => {
    const { user, onSetInclude } = await renderExpanded();
    await user.click(
      within(rowEl(HOSTNAME)).getByRole("button", { name: `Include ${HOSTNAME}` }),
    );
    expect(onSetInclude).toHaveBeenCalledWith(
      row(VIEW.machine_state, HOSTNAME),
      true,
    );
  });

  it("asks before a warn include, with focus on Cancel", async () => {
    const { user, onSetInclude } = await renderExpanded();
    const include = within(rowEl(IQN)).getByRole("button", {
      name: `Include ${IQN}…`,
    });
    await user.click(include);
    const dialog = await screen.findByRole("dialog", {
      name: "Include this host's identity in the image?",
    });
    expect(dialog).toHaveTextContent(
      "Every machine built from this image will share this identity.",
    );
    expect(dialog).toHaveTextContent(
      "Leave it out and it is recreated at first boot.",
    );
    expect(within(dialog).getByRole("button", { name: "Cancel" })).toHaveFocus();
    expect(onSetInclude).not.toHaveBeenCalled();

    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(onSetInclude).not.toHaveBeenCalled();
    expect(include).toHaveFocus();

    await user.click(include);
    await user.click(
      await screen.findByRole("button", { name: "Include in image" }),
    );
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(onSetInclude).toHaveBeenCalledTimes(1);
    expect(onSetInclude).toHaveBeenCalledWith(row(VIEW.machine_state, IQN), true);
  });

  it("shows an included row's consequence and an Exclude with no dialog", async () => {
    const { user, onSetInclude } = await renderExpanded(INCLUDED.machine_state);
    const iqn = rowEl(IQN);
    expect(iqn).toHaveTextContent("Included by you");
    expect(iqn).toHaveTextContent(
      "In the image: every machine built from it shares this identity",
    );
    expect(rowEl(HOSTNAME)).toHaveTextContent(
      "In the image, copied from this host",
    );
    await user.click(
      within(iqn).getByRole("button", { name: `Exclude ${IQN}` }),
    );
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(onSetInclude).toHaveBeenCalledWith(
      row(INCLUDED.machine_state, IQN),
      false,
    );
  });

  it("disables a row's button while its request is in flight", async () => {
    render(
      <MachineStateGroup
        rows={VIEW.machine_state}
        stepCount={2}
        onSetInclude={vi.fn()}
        pendingKeys={new Set([`machine-state:${HOSTNAME}`])}
      />,
    );
    await userEvent.click(screen.getByRole("button", { name: /^Machine state/ }));
    expect(
      within(rowEl(HOSTNAME)).getByRole("button", { name: `Include ${HOSTNAME}` }),
    ).toBeDisabled();
    expect(
      within(rowEl(IQN)).getByRole("button", { name: `Include ${IQN}…` }),
    ).toBeEnabled();
  });

  it("shows a refusal on its row, and dismisses it", async () => {
    const onDismissFailure = vi.fn();
    render(
      <MachineStateGroup
        rows={VIEW.machine_state}
        stepCount={2}
        onSetInclude={vi.fn()}
        failures={
          new Map([
            [
              `machine-state:${HOSTNAME}`,
              { kind: "not_carried" as const, text: "Not carried: withheld." },
            ],
          ])
        }
        onDismissFailure={onDismissFailure}
      />,
    );
    await userEvent.click(screen.getByRole("button", { name: /^Machine state/ }));
    const li = rowEl(HOSTNAME);
    expect(within(li).getByText("Not carried: withheld.")).toBeInTheDocument();
    await userEvent.click(
      within(li).getByRole("button", {
        name: `Dismiss the message for ${HOSTNAME}`,
      }),
    );
    expect(onDismissFailure).toHaveBeenCalledWith(`machine-state:${HOSTNAME}`);
  });

  it("filters on path, label and notice, and expands while searching", () => {
    const { rerender } = render(
      <MachineStateGroup
        rows={VIEW.machine_state}
        stepCount={2}
        onSetInclude={vi.fn()}
        filterText="first boot"
      />,
    );
    expect(
      screen.getByRole("button", { name: /^Machine state/ }),
    ).toHaveAttribute("aria-expanded", "true");
    expect(
      screen
        .getAllByRole("listitem")
        .map((li) => li.querySelector("code")?.textContent),
    ).toEqual([SSH_KEY, IQN]);
    expect(screen.queryByRole("heading", { name: "Generated" })).toBeNull();

    rerender(
      <MachineStateGroup
        rows={VIEW.machine_state}
        stepCount={2}
        onSetInclude={vi.fn()}
        filterText="host credential"
      />,
    );
    expect(
      screen
        .getAllByRole("listitem")
        .map((li) => li.querySelector("code")?.textContent),
    ).toEqual([KEYTAB, SSH_KEY]);

    rerender(
      <MachineStateGroup
        rows={VIEW.machine_state}
        stepCount={2}
        onSetInclude={vi.fn()}
        filterText=""
      />,
    );
    expect(
      screen.getByRole("button", { name: /^Machine state/ }),
    ).toHaveAttribute("aria-expanded", "true");
    expect(screen.getAllByRole("listitem")).toHaveLength(6);
  });

  it("expands when a reveal names one of its rows", () => {
    render(
      <MachineStateGroup
        rows={VIEW.machine_state}
        stepCount={2}
        onSetInclude={vi.fn()}
        revealItemId={`machine-state:${KEYTAB}`}
      />,
    );
    expect(
      screen.getByRole("button", { name: /^Machine state/ }),
    ).toHaveAttribute("aria-expanded", "true");
  });
});
