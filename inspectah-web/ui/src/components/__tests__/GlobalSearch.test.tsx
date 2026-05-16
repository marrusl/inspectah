import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { GlobalSearch } from "../GlobalSearch";
import type { GlobalSearchProps } from "../GlobalSearch";
import type { DecisionItemKind } from "../DecisionItem";
import type { ContextSection } from "../../api/types";

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
    isOpen: true,
    onClose: vi.fn(),
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

describe("GlobalSearch", () => {
  it("renders nothing when closed", () => {
    const { container } = render(
      <GlobalSearch
        isOpen={false}
        onClose={vi.fn()}
        packageItems={[]}
        configItems={[]}
        contextSections={null}
        onNavigate={vi.fn()}
      />,
    );

    expect(container.innerHTML).toBe("");
  });

  it("renders modal with search input when open", () => {
    renderGlobalSearch();

    expect(screen.getByTestId("global-search-modal")).toBeInTheDocument();
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

  it("navigates on Enter and calls onClose", async () => {
    const { props } = renderGlobalSearch();

    const input = screen.getByLabelText("Search all sections");
    await userEvent.type(input, "httpd");

    // Press Enter to select first result
    fireEvent.keyDown(input, { key: "Enter" });

    expect(props.onNavigate).toHaveBeenCalledTimes(1);
    expect(props.onClose).toHaveBeenCalledTimes(1);
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
    expect(props.onClose).toHaveBeenCalledTimes(1);
  });

  it("shows section labels in results", async () => {
    renderGlobalSearch();

    const input = screen.getByLabelText("Search all sections");
    await userEvent.type(input, "httpd");

    // Should show section labels like "Packages", "Config Files", "Services"
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
});
