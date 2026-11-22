import { describe, it, expect, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { SysctlPanel } from "../SysctlPanel";
import type {
  SysctlHostView,
  SysctlLineRefDto,
  SysctlViewDto,
} from "../../api/types";

const HOSTILE = "<img src=x onerror=alert(1)>";

function at(path: string, line: number, link_target: string | null = null): SysctlLineRefDto {
  return { path, line, link_target };
}

function host(overrides: Partial<SysctlHostView> = {}): SysctlViewDto {
  return {
    state: "host",
    explanation: { state: "resolved", settings: [], drift: [] },
    vendor_replacements: [],
    not_carried: [],
    as_written: [],
    files: [],
    tuned_active: null,
    ...overrides,
  };
}

function renderPanel(sysctl: SysctlViewDto, onNavigateToConfig = vi.fn()) {
  render(<SysctlPanel sysctl={sysctl} onNavigateToConfig={onNavigateToConfig} />);
  return { panel: screen.getByRole("region", { name: "Sysctl" }), onNavigateToConfig };
}

/** A paragraph of the panel whose text is exactly `text`. */
function paragraph(panel: HTMLElement, text: string) {
  return within(panel).getByText(
    (_, el) => el?.tagName === "P" && el.textContent === text,
  );
}

describe("SysctlPanel", () => {
  it("renders host values as text, never as markup", () => {
    const { panel } = renderPanel(
      host({
        explanation: {
          state: "resolved",
          settings: [
            {
              key: "kernel.core_pattern",
              outcome: {
                kind: "writes",
                value: HOSTILE,
                from: at("/etc/sysctl.d/50-core.conf", 3),
                runtime: { kind: "differs", runtime: "[link](http://example.com)" },
              },
              overwritten: [],
            },
          ],
          drift: [
            {
              key: "kernel.core_pattern",
              file_value: HOSTILE,
              runtime: "[link](http://example.com)",
              from: at("/etc/sysctl.d/50-core.conf", 3),
            },
          ],
        },
        as_written: [
          { kind: "unparsed", at: at("/etc/sysctl.d/60-x.conf", 1), text: HOSTILE },
        ],
      }),
    );
    expect(panel.querySelector("img")).toBeNull();
    expect(panel.querySelector("a[href^='http']")).toBeNull();
    const table = within(panel).getByRole("table", { name: "Sysctl settings" });
    const cells = within(table).getAllByRole("cell");
    expect(cells.map((c) => c.textContent)).toEqual([
      "kernel.core_pattern",
      HOSTILE,
      "[link](http://example.com)",
      "/etc/sysctl.d/50-core.conf:3",
    ]);
    paragraph(panel, `/etc/sysctl.d/60-x.conf:1: unparsed line ${HOSTILE}.`);
    paragraph(
      panel,
      `kernel.core_pattern runs at [link](http://example.com); the files request ${HOSTILE} (/etc/sysctl.d/50-core.conf:3).`,
    );
  });

  it("shows control characters as escapes and an empty value as (empty)", () => {
    const { panel } = renderPanel(
      host({
        as_written: [
          {
            kind: "glob_assign",
            at: at("/etc/sysctl.d/50-\nrp.conf", 2),
            pattern: "net.ipv4.conf.*.rp_filter",
            value: "",
          },
        ],
      }),
    );
    paragraph(
      panel,
      "/etc/sysctl.d/50-\\u{a}rp.conf:2: glob net.ipv4.conf.*.rp_filter set to (empty), not expanded.",
    );
  });

  it("links a config_path to its Configuration Files row", async () => {
    const { panel, onNavigateToConfig } = renderPanel(
      host({
        not_carried: [
          {
            path: "/etc/sysctl.d/99-sysctl.conf",
            reason: { kind: "excluded", config_path: "/etc/sysctl.conf" },
          },
        ],
        files: [
          {
            path: "/etc/sysctl.d/99-sysctl.conf",
            dir: "etc",
            body: { kind: "read", link_target: "/etc/sysctl.conf" },
            config_path: "/etc/sysctl.conf",
          },
        ],
      }),
    );
    paragraph(
      panel,
      "/etc/sysctl.d/99-sysctl.conf: the Configuration Files row that carries it, /etc/sysctl.conf, is excluded.",
    );
    const links = within(panel).getAllByRole("link", { name: "/etc/sysctl.conf" });
    expect(links.length).toBeGreaterThanOrEqual(2);
    for (const link of links) {
      await userEvent.click(link);
    }
    expect(onNavigateToConfig.mock.calls).toEqual(
      links.map(() => ["/etc/sysctl.conf"]),
    );
  });

  it("says a file the export refuses is not carried", () => {
    // The wording of `case_19_refused_path_audit`: the refusal as plain
    // copy, the paths as host values, and a vendor line that never claims
    // the replacement reaches the target.
    const refusal = "path contains a line break or control character";
    const { panel } = renderPanel(
      host({
        vendor_replacements: [
          {
            action: "shadows",
            path: "/etc/sysctl.d/70-new\nline.conf",
            vendor_path: "/usr/lib/sysctl.d/70-new\nline.conf",
            carriage: {
              kind: "refused",
              config_path: "/etc/sysctl.d/70-new\nline.conf",
              reason: refusal,
            },
          },
        ],
        not_carried: [
          {
            path: "/etc/sysctl.d/70-new\nline.conf",
            reason: {
              kind: "refused",
              config_path: "/etc/sysctl.d/70-new\nline.conf",
              reason: refusal,
            },
          },
          {
            path: "/etc/sysctl.d/99-sysctl.conf",
            reason: {
              kind: "refused",
              config_path: `/etc/sysctl\n${HOSTILE}.conf`,
              reason: refusal,
            },
          },
        ],
      }),
    );
    expect(panel.querySelector("img")).toBeNull();
    paragraph(
      panel,
      "/etc/sysctl.d/70-new\\u{a}line.conf replaces /usr/lib/sysctl.d/70-new\\u{a}line.conf on this host. It is not carried, so on the target the vendor file applies.",
    );
    paragraph(
      panel,
      `/etc/sysctl.d/70-new\\u{a}line.conf: its Configuration Files row is included, but the export refuses the path (${refusal}) and does not write the file.`,
    );
    paragraph(
      panel,
      `/etc/sysctl.d/99-sysctl.conf: the Configuration Files row that carries it, /etc/sysctl\\u{a}${HOSTILE}.conf, is included, but the export refuses that path (${refusal}) and does not write the file.`,
    );
    expect(within(panel).queryByText(/on the target as well/)).toBeNull();
    // The refusal is copy, not a host value: never a code span.
    for (const code of panel.querySelectorAll("code")) {
      expect(code.textContent).not.toContain(refusal);
    }
  });

  it("shows the unknown explanation in place of the table", () => {
    const { panel } = renderPanel(
      host({
        explanation: {
          state: "unknown",
          inputs: [
            { kind: "redacted_file", path: "/etc/sysctl.d/80-app.conf" },
            { kind: "redacted_runtime", key: "kernel.core_pattern" },
          ],
        },
      }),
    );
    expect(within(panel).queryByRole("table", { name: "Sysctl settings" })).toBeNull();
    paragraph(
      panel,
      "/etc/sysctl.d/80-app.conf held text that redaction replaced, so what it sets is not known.",
    );
    paragraph(
      panel,
      "The running value of kernel.core_pattern held text that redaction removed.",
    );
  });

  it("says a withheld machine-state file was not read", () => {
    const { panel } = renderPanel(
      host({
        explanation: {
          state: "unknown",
          inputs: [{ kind: "withheld_file", path: "/etc/sysctl.d/90-key.conf" }],
        },
        files: [
          {
            path: "/etc/sysctl.d/90-key.conf",
            dir: "etc",
            body: { kind: "withheld" },
            config_path: null,
          },
        ],
      }),
    );
    paragraph(
      panel,
      "/etc/sysctl.d/90-key.conf is a machine-state file whose contents were not captured, so what it sets is not known.",
    );
    const files = within(panel).getByRole("table", { name: "Sysctl files" });
    expect(
      within(files).getByText("machine-state file, contents not captured"),
    ).toBeInTheDocument();
  });

  it("names the active tuned profile in the drift paragraph", () => {
    const { panel } = renderPanel(
      host({
        explanation: {
          state: "resolved",
          settings: [
            {
              key: "vm.swappiness",
              outcome: {
                kind: "cancelled",
                by: at("/etc/sysctl.d/90-a.conf", 1, "/etc/sysctl.conf"),
                runtime: null,
              },
              overwritten: [],
            },
          ],
          drift: [
            {
              key: "vm.dirty_ratio",
              file_value: "20",
              runtime: "30",
              from: at("/etc/sysctl.d/90-a.conf", 2),
            },
          ],
        },
        tuned_active: "throughput-performance",
      }),
    );
    expect(
      within(panel).getByText((_, el) => el?.tagName === "P" && /tuned profile throughput-performance is active on this host\.$/.test(el.textContent ?? "")),
    ).toBeInTheDocument();
    const cells = within(panel).getAllByRole("cell").map((c) => c.textContent);
    expect(cells).toEqual([
      "vm.swappiness",
      "not written",
      "not exposed",
      "/etc/sysctl.d/90-a.conf:1 (link to /etc/sysctl.conf)",
    ]);
  });

  it("holds no control that could change the session", () => {
    const { panel } = renderPanel(
      host({
        vendor_replacements: [
          {
            action: "shadows",
            path: "/etc/sysctl.d/50-x.conf",
            vendor_path: "/usr/lib/sysctl.d/50-x.conf",
            carriage: { kind: "carried", config_path: "/etc/sysctl.d/50-x.conf" },
          },
        ],
      }),
    );
    expect(within(panel).queryAllByRole("checkbox")).toHaveLength(0);
    expect(within(panel).queryAllByRole("switch")).toHaveLength(0);
    expect(within(panel).queryAllByRole("button")).toHaveLength(0);
  });
});
