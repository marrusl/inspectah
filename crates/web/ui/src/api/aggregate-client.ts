import type {
  AggregateViewResponse,
  AggregateDiffRequest,
  AggregateDiffResponse,
} from "./types";
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

// --- Aggregate endpoints ---

export function fetchAggregateView(): Promise<AggregateViewResponse> {
  return getJson("/api/aggregate/view");
}

export function fetchAggregateDiff(
  req: AggregateDiffRequest,
): Promise<AggregateDiffResponse> {
  return postJson("/api/aggregate/diff", req);
}
