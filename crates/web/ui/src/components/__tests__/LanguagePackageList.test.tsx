import { describe, it, expect } from "vitest";
import type {
  LanguagePackageEnv,
  UnmanagedFileItem,
  UnmanagedFileGroup,
  ProvenanceSignals,
  RpmUploadRowState,
} from "../../api/types";

// --- Test data factories ---

const DEFAULT_PROVENANCE: ProvenanceSignals = {
  file_type: "elf_binary",
  last_modified: 1700000000,
  uid: 0,
  gid: 0,
  permissions: "0755",
  mutability: false,
  writable_mount: false,
  service_working_dir: false,
};

function makeLangEnv(
  ecosystem: LanguagePackageEnv["ecosystem"],
  path: string,
  packages: string[],
  overrides?: Partial<LanguagePackageEnv>,
): LanguagePackageEnv {
  return {
    ecosystem,
    path,
    method:
      ecosystem === "pip"
        ? "pip list"
        : ecosystem === "npm"
          ? "npm lockfile"
          : "gem lockfile",
    packages,
    confidence: "high",
    manifest_basis:
      ecosystem === "pip"
        ? "requirements.txt"
        : ecosystem === "npm"
          ? "package-lock.json"
          : "Gemfile.lock",
    include: true,
    ...overrides,
  };
}

function makeUnmanagedFile(
  path: string,
  overrides?: Partial<UnmanagedFileItem>,
): UnmanagedFileItem {
  return {
    path,
    size: 1024,
    is_var_path: path.startsWith("/var/"),
    include: true,
    provenance: { ...DEFAULT_PROVENANCE },
    ...overrides,
  };
}

describe("Type contracts", () => {
  it("LanguagePackageEnv factory produces valid shape", () => {
    const env = makeLangEnv("pip", "/opt/myapp/venv", ["flask", "requests"]);
    expect(env.ecosystem).toBe("pip");
    expect(env.path).toBe("/opt/myapp/venv");
    expect(env.packages).toHaveLength(2);
    expect(env.confidence).toBe("high");
  });

  it("UnmanagedFileItem factory carries provenance signals", () => {
    const regular = makeUnmanagedFile("/opt/splunk/bin/splunkd", {
      provenance: {
        ...DEFAULT_PROVENANCE,
        mutability: true,
        writable_mount: true,
      },
    });
    expect(regular.is_var_path).toBe(false);
    expect(regular.provenance.mutability).toBe(true);
    expect(regular.provenance.writable_mount).toBe(true);
    expect(regular.provenance.service_working_dir).toBe(false);

    const varFile = makeUnmanagedFile("/var/lib/myapp/data.db");
    expect(varFile.is_var_path).toBe(true);
  });

  it("RpmUploadRowState covers all 5 states", () => {
    const states: RpmUploadRowState[] = [
      "cached_excluded",
      "cached_included",
      "needs_upload",
      "uploaded_excluded",
      "uploaded_included",
    ];
    expect(states).toHaveLength(5);
  });
});
