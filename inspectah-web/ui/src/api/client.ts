import type {
  ViewResponse,
  ContextSection,
  AnnotatedOp,
  ChangesSummary,
  HealthResponse,
  RefinementOp,
  UserPreviewResponse,
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

// --- Read endpoints ---

export function fetchHealth(): Promise<HealthResponse> {
  return getJson("/api/health");
}

export function fetchView(): Promise<ViewResponse> {
  return getJson("/api/view");
}

export function fetchSections(): Promise<ContextSection[]> {
  return getJson("/api/snapshot/sections");
}

export function fetchOps(): Promise<AnnotatedOp[]> {
  return getJson("/api/ops");
}

export function fetchChanges(): Promise<ChangesSummary> {
  return getJson("/api/changes");
}

export function fetchViewed(): Promise<{ ids: string[] }> {
  return getJson("/api/viewed");
}

// --- Mutation endpoints ---

export function applyOp(op: RefinementOp): Promise<ViewResponse> {
  return postJson("/api/op", op);
}

export function excludeRepo(sectionId: string): Promise<ViewResponse> {
  return applyOp({ op: "ExcludeRepo", target: { section_id: sectionId } });
}

export function includeRepo(sectionId: string): Promise<ViewResponse> {
  return applyOp({ op: "IncludeRepo", target: { section_id: sectionId } });
}

export function undo(): Promise<ViewResponse> {
  return postJson("/api/undo", {});
}

export function redo(): Promise<ViewResponse> {
  return postJson("/api/redo", {});
}

export async function markViewed(id: string): Promise<void> {
  const res = await fetch("/api/viewed", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ id }),
  });
  if (!res.ok && res.status !== 204) return parseJsonError(res);
}

export async function exportTarball(
  generation: number,
  acknowledgeSensitive = false,
): Promise<Blob> {
  const headers: Record<string, string> = {
    "Content-Type": "application/json",
  };
  if (acknowledgeSensitive) {
    headers["X-Acknowledge-Sensitive"] = "true";
  }
  const res = await fetch("/api/tarball", {
    method: "POST",
    headers,
    body: JSON.stringify({ generation }),
  });
  if (!res.ok) return parseJsonError(res);
  return res.blob();
}

// --- User decision endpoints ---

export function setUserStrategy(
  username: string,
  strategy: "skip" | "useradd",
): Promise<ViewResponse> {
  return postJson("/api/user-strategy", { username, strategy });
}

export function setUserPassword(
  username: string,
  choice: "none" | "preserve" | "new",
  hash?: string,
): Promise<ViewResponse> {
  return postJson("/api/user-password", { username, choice, hash });
}

export function fetchUserPreview(
  reveal = false,
): Promise<UserPreviewResponse> {
  const url = reveal ? "/api/user-preview?reveal=true" : "/api/user-preview";
  return getJson(url);
}
