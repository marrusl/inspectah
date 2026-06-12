import { describe, it, expect, vi, beforeEach } from "vitest";
import type {
  ViewResponse,
  ReferenceSection,
  AnnotatedTimelineEntry,
  ChangesSummary,
  HealthResponse,
  RefinementOp,
} from "../types";
import { ApiError } from "../types";

// Import will be resolved after client.ts is created
import {
  fetchHealth,
  fetchView,
  fetchSections,
  fetchOps,
  fetchChanges,
  fetchViewed,
  applyOp,
  undo,
  redo,
  markViewed,
  exportTarball,
} from "../client";

// --- Test fixtures ---

const mockHealth: HealthResponse = {
  status: "ok",
  host: {
    hostname: "test-host",
    os_name: "Red Hat Enterprise Linux",
    os_version: "9.4",
    os_id: "rhel",
    system_type: "package-mode",
    schema_version: 14,
  },
  completeness: "full",
  policy: { distro_repos: ["baseos", "appstream", "crb"] },
  fleet: null,
  session_is_sensitive: false,
};

const mockView: ViewResponse = {
  packages: [
    {
      entry: {
        name: "httpd",
        epoch: "0",
        version: "2.4.57",
        release: "11.el9",
        arch: "x86_64",
        state: "added",
        include: true,
        source_repo: "appstream",
        fleet: null,
      },
      attention: [
        {
          level: "needs_review",
          reason: "package_user_added",
          detail: null,
        },
      ],
      triage: {
        triage: { mode: "single_host" as const, investigate: null },
        primary_reason: "package_user_added" as const,
        annotations: [],
      },
    },
  ],
  config_files: [
    {
      entry: {
        path: "/etc/httpd/conf/httpd.conf",
        kind: "rpm_owned_modified",
        category: "other",
        content: "ServerRoot /etc/httpd",
        rpm_va_flags: "5",
        package: "httpd",
        diff_against_rpm: "--- a\n+++ b",
        include: true,
        tie: false,
        tie_winner: false,
        fleet: null,
      },
      attention: [
        {
          level: "needs_review",
          reason: "config_modified",
          detail: "Modified from RPM default",
        },
      ],
      triage: {
        triage: { mode: "single_host" as const, investigate: null },
        primary_reason: "config_modified" as const,
        annotations: [],
      },
    },
  ],
  containerfile_preview:
    "FROM registry.redhat.io/rhel9:latest\nRUN dnf install -y httpd",
  stats: {
    sections: [
      { kind: "package", total: 10, included: 8, excluded: 2 },
      { kind: "config", total: 5, included: 4, excluded: 1 },
    ],
    needs_review_count: 3,
    ops_applied: 1,
    can_undo: true,
    can_redo: false,
    baseline_available: true,
  },
  generation: 1,
  repo_groups: [
    {
      section_id: "appstream",
      provenance: "verified",
      is_distro: true,
      tier: "distro",
      package_count: 1,
      enabled: true,
    },
  ],
  version_changes: [],
  service_states: [],
  service_dropins: [],
  quadlets: [],
  flatpaks: [],
  sysctls: [],
  tuned: [],
  users_groups_decisions: [],
  package_groups: [],
  session_is_sensitive: false,
};

const mockSections: ReferenceSection[] = [
  {
    id: "services",
    display_name: "Systemd Services",
    items: [
      {
        id: "httpd.service",
        title: "httpd.service",
        subtitle: "enabled",
        detail: "Apache HTTP Server",
        searchable_text: "httpd.service enabled Apache HTTP Server",
      },
    ],
  },
];

// Mock data for /api/ops endpoint (AnnotatedTimelineEntry[])
const mockOpsResponse: AnnotatedTimelineEntry[] = [
  {
    kind: "Op" as const,
    op: "ExcludePackage" as const,
    target: { name: "nano", arch: "x86_64" },
    active: true,
  },
  {
    kind: "Op" as const,
    op: "IncludePackage" as const,
    target: { name: "nano", arch: "x86_64" },
    active: false,
  },
];

// fetchOps now returns AnnotatedTimelineEntry[] directly (no filtering)
const mockOps = mockOpsResponse;

const mockChanges: ChangesSummary = {
  packages_included: [],
  packages_excluded: [{ name: "nano", arch: "x86_64" }],
  configs_included: [],
  configs_excluded: ["/etc/nanorc"],
  repos_excluded: [],
  variants_changed: 0,
  is_dirty: true,
};

// --- Test helpers ---

function mockFetchSuccess(body: unknown, status = 200): void {
  vi.spyOn(globalThis, "fetch").mockResolvedValueOnce(
    new Response(JSON.stringify(body), {
      status,
      headers: { "Content-Type": "application/json" },
    }),
  );
}

function mockFetchNoContent(): void {
  vi.spyOn(globalThis, "fetch").mockResolvedValueOnce(
    new Response(null, { status: 204 }),
  );
}

function mockFetchError(status: number, error: string): void {
  vi.spyOn(globalThis, "fetch").mockResolvedValueOnce(
    new Response(JSON.stringify({ error }), {
      status,
      headers: { "Content-Type": "application/json" },
    }),
  );
}

function mockFetchBlob(data: ArrayBuffer): void {
  vi.spyOn(globalThis, "fetch").mockResolvedValueOnce(
    new Response(data, {
      status: 200,
      headers: {
        "Content-Type": "application/gzip",
        "Content-Disposition": 'attachment; filename="export.tar.gz"',
      },
    }),
  );
}

function lastFetchCall(): { url: string; init: RequestInit } {
  const calls = vi.mocked(globalThis.fetch).mock.calls;
  const [url, init] = calls[calls.length - 1];
  return { url: url as string, init: init as RequestInit };
}

// --- Tests ---

beforeEach(() => {
  vi.restoreAllMocks();
});

describe("GET endpoints", () => {
  describe("fetchHealth", () => {
    it("sends GET to /api/health and returns typed response", async () => {
      mockFetchSuccess(mockHealth);
      const result = await fetchHealth();
      const { url, init } = lastFetchCall();
      expect(url).toBe("/api/health");
      expect(init.method).toBe("GET");
      expect(result).toEqual(mockHealth);
    });

    it("throws ApiError on server error", async () => {
      mockFetchError(500, "internal server error");
      try {
        await fetchHealth();
        expect.fail("should have thrown");
      } catch (e) {
        expect(e).toBeInstanceOf(ApiError);
        expect((e as ApiError).status).toBe(500);
        expect((e as ApiError).body.error).toBe("internal server error");
      }
    });
  });

  describe("fetchView", () => {
    it("sends GET to /api/view and returns ViewResponse", async () => {
      mockFetchSuccess(mockView);
      const result = await fetchView();
      const { url, init } = lastFetchCall();
      expect(url).toBe("/api/view");
      expect(init.method).toBe("GET");
      expect(result).toEqual(mockView);
      expect(result.packages).toHaveLength(1);
      expect(result.stats.can_undo).toBe(true);
    });
  });

  describe("fetchSections", () => {
    it("sends GET to /api/snapshot/sections and returns ReferenceSection[]", async () => {
      mockFetchSuccess(mockSections);
      const result = await fetchSections();
      const { url, init } = lastFetchCall();
      expect(url).toBe("/api/snapshot/sections");
      expect(init.method).toBe("GET");
      expect(result).toEqual(mockSections);
      expect(result[0].items[0].searchable_text).toContain("httpd");
    });
  });

  describe("fetchOps", () => {
    it("sends GET to /api/ops and returns AnnotatedTimelineEntry[]", async () => {
      mockFetchSuccess(mockOpsResponse);
      const result = await fetchOps();
      const { url, init } = lastFetchCall();
      expect(url).toBe("/api/ops");
      expect(init.method).toBe("GET");
      expect(result).toEqual(mockOps);
      expect(result[0].active).toBe(true);
      expect(result[1].active).toBe(false);
    });
  });

  describe("fetchChanges", () => {
    it("sends GET to /api/changes and returns ChangesSummary", async () => {
      mockFetchSuccess(mockChanges);
      const result = await fetchChanges();
      const { url, init } = lastFetchCall();
      expect(url).toBe("/api/changes");
      expect(init.method).toBe("GET");
      expect(result.is_dirty).toBe(true);
      expect(result.packages_excluded).toHaveLength(1);
    });
  });

  describe("fetchViewed", () => {
    it("sends GET to /api/viewed and returns id list", async () => {
      const viewedData = {
        ids: ["packages:httpd.x86_64", "services:sshd.service"],
      };
      mockFetchSuccess(viewedData);
      const result = await fetchViewed();
      const { url, init } = lastFetchCall();
      expect(url).toBe("/api/viewed");
      expect(init.method).toBe("GET");
      expect(result.ids).toHaveLength(2);
    });
  });
});

describe("POST mutation endpoints", () => {
  describe("applyOp", () => {
    it("sends POST to /api/op with RefinementOp body", async () => {
      mockFetchSuccess(mockView);
      const op: RefinementOp = {
        op: "ExcludePackage",
        target: { name: "nano", arch: "x86_64" },
      };
      const result = await applyOp(op);
      const { url, init } = lastFetchCall();
      expect(url).toBe("/api/op");
      expect(init.method).toBe("POST");
      expect(init.headers).toEqual(
        expect.objectContaining({ "Content-Type": "application/json" }),
      );
      // applyOp wraps the RefinementOp in a TimelineEntry
      expect(JSON.parse(init.body as string)).toEqual({ kind: "Op", ...op });
      expect(result).toEqual(mockView);
    });

    it("sends ExcludeConfig op correctly", async () => {
      mockFetchSuccess(mockView);
      const op: RefinementOp = {
        op: "ExcludeConfig",
        target: { path: "/etc/httpd/conf/httpd.conf" },
      };
      await applyOp(op);
      const { init } = lastFetchCall();
      const body = JSON.parse(init.body as string);
      expect(body.kind).toBe("Op");
      expect(body.op).toBe("ExcludeConfig");
      expect(body.target.path).toBe("/etc/httpd/conf/httpd.conf");
    });

    it("throws ApiError on 422 unknown target", async () => {
      mockFetchError(422, "unknown target: nonexistent.x86_64");
      const op: RefinementOp = {
        op: "ExcludePackage",
        target: { name: "nonexistent", arch: "x86_64" },
      };
      try {
        await applyOp(op);
        expect.fail("should have thrown");
      } catch (e) {
        expect(e).toBeInstanceOf(ApiError);
        expect((e as ApiError).status).toBe(422);
        expect((e as ApiError).body.error).toContain("unknown target");
      }
    });
  });

  describe("undo", () => {
    it("sends POST to /api/undo with empty body", async () => {
      mockFetchSuccess(mockView);
      const result = await undo();
      const { url, init } = lastFetchCall();
      expect(url).toBe("/api/undo");
      expect(init.method).toBe("POST");
      expect(init.headers).toEqual(
        expect.objectContaining({ "Content-Type": "application/json" }),
      );
      expect(JSON.parse(init.body as string)).toEqual({});
      expect(result).toEqual(mockView);
    });

    it("throws ApiError on 409 nothing to undo", async () => {
      mockFetchError(409, "nothing to undo");
      try {
        await undo();
        expect.fail("should have thrown");
      } catch (e) {
        expect(e).toBeInstanceOf(ApiError);
        expect((e as ApiError).status).toBe(409);
      }
    });
  });

  describe("redo", () => {
    it("sends POST to /api/redo with empty body", async () => {
      mockFetchSuccess(mockView);
      const result = await redo();
      const { url, init } = lastFetchCall();
      expect(url).toBe("/api/redo");
      expect(init.method).toBe("POST");
      expect(JSON.parse(init.body as string)).toEqual({});
      expect(result).toEqual(mockView);
    });

    it("throws ApiError on 409 nothing to redo", async () => {
      mockFetchError(409, "nothing to redo");
      try {
        await redo();
        expect.fail("should have thrown");
      } catch (e) {
        expect(e).toBeInstanceOf(ApiError);
        expect((e as ApiError).status).toBe(409);
      }
    });
  });

  describe("markViewed", () => {
    it("sends POST to /api/viewed with id body and handles 204", async () => {
      mockFetchNoContent();
      await markViewed("packages:httpd.x86_64");
      const { url, init } = lastFetchCall();
      expect(url).toBe("/api/viewed");
      expect(init.method).toBe("POST");
      expect(JSON.parse(init.body as string)).toEqual({
        id: "packages:httpd.x86_64",
      });
    });

    it("does not throw on 204 No Content", async () => {
      mockFetchNoContent();
      await expect(
        markViewed("services:sshd.service"),
      ).resolves.toBeUndefined();
    });
  });

  describe("exportTarball", () => {
    it("sends POST to /api/tarball with generation and returns Blob", async () => {
      const tarData = new Uint8Array([0x1f, 0x8b, 0x08, 0x00]).buffer;
      mockFetchBlob(tarData);
      const result = await exportTarball(7);
      const { url, init } = lastFetchCall();
      expect(url).toBe("/api/tarball");
      expect(init.method).toBe("POST");
      expect(JSON.parse(init.body as string)).toEqual({ generation: 7 });
      expect(result).toBeInstanceOf(Blob);
      expect(result.size).toBe(4);
    });

    it("throws ApiError on 409 stale generation", async () => {
      mockFetchError(409, "stale generation: expected 7, got 5");
      try {
        await exportTarball(5);
        expect.fail("should have thrown");
      } catch (e) {
        expect(e).toBeInstanceOf(ApiError);
        expect((e as ApiError).status).toBe(409);
        expect((e as ApiError).body.error).toContain("stale generation");
      }
    });
  });
});

describe("error handling", () => {
  it("includes status code in ApiError", async () => {
    mockFetchError(400, "bad request");
    try {
      await fetchView();
      expect.fail("should have thrown");
    } catch (e) {
      expect(e).toBeInstanceOf(ApiError);
      const apiErr = e as ApiError;
      expect(apiErr.status).toBe(400);
      expect(apiErr.body).toEqual({ error: "bad request" });
      expect(apiErr.message).toBe("bad request");
    }
  });

  it("ApiError extends Error", () => {
    const err = new ApiError(404, { error: "not found" });
    expect(err).toBeInstanceOf(Error);
    expect(err.name).toBe("ApiError");
    expect(err.message).toBe("not found");
  });
});
