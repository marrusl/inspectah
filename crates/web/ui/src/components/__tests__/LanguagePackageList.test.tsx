import { describe, it, expect } from "vitest";
import type {
  LanguagePackageEnv,
  LanguagePackageDto,
  UnmanagedFileItem,
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

/** Build a LanguagePackageDto array from simple name strings. */
function makePkgs(names: string[]): LanguagePackageDto[] {
  return names.map((name) => ({
    name,
    detected_version: "1.0.0",
    pinned: false,
  }));
}

function makeLangEnv(
  ecosystem: LanguagePackageEnv["ecosystem"],
  path: string,
  packages: LanguagePackageDto[],
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
    has_c_extensions: false,
    system_site_packages: false,
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
    const env = makeLangEnv("pip", "/opt/myapp/venv", makePkgs(["flask", "requests"]));
    expect(env.ecosystem).toBe("pip");
    expect(env.path).toBe("/opt/myapp/venv");
    expect(env.packages).toHaveLength(2);
    expect(env.packages[0].name).toBe("flask");
    expect(env.packages[0].detected_version).toBe("1.0.0");
    expect(env.packages[0].pinned).toBe(false);
    expect(env.confidence).toBe("high");
    expect(env.has_c_extensions).toBe(false);
    expect(env.system_site_packages).toBe(false);
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

import { render, screen, fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { vi } from "vitest";
import { LanguagePackageList } from "../LanguagePackageList";

describe("LanguagePackageList", () => {
  const envs: LanguagePackageEnv[] = [
    makeLangEnv("pip", "/opt/myapp/venv", makePkgs(["flask", "requests", "gunicorn"])),
    makeLangEnv("npm", "/srv/webapp", makePkgs(["express", "lodash"]), {
      confidence: "medium",
      include: false,
    }),
    makeLangEnv("gem", "/opt/rails-app", makePkgs(["rails", "puma"])),
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

  it("renders empty state when no environments", () => {
    render(
      <LanguagePackageList
        environments={[]}
        onToggle={vi.fn()}
        isPending={false}
      />,
    );
    expect(screen.getByText("None detected.")).toBeInTheDocument();
  });
});

// --- Badge tests ---

describe("LanguagePackageList badges", () => {
  it("renders C extensions badge when has_c_extensions is true", () => {
    const env = makeLangEnv("pip", "/opt/app/venv", makePkgs(["numpy"]), {
      has_c_extensions: true,
    });
    render(
      <LanguagePackageList
        environments={[env]}
        onToggle={vi.fn()}
        isPending={false}
      />,
    );
    expect(screen.getByText("C extensions")).toBeInTheDocument();
    expect(
      screen.getByLabelText("This environment contains C extensions"),
    ).toBeInTheDocument();
  });

  it("does not render C extensions badge when has_c_extensions is false", () => {
    const env = makeLangEnv("pip", "/opt/app/venv", makePkgs(["flask"]));
    render(
      <LanguagePackageList
        environments={[env]}
        onToggle={vi.fn()}
        isPending={false}
      />,
    );
    expect(screen.queryByText("C extensions")).not.toBeInTheDocument();
  });

  it("renders system site-packages badge when system_site_packages is true", () => {
    const env = makeLangEnv("pip", "/opt/app/venv", makePkgs(["flask"]), {
      system_site_packages: true,
    });
    render(
      <LanguagePackageList
        environments={[env]}
        onToggle={vi.fn()}
        isPending={false}
      />,
    );
    expect(screen.getByText("system site-packages")).toBeInTheDocument();
    expect(
      screen.getByLabelText("This environment uses system site-packages"),
    ).toBeInTheDocument();
  });

  it("does not render system site-packages badge when false", () => {
    const env = makeLangEnv("pip", "/opt/app/venv", makePkgs(["flask"]));
    render(
      <LanguagePackageList
        environments={[env]}
        onToggle={vi.fn()}
        isPending={false}
      />,
    );
    expect(screen.queryByText("system site-packages")).not.toBeInTheDocument();
  });

  it("renders both badges when both flags are true", () => {
    const env = makeLangEnv("pip", "/opt/app/venv", makePkgs(["numpy"]), {
      has_c_extensions: true,
      system_site_packages: true,
    });
    render(
      <LanguagePackageList
        environments={[env]}
        onToggle={vi.fn()}
        isPending={false}
      />,
    );
    expect(screen.getByText("C extensions")).toBeInTheDocument();
    expect(screen.getByText("system site-packages")).toBeInTheDocument();
  });
});

// --- Expandable sublist tests (npm global) ---

describe("LanguagePackageList expandable sublist", () => {
  const npmGlobalEnv = makeLangEnv(
    "npm",
    "/usr/lib/node_modules",
    [
      { name: "typescript", detected_version: "5.4.0", pinned: false },
      { name: "eslint", detected_version: "8.57.0", pinned: true },
    ],
    { method: "npm global" },
  );

  it("renders expand button for npm global environments", () => {
    render(
      <LanguagePackageList
        environments={[npmGlobalEnv]}
        onToggle={vi.fn()}
        isPending={false}
      />,
    );
    const expandBtn = screen.getByTestId(
      "lang-env-expand-npm:/usr/lib/node_modules",
    );
    expect(expandBtn).toBeInTheDocument();
    // aria-expanded lives on the focusable env row, not the nested button
    const envRow = screen.getByTestId(
      "lang-env-row-npm:/usr/lib/node_modules",
    );
    expect(envRow).toHaveAttribute("aria-expanded", "false");
  });

  it("does not render expand button for non-npm-global environments", () => {
    const pipEnv = makeLangEnv("pip", "/opt/app/venv", makePkgs(["flask"]));
    render(
      <LanguagePackageList
        environments={[pipEnv]}
        onToggle={vi.fn()}
        isPending={false}
      />,
    );
    expect(
      screen.queryByTestId("lang-env-expand-pip:/opt/app/venv"),
    ).not.toBeInTheDocument();
  });

  it("toggles package sublist on expand button click", async () => {
    const user = userEvent.setup();
    render(
      <LanguagePackageList
        environments={[npmGlobalEnv]}
        onToggle={vi.fn()}
        isPending={false}
        onSetPackagePin={vi.fn()}
      />,
    );

    // Sublist not visible initially
    expect(
      screen.queryByTestId("lang-env-sublist-npm:/usr/lib/node_modules"),
    ).not.toBeInTheDocument();

    // Click expand
    const expandBtn = screen.getByTestId(
      "lang-env-expand-npm:/usr/lib/node_modules",
    );
    await user.click(expandBtn);

    // Sublist visible with package names and versions
    const sublist = screen.getByTestId(
      "lang-env-sublist-npm:/usr/lib/node_modules",
    );
    expect(sublist).toBeInTheDocument();
    // aria-expanded lives on the focusable env row, not the nested button
    const envRow = screen.getByTestId(
      "lang-env-row-npm:/usr/lib/node_modules",
    );
    expect(envRow).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByText("typescript")).toBeInTheDocument();
    expect(screen.getByText("5.4.0")).toBeInTheDocument();
    expect(screen.getByText("eslint")).toBeInTheDocument();
    expect(screen.getByText("8.57.0")).toBeInTheDocument();

    // Click collapse
    await user.click(expandBtn);
    expect(
      screen.queryByTestId("lang-env-sublist-npm:/usr/lib/node_modules"),
    ).not.toBeInTheDocument();
    expect(envRow).toHaveAttribute("aria-expanded", "false");
  });

  it("renders pin checkboxes reflecting pinned state", async () => {
    const user = userEvent.setup();
    const onSetPackagePin = vi.fn();
    render(
      <LanguagePackageList
        environments={[npmGlobalEnv]}
        onToggle={vi.fn()}
        isPending={false}
        onSetPackagePin={onSetPackagePin}
      />,
    );

    // Expand
    await user.click(
      screen.getByTestId("lang-env-expand-npm:/usr/lib/node_modules"),
    );

    // typescript is unpinned, eslint is pinned
    const tsPin = screen.getByTestId("lang-pkg-pin-typescript");
    expect(tsPin).not.toBeChecked();
    const eslintPin = screen.getByTestId("lang-pkg-pin-eslint");
    expect(eslintPin).toBeChecked();

    // Click pin for typescript (should toggle to pinned=true)
    await user.click(tsPin);
    expect(onSetPackagePin).toHaveBeenCalledWith(
      "npm",
      "/usr/lib/node_modules",
      "typescript",
      true,
    );
  });

  it("does not render pin checkboxes when onSetPackagePin not provided", async () => {
    const user = userEvent.setup();
    render(
      <LanguagePackageList
        environments={[npmGlobalEnv]}
        onToggle={vi.fn()}
        isPending={false}
      />,
    );

    await user.click(
      screen.getByTestId("lang-env-expand-npm:/usr/lib/node_modules"),
    );
    expect(
      screen.queryByTestId("lang-pkg-pin-typescript"),
    ).not.toBeInTheDocument();
  });

  it("renders bulk pin button with correct label", async () => {
    const user = userEvent.setup();
    const onSetBulkPackagePin = vi.fn();
    render(
      <LanguagePackageList
        environments={[npmGlobalEnv]}
        onToggle={vi.fn()}
        isPending={false}
        onSetBulkPackagePin={onSetBulkPackagePin}
      />,
    );

    await user.click(
      screen.getByTestId("lang-env-expand-npm:/usr/lib/node_modules"),
    );

    // Not all pinned => "Pin all"
    const bulkBtn = screen.getByTestId(
      "lang-env-bulk-pin-npm:/usr/lib/node_modules",
    );
    expect(bulkBtn).toHaveTextContent("Pin all");

    await user.click(bulkBtn);
    expect(onSetBulkPackagePin).toHaveBeenCalledWith(
      "npm",
      "/usr/lib/node_modules",
      true,
    );
  });

  it("shows 'Unpin all' when all packages are pinned", async () => {
    const user = userEvent.setup();
    const allPinnedEnv = makeLangEnv(
      "npm",
      "/usr/lib/node_modules",
      [
        { name: "typescript", detected_version: "5.4.0", pinned: true },
        { name: "eslint", detected_version: "8.57.0", pinned: true },
      ],
      { method: "npm global" },
    );
    render(
      <LanguagePackageList
        environments={[allPinnedEnv]}
        onToggle={vi.fn()}
        isPending={false}
        onSetBulkPackagePin={vi.fn()}
      />,
    );

    await user.click(
      screen.getByTestId("lang-env-expand-npm:/usr/lib/node_modules"),
    );
    expect(
      screen.getByTestId("lang-env-bulk-pin-npm:/usr/lib/node_modules"),
    ).toHaveTextContent("Unpin all");
  });
});

// --- Keyboard navigation tests ---

describe("LanguagePackageList keyboard navigation", () => {
  const npmGlobalEnv = makeLangEnv(
    "npm",
    "/usr/lib/node_modules",
    [
      { name: "typescript", detected_version: "5.4.0", pinned: false },
      { name: "eslint", detected_version: "8.57.0", pinned: true },
      { name: "prettier", detected_version: "3.2.0", pinned: false },
    ],
    { method: "npm global" },
  );

  it("Enter on expandable env row toggles expand/collapse", () => {
    render(
      <LanguagePackageList
        environments={[npmGlobalEnv]}
        onToggle={vi.fn()}
        isPending={false}
        onSetPackagePin={vi.fn()}
      />,
    );
    const envRow = screen.getByTestId(
      "lang-env-row-npm:/usr/lib/node_modules",
    );

    envRow.focus();
    fireEvent.keyDown(envRow, { key: "Enter" });
    expect(
      screen.getByTestId("lang-env-sublist-npm:/usr/lib/node_modules"),
    ).toBeInTheDocument();

    fireEvent.keyDown(envRow, { key: "Enter" });
    expect(
      screen.queryByTestId("lang-env-sublist-npm:/usr/lib/node_modules"),
    ).not.toBeInTheDocument();
  });

  it("ArrowDown moves focus to next package row in sublist", async () => {
    const user = userEvent.setup();
    render(
      <LanguagePackageList
        environments={[npmGlobalEnv]}
        onToggle={vi.fn()}
        isPending={false}
        onSetPackagePin={vi.fn()}
      />,
    );

    await user.click(
      screen.getByTestId("lang-env-expand-npm:/usr/lib/node_modules"),
    );

    const sublist = screen.getByTestId(
      "lang-env-sublist-npm:/usr/lib/node_modules",
    );
    screen.getByTestId("lang-pkg-typescript").focus();

    fireEvent.keyDown(sublist, { key: "ArrowDown" });
    expect(document.activeElement).toBe(
      screen.getByTestId("lang-pkg-eslint"),
    );

    fireEvent.keyDown(sublist, { key: "ArrowDown" });
    expect(document.activeElement).toBe(
      screen.getByTestId("lang-pkg-prettier"),
    );
  });

  it("ArrowUp moves focus to previous package row in sublist", async () => {
    const user = userEvent.setup();
    render(
      <LanguagePackageList
        environments={[npmGlobalEnv]}
        onToggle={vi.fn()}
        isPending={false}
        onSetPackagePin={vi.fn()}
      />,
    );

    await user.click(
      screen.getByTestId("lang-env-expand-npm:/usr/lib/node_modules"),
    );

    const sublist = screen.getByTestId(
      "lang-env-sublist-npm:/usr/lib/node_modules",
    );
    screen.getByTestId("lang-pkg-prettier").focus();

    fireEvent.keyDown(sublist, { key: "ArrowUp" });
    expect(document.activeElement).toBe(
      screen.getByTestId("lang-pkg-eslint"),
    );
  });

  it("Escape collapses sublist and returns focus to env row", async () => {
    const user = userEvent.setup();
    render(
      <LanguagePackageList
        environments={[npmGlobalEnv]}
        onToggle={vi.fn()}
        isPending={false}
        onSetPackagePin={vi.fn()}
      />,
    );

    await user.click(
      screen.getByTestId("lang-env-expand-npm:/usr/lib/node_modules"),
    );

    const sublist = screen.getByTestId(
      "lang-env-sublist-npm:/usr/lib/node_modules",
    );
    screen.getByTestId("lang-pkg-typescript").focus();

    fireEvent.keyDown(sublist, { key: "Escape" });
    expect(
      screen.queryByTestId("lang-env-sublist-npm:/usr/lib/node_modules"),
    ).not.toBeInTheDocument();
    // Focus returns to the env row, not the nested expand button
    expect(document.activeElement).toBe(
      screen.getByTestId("lang-env-row-npm:/usr/lib/node_modules"),
    );
  });
});

// --- Search integration tests ---

describe("LanguagePackageList search integration", () => {
  const npmGlobalEnv = makeLangEnv(
    "npm",
    "/usr/lib/node_modules",
    [
      { name: "typescript", detected_version: "5.4.0", pinned: false },
      { name: "eslint", detected_version: "8.57.0", pinned: true },
    ],
    { method: "npm global" },
  );

  it("auto-expands environments when searchQuery matches a package name", () => {
    const { rerender } = render(
      <LanguagePackageList
        environments={[npmGlobalEnv]}
        onToggle={vi.fn()}
        isPending={false}
        onSetPackagePin={vi.fn()}
      />,
    );

    expect(
      screen.queryByTestId("lang-env-sublist-npm:/usr/lib/node_modules"),
    ).not.toBeInTheDocument();

    rerender(
      <LanguagePackageList
        environments={[npmGlobalEnv]}
        onToggle={vi.fn()}
        isPending={false}
        onSetPackagePin={vi.fn()}
        searchQuery="type"
      />,
    );

    expect(
      screen.getByTestId("lang-env-sublist-npm:/usr/lib/node_modules"),
    ).toBeInTheDocument();
  });

  it("highlights matching packages with data-search-match attribute", () => {
    render(
      <LanguagePackageList
        environments={[npmGlobalEnv]}
        onToggle={vi.fn()}
        isPending={false}
        onSetPackagePin={vi.fn()}
        searchQuery="type"
      />,
    );

    const tsRow = screen.getByTestId("lang-pkg-typescript");
    expect(tsRow).toHaveAttribute("data-search-match", "true");

    const eslintRow = screen.getByTestId("lang-pkg-eslint");
    expect(eslintRow).not.toHaveAttribute("data-search-match");
  });

  it("does not auto-expand when searchQuery has no package matches", () => {
    render(
      <LanguagePackageList
        environments={[npmGlobalEnv]}
        onToggle={vi.fn()}
        isPending={false}
        onSetPackagePin={vi.fn()}
        searchQuery="nonexistent"
      />,
    );

    expect(
      screen.queryByTestId("lang-env-sublist-npm:/usr/lib/node_modules"),
    ).not.toBeInTheDocument();
  });
});

// --- aria-live announcement tests ---

describe("LanguagePackageList aria-live announcements", () => {
  const npmGlobalEnv = makeLangEnv(
    "npm",
    "/usr/lib/node_modules",
    [
      { name: "typescript", detected_version: "5.4.0", pinned: false },
      { name: "eslint", detected_version: "8.57.0", pinned: false },
    ],
    { method: "npm global" },
  );

  it("announces pin count after bulk pin all", async () => {
    const user = userEvent.setup();
    render(
      <LanguagePackageList
        environments={[npmGlobalEnv]}
        onToggle={vi.fn()}
        isPending={false}
        onSetBulkPackagePin={vi.fn()}
      />,
    );

    await user.click(
      screen.getByTestId("lang-env-expand-npm:/usr/lib/node_modules"),
    );
    await user.click(
      screen.getByTestId("lang-env-bulk-pin-npm:/usr/lib/node_modules"),
    );

    expect(screen.getByTestId("lang-pkg-announcement")).toHaveTextContent(
      "Pinned 2 packages in npm globals",
    );
  });

  it("announces unpin count after bulk unpin all", async () => {
    const user = userEvent.setup();
    const allPinnedEnv = makeLangEnv(
      "npm",
      "/usr/lib/node_modules",
      [
        { name: "typescript", detected_version: "5.4.0", pinned: true },
        { name: "eslint", detected_version: "8.57.0", pinned: true },
      ],
      { method: "npm global" },
    );
    render(
      <LanguagePackageList
        environments={[allPinnedEnv]}
        onToggle={vi.fn()}
        isPending={false}
        onSetBulkPackagePin={vi.fn()}
      />,
    );

    await user.click(
      screen.getByTestId("lang-env-expand-npm:/usr/lib/node_modules"),
    );
    await user.click(
      screen.getByTestId("lang-env-bulk-pin-npm:/usr/lib/node_modules"),
    );

    expect(screen.getByTestId("lang-pkg-announcement")).toHaveTextContent(
      "Unpinned 2 packages in npm globals",
    );
  });
});

// --- Package pin keyboard toggle (Rider A) ---

describe("LanguagePackageList package pin keyboard toggle", () => {
  const npmGlobalEnv = makeLangEnv(
    "npm",
    "/usr/lib/node_modules",
    [
      { name: "typescript", detected_version: "5.4.0", pinned: false },
      { name: "eslint", detected_version: "8.57.0", pinned: true },
    ],
    { method: "npm global" },
  );

  async function expandSublist(onSetPackagePin = vi.fn()) {
    const user = userEvent.setup();
    render(
      <LanguagePackageList
        environments={[npmGlobalEnv]}
        onToggle={vi.fn()}
        isPending={false}
        onSetPackagePin={onSetPackagePin}
      />,
    );
    await user.click(
      screen.getByTestId("lang-env-expand-npm:/usr/lib/node_modules"),
    );
    return onSetPackagePin;
  }

  // The package row is focusable, so keyboard users reach it and then have
  // nowhere to go: arrows navigate, Escape collapses, and the pin was
  // reachable only by tabbing into the checkbox inside the row.
  it("Enter on a focused package row pins it", async () => {
    const onSetPackagePin = await expandSublist();
    const row = screen.getByTestId("lang-pkg-typescript");
    row.focus();
    fireEvent.keyDown(row, { key: "Enter" });
    expect(onSetPackagePin).toHaveBeenCalledWith(
      "npm",
      "/usr/lib/node_modules",
      "typescript",
      true,
    );
  });

  it("Space on a focused package row pins it", async () => {
    const onSetPackagePin = await expandSublist();
    const row = screen.getByTestId("lang-pkg-typescript");
    row.focus();
    fireEvent.keyDown(row, { key: " " });
    expect(onSetPackagePin).toHaveBeenCalledWith(
      "npm",
      "/usr/lib/node_modules",
      "typescript",
      true,
    );
  });

  it("unpins an already-pinned package", async () => {
    const onSetPackagePin = await expandSublist();
    const row = screen.getByTestId("lang-pkg-eslint");
    row.focus();
    fireEvent.keyDown(row, { key: "Enter" });
    expect(onSetPackagePin).toHaveBeenCalledWith(
      "npm",
      "/usr/lib/node_modules",
      "eslint",
      false,
    );
  });

  it("announces the pin change for screen readers", async () => {
    await expandSublist();
    const row = screen.getByTestId("lang-pkg-typescript");
    row.focus();
    fireEvent.keyDown(row, { key: "Enter" });
    expect(screen.getByTestId("lang-pkg-announcement")).toHaveTextContent(
      "Pinned typescript",
    );
  });

  it("does not double-toggle when the key lands on the pin checkbox itself", async () => {
    // The checkbox handles Space natively; the row handler must not fire a
    // second op for the same keystroke.
    const onSetPackagePin = await expandSublist();
    fireEvent.keyDown(screen.getByTestId("lang-pkg-pin-typescript"), {
      key: " ",
    });
    expect(onSetPackagePin).not.toHaveBeenCalled();
  });

  it("does not toggle a pin while a mutation is in flight", async () => {
    const user = userEvent.setup();
    const onSetPackagePin = vi.fn();
    render(
      <LanguagePackageList
        environments={[npmGlobalEnv]}
        onToggle={vi.fn()}
        isPending={true}
        onSetPackagePin={onSetPackagePin}
      />,
    );
    await user.click(
      screen.getByTestId("lang-env-expand-npm:/usr/lib/node_modules"),
    );
    const row = screen.getByTestId("lang-pkg-typescript");
    row.focus();
    fireEvent.keyDown(row, { key: "Enter" });
    expect(onSetPackagePin).not.toHaveBeenCalled();
  });

  it("advertises the row shortcut to assistive technology", async () => {
    await expandSublist();
    expect(
      screen.getByTestId("lang-pkg-typescript").getAttribute("aria-keyshortcuts"),
    ).toBe("Enter Space");
  });
});
