import { describe, it, expect } from "vitest";
import { render, screen, within } from "@testing-library/react";
import { ItemDetailPane } from "../ItemDetailPane";
import type { AggregateItem, ItemId } from "../../../api/types";

function makeItem(
  overrides: Partial<AggregateItem> & { item_id: ItemId },
): AggregateItem {
  return {
    include: true,
    triage: {
      bucket: "universal" as const,
      prevalence: { count: 1, total: 1 },
    },
    prevalence: { count: 2, total: 3 },
    source_repo: "",
    ...overrides,
  };
}

describe("ItemDetailPane", () => {
  it("renders base fields for a package item", () => {
    const item = makeItem({
      item_id: { kind: "Package", key: { name: "httpd", arch: "x86_64" } },
    });

    render(<ItemDetailPane item={item} />);

    // "Package" appears as both the dt label and the Kind dd value
    const packageTexts = screen.getAllByText("Package");
    expect(packageTexts.length).toBe(2);
    expect(screen.getByText("httpd.x86_64")).toBeInTheDocument();
    expect(screen.getByText("2/3 hosts")).toBeInTheDocument();
  });

  it("renders base fields for a path item", () => {
    const item = makeItem({
      item_id: {
        kind: "UnmanagedFile",
        key: { path: "/opt/app/server" },
      },
    });

    render(<ItemDetailPane item={item} />);

    expect(screen.getByText("Path")).toBeInTheDocument();
    expect(screen.getByText("/opt/app/server")).toBeInTheDocument();
  });

  describe("language_packages section", () => {
    const langItem = makeItem({
      item_id: {
        kind: "LanguageEnv",
        key: { ecosystem: "pip", path: "/opt/app/venv" },
      },
      section_metadata: {
        ecosystem: "pip",
        confidence: "high",
        package_count: 2,
        manifest_basis: "requirements.txt",
        packages: [
          { name: "flask", version: "2.3.3" },
          { name: "requests", version: "2.31.0" },
        ],
      },
    });

    it("renders full package list in detail pane", () => {
      render(<ItemDetailPane item={langItem} sectionId="language_packages" />);

      const table = screen.getByTestId("detail-package-table");
      expect(table).toBeInTheDocument();

      // Check header row
      expect(within(table).getByText("Package")).toBeInTheDocument();
      expect(within(table).getByText("Version")).toBeInTheDocument();

      // Check package rows
      expect(within(table).getByText("flask")).toBeInTheDocument();
      expect(within(table).getByText("2.3.3")).toBeInTheDocument();
      expect(within(table).getByText("requests")).toBeInTheDocument();
      expect(within(table).getByText("2.31.0")).toBeInTheDocument();
    });

    it("renders confidence level", () => {
      render(<ItemDetailPane item={langItem} sectionId="language_packages" />);

      const el = screen.getByTestId("detail-confidence");
      expect(el).toHaveTextContent("high");
    });

    it("renders manifest basis", () => {
      render(<ItemDetailPane item={langItem} sectionId="language_packages" />);

      expect(
        screen.getByText("from requirements.txt"),
      ).toBeInTheDocument();
    });

    it("omits manifest basis when null", () => {
      const item = makeItem({
        item_id: {
          kind: "LanguageEnv",
          key: { ecosystem: "pip", path: "/opt/app/venv" },
        },
        section_metadata: {
          ecosystem: "pip",
          confidence: "medium",
          package_count: 0,
          manifest_basis: null,
          packages: [],
        },
      });

      render(<ItemDetailPane item={item} sectionId="language_packages" />);

      expect(screen.queryByTestId("detail-manifest-basis")).not.toBeInTheDocument();
    });

    it("does not render package table without sectionId", () => {
      render(<ItemDetailPane item={langItem} />);

      expect(screen.queryByTestId("detail-package-table")).not.toBeInTheDocument();
    });
  });

  describe("unmanaged_files section", () => {
    const unmanagedItem = makeItem({
      item_id: {
        kind: "UnmanagedFile",
        key: { path: "/opt/app/server" },
      },
      section_metadata: {
        file_type: "elf_binary",
        size: 1048576,
        under_var: false,
        provenance: {
          last_modified: 1700000000,
          uid: 1000,
          gid: 1000,
          permissions: "rwxr-xr-x",
          writable_mount: true,
          mutability: false,
          service_working_dir: true,
        },
      },
    });

    it("renders provenance signals in detail pane", () => {
      render(
        <ItemDetailPane item={unmanagedItem} sectionId="unmanaged_files" />,
      );

      const prov = screen.getByTestId("detail-provenance");
      expect(prov).toBeInTheDocument();

      // Check key provenance fields
      expect(within(prov).getByText("Permissions")).toBeInTheDocument();
      expect(within(prov).getByText("rwxr-xr-x")).toBeInTheDocument();
      expect(within(prov).getByText("UID")).toBeInTheDocument();
      // UID and GID both have value 1000 — verify both are present
      const thousandTexts = within(prov).getAllByText("1000");
      expect(thousandTexts.length).toBe(2);
    });

    it("renders last modified as human-readable date", () => {
      render(
        <ItemDetailPane item={unmanagedItem} sectionId="unmanaged_files" />,
      );

      const prov = screen.getByTestId("detail-provenance");
      // 1700000000 = 2023-11-14T22:13:20Z
      expect(within(prov).getByText("Last modified")).toBeInTheDocument();
      // Just check the date is rendered (locale-dependent, check year)
      expect(within(prov).getByTestId("detail-last-modified")).toHaveTextContent(
        "2023",
      );
    });

    it("renders writable mount indicator", () => {
      render(
        <ItemDetailPane item={unmanagedItem} sectionId="unmanaged_files" />,
      );

      const prov = screen.getByTestId("detail-provenance");
      expect(within(prov).getByText("Writable mount")).toBeInTheDocument();
      // "Yes" appears for both writable_mount and service_working_dir
      const yesTexts = within(prov).getAllByText("Yes");
      expect(yesTexts.length).toBeGreaterThanOrEqual(1);
    });

    it("renders service working dir indicator", () => {
      render(
        <ItemDetailPane item={unmanagedItem} sectionId="unmanaged_files" />,
      );

      const prov = screen.getByTestId("detail-provenance");
      expect(within(prov).getByText("Service working dir")).toBeInTheDocument();
    });

    it("renders GID field", () => {
      render(
        <ItemDetailPane item={unmanagedItem} sectionId="unmanaged_files" />,
      );

      const prov = screen.getByTestId("detail-provenance");
      expect(within(prov).getByText("GID")).toBeInTheDocument();
    });

    it("does not render provenance without sectionId", () => {
      render(<ItemDetailPane item={unmanagedItem} />);

      expect(
        screen.queryByTestId("detail-provenance"),
      ).not.toBeInTheDocument();
    });
  });
});
