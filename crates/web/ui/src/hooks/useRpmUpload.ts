import { useState, useCallback, useMemo } from "react";
import type { RpmUploadRowState } from "../api/types";

interface RepolessEntry {
  name: string;
  arch: string;
  repoless_annotation: string;
  repoless_cached: boolean;
}

interface ValidationResult {
  valid: boolean;
  error?: string;
}

interface BatchMatchResult {
  matched: Array<{ packageName: string; file: File }>;
  unmatched: File[];
  conflicts: Array<{ packageName: string; files: File[] }>;
}

export interface UseRpmUploadResult {
  /** Initialize from backend PackageEntry fields. */
  initFromBackend: (entries: RepolessEntry[]) => void;
  /** Get the current row state for a package. Returns undefined for non-repoless packages. */
  getRowState: (packageName: string) => RpmUploadRowState | undefined;
  /** Upload an RPM via POST /api/upload-rpm. Transitions to uploaded_excluded on success. */
  uploadRpm: (packageName: string, file: File) => Promise<void>;
  /** Remove a local upload, reverting to needs_upload. */
  removeUpload: (packageName: string) => void;
  /** Validate a filename against expected NEVRA. */
  validateFilename: (
    packageName: string,
    arch: string,
    filename: string,
  ) => ValidationResult;
  /** Number of packages still needing uploads. */
  needsUploadCount: number;
  /** Match multiple files to registered packages by name prefix. */
  batchMatch: (files: File[]) => BatchMatchResult;
  /** Apply a batch match result — upload all matched files via backend. */
  applyBatchMatch: (matched: BatchMatchResult["matched"]) => Promise<void>;
  /** Get names of packages that need RPM uploads. */
  needsUploadPackages: string[];
}

/** Extract the package name prefix from an RPM filename (before first hyphen followed by a digit). */
function extractPackageName(filename: string): string | null {
  const match = filename.match(/^(.+?)-\d/);
  return match ? match[1] : null;
}

export function useRpmUpload(): UseRpmUploadResult {
  const [repolessMap, setRepolessMap] = useState<Map<string, RepolessEntry>>(
    () => new Map(),
  );
  const [uploadedSet, setUploadedSet] = useState<Set<string>>(() => new Set());

  const initFromBackend = useCallback((entries: RepolessEntry[]) => {
    const map = new Map<string, RepolessEntry>();
    for (const e of entries) {
      map.set(e.name, e);
    }
    setRepolessMap(map);
  }, []);

  const getRowState = useCallback(
    (packageName: string): RpmUploadRowState | undefined => {
      const entry = repolessMap.get(packageName);
      if (!entry) return undefined;

      if (uploadedSet.has(packageName)) {
        return "uploaded_excluded";
      }
      if (entry.repoless_cached) {
        return "cached_excluded";
      }
      return "needs_upload";
    },
    [repolessMap, uploadedSet],
  );

  const uploadRpm = useCallback(async (packageName: string, file: File) => {
    const formData = new FormData();
    formData.append("file", file);

    const response = await fetch("/api/upload-rpm", {
      method: "POST",
      body: formData,
    });

    if (!response.ok) {
      throw new Error(`Upload failed: ${response.statusText}`);
    }

    setUploadedSet((prev) => {
      const next = new Set(prev);
      next.add(packageName);
      return next;
    });
  }, []);

  const removeUpload = useCallback((packageName: string) => {
    setUploadedSet((prev) => {
      const next = new Set(prev);
      next.delete(packageName);
      return next;
    });
  }, []);

  const validateFilename = useCallback(
    (packageName: string, arch: string, filename: string): ValidationResult => {
      if (!filename.endsWith(".rpm")) {
        return {
          valid: false,
          error: `File must end in .rpm, got "${filename}"`,
        };
      }

      const extractedName = extractPackageName(filename);
      if (!extractedName || extractedName !== packageName) {
        return {
          valid: false,
          error: `Filename must match package "${packageName}", got "${extractedName ?? filename}"`,
        };
      }

      const archPattern = `.${arch}.rpm`;
      if (
        !filename.endsWith(archPattern) &&
        !filename.endsWith(".noarch.rpm")
      ) {
        return {
          valid: false,
          error: `Expected architecture "${arch}" or "noarch", check filename`,
        };
      }

      return { valid: true };
    },
    [],
  );

  const needsUploadCount = useMemo(() => {
    let count = 0;
    for (const [name, entry] of repolessMap) {
      if (!entry.repoless_cached && !uploadedSet.has(name)) {
        count++;
      }
    }
    return count;
  }, [repolessMap, uploadedSet]);

  const needsUploadPackages = useMemo(() => {
    const pkgs: string[] = [];
    for (const [name, entry] of repolessMap) {
      if (!entry.repoless_cached && !uploadedSet.has(name)) {
        pkgs.push(name);
      }
    }
    return pkgs;
  }, [repolessMap, uploadedSet]);

  const batchMatch = useCallback(
    (files: File[]): BatchMatchResult => {
      const matched: BatchMatchResult["matched"] = [];
      const unmatched: File[] = [];
      const conflictMap = new Map<string, File[]>();

      for (const file of files) {
        const extractedName = extractPackageName(file.name);
        if (!extractedName || !repolessMap.has(extractedName)) {
          unmatched.push(file);
          continue;
        }

        const existing = conflictMap.get(extractedName);
        if (existing) {
          existing.push(file);
        } else if (matched.some((m) => m.packageName === extractedName)) {
          const prev = matched.find((m) => m.packageName === extractedName)!;
          conflictMap.set(extractedName, [prev.file, file]);
          matched.splice(matched.indexOf(prev), 1);
        } else {
          matched.push({ packageName: extractedName, file });
        }
      }

      return {
        matched,
        unmatched,
        conflicts: Array.from(conflictMap.entries()).map(
          ([packageName, conflictFiles]) => ({
            packageName,
            files: conflictFiles,
          }),
        ),
      };
    },
    [repolessMap],
  );

  const applyBatchMatch = useCallback(
    async (matched: BatchMatchResult["matched"]) => {
      for (const { packageName, file } of matched) {
        await uploadRpm(packageName, file);
      }
    },
    [uploadRpm],
  );

  return {
    initFromBackend,
    getRowState,
    uploadRpm,
    removeUpload,
    validateFilename,
    needsUploadCount,
    batchMatch,
    applyBatchMatch,
    needsUploadPackages,
  };
}
