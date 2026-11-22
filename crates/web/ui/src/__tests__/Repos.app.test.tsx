/// <reference types="vite/client" />
/**
 * The Repositories section through the whole application, against the
 * bodies the real handlers return for the repo host (the `repos` and
 * `repos-excluded` sessions of
 * `crates/web/tests/system_tuning_fixtures_test.rs`). `fetch` is the only
 * thing replaced.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import App from "../App";
import { ContainerfilePanel } from "../components/ContainerfilePanel";

const FIXTURES = import.meta.glob<{ default: unknown }>(
  "./fixtures/system-tuning/repos*.json",
  { eager: true },
);

function fixture(name: string): unknown {
  const entry = FIXTURES[`./fixtures/system-tuning/${name}.json`];
  if (!entry) throw new Error(`no recorded fixture ${name}.json`);
  return structuredClone(entry.default);
}

let routes: Record<string, unknown>;
const posts: unknown[] = [];

function serve(view: unknown, afterOp?: unknown) {
  routes = {
    "/api/health": fixture("repos.health"),
    "/api/groups": fixture("repos.groups"),
    "/api/view": view,
    "/api/snapshot/sections": [],
    "/api/viewed": { ids: [] },
    "/api/ops": [],
  };
  posts.length = 0;
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      if (url === "/api/op" && init?.method === "POST") {
        posts.push(JSON.parse(String(init.body)));
        if (afterOp !== undefined) routes["/api/view"] = afterOp;
        return { ok: true, status: 200, json: async () => routes["/api/view"] };
      }
      if (!(url in routes)) {
        return { ok: false, status: 404, json: async () => ({ error: url }) };
      }
      return { ok: true, status: 200, json: async () => routes[url] };
    }),
  );
}

beforeEach(() => {
  // jsdom has no layout; the reveal scrolls before it focuses.
  Element.prototype.scrollIntoView = vi.fn();
});
afterEach(() => {
  vi.unstubAllGlobals();
});

async function openSection(link: RegExp, heading: string) {
  const user = userEvent.setup();
  const nav = await screen.findByRole("navigation", { name: "Sections" });
  await user.click(await within(nav).findByRole("link", { name: link }));
  await screen.findByRole("heading", { level: 2, name: heading });
}

describe("a repo-owned Configuration row's link", () => {
  it("reveals the owning repo and focuses its switch", async () => {
    serve(fixture("repos.view"));
    render(<App />);
    await openSection(/^Configuration Files /, "Config Files");
    const row = await screen.findByTestId(
      "decision-item-configs:/etc/yum.repos.d/company.repo",
    );
    await userEvent.click(
      within(row).getByRole("button", { name: "Go to repo baseos" }),
    );
    await screen.findByRole("heading", { level: 2, name: "Packages" });
    const repoRow = await screen.findByTestId("repo-bar-row-baseos");
    await waitFor(() =>
      expect(within(repoRow).getByRole("switch")).toHaveFocus(),
    );
  });
});

describe("a package whose repo the export no longer carries", () => {
  it("offers the upload while the route is lost, and drops it once the repo is back", async () => {
    // baseos excluded: site-tool stays included and loses its route. The
    // view after the include is the scan's, where baseos is carried; the
    // client keeps the newest generation, so it is numbered after.
    const excluded = fixture("repos-excluded.view") as { generation: number };
    const restored = fixture("repos.view") as { generation: number };
    restored.generation = excluded.generation + 1;
    serve(excluded, restored);
    render(<App />);
    await screen.findByRole("heading", { level: 2, name: "Packages" });
    expect(
      await screen.findByRole("button", {
        name: "Upload RPM for site-tool.x86_64",
      }),
    ).toBeInTheDocument();

    const baseos = await screen.findByTestId("repo-bar-row-baseos");
    await userEvent.click(within(baseos).getByRole("switch"));
    await waitFor(() =>
      expect(posts).toContainEqual({
        kind: "Op",
        op: "SetInclude",
        target: {
          item_id: { kind: "Repo", key: { path: "baseos" } },
          include: true,
        },
      }),
    );
    // The view the server answers with carries baseos again, so
    // site-tool has no annotation and its row its checkbox back.
    await waitFor(() =>
      expect(
        screen.queryByRole("button", {
          name: "Upload RPM for site-tool.x86_64",
        }),
      ).toBeNull(),
    );
    expect(
      screen.getByRole("checkbox", { name: "site-tool.x86_64" }),
    ).toBeChecked();
  });
});

describe("the preview of a package whose repo the export no longer carries", () => {
  it("shows the carriage-only FIXME naming the switch that restores it", () => {
    const { containerfile_preview } = fixture("repos-excluded.view") as {
      containerfile_preview: string;
    };
    render(
      <ContainerfilePanel
        content={containerfile_preview}
        isOpen
        onToggle={vi.fn()}
        loading={false}
      />,
    );
    expect(document.body.textContent).toContain(
      "# FIXME: re-include repo baseos, which brings back its repo files and keys, or upload the RPM; site-tool installs from repo baseos, whose definition this export does not carry",
    );
  });
});
