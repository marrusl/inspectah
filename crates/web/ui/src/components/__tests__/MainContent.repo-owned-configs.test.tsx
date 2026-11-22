/**
 * Configuration rows a repo owns (spec § 7.3, T8), over the recorded
 * `repos` and `repos-excluded` sessions of
 * `crates/web/tests/system_tuning_fixtures_test.rs`: read-only, decided
 * under Repositories, explained by what the export does with the file,
 * and linked to the owning repo's switch.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MainContent } from "../MainContent";
import type { ViewResponse } from "../../api/types";
import scannedJson from "../../__tests__/fixtures/system-tuning/repos.view.json";
import excludedJson from "../../__tests__/fixtures/system-tuning/repos-excluded.view.json";

function body(json: unknown): ViewResponse {
  return structuredClone(json) as ViewResponse;
}

const props = {
  activeSection: "configs",
  loading: false,
  sections: null,
  onViewUpdate: vi.fn(),
  onMutationError: vi.fn(),
  sectionSearchOpen: false,
  onSectionSearchClose: vi.fn(),
};

function row(path: string): HTMLElement {
  return screen.getByTestId(`decision-item-configs:${path}`);
}

function explanation(path: string): string {
  return within(row(path)).getByTestId(/^shadow-owner-explanation-/)
    .textContent!;
}

describe("repo-owned Configuration rows", () => {
  beforeEach(() => {
    vi.stubGlobal(
      "fetch",
      vi.fn(() =>
        Promise.resolve({ ok: true, json: () => Promise.resolve({ ids: [] }) }),
      ),
    );
  });
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("are read-only and say they are decided under Repositories", () => {
    render(<MainContent {...props} viewData={body(scannedJson)} />);
    const custom = row("/etc/yum.repos.d/custom.repo");
    expect(within(custom).queryByRole("checkbox")).toBeNull();
    expect(within(custom).queryByTestId("inventory-badge")).toBeNull();
    expect(within(custom).getByTestId("shadow-owner-badge")).toHaveTextContent(
      "Decided under Repositories",
    );
    expect(within(custom).getByTestId("shadow-owner-state")).toHaveTextContent(
      "Ships",
    );
    expect(explanation("/etc/yum.repos.d/custom.repo")).toBe(
      "This file belongs to repo custom. Its switch under Repositories, in " +
        "Packages, decides whether the file ships.",
    );
    // No repo-owned row anywhere in the section offers a checkbox.
    for (const cfg of body(scannedJson).config_files) {
      if (cfg.entry.shadowed_by === "repositories") {
        expect(within(row(cfg.entry.path)).queryByRole("checkbox")).toBeNull();
      }
    }
  });

  it("explains each export state the recording holds", () => {
    render(<MainContent {...props} viewData={body(excludedJson)} />);
    expect(
      within(row("/etc/yum.repos.d/custom.repo")).getByTestId(
        "shadow-owner-state",
      ),
    ).toHaveTextContent("Not shipped");
    expect(explanation("/etc/yum.repos.d/custom.repo")).toBe(
      "This file belongs to repo custom, and the export leaves it out. Its " +
        "switch under Repositories decides whether it ships.",
    );
    expect(explanation("/etc/pki/rpm-gpg/RPM-GPG-KEY-shared")).toBe(
      "This file belongs to repo a, b. Its switch under Repositories, in " +
        "Packages, decides whether the file ships. Repo a is excluded, but " +
        "repo b still needs this file, so it ships.",
    );
    expect(explanation("/etc/pki/rpm-gpg/RPM-GPG-KEY-notpgp")).toBe(
      "The scan did not store this file because it is not a PGP public " +
        "key, so the export never copies it.",
    );
    expect(
      within(row("/etc/pki/rpm-gpg/RPM-GPG-KEY-notpgp")).getByTestId(
        "shadow-owner-state",
      ),
    ).toHaveTextContent("Not shipped");
    expect(explanation("/etc/pki/rpm-gpg/token=vendor")).toMatch(
      /^The scan found this key because a repo file named it, but redaction changed that gpgkey= reference \(\/etc\/yum\.repos\.d\/vend\.repo\)/,
    );
  });

  it("explains a default repo file whose owner has no switch, with no link", () => {
    render(
      <MainContent
        {...props}
        viewData={body(scannedJson)}
        onNavigateToItem={vi.fn()}
      />,
    );
    const redhat = row("/etc/yum.repos.d/redhat.repo");
    expect(explanation("/etc/yum.repos.d/redhat.repo")).toBe(
      "This is a default repo file, which the export never copies. The " +
        "build is expected to get it from the base image or the " +
        "subscription mount.",
    );
    expect(within(redhat).queryByTestId("shadow-owner-link")).toBeNull();
    expect(within(redhat).getByTestId("shadow-owner-state")).toHaveTextContent(
      "Not shipped",
    );
  });

  it("links to the owning repo's row, a carriage-only one included", async () => {
    const onNavigateToItem = vi.fn();
    render(
      <MainContent
        {...props}
        viewData={body(scannedJson)}
        onNavigateToItem={onNavigateToItem}
      />,
    );
    await userEvent.click(
      within(row("/etc/yum.repos.d/company.repo")).getByRole("button", {
        name: "Go to repo baseos",
      }),
    );
    expect(onNavigateToItem).toHaveBeenLastCalledWith("packages", "repo:baseos");
    // A file whose every section is disabled, with no package: its
    // carriage-only owner is still the one control.
    await userEvent.click(
      within(row("/etc/yum.repos.d/dis.repo")).getByRole("button", {
        name: "Go to repo dis",
      }),
    );
    expect(onNavigateToItem).toHaveBeenLastCalledWith("packages", "repo:dis");
    // Two owners: the first by section id.
    await userEvent.click(
      within(row("/etc/pki/rpm-gpg/RPM-GPG-KEY-shared")).getByRole("button", {
        name: "Go to repo a",
      }),
    );
    expect(onNavigateToItem).toHaveBeenLastCalledWith("packages", "repo:a");
  });

  it("joins through recorded_paths: a // key path and a relative ostree path", () => {
    const view = body(scannedJson);
    // The shared key's recording already holds its `//` spelling; give
    // the config row that spelling.
    const shared = view.config_files.find(
      (c) => c.entry.path === "/etc/pki/rpm-gpg/RPM-GPG-KEY-shared",
    )!;
    shared.entry.path = "/etc/pki/rpm-gpg//RPM-GPG-KEY-shared";
    // An ostree scan records a relative path; the server lists it among
    // the destination's spellings.
    const custom = view.config_files.find(
      (c) => c.entry.path === "/etc/yum.repos.d/custom.repo",
    )!;
    custom.entry.path = "etc/yum.repos.d/custom.repo";
    view.repo_owned
      .find((e) => e.destination === "/etc/yum.repos.d/custom.repo")!
      .recorded_paths.push("etc/yum.repos.d/custom.repo");

    render(<MainContent {...props} viewData={view} />);
    expect(explanation("/etc/pki/rpm-gpg//RPM-GPG-KEY-shared")).toMatch(
      /^This file belongs to repo a, b\./,
    );
    expect(explanation("etc/yum.repos.d/custom.repo")).toMatch(
      /^This file belongs to repo custom\./,
    );
  });
});
