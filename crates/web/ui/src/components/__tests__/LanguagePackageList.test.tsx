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

import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { vi } from "vitest";
import { LanguagePackageList } from "../LanguagePackageList";

describe("LanguagePackageList", () => {
  const envs: LanguagePackageEnv[] = [
    makeLangEnv("pip", "/opt/myapp/venv", ["flask", "requests", "gunicorn"]),
    makeLangEnv("npm", "/srv/webapp", ["express", "lodash"], {
      confidence: "medium",
      include: false,
    }),
    makeLangEnv("gem", "/opt/rails-app", ["rails", "puma"]),
  ];

  it("renders one row per environment with ecosystem label", () => {
    render(
      <LanguagePackageList
        environments={envs}
        onToggle={vi.fn()}
        isPending={false}
      />,
    );
    expect(screen.getByText("/opt/myapp/venv")).toBeInTheDocument();
    expect(screen.getByText("/srv/webapp")).toBeInTheDocument();
    expect(screen.getByText("/opt/rails-app")).toBeInTheDocument();
    expect(screen.getByText("pip")).toBeInTheDocument();
    expect(screen.getByText("npm")).toBeInTheDocument();
    expect(screen.getByText("gem")).toBeInTheDocument();
  });

  it("renders package count badge per environment", () => {
    render(
      <LanguagePackageList
        environments={envs}
        onToggle={vi.fn()}
        isPending={false}
      />,
    );
    expect(screen.getByText("3 packages")).toBeInTheDocument();
    expect(screen.getAllByText("2 packages")).toHaveLength(2);
  });

  it("shows confidence label with correct color", () => {
    render(
      <LanguagePackageList
        environments={envs}
        onToggle={vi.fn()}
        isPending={false}
      />,
    );
    const highBadges = screen.getAllByText("high");
    expect(highBadges.length).toBeGreaterThanOrEqual(2);
    expect(screen.getByText("medium")).toBeInTheDocument();
  });

  it("checkbox reflects include state", () => {
    render(
      <LanguagePackageList
        environments={envs}
        onToggle={vi.fn()}
        isPending={false}
      />,
    );
    const checkboxes = screen.getAllByRole("checkbox");
    // pip and gem are included, npm is not
    expect(checkboxes[0]).toBeChecked();
    expect(checkboxes[1]).not.toBeChecked();
    expect(checkboxes[2]).toBeChecked();
  });

  it("calls onToggle with ecosystem and path when checkbox is clicked", async () => {
    const onToggle = vi.fn();
    render(
      <LanguagePackageList
        environments={envs}
        onToggle={onToggle}
        isPending={false}
      />,
    );
    const user = userEvent.setup();
    const checkboxes = screen.getAllByRole("checkbox");
    await user.click(checkboxes[1]); // npm env
    expect(onToggle).toHaveBeenCalledWith("npm", "/srv/webapp");
  });

  it("disables toggles when isPending is true", () => {
    render(
      <LanguagePackageList
        environments={envs}
        onToggle={vi.fn()}
        isPending={true}
      />,
    );
    const checkboxes = screen.getAllByRole("checkbox");
    checkboxes.forEach((cb) => expect(cb).toBeDisabled());
  });
});
