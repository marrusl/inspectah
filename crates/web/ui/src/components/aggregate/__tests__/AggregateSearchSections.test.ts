import { describe, it, expect } from "vitest";
import { buildAggregateSearchSections } from "../../AggregateApp";
import type {
  AggregateSection,
  ItemId,
  LanguagePackageMetadata,
  UnmanagedFileMetadata,
} from "../../../api/types";

/** Helper to create a minimal AggregateSection with flat items. */
function makeSection(
  id: string,
  displayName: string,
  items: Array<{
    item_id: { kind: string; key: Record<string, unknown> };
    section_metadata?: Record<string, unknown>;
  }>,
): AggregateSection {
  return {
    id,
    display_name: displayName,
    is_decision_section: false,
    items: items.map((i) => ({
      item_id: i.item_id as unknown as ItemId,
      include: true,
      triage: {
        prevalence: { count: 1, total: 1 },
        primary_reason: null,
        annotations: [],
      },
      prevalence: { count: 1, total: 1 },
      source_repo: "",
      section_metadata: i.section_metadata,
    })),
  } as unknown as AggregateSection;
}

describe("buildAggregateSearchSections", () => {
  it("includes language package metadata in searchable_text", () => {
    const meta: LanguagePackageMetadata = {
      ecosystem: "pip",
      confidence: "high",
      package_count: 2,
      manifest_basis: "requirements.txt",
      packages: [
        { name: "flask", version: "2.0.0" },
        { name: "requests", version: "2.28.0" },
      ],
    };

    const sections = [
      makeSection("language_packages", "Language Packages", [
        {
          item_id: {
            kind: "LanguageEnv",
            key: { ecosystem: "pip", path: "/opt/myapp/venv" },
          },
          section_metadata: meta as unknown as Record<string, unknown>,
        },
      ]),
    ];

    const result = buildAggregateSearchSections(sections);
    expect(result).toHaveLength(1);
    expect(result[0].id).toBe("language_packages");
    expect(result[0].items).toHaveLength(1);

    const text = result[0].items[0].searchable_text;
    // Should include ecosystem
    expect(text).toContain("pip");
    // Should include environment path
    expect(text).toContain("/opt/myapp/venv");
    // Should include package names
    expect(text).toContain("flask");
    expect(text).toContain("requests");
    // Should include manifest basis
    expect(text).toContain("requirements.txt");
  });

  it("searching 'flask' matches a language package env containing flask", () => {
    const meta: LanguagePackageMetadata = {
      ecosystem: "pip",
      confidence: "high",
      package_count: 3,
      manifest_basis: "dist-info",
      packages: [
        { name: "flask", version: "2.0.0" },
        { name: "werkzeug", version: "2.0.0" },
        { name: "jinja2", version: "3.1.0" },
      ],
    };

    const sections = [
      makeSection("language_packages", "Language Packages", [
        {
          item_id: {
            kind: "LanguageEnv",
            key: { ecosystem: "pip", path: "/opt/webapp/venv" },
          },
          section_metadata: meta as unknown as Record<string, unknown>,
        },
      ]),
    ];

    const result = buildAggregateSearchSections(sections);
    const text = result[0].items[0].searchable_text;
    expect(text.toLowerCase()).toContain("flask");
  });

  it("includes unmanaged file metadata in searchable_text", () => {
    const meta: UnmanagedFileMetadata = {
      file_type: "elf_binary",
      size: 4096,
      under_var: false,
      provenance: {
        last_modified: 1700000000,
        uid: 0,
        gid: 0,
        permissions: "0755",
        writable_mount: false,
        mutability: false,
        service_working_dir: false,
      },
    };

    const sections = [
      makeSection("unmanaged_files", "Unmanaged Files", [
        {
          item_id: {
            kind: "UnmanagedFile",
            key: { path: "/opt/splunk/bin/splunkd" },
          },
          section_metadata: meta as unknown as Record<string, unknown>,
        },
      ]),
    ];

    const result = buildAggregateSearchSections(sections);
    expect(result).toHaveLength(1);
    expect(result[0].id).toBe("unmanaged_files");
    expect(result[0].items).toHaveLength(1);

    const text = result[0].items[0].searchable_text;
    // Should include file path
    expect(text).toContain("/opt/splunk/bin/splunkd");
    // Should include file type
    expect(text).toContain("elf_binary");
  });

  it("searching 'elf' matches an unmanaged file with elf file_type", () => {
    const meta: UnmanagedFileMetadata = {
      file_type: "elf_binary",
      size: 2048,
      under_var: false,
      provenance: {
        last_modified: 1700000000,
        uid: 0,
        gid: 0,
        permissions: "0755",
        writable_mount: false,
        mutability: false,
        service_working_dir: false,
      },
    };

    const sections = [
      makeSection("unmanaged_files", "Unmanaged Files", [
        {
          item_id: {
            kind: "UnmanagedFile",
            key: { path: "/usr/local/bin/custom" },
          },
          section_metadata: meta as unknown as Record<string, unknown>,
        },
      ]),
    ];

    const result = buildAggregateSearchSections(sections);
    const text = result[0].items[0].searchable_text;
    expect(text.toLowerCase()).toContain("elf");
  });

  it("handles items without section_metadata gracefully", () => {
    const sections = [
      makeSection("language_packages", "Language Packages", [
        {
          item_id: {
            kind: "LanguageEnv",
            key: { ecosystem: "npm", path: "/opt/app" },
          },
          // No section_metadata
        },
      ]),
      makeSection("unmanaged_files", "Unmanaged Files", [
        {
          item_id: {
            kind: "UnmanagedFile",
            key: { path: "/tmp/somefile" },
          },
          // No section_metadata
        },
      ]),
    ];

    const result = buildAggregateSearchSections(sections);
    // Should still include the display name as searchable text
    expect(result[0].items[0].searchable_text).toContain("npm:/opt/app");
    expect(result[1].items[0].searchable_text).toContain("/tmp/somefile");
  });

  it("does not modify search text for non-metadata sections", () => {
    const sections = [
      makeSection("services", "Services", [
        {
          item_id: {
            kind: "Service",
            key: { unit: "httpd.service" },
          },
        },
      ]),
    ];

    const result = buildAggregateSearchSections(sections);
    expect(result[0].items[0].searchable_text).toBe("httpd.service");
  });
});
