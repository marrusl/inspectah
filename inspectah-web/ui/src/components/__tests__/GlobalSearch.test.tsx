import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { GlobalSearch } from "../GlobalSearch";
import type { GlobalSearchProps } from "../GlobalSearch";
import type { DecisionItemKind } from "../DecisionItem";
import type { ContextSection, TriageAnnotation } from "../../api/types";

const DEFAULT_TRIAGE = { triage: { mode: "single_host" as const, baseline: null }, primary_reason: "package_baseline_match" as const, annotations: [] as TriageAnnotation[] };

function makePackageItem(name: string, arch = "x86_64"): DecisionItemKind {
  return {
    type: "package",
    data: {
      entry: {
        name,
        arch,
        epoch: "0",
        version: "1.0.0",
        release: "1.el9",
        state: "added",
        include: true,
        source_repo: "baseos",
        fleet: null,
      },
      attention: [],
      triage: DEFAULT_TRIAGE,
    },
  };
}

function makeConfigItem(path: string): DecisionItemKind {
  return {
    type: "config",
    data: {
      entry: {
        path,
        kind: "rpm_owned_modified",
        category: "other",
        content: "",
        rpm_va_flags: null,
        package: null,
        diff_against_rpm: null,
        include: true,
        tie: false,
        tie_winner: false,
        fleet: null,
      },
      attention: [],
      triage: { triage: { mode: "single_host" as const, baseline: null }, primary_reason: "config_baseline_match" as const, annotations: [] as TriageAnnotation[] },
    },
  };
}

function makeContextSection(id: string, items: { id: string; title: string }[]): ContextSection {
  return {
    id,
    display_name: id.charAt(0).toUpperCase() + id.slice(1),
    items: items.map((item) => ({
      id: item.id,
      title: item.title,
      subtitle: null,
      detail: null,
      searchable_text: item.title.toLowerCase(),
    })),
  };
}

function renderGlobalSearch(overrides: Partial<GlobalSearchProps> = {}) {
  const defaultProps: GlobalSearchProps = {
    packageItems: [makePackageItem("httpd"), makePackageItem("nginx")],
    configItems: [makeConfigItem("/etc/httpd/httpd.conf")],
    contextSections: [
      makeContextSection("services", [
        { id: "svc:httpd", title: "httpd.service" },
        { id: "svc:nginx", title: "nginx.service" },
      ]),
    ],
    onNavigate: vi.fn(),
    ...overrides,
  };
  return { ...render(<GlobalSearch {...defaultProps} />), props: defaultProps };
}

describe("GlobalSearch auto-reveal", () => {
  it("auto-expands collapsed baseline summary when search selects item inside it", async () => {
    // Create a baseline-match package (Tier 1, collapsed by default)
    const baselineItem: DecisionItemKind = {
      type: "package",
      data: {
        entry: {
          name: "glibc",
          arch: "x86_64",
          epoch: "0",
          version: "2.34",
          release: "1.el9",
          state: "added",
          include: true,
          source_repo: "baseos",
          fleet: null,
        },
        attention: [
          { level: "routine", reason: "package_baseline_match", detail: null },
        ],
        triage: { triage: { mode: "single_host" as const, baseline: null }, primary_reason: "package_baseline_match" as const, annotations: [] as TriageAnnotation[] },
      },
    };

    const onNavigate = vi.fn();
    renderGlobalSearch({
      packageItems: [baselineItem],
      onNavigate,
    });

    const input = screen.getByLabelText("Search all sections");
    await userEvent.type(input, "glibc");

    // Should find the baseline item in search results
    const results = screen.getByTestId("global-search-results");
    const options = within(results).getAllByRole("option");
    expect(options.length).toBeGreaterThanOrEqual(1);

    // Select the result
    fireEvent.keyDown(input, { key: "Enter" });

    // onNavigate should be called with the correct section and item ID
    expect(onNavigate).toHaveBeenCalledWith("packages", "packages:glibc.x86_64");
  });

  it("auto-expands collapsed config-managed summary when search selects item inside it", async () => {
    const configManagedItem: DecisionItemKind = {
      type: "config",
      data: {
        entry: {
          path: "/etc/yum.conf",
          kind: "rpm_owned_default",
          category: "other",
          content: "",
          rpm_va_flags: null,
          package: "yum",
          diff_against_rpm: null,
          include: true,
          tie: false,
          tie_winner: false,
          fleet: null,
        },
        attention: [
          { level: "routine", reason: "config_default", detail: null },
        ],
        triage: { triage: { mode: "single_host" as const, baseline: null }, primary_reason: "config_default" as const, annotations: [] as TriageAnnotation[] },
      },
    };

    const onNavigate = vi.fn();
    renderGlobalSearch({
      configItems: [configManagedItem],
      onNavigate,
    });

    const input = screen.getByLabelText("Search all sections");
    await userEvent.type(input, "yum.conf");

    const results = screen.getByTestId("global-search-results");
    const options = within(results).getAllByRole("option");
    expect(options.length).toBeGreaterThanOrEqual(1);

    fireEvent.keyDown(input, { key: "Enter" });
    expect(onNavigate).toHaveBeenCalledWith("configs", "configs:/etc/yum.conf");
  });
});

describe("GlobalSearch", () => {
  it("renders search input in sidebar", () => {
    renderGlobalSearch();

    expect(screen.getByTestId("sidebar-search")).toBeInTheDocument();
    expect(screen.getByTestId("global-search-input")).toBeInTheDocument();
  });

  it("shows no results initially (empty query)", () => {
    renderGlobalSearch();

    expect(screen.queryByTestId("global-search-results")).not.toBeInTheDocument();
  });

  it("filters results when user types a query", async () => {
    renderGlobalSearch();

    const input = screen.getByLabelText("Search all sections");
    await userEvent.type(input, "httpd");

    const results = screen.getByTestId("global-search-results");
    expect(results).toBeInTheDocument();

    // Should match httpd package, httpd.conf config, httpd.service
    const options = within(results).getAllByRole("option");
    expect(options.length).toBeGreaterThanOrEqual(2);
  });

  it("shows 'No results found' for non-matching query", async () => {
    renderGlobalSearch();

    const input = screen.getByLabelText("Search all sections");
    await userEvent.type(input, "zzzznonexistent");

    expect(screen.getByText("No results found")).toBeInTheDocument();
  });

  it("navigates on Enter", async () => {
    const { props } = renderGlobalSearch();

    const input = screen.getByLabelText("Search all sections");
    await userEvent.type(input, "httpd");

    // Press Enter to select first result
    fireEvent.keyDown(input, { key: "Enter" });

    expect(props.onNavigate).toHaveBeenCalledTimes(1);
  });

  it("navigates with ArrowDown then Enter", async () => {
    const { props } = renderGlobalSearch();

    const input = screen.getByLabelText("Search all sections");
    await userEvent.type(input, "httpd");

    // ArrowDown to second result, then Enter
    fireEvent.keyDown(input, { key: "ArrowDown" });
    fireEvent.keyDown(input, { key: "Enter" });

    expect(props.onNavigate).toHaveBeenCalledTimes(1);
  });

  it("navigates on click", async () => {
    const { props } = renderGlobalSearch();

    const input = screen.getByLabelText("Search all sections");
    await userEvent.type(input, "httpd");

    const firstResult = screen.getByTestId("global-search-result-0");
    await userEvent.click(firstResult);

    expect(props.onNavigate).toHaveBeenCalledTimes(1);
  });

  it("shows section labels in results", async () => {
    renderGlobalSearch();

    const input = screen.getByLabelText("Search all sections");
    await userEvent.type(input, "httpd");

    // Should show section labels like "Packages"
    expect(screen.getByText("Packages")).toBeInTheDocument();
  });

  it("clears results when query is cleared", async () => {
    renderGlobalSearch();

    const input = screen.getByLabelText("Search all sections");
    await userEvent.type(input, "httpd");
    expect(screen.getByTestId("global-search-results")).toBeInTheDocument();

    await userEvent.clear(input);
    expect(screen.queryByTestId("global-search-results")).not.toBeInTheDocument();
  });

  it("clears query on Escape", async () => {
    renderGlobalSearch();

    const input = screen.getByLabelText("Search all sections");
    await userEvent.type(input, "httpd");
    expect(screen.getByTestId("global-search-results")).toBeInTheDocument();

    fireEvent.keyDown(input, { key: "Escape" });
    expect(screen.queryByTestId("global-search-results")).not.toBeInTheDocument();
  });
});
