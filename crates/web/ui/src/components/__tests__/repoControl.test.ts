import { describe, expect, it } from "vitest";
import type {
  RepoControl,
  RepoGroupInfo,
  RepoOwnedEntry,
  RepoOwnedExport,
} from "../../api/types";
import { ApiError } from "../../api/types";
import {
  hasRepoSwitch,
  inventoryRefusalText,
  owningSwitch,
  ownedStateWord,
  repoCountText,
  repoFailure,
  repoNoControlText,
  repoOwnedExplanation,
  repoSwitchChecked,
} from "../repoControl";
import excluded from "../../__tests__/fixtures/system-tuning/repos-excluded.view.json";
import scanned from "../../__tests__/fixtures/system-tuning/repos.view.json";

function repo(overrides: Partial<RepoGroupInfo>): RepoGroupInfo {
  return {
    section_id: "custom",
    provenance: "verified",
    is_distro: false,
    tier: "third_party",
    package_count: 2,
    effective_member_count: 2,
    effective_included: 2,
    effective_unlisted: 0,
    control: "full",
    kept: true,
    carriage_count: 2,
    carriage_included: 2,
    owns: [],
    ...overrides,
  };
}

/** The recorded rows, by section id. */
function recordedRow(
  view: { repo_groups: unknown[] },
  id: string,
): RepoGroupInfo {
  const row = (view.repo_groups as RepoGroupInfo[]).find(
    (r) => r.section_id === id,
  );
  if (!row) throw new Error(`no recorded row ${id}`);
  return row;
}

function recordedEntry(
  view: { repo_owned: unknown[] },
  destination: string,
): RepoOwnedEntry {
  const entry = (view.repo_owned as RepoOwnedEntry[]).find(
    (e) => e.destination === destination,
  );
  if (!entry) throw new Error(`no recorded entry ${destination}`);
  return entry;
}

describe("the repo switch (spec § 5.5, T8)", () => {
  // control, kept, carriage i/n, members i/n -> shown, checked
  const table: Array<
    [string, Partial<RepoGroupInfo>, boolean, boolean]
  > = [
    ["no control", { control: "none" }, false, false],
    [
      "zero members, kept, all carried",
      { effective_member_count: 0, effective_included: 0 },
      true,
      true,
    ],
    [
      "zero members, not kept, nothing carried",
      {
        effective_member_count: 0,
        effective_included: 0,
        kept: false,
        carriage_included: 0,
      },
      true,
      false,
    ],
    [
      "zero members, not kept, a file kept by another repo",
      {
        effective_member_count: 0,
        effective_included: 0,
        kept: false,
        carriage_included: 1,
      },
      true,
      false,
    ],
    [
      "members all included, not kept",
      { kept: false, carriage_included: 0 },
      true,
      false,
    ],
    [
      "kept with carriage incomplete",
      { carriage_included: 1 },
      true,
      false,
    ],
    [
      "kept, a member excluded",
      { effective_included: 1 },
      true,
      false,
    ],
    [
      "aggregate narrowing: kept, nothing carried, no members",
      {
        effective_member_count: 0,
        effective_included: 0,
        carriage_included: 0,
      },
      true,
      false,
    ],
    [
      "carriage-only distro row with mixed members",
      {
        control: "carriage_only",
        is_distro: true,
        tier: "distro",
        effective_member_count: 10,
        effective_included: 4,
      },
      true,
      true,
    ],
    [
      "carriage-only row, not kept",
      { control: "carriage_only", kept: false, carriage_included: 0 },
      true,
      false,
    ],
  ];

  it.each(table)("%s", (_name, overrides, shown, checked) => {
    const r = repo(overrides);
    expect(hasRepoSwitch(r)).toBe(shown);
    if (shown) {
      expect(repoSwitchChecked(r)).toBe(checked);
    }
  });

  it("reads the recorded rows the way the server decided them", () => {
    expect(repoSwitchChecked(recordedRow(scanned, "custom"))).toBe(true);
    expect(repoSwitchChecked(recordedRow(excluded, "custom"))).toBe(false);
    // `a` is excluded and its shared key still ships for `b`.
    const a = recordedRow(excluded, "a");
    expect(a.carriage_included).toBe(1);
    expect(repoSwitchChecked(a)).toBe(false);
    // A carriage-only distro row: excluded, its packages untouched.
    const baseos = recordedRow(excluded, "baseos");
    expect(baseos.control).toBe("carriage_only");
    expect(baseos.effective_included).toBe(baseos.effective_member_count);
    expect(repoSwitchChecked(baseos)).toBe(false);
  });
});

/**
 * A stateful fake of the server's no-op rule (spec § 5.3): an op is
 * accepted when it moves a member, a file or key, or the decision.
 * Extends the refine-ui-fixup spec's test 12 fake with the decision and
 * the carriage.
 */
interface FakeRepo {
  control: RepoControl;
  kept: boolean;
  members: boolean[];
  /** Locked members never move. */
  locked: boolean[];
  /** Each carriable entry, and whether another kept repo keeps it. */
  carriage: { included: boolean; keptElsewhere: boolean }[];
}

function asRow(f: FakeRepo): RepoGroupInfo {
  const effective = f.members.filter((_, i) => !f.locked[i]);
  return repo({
    control: f.control,
    kept: f.kept,
    effective_member_count: effective.length,
    effective_included: effective.filter(Boolean).length,
    carriage_count: f.carriage.length,
    carriage_included: f.carriage.filter((c) => c.included).length,
  });
}

/** Applies the op, or returns the 422 text the server would send. */
function serverApply(f: FakeRepo, include: boolean): string | null {
  if (f.control === "none") return "nothing to decide";
  const before = JSON.stringify(f);
  const next: FakeRepo = structuredClone(f);
  next.kept = include;
  for (const c of next.carriage) {
    c.included = include || c.keptElsewhere;
  }
  if (next.control === "full") {
    next.members = next.members.map((m, i) => (next.locked[i] ? m : include));
  }
  if (JSON.stringify(next) === before) {
    return include ? "already kept" : "already excluded";
  }
  Object.assign(f, next);
  return null;
}

function* fakeStates(): Generator<FakeRepo> {
  const controls: RepoControl[] = ["full", "carriage_only"];
  const memberSets: Array<[boolean[], boolean[]]> = [
    [[], []],
    [[true, true], [false, false]],
    [[true, false], [false, false]],
    [[false, false], [false, false]],
    [[false, true], [true, false]],
  ];
  const carriageSets = [
    [],
    [{ included: true, keptElsewhere: false }],
    [{ included: false, keptElsewhere: false }],
    [
      { included: true, keptElsewhere: true },
      { included: false, keptElsewhere: false },
    ],
  ];
  for (const control of controls) {
    for (const kept of [true, false]) {
      for (const [members, locked] of memberSets) {
        for (const carriage of carriageSets) {
          const hasMember = locked.some((l) => !l);
          // The server gives no switch where an op could move nothing.
          if (control === "full" && !hasMember && carriage.length === 0)
            continue;
          if (control === "carriage_only" && carriage.length === 0) continue;
          yield { control, kept, members, locked, carriage };
        }
      }
    }
  }
}

describe("a click sequence never posts a refused op (T8)", () => {
  it("over every state, six clicks in a row", () => {
    let checked = 0;
    for (const start of fakeStates()) {
      const f = structuredClone(start);
      for (let click = 0; click < 6; click++) {
        const row = asRow(f);
        expect(hasRepoSwitch(row)).toBe(true);
        const refused = serverApply(f, !repoSwitchChecked(row));
        expect(refused, JSON.stringify(start)).toBeNull();
        checked++;
      }
    }
    expect(checked).toBeGreaterThan(100);
  });
});

describe("the count text (spec § 7.2)", () => {
  it("names members, then files only when some are left out", () => {
    expect(repoCountText(repo({}))).toBe("2 of 2 packages included");
    expect(repoCountText(repo({ carriage_included: 1 }))).toBe(
      "2 of 2 packages included, repo files 1 of 2 carried",
    );
    expect(
      repoCountText(
        repo({ effective_included: 0, carriage_included: 0, kept: false }),
      ),
    ).toBe("2 packages excluded, repo files 0 of 2 carried, excluded in refine");
  });

  it("names files alone with no members and for a carriage-only switch", () => {
    expect(
      repoCountText(repo({ effective_member_count: 0, effective_included: 0 })),
    ).toBe("repo files 2 of 2 carried");
    expect(repoCountText(repo({ control: "carriage_only" }))).toBe(
      "repo files 2 of 2 carried",
    );
    expect(repoCountText(recordedRow(excluded, "baseos"))).toBe(
      "repo files 0 of 2 carried, excluded in refine",
    );
  });

  it("gives a row with no switch its reason", () => {
    expect(repoNoControlText(repo({ control: "none", is_distro: true }))).toBe(
      "always included",
    );
    expect(
      repoNoControlText(repo({ control: "none", provenance: "disabled" })),
    ).toBe("disabled on host");
    expect(repoNoControlText(repo({ control: "none" }))).toBe(
      "nothing to decide: no package to change and no file or key the export can carry",
    );
  });
});

function entry(exp: RepoOwnedExport, overrides: Partial<RepoOwnedEntry> = {}) {
  return {
    destination: "/etc/yum.repos.d/x.repo",
    recorded_paths: ["/etc/yum.repos.d/x.repo"],
    kind: "repo_file" as const,
    owners: [{ section_id: "x", control: "full" as const, kept: true }],
    export: exp,
    ...overrides,
  };
}

describe("owned entries (spec § 7.2 and § 7.3)", () => {
  it("gives one state word per export state", () => {
    expect(ownedStateWord(entry({ state: "ships", kept_by: [] }))).toBe("");
    expect(
      ownedStateWord(
        entry({ state: "ships", kept_by: [{ section_id: "b" }] }),
      ),
    ).toBe("kept by b");
    expect(ownedStateWord(entry({ state: "not_shipped" }))).toBe(
      "not shipped",
    );
    expect(
      ownedStateWord(entry({ state: "base_image", package: "epel-release" })),
    ).toBe("base image");
    expect(ownedStateWord(entry({ state: "default_repo_file" }))).toBe(
      "default repo file",
    );
    expect(
      ownedStateWord(entry({ state: "rejected", reason: "not_pgp" })),
    ).toBe("rejected: is not a PGP public key");
    expect(
      ownedStateWord(entry({ state: "withheld", reason: "a credential." })),
    ).toBe("withheld: a credential");
  });

  it("explains each export state", () => {
    const shared = recordedEntry(excluded, "/etc/pki/rpm-gpg/RPM-GPG-KEY-shared");
    expect(repoOwnedExplanation(shared)).toBe(
      "This file belongs to repo a, b. Its switch under Repositories, in " +
        "Packages, decides whether the file ships. Repo a is excluded, but " +
        "repo b still needs this file, so it ships.",
    );
    expect(
      repoOwnedExplanation(
        entry(
          {
            state: "ships",
            kept_by: [{ section_id: "d", enablerepo_package: "pd" }],
          },
          {
            owners: [
              { section_id: "a", control: "full", kept: false },
              { section_id: "d", control: "carriage_only", kept: true },
            ],
          },
        ),
      ),
    ).toContain(
      "Repo a is excluded, but repo d, whose package pd installs with " +
        "--enablerepo still needs this file, so it ships.",
    );
    expect(
      repoOwnedExplanation(
        recordedEntry(excluded, "/etc/yum.repos.d/custom.repo"),
      ),
    ).toBe(
      "This file belongs to repo custom, and the export leaves it out. Its " +
        "switch under Repositories decides whether it ships.",
    );
    expect(
      repoOwnedExplanation(
        entry({ state: "base_image", package: "epel-release" }),
      ),
    ).toBe(
      "The base image ships this key (epel-release), so the export never copies it.",
    );
    expect(
      repoOwnedExplanation(
        recordedEntry(scanned, "/etc/yum.repos.d/redhat.repo"),
      ),
    ).toBe(
      "This is a default repo file, which the export never copies. The " +
        "build is expected to get it from the base image or the " +
        "subscription mount.",
    );
    expect(
      repoOwnedExplanation(
        recordedEntry(scanned, "/etc/pki/rpm-gpg/RPM-GPG-KEY-notpgp"),
      ),
    ).toBe(
      "The scan did not store this file because it is not a PGP public " +
        "key, so the export never copies it.",
    );
    expect(
      repoOwnedExplanation(
        entry({ state: "rejected", reason: "private_key_material" }),
      ),
    ).toContain("because it holds private key material");
    expect(
      repoOwnedExplanation(
        entry({ state: "withheld", reason: "it holds a credential" }),
      ),
    ).toBe("The export withholds this file: it holds a credential.");
  });

  it("explains a file or key no repo owns", () => {
    expect(
      repoOwnedExplanation(
        recordedEntry(scanned, "/etc/pki/rpm-gpg/token=vendor"),
      ),
    ).toBe(
      "The scan found this key because a repo file named it, but redaction " +
        "changed that gpgkey= reference (/etc/yum.repos.d/vend.repo), so no " +
        "repo file the export copies names it and the export never copies " +
        "it. Review the redacted reference.",
    );
    const headers = entry(
      {
        state: "no_owner",
        cause: { kind: "redacted", repo_files: ["/etc/yum.repos.d/x.repo"] },
      },
      { owners: [] },
    );
    expect(repoOwnedExplanation(headers)).toBe(
      "Redaction changed the [section] headers in this file, so it defines " +
        "no repo and the export never copies it. Review the redacted headers.",
    );
    expect(
      repoOwnedExplanation({
        ...headers,
        export: {
          state: "no_owner",
          cause: { kind: "redacted", repo_files: [] },
        },
      }),
    ).toMatch(/^Its \[section\] headers were changed after the scan, /);
    expect(
      repoOwnedExplanation(
        entry(
          {
            state: "no_owner",
            cause: {
              kind: "fleet",
              hosts: ["h1", "h2"],
              redacted_repo_files: [],
            },
          },
          { kind: "gpg_key", owners: [] },
        ),
      ),
    ).toBe(
      "Every host that has this key had it because a repo file named it, " +
        "but no version of a repo file that the export copies names it, so " +
        "the export never copies it. Hosts with it: h1, h2.",
    );
  });

  it("links to the first owner, by section id, that has a switch", () => {
    expect(
      owningSwitch(recordedEntry(scanned, "/etc/yum.repos.d/epel.repo")),
    ).toBe("epel");
    expect(
      owningSwitch(recordedEntry(scanned, "/etc/pki/rpm-gpg/RPM-GPG-KEY-shared")),
    ).toBe("a");
    expect(
      owningSwitch(recordedEntry(scanned, "/etc/yum.repos.d/redhat.repo")),
    ).toBeNull();
    expect(
      owningSwitch(recordedEntry(scanned, "/etc/pki/rpm-gpg/token=vendor")),
    ).toBeNull();
  });
});

describe("refusals", () => {
  it("states a repo no-op as information, and anything else as not saved", () => {
    expect(
      repoFailure(
        new ApiError(422, {
          error:
            "nothing to change: repo 'custom' is already kept, with its files and keys carried and every effective member included",
        }),
      ),
    ).toEqual({
      kind: "nothing_to_change",
      text: "Nothing to change: repo 'custom' is already kept, with its files and keys carried and every effective member included.",
    });
    expect(repoFailure(new Error("boom"))).toEqual({
      kind: "failed",
      text: "The change was not saved: boom",
    });
  });

  it("states an inventory refusal as information", () => {
    expect(
      inventoryRefusalText(
        new ApiError(422, {
          error:
            "inventory finding is not toggleable: /etc/yum.repos.d/custom.repo",
        }),
      ),
    ).toBe(
      "Nothing was changed: /etc/yum.repos.d/custom.repo is decided by the section that owns it, not by this row.",
    );
    expect(inventoryRefusalText(new ApiError(422, { error: "other" }))).toBe(
      null,
    );
  });
});
