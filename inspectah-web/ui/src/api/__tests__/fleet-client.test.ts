import { describe, it, expect, vi, beforeEach } from "vitest";
import type {
  FleetViewResponse,
  FleetDiffRequest,
  FleetDiffResponse,
} from "../types";
import { ApiError } from "../types";
import { fetchFleetView, fetchFleetDiff } from "../fleet-client";

// --- Test fixtures ---

const mockFleetView: FleetViewResponse = {
  generation: 1,
  can_undo: false,
  can_redo: false,
  containerfile_preview:
    "FROM registry.access.redhat.com/ubi9/ubi:latest\nRUN dnf install -y httpd",
  session_is_sensitive: false,
  summary: {
    host_count: 5,
    actionable_variant_items: [
      {
        item_id: { kind: "Package", key: { name: "httpd", arch: "x86_64" } },
        section_id: "packages",
        variant_count: 2,
        max_host_spread: 3,
      },
    ],
    informational_variant_count: 10,
  },
  sections: [
    {
      id: "packages",
      display_name: "Packages",
      is_decision_section: true,
      zones: {
        consensus: {
          items: [
            {
              item_id: {
                kind: "Package",
                key: { name: "httpd", arch: "x86_64" },
              },
              include: true,
              triage: {
                bucket: "investigate" as const,
                prevalence: { count: 4, total: 5 },
              },
              source_repo: "appstream",
              prevalence: {
                count: 4,
                total: 5,
              },
              variants: {
                count: 2,
                selected: "abc123",
                options: [
                  {
                    hash: "abc123",
                    hosts: ["host1", "host2", "host3"],
                    host_count: 3,
                    selected: true,
                  },
                  {
                    hash: "def456",
                    hosts: ["host4", "host5"],
                    host_count: 2,
                    selected: false,
                  },
                ],
              },
            },
          ],
          count: 1,
        },
        near_consensus: {
          items: [],
          count: 0,
        },
        divergent: {
          items: [],
          count: 0,
        },
      },
    },
  ],
  repo_groups: [],
  repo_conflict_count: 0,
};

const mockDiffRequest: FleetDiffRequest = {
  item_id: { kind: "Package", key: { name: "httpd", arch: "x86_64" } },
  base: "abc123",
  target: "def456",
};

const mockDiffResponse: FleetDiffResponse = {
  base_hash: "abc123",
  target_hash: "def456",
  base_hosts: ["host1", "host2", "host3"],
  target_hosts: ["host4", "host5"],
  hunks: [
    {
      base_range: { start: 1, count: 3 },
      target_range: { start: 1, count: 3 },
      changes: [
        { kind: "equal", content: "Name        : httpd" },
        { kind: "delete", content: "Version     : 2.4.51" },
        { kind: "insert", content: "Version     : 2.4.52" },
      ],
    },
  ],
  stats: {
    total_changes: 2,
    insertions: 1,
    deletions: 1,
  },
};

describe("fleet-client", () => {
  let mockFetch: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    vi.resetAllMocks();
    mockFetch = vi.fn();
    vi.stubGlobal("fetch", mockFetch);
  });

  describe("fetchFleetView", () => {
    it("fetches fleet view successfully", async () => {
      mockFetch.mockResolvedValue({
        ok: true,
        json: async () => mockFleetView,
      });

      const result = await fetchFleetView();

      expect(mockFetch).toHaveBeenCalledWith("/api/fleet/view", {
        method: "GET",
      });
      expect(result).toEqual(mockFleetView);
    });

    it("throws ApiError on non-200 response", async () => {
      mockFetch.mockResolvedValue({
        ok: false,
        status: 500,
        json: async () => ({ error: "Internal server error" }),
      });

      await expect(fetchFleetView()).rejects.toThrow(ApiError);
      await expect(fetchFleetView()).rejects.toMatchObject({
        status: 500,
        body: { error: "Internal server error" },
      });
    });
  });

  describe("fetchFleetDiff", () => {
    it("posts diff request and returns response", async () => {
      mockFetch.mockResolvedValue({
        ok: true,
        json: async () => mockDiffResponse,
      });

      const result = await fetchFleetDiff(mockDiffRequest);

      expect(mockFetch).toHaveBeenCalledWith("/api/fleet/diff", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(mockDiffRequest),
      });
      expect(result).toEqual(mockDiffResponse);
    });

    it("correctly serializes ItemId in request", async () => {
      mockFetch.mockResolvedValue({
        ok: true,
        json: async () => mockDiffResponse,
      });

      const configItemRequest: FleetDiffRequest = {
        item_id: {
          kind: "Config",
          key: { path: "/etc/httpd/conf/httpd.conf" },
        },
        base: "xyz789",
        target: "uvw012",
      };

      await fetchFleetDiff(configItemRequest);

      const callArgs = mockFetch.mock.calls[0];
      const requestBody = JSON.parse(callArgs[1].body);
      expect(requestBody.item_id).toEqual({
        kind: "Config",
        key: { path: "/etc/httpd/conf/httpd.conf" },
      });
    });

    it("throws ApiError on non-200 response", async () => {
      mockFetch.mockResolvedValue({
        ok: false,
        status: 400,
        json: async () => ({ error: "Invalid diff request" }),
      });

      await expect(fetchFleetDiff(mockDiffRequest)).rejects.toThrow(ApiError);
      await expect(fetchFleetDiff(mockDiffRequest)).rejects.toMatchObject({
        status: 400,
        body: { error: "Invalid diff request" },
      });
    });
  });
});
