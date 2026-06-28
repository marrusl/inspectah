import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MainContent } from "../MainContent";
import type { ViewResponse } from "../../api/types";

// Mock the API client (required by MainContent internals)
vi.mock("../../api/client", () => ({
  ungroupGroup: vi.fn(),
  applyOp: vi.fn(),
}));

/** Minimal ViewResponse that renders without crashing for non-package sections. */
function makeViewData(
  overrides: Partial<ViewResponse> = {},
): Partial<ViewResponse> {
  return {
    packages: [],
    config_files: [],
    repo_groups: [],
    package_groups: [],
    generation: 1,
    stats: {
      sections: [],
      needs_review_count: 0,
      ops_applied: 0,
      can_undo: false,
      can_redo: false,
      baseline_available: false,
    },
    ...overrides,
  };
}

const langPkgFixture: ViewResponse["language_packages"] = [
  {
    ecosystem: "pip",
    path: "/opt/myapp/venv",
    method: "pip list",
    packages: ["flask", "requests", "gunicorn"],
    confidence: "high",
    manifest_basis: "requirements.txt",
    include: true,
  },
  {
    ecosystem: "npm",
    path: "/opt/other/app",
    method: "npm lockfile",
    packages: ["express"],
    confidence: "high",
    manifest_basis: "package-lock.json",
    include: true,
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
          }) as ViewResponse
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
          }) as ViewResponse
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
          }) as ViewResponse
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
          }) as ViewResponse
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
          }) as ViewResponse
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
          }) as ViewResponse
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
          }) as ViewResponse
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
          }) as ViewResponse
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
          }) as ViewResponse
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
          }) as ViewResponse
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
          }) as ViewResponse
        }
        {...defaultProps}
        onToggleUnmanagedFile={onToggleUnmanagedFile}
      />,
    );
    const user = userEvent.setup();
    const checkbox = screen.getByLabelText(
      "Toggle /opt/splunk/bin/splunkd",
    );
    await user.click(checkbox);
    expect(onToggleUnmanagedFile).toHaveBeenCalledWith(
      "/opt/splunk/bin/splunkd",
    );
  });
});
