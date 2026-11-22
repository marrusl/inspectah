import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MainContent } from "../MainContent";
import type { ViewResponse } from "../../api/types";
import { emptyView } from "../../test-utils/emptyView";

// Mock the API client (required by MainContent internals)
vi.mock("../../api/client", () => ({
  ungroupGroup: vi.fn(),
  applyOp: vi.fn(),
}));

/** An empty view with a test's fields over it. */
function makeViewData(overrides: Partial<ViewResponse> = {}): ViewResponse {
  return emptyView(overrides);
}

const langPkgFixture: ViewResponse["language_packages"] = [
  {
    ecosystem: "pip",
    path: "/opt/myapp/venv",
    method: "pip list",
    packages: [
      { name: "flask", detected_version: "2.3.3", pinned: false },
      { name: "requests", detected_version: "2.31.0", pinned: false },
      { name: "gunicorn", detected_version: "21.2.0", pinned: false },
    ],
    confidence: "high",
    manifest_basis: "requirements.txt",
    include: true,
    has_c_extensions: false,
    system_site_packages: false,
  },
  {
    ecosystem: "npm",
    path: "/opt/other/app",
    method: "npm lockfile",
    packages: [
      { name: "express", detected_version: "4.18.0", pinned: false },
    ],
    confidence: "high",
    manifest_basis: "package-lock.json",
    include: true,
    has_c_extensions: false,
    system_site_packages: false,
  },
];

const unmanagedFixture: ViewResponse["unmanaged_files"] = [
  {
    directory: "/opt/splunk",
    items: [
      {
        path: "/opt/splunk/bin/splunkd",
        size: 24000000,
        is_var_path: false,
        include: true,
        provenance: {
          file_type: "elf_binary",
          last_modified: 1700000000,
          uid: 0,
          gid: 0,
          permissions: "0755",
          mutability: false,
          writable_mount: false,
          service_working_dir: false,
        },
      },
    ],
  },
];

const defaultProps = {
  loading: false,
  sections: null,
  onViewUpdate: vi.fn(),
  onMutationError: vi.fn(),
  sectionSearchOpen: false,
  onSectionSearchClose: vi.fn(),
  onToggleLangEnv: vi.fn(),
  onToggleUnmanagedFile: vi.fn(),
  onToggleUnmanagedGroup: vi.fn(),
  onUnmanagedIncludeNone: vi.fn(),
  onUnmanagedResetAll: vi.fn(),
  isPending: false,
};

describe("MainContent — Language Packages section", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders LanguagePackageList when activeSection is language_packages", () => {
    render(
      <MainContent
        activeSection="language_packages"
        viewData={
          makeViewData({
            language_packages: langPkgFixture,
          })
        }
        {...defaultProps}
      />,
    );
    expect(screen.getByTestId("language-package-list")).toBeInTheDocument();
  });

  it("renders section heading", () => {
    render(
      <MainContent
        activeSection="language_packages"
        viewData={
          makeViewData({
            language_packages: langPkgFixture,
          })
        }
        {...defaultProps}
      />,
    );
    expect(
      screen.getByRole("heading", { name: "Language Packages" }),
    ).toBeInTheDocument();
  });

  it("SectionSearch filters language package environments by path", async () => {
    render(
      <MainContent
        activeSection="language_packages"
        viewData={
          makeViewData({
            language_packages: langPkgFixture,
          })
        }
        {...defaultProps}
        sectionSearchOpen={true}
      />,
    );
    const user = userEvent.setup();
    const searchInput = screen.getByPlaceholderText("Filter items...");
    await user.type(searchInput, "myapp");
    // The pip env at /opt/myapp/venv matches
    expect(screen.getByText("/opt/myapp/venv")).toBeInTheDocument();
    // The npm env at /opt/other/app does not match "myapp"
    expect(screen.queryByText("/opt/other/app")).not.toBeInTheDocument();
  });

  it("shows empty state when filter matches nothing", async () => {
    render(
      <MainContent
        activeSection="language_packages"
        viewData={
          makeViewData({
            language_packages: langPkgFixture,
          })
        }
        {...defaultProps}
        sectionSearchOpen={true}
      />,
    );
    const user = userEvent.setup();
    const searchInput = screen.getByPlaceholderText("Filter items...");
    await user.type(searchInput, "zzzznotfound");
    expect(screen.getByText("No items match your search")).toBeInTheDocument();
  });

  it("reveal highlighting sets data-revealed on target item", () => {
    render(
      <MainContent
        activeSection="language_packages"
        viewData={
          makeViewData({
            language_packages: langPkgFixture,
          })
        }
        {...defaultProps}
        revealItemId="pip:/opt/myapp/venv"
      />,
    );
    const item = screen.getByTestId("lang-env-row-pip:/opt/myapp/venv");
    expect(item).toHaveAttribute("data-revealed", "true");
  });

  it("passes onToggle through to LanguagePackageList", async () => {
    const onToggleLangEnv = vi.fn();
    render(
      <MainContent
        activeSection="language_packages"
        viewData={
          makeViewData({
            language_packages: langPkgFixture,
          })
        }
        {...defaultProps}
        onToggleLangEnv={onToggleLangEnv}
      />,
    );
    const user = userEvent.setup();
    const checkbox = screen.getByLabelText(
      "Toggle pip environment at /opt/myapp/venv",
    );
    await user.click(checkbox);
    expect(onToggleLangEnv).toHaveBeenCalledWith("pip", "/opt/myapp/venv");
  });
});

describe("MainContent — Unmanaged Files section", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders UnmanagedFileList when activeSection is unmanaged_files", () => {
    render(
      <MainContent
        activeSection="unmanaged_files"
        viewData={
          makeViewData({
            unmanaged_files: unmanagedFixture,
          })
        }
        {...defaultProps}
      />,
    );
    expect(screen.getByTestId("unmanaged-file-list")).toBeInTheDocument();
  });

  it("renders section heading", () => {
    render(
      <MainContent
        activeSection="unmanaged_files"
        viewData={
          makeViewData({
            unmanaged_files: unmanagedFixture,
          })
        }
        {...defaultProps}
      />,
    );
    expect(
      screen.getByRole("heading", { name: "Unmanaged Files" }),
    ).toBeInTheDocument();
  });

  it("SectionSearch filters unmanaged files by path", async () => {
    render(
      <MainContent
        activeSection="unmanaged_files"
        viewData={
          makeViewData({
            unmanaged_files: unmanagedFixture,
          })
        }
        {...defaultProps}
        sectionSearchOpen={true}
      />,
    );
    const user = userEvent.setup();
    const searchInput = screen.getByPlaceholderText("Filter items...");
    await user.type(searchInput, "splunkd");
    // UnmanagedFileList renders only the filename ("splunkd") as visible text;
    // the full path lives in the aria-label of the row and the toggle checkbox.
    expect(screen.getByText("splunkd")).toBeInTheDocument();
    expect(
      screen.getByLabelText("Toggle /opt/splunk/bin/splunkd"),
    ).toBeInTheDocument();
  });

  it("shows empty state when filter matches nothing", async () => {
    render(
      <MainContent
        activeSection="unmanaged_files"
        viewData={
          makeViewData({
            unmanaged_files: unmanagedFixture,
          })
        }
        {...defaultProps}
        sectionSearchOpen={true}
      />,
    );
    const user = userEvent.setup();
    const searchInput = screen.getByPlaceholderText("Filter items...");
    await user.type(searchInput, "zzzznotfound");
    expect(screen.getByText("No items match your search")).toBeInTheDocument();
  });

  it("passes callbacks through to UnmanagedFileList", async () => {
    const onToggleUnmanagedFile = vi.fn();
    render(
      <MainContent
        activeSection="unmanaged_files"
        viewData={
          makeViewData({
            unmanaged_files: unmanagedFixture,
          })
        }
        {...defaultProps}
        onToggleUnmanagedFile={onToggleUnmanagedFile}
      />,
    );
    const user = userEvent.setup();
    const checkbox = screen.getByLabelText("Toggle /opt/splunk/bin/splunkd");
    await user.click(checkbox);
    expect(onToggleUnmanagedFile).toHaveBeenCalledWith(
      "/opt/splunk/bin/splunkd",
    );
  });
});

describe("MainContent — Focus management", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("ArrowDown from SectionSearch focuses first group header in language_packages", async () => {
    render(
      <MainContent
        activeSection="language_packages"
        viewData={
          makeViewData({
            language_packages: langPkgFixture,
          })
        }
        {...defaultProps}
        sectionSearchOpen={true}
      />,
    );
    const user = userEvent.setup();
    const searchInput = screen.getByPlaceholderText("Filter items...");
    searchInput.focus();
    await user.keyboard("{ArrowDown}");

    // Should focus the first language package environment row
    const firstRow = screen.getByTestId("lang-env-row-pip:/opt/myapp/venv");
    expect(document.activeElement).toBe(firstRow);
  });

  it("ArrowDown from SectionSearch focuses first group header in unmanaged_files", async () => {
    render(
      <MainContent
        activeSection="unmanaged_files"
        viewData={
          makeViewData({
            unmanaged_files: unmanagedFixture,
          })
        }
        {...defaultProps}
        sectionSearchOpen={true}
      />,
    );
    const user = userEvent.setup();
    const searchInput = screen.getByPlaceholderText("Filter items...");
    searchInput.focus();
    await user.keyboard("{ArrowDown}");

    // Should focus the first group header (the directory group button)
    const firstGroupHeader = screen.getByRole("button", {
      name: /\/opt\/splunk/,
    });
    expect(document.activeElement).toBe(firstGroupHeader);
  });
});

describe("MainContent: unit section", () => {
  const triage = {
    triage: { mode: "single_host" as const, site: null },
    primary_reason: "service_non_default_state",
    annotations: [],
  } as unknown as ViewResponse["unit_states"][number]["triage"];

  const unitStates: ViewResponse["unit_states"] = [
    {
      unit: "kdump.service",
      kind: "service",
      fragment: "vendor",
      triage,
      include: true,
      locked: false,
      current_state: "disabled",
      default_state: "enable",
      required_config: [],
      data_paths: [],
    },
  ];

  // A full shadow on a unit at its preset has no state row: it is only
  // a drop-in row. The plain drop-in is the control the count must skip.
  const unitDropins: ViewResponse["unit_dropins"] = [
    {
      unit: "sshd.service",
      path: "/etc/systemd/system/sshd.service",
      config_paths: ["/etc/systemd/system/sshd.service"],
      triage,
      include: true,
      locked: false,
      shadow_type: "full_shadow",
      shadow_rationale: "Full unit override replaces vendor sshd.service",
    },
    {
      unit: "kdump.service",
      path: "/etc/systemd/system/kdump.service.d/override.conf",
      config_paths: ["/etc/systemd/system/kdump.service.d/override.conf"],
      triage,
      include: true,
      locked: false,
      shadow_type: "drop_in",
    },
  ];

  it("renders the rows of unit_states under section id units", () => {
    render(
      <MainContent
        activeSection="units"
        viewData={
          makeViewData({
            unit_states: unitStates,
            unit_dropins: [],
            mount_findings: [],
            manual_links: [],
            undetermined_installs: [],
          })
        }
        {...defaultProps}
      />,
    );
    expect(screen.getByTestId("service-item-kdump.service")).toBeInTheDocument();
    expect(screen.getByRole("heading", { level: 2 })).toHaveTextContent(
      /^Systemd units$/,
    );
  });

  it("shows no shadow-override count in the heading", () => {
    render(
      <MainContent
        activeSection="units"
        viewData={
          makeViewData({
            unit_states: unitStates,
            unit_dropins: unitDropins,
            mount_findings: [],
            manual_links: [],
            undetermined_installs: [],
          })
        }
        {...defaultProps}
      />,
    );
    expect(screen.getByRole("heading", { level: 2 })).toHaveTextContent(
      /^Systemd units$/,
    );
    // Both drop-ins are listed.
    expect(
      screen.getByTestId("dropin-item-/etc/systemd/system/sshd.service"),
    ).toBeInTheDocument();
    expect(
      screen.getByTestId(
        "dropin-item-/etc/systemd/system/kdump.service.d/override.conf",
      ),
    ).toBeInTheDocument();
  });
});
