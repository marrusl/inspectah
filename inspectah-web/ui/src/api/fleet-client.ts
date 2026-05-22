import type { FleetViewResponse, FleetDiffRequest, FleetDiffResponse } from "./types";
import { ApiError } from "./types";

// --- Internal helpers ---

async function parseJsonError(res: Response): Promise<never> {
  const body = await res.json();
  throw new ApiError(res.status, body);
}

async function getJson<T>(url: string): Promise<T> {
  const res = await fetch(url, { method: "GET" });
  if (!res.ok) return parseJsonError(res);
  return res.json() as Promise<T>;
}

async function postJson<T>(url: string, body: unknown): Promise<T> {
  const res = await fetch(url, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  if (!res.ok) return parseJsonError(res);
  return res.json() as Promise<T>;
}

// --- Fleet endpoints ---

export function fetchFleetView(): Promise<FleetViewResponse> {
  return getJson("/api/fleet/view");
}

export function fetchFleetDiff(req: FleetDiffRequest): Promise<FleetDiffResponse> {
  return postJson("/api/fleet/diff", req);
}
