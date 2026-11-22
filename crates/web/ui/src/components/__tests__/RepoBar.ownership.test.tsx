/**
 * The Repositories section over recorded server bodies (spec T8): each
 * row's control, the files and keys it owns with what the export does with
 * them, and the entries no switch decides. The bodies are the `repos` and
 * `repos-excluded` sessions of `crates/web/tests/system_tuning_fixtures_test.rs`.
 */
import { render, screen, within, fireEvent } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { RepoBar } from "../RepoBar";
import type { RepoGroupInfo, RepoOwnedEntry } from "../../api/types";
import scannedJson from "../../__tests__/fixtures/system-tuning/repos.view.json";
import excludedJson from "../../__tests__/fixtures/system-tuning/repos-excluded.view.json";
import aggregateJson from "../../__tests__/fixtures/system-tuning/repos-aggregate.aggregate-view.json";

interface RepoBody {
  repo_groups: RepoGroupInfo[];
  repo_owned: RepoOwnedEntry[];
}
const scanned = scannedJson as unknown as RepoBody;
const excluded = excludedJson as unknown as RepoBody;
const aggregate = aggregateJson as unknown as { repo_groups: RepoGroupInfo[] };

function renderBody(body: RepoBody, onToggle = vi.fn()) {
  render(
    <RepoBar
      repos={body.repo_groups}
      repoOwned={body.repo_owned}
      onToggle={onToggle}
    />,
  );
  return onToggle;
}

function row(sectionId: string): HTMLElement {
  return screen.getByTestId(`repo-bar-row-${sectionId}`);
}

describe("Repositories rows from the recorded scan", () => {
  it("gives each control its switch, and no switch to a row with none", () => {
    renderBody(scanned);
    // appstream: a distro repo with no file it could carry.
    expect(within(row("appstream")).queryByRole("switch")).toBeNull();
    expect(within(row("appstream")).getByText("always included")).toBeTruthy();
    // custom: a full switch, members and files.
    expect(
      within(row("custom")).getByRole("switch", {
        name: "custom: 2 of 2 packages included",
      }),
    ).toBeChecked();
    // a: no members, files only.
    expect(
      within(row("a")).getByRole("switch", {
        name: "a: repo files 2 of 2 carried",
      }),
    ).toBeChecked();
  });

  it("labels a carriage-only switch as moving files and keys only", () => {
    renderBody(scanned);
    const baseos = row("baseos");
    expect(within(baseos).getByText("always included")).toBeTruthy();
    expect(
      within(baseos).getByText("switch moves files and keys only"),
    ).toBeTruthy();
    expect(
      within(baseos).getByRole("switch", {
        name: "baseos: repo files 2 of 2 carried; switch moves files and keys only, packages unchanged",
      }),
    ).toBeChecked();
    // A repo file whose every section is disabled, with no package.
    const dis = row("dis");
    expect(within(dis).getByText("disabled on host")).toBeTruthy();
    expect(
      within(dis).getByRole("switch", { name: /^dis: repo files 2 of 2 carried;/ }),
    ).toBeChecked();
  });

  it("posts exclude from a checked carriage-only switch", () => {
    const onToggle = renderBody(scanned);
    fireEvent.click(within(row("baseos")).getByRole("switch"));
    expect(onToggle).toHaveBeenCalledWith("baseos", false);
  });

  it("lists what each repo owns, by basename, named by destination and state", () => {
    renderBody(scanned);
    // Queried by accessible name: the full destination and the state.
    expect(
      screen.getByLabelText("/etc/yum.repos.d/custom.repo: ships"),
    ).toHaveTextContent("custom.repo");
    expect(
      screen.getByLabelText(
        "/etc/pki/rpm-gpg/RPM-GPG-KEY-notpgp: rejected: is not a PGP public key",
      ),
    ).toHaveTextContent("RPM-GPG-KEY-notpgprejected: is not a PGP public key");
    // A key two repos name is on both rows.
    expect(
      within(row("a")).getByLabelText(
        "/etc/pki/rpm-gpg/RPM-GPG-KEY-shared: ships",
      ),
    ).toBeTruthy();
    expect(
      within(row("b")).getByLabelText(
        "/etc/pki/rpm-gpg/RPM-GPG-KEY-shared: ships",
      ),
    ).toBeTruthy();
    // `vend` owns its file, not the key redaction cut loose.
    expect(
      within(row("vend")).queryByLabelText(/token=vendor/),
    ).toBeNull();
  });

  it("lists the files and keys no switch decides, with why", () => {
    renderBody(scanned);
    const unlisted = screen.getByTestId("repo-bar-unlisted");
    expect(
      within(unlisted).getByText("Files and keys no repo switch decides"),
    ).toBeTruthy();
    expect(within(unlisted).getByText("/etc/yum.repos.d/redhat.repo")).toBeTruthy();
    expect(within(unlisted).getByText("default repo file")).toBeTruthy();
    expect(
      within(unlisted).getByText(/^This is a default repo file, which the export never copies\./),
    ).toBeTruthy();
    expect(within(unlisted).getByText("/etc/pki/rpm-gpg/token=vendor")).toBeTruthy();
    expect(within(unlisted).getByText("no owner")).toBeTruthy();
    expect(
      within(unlisted).getByText(/redaction changed that gpgkey= reference/),
    ).toBeTruthy();
    // Nothing a row already lists.
    expect(within(unlisted).queryByText(/custom\.repo/)).toBeNull();
  });
});

describe("Repositories rows after excluding custom, a and baseos", () => {
  it("reads each excluded repo off and says so", () => {
    renderBody(excluded);
    expect(
      within(row("custom")).getByRole("switch", {
        name: "custom: 2 packages excluded, repo files 0 of 2 carried, excluded in refine",
      }),
    ).not.toBeChecked();
    expect(
      within(row("a")).getByRole("switch", {
        name: "a: repo files 1 of 2 carried, excluded in refine",
      }),
    ).not.toBeChecked();
    expect(
      within(row("baseos")).getByRole("switch", {
        name: /^baseos: repo files 0 of 2 carried, excluded in refine;/,
      }),
    ).not.toBeChecked();
  });

  it("says which files are left out and which another repo keeps", () => {
    renderBody(excluded);
    expect(
      within(row("custom")).getByLabelText(
        "/etc/yum.repos.d/custom.repo: not shipped",
      ),
    ).toHaveTextContent("custom.reponot shipped");
    expect(
      within(row("a")).getByLabelText(
        "/etc/pki/rpm-gpg/RPM-GPG-KEY-shared: kept by b",
      ),
    ).toHaveTextContent("RPM-GPG-KEY-sharedkept by b");
    expect(
      within(row("a")).getByLabelText("/etc/yum.repos.d/a.repo: not shipped"),
    ).toBeTruthy();
  });

  it("posts include from every excluded row", () => {
    const onToggle = renderBody(excluded);
    for (const id of ["custom", "a", "baseos"]) {
      fireEvent.click(within(row(id)).getByRole("switch"));
      expect(onToggle).toHaveBeenLastCalledWith(id, true);
    }
  });
});

describe("Repositories rows on a fleet", () => {
  it("keeps the switch, and names owned files without a state", () => {
    const onToggle = vi.fn();
    render(<RepoBar repos={aggregate.repo_groups} onToggle={onToggle} />);
    // custom.repo and its key are on one host of two: narrowed out, so
    // the repo is kept with nothing carried, and the switch reads off.
    const sw = within(row("custom")).getByRole("switch", {
      name: "custom: repo files 0 of 2 carried",
    });
    expect(sw).not.toBeChecked();
    fireEvent.click(sw);
    expect(onToggle).toHaveBeenCalledWith("custom", true);
    expect(
      screen.getByLabelText("/etc/yum.repos.d/custom.repo"),
    ).toHaveTextContent("custom.repo");
    expect(screen.queryByTestId("repo-bar-unlisted")).toBeNull();
  });
});
