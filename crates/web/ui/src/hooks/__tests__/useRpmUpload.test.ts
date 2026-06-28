import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook, act } from "@testing-library/react";
import { useRpmUpload } from "../useRpmUpload";

// Mock fetch for POST /api/upload-rpm
const mockFetch = vi.fn();
globalThis.fetch = mockFetch;

beforeEach(() => {
  mockFetch.mockReset();
  mockFetch.mockResolvedValue({ ok: true, json: async () => ({ ok: true }) });
});

describe("useRpmUpload", () => {
  it("derives cached_excluded state from backend fields", () => {
    const { result } = renderHook(() => useRpmUpload());
    act(() => {
      result.current.initFromBackend([
        {
          name: "custom-tool",
          arch: "x86_64",
          repoless_annotation: "No repo source — cached RPM bundled",
          repoless_cached: true,
        },
      ]);
    });
    expect(result.current.getRowState("custom-tool.x86_64")).toBe(
      "cached_excluded",
    );
  });

  it("derives needs_upload state from backend fields when not cached", () => {
    const { result } = renderHook(() => useRpmUpload());
    act(() => {
      result.current.initFromBackend([
        {
          name: "my-agent",
          arch: "x86_64",
          repoless_annotation: "No repo source — manual resolution needed",
          repoless_cached: false,
        },
      ]);
    });
    expect(result.current.getRowState("my-agent.x86_64")).toBe("needs_upload");
  });

  it("returns undefined for non-repoless packages", () => {
    const { result } = renderHook(() => useRpmUpload());
    act(() => {
      result.current.initFromBackend([]);
    });
    expect(
      result.current.getRowState("normal-package.x86_64"),
    ).toBeUndefined();
  });

  it("uploadRpm calls POST /api/upload-rpm and transitions state", async () => {
    const { result } = renderHook(() => useRpmUpload());
    act(() => {
      result.current.initFromBackend([
        {
          name: "nginx",
          arch: "x86_64",
          repoless_annotation: "manual resolution",
          repoless_cached: false,
        },
      ]);
    });
    const mockFile = new File(
      ["rpm-content"],
      "nginx-1.24-1.el9.x86_64.rpm",
      {
        type: "application/x-rpm",
      },
    );
    await act(async () => {
      await result.current.uploadRpm("nginx.x86_64", mockFile);
    });
    // Verify POST was called
    expect(mockFetch).toHaveBeenCalledWith(
      "/api/upload-rpm",
      expect.objectContaining({ method: "POST" }),
    );
    expect(result.current.getRowState("nginx.x86_64")).toBe(
      "uploaded_excluded",
    );
  });

  it("removeUpload transitions back to needs_upload", async () => {
    const { result } = renderHook(() => useRpmUpload());
    act(() => {
      result.current.initFromBackend([
        {
          name: "nginx",
          arch: "x86_64",
          repoless_annotation: "manual resolution",
          repoless_cached: false,
        },
      ]);
    });
    const mockFile = new File(["rpm-content"], "nginx-1.24-1.el9.x86_64.rpm");
    await act(async () => {
      await result.current.uploadRpm("nginx.x86_64", mockFile);
    });
    expect(result.current.getRowState("nginx.x86_64")).toBe(
      "uploaded_excluded",
    );
    act(() => {
      result.current.removeUpload("nginx.x86_64");
    });
    expect(result.current.getRowState("nginx.x86_64")).toBe("needs_upload");
  });

  it("validateFilename accepts matching NEVRA", () => {
    const { result } = renderHook(() => useRpmUpload());
    expect(
      result.current.validateFilename(
        "nginx",
        "x86_64",
        "nginx-1.24-1.el9.x86_64.rpm",
      ),
    ).toEqual({ valid: true });
  });

  it("validateFilename rejects wrong package name", () => {
    const { result } = renderHook(() => useRpmUpload());
    const validation = result.current.validateFilename(
      "nginx",
      "x86_64",
      "httpd-2.4-1.el9.x86_64.rpm",
    );
    expect(validation.valid).toBe(false);
    expect(validation.error).toContain("nginx");
  });

  it("validateFilename rejects non-.rpm extension", () => {
    const { result } = renderHook(() => useRpmUpload());
    const validation = result.current.validateFilename(
      "nginx",
      "x86_64",
      "nginx-1.24-1.el9.x86_64.tar.gz",
    );
    expect(validation.valid).toBe(false);
    expect(validation.error).toContain(".rpm");
  });

  it("needsUploadCount counts packages still needing uploads", async () => {
    const { result } = renderHook(() => useRpmUpload());
    act(() => {
      result.current.initFromBackend([
        {
          name: "nginx",
          arch: "x86_64",
          repoless_annotation: "manual",
          repoless_cached: false,
        },
        {
          name: "custom-agent",
          arch: "x86_64",
          repoless_annotation: "manual",
          repoless_cached: false,
        },
        {
          name: "my-tool",
          arch: "x86_64",
          repoless_annotation: "manual",
          repoless_cached: false,
        },
      ]);
    });
    expect(result.current.needsUploadCount).toBe(3);
    const mockFile = new File(["rpm"], "nginx-1.0-1.el9.x86_64.rpm");
    await act(async () => {
      await result.current.uploadRpm("nginx.x86_64", mockFile);
    });
    expect(result.current.needsUploadCount).toBe(2);
  });

  it("batchMatch matches files to packages by name.arch", () => {
    const { result } = renderHook(() => useRpmUpload());
    act(() => {
      result.current.initFromBackend([
        {
          name: "nginx",
          arch: "x86_64",
          repoless_annotation: "manual",
          repoless_cached: false,
        },
        {
          name: "custom-agent",
          arch: "x86_64",
          repoless_annotation: "manual",
          repoless_cached: false,
        },
      ]);
    });
    const files = [
      new File(["rpm1"], "nginx-1.24-1.el9.x86_64.rpm"),
      new File(["rpm2"], "custom-agent-2.0-1.el9.x86_64.rpm"),
      new File(["rpm3"], "unrelated-3.0-1.el9.x86_64.rpm"),
    ];
    let matchResult: ReturnType<typeof result.current.batchMatch>;
    act(() => {
      matchResult = result.current.batchMatch(files);
    });
    expect(matchResult!.matched).toHaveLength(2);
    expect(matchResult!.unmatched).toHaveLength(1);
    expect(matchResult!.conflicts).toHaveLength(0);
  });

  it("batchMatch detects conflicts when multiple files match same package", () => {
    const { result } = renderHook(() => useRpmUpload());
    act(() => {
      result.current.initFromBackend([
        {
          name: "nginx",
          arch: "x86_64",
          repoless_annotation: "manual",
          repoless_cached: false,
        },
      ]);
    });
    const files = [
      new File(["rpm1"], "nginx-1.24-1.el9.x86_64.rpm"),
      new File(["rpm2"], "nginx-1.25-1.el9.x86_64.rpm"),
    ];
    let matchResult: ReturnType<typeof result.current.batchMatch>;
    act(() => {
      matchResult = result.current.batchMatch(files);
    });
    expect(matchResult!.conflicts).toHaveLength(1);
    expect(matchResult!.conflicts[0].packageName).toBe("nginx.x86_64");
    expect(matchResult!.conflicts[0].files).toHaveLength(2);
  });

  it("disambiguates multilib packages with same name but different arch", () => {
    const { result } = renderHook(() => useRpmUpload());
    act(() => {
      result.current.initFromBackend([
        {
          name: "glibc",
          arch: "x86_64",
          repoless_annotation: "manual",
          repoless_cached: false,
        },
        {
          name: "glibc",
          arch: "i686",
          repoless_annotation: "manual",
          repoless_cached: false,
        },
      ]);
    });
    expect(result.current.getRowState("glibc.x86_64")).toBe("needs_upload");
    expect(result.current.getRowState("glibc.i686")).toBe("needs_upload");
    expect(result.current.needsUploadCount).toBe(2);

    const files = [
      new File(["rpm1"], "glibc-2.34-1.el9.x86_64.rpm"),
      new File(["rpm2"], "glibc-2.34-1.el9.i686.rpm"),
    ];
    let matchResult: ReturnType<typeof result.current.batchMatch>;
    act(() => {
      matchResult = result.current.batchMatch(files);
    });
    expect(matchResult!.matched).toHaveLength(2);
    expect(matchResult!.unmatched).toHaveLength(0);
    expect(matchResult!.conflicts).toHaveLength(0);
  });
});
