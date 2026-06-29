import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { AggregateItemRow, formatFileType, formatSize } from "../AggregateItemRow";
import type { AggregateItem, ItemId } from "../../../api/types";
import type { UseVariantAckResult } from "../../../hooks/useVariantAck";

const defaultAck: UseVariantAckResult = {
  isAcked: () => false,
  getStatus: () => "unreviewed" as const,
  confirm: vi.fn(),
  unconfirm: vi.fn(),
  markChanged: vi.fn(),
  unackedCount: 0,
  totalCount: 0,
};

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

describe("AggregateItemRow", () => {
  it("renders item name from Package item_id", () => {
    const item = makeItem({
      item_id: { kind: "Package", key: { name: "httpd", arch: "x86_64" } },
    });

    render(
      <AggregateItemRow
        item={item}
        isDecisionSection={true}
        onToggle={vi.fn()}
        ack={defaultAck}
      />,
    );

    expect(screen.getByText("httpd.x86_64")).toBeInTheDocument();
  });

  it("renders item name from Config item_id", () => {
    const item = makeItem({
      item_id: { kind: "Config", key: { path: "/etc/httpd/conf/httpd.conf" } },
    });

    render(
      <AggregateItemRow
        item={item}
        isDecisionSection={true}
        onToggle={vi.fn()}
        ack={defaultAck}
      />,
    );

    expect(screen.getByText("/etc/httpd/conf/httpd.conf")).toBeInTheDocument();
  });

  it("renders prevalence chip", () => {
    const item = makeItem({
      item_id: { kind: "Package", key: { name: "httpd", arch: "x86_64" } },
      prevalence: { count: 8, total: 12 },
    });

    render(
      <AggregateItemRow
        item={item}
        isDecisionSection={true}
        onToggle={vi.fn()}
        ack={defaultAck}
      />,
    );

    expect(screen.getByText("8/12 hosts")).toBeInTheDocument();
  });

  it("renders variant count when variants exist", () => {
    const item = makeItem({
      item_id: { kind: "Package", key: { name: "httpd", arch: "x86_64" } },
      variants: {
        count: 3,
        selected: "abc123",
        options: [
          { hash: "abc123", hosts: ["h1"], host_count: 1, selected: true },
          { hash: "def456", hosts: ["h2"], host_count: 1, selected: false },
          { hash: "ghi789", hosts: ["h3"], host_count: 1, selected: false },
        ],
      },
    });

    render(
      <AggregateItemRow
        item={item}
        isDecisionSection={true}
        onToggle={vi.fn()}
        ack={defaultAck}
      />,
    );

    expect(screen.getByText("3 variants")).toBeInTheDocument();
  });

  it("does not render variant indicator when no variants", () => {
    const item = makeItem({
      item_id: { kind: "Package", key: { name: "httpd", arch: "x86_64" } },
    });

    render(
      <AggregateItemRow
        item={item}
        isDecisionSection={true}
        onToggle={vi.fn()}
        ack={defaultAck}
      />,
    );

    expect(screen.queryByText(/variants/)).not.toBeInTheDocument();
  });

  it("renders toggle for decision sections", () => {
    const item = makeItem({
      item_id: { kind: "Package", key: { name: "httpd", arch: "x86_64" } },
    });

    render(
      <AggregateItemRow
        item={item}
        isDecisionSection={true}
        onToggle={vi.fn()}
        ack={defaultAck}
      />,
    );

    expect(screen.getByRole("switch")).toBeInTheDocument();
  });

  it("does not render toggle for context sections", () => {
    const item = makeItem({
      item_id: { kind: "Package", key: { name: "httpd", arch: "x86_64" } },
    });

    render(
      <AggregateItemRow
        item={item}
        isDecisionSection={false}
        onToggle={vi.fn()}
        ack={defaultAck}
      />,
    );

    expect(screen.queryByRole("switch")).not.toBeInTheDocument();
  });

  it("sets data-item-id attribute", () => {
    const itemId: ItemId = {
      kind: "Package",
      key: { name: "httpd", arch: "x86_64" },
    };
    const item = makeItem({ item_id: itemId });

    render(
      <AggregateItemRow
        item={item}
        isDecisionSection={true}
        onToggle={vi.fn()}
        ack={defaultAck}
      />,
    );

    const row = screen.getByTestId("aggregate-item-row");
    expect(row).toHaveAttribute("data-item-id", JSON.stringify(itemId));
  });

  it("calls onToggle when toggle is switched", async () => {
    const user = userEvent.setup();
    const onToggle = vi.fn();
    const item = makeItem({
      item_id: { kind: "Package", key: { name: "httpd", arch: "x86_64" } },
      include: true,
    });

    render(
      <AggregateItemRow
        item={item}
        isDecisionSection={true}
        onToggle={onToggle}
        ack={defaultAck}
      />,
    );

    const toggle = screen.getByRole("switch", { name: /toggle httpd/i });
    await user.click(toggle);

    expect(onToggle).toHaveBeenCalledWith(
      { kind: "Package", key: { name: "httpd", arch: "x86_64" } },
      false,
    );
  });

  it("calls onExpandVariant when variant indicator clicked", async () => {
    const user = userEvent.setup();
    const onExpand = vi.fn();
    const item = makeItem({
      item_id: { kind: "Package", key: { name: "httpd", arch: "x86_64" } },
      variants: {
        count: 3,
        selected: "abc123",
        options: [
          { hash: "abc123", hosts: ["h1"], host_count: 1, selected: true },
          { hash: "def456", hosts: ["h2"], host_count: 1, selected: false },
          { hash: "ghi789", hosts: ["h3"], host_count: 1, selected: false },
        ],
      },
    });

    render(
      <AggregateItemRow
        item={item}
        isDecisionSection={true}
        onToggle={vi.fn()}
        ack={defaultAck}
        onExpandVariant={onExpand}
      />,
    );

    const variantBtn = screen.getByText("3 variants");
    await user.click(variantBtn);

    expect(onExpand).toHaveBeenCalledWith({
      kind: "Package",
      key: { name: "httpd", arch: "x86_64" },
    });
  });

  it("does not render attention badges in aggregate item rows", () => {
    const levels = ["needs_review", "informational", "routine"];
    for (const level of levels) {
      const item = makeItem({
        item_id: {
          kind: "Package",
          key: { name: `test-${level}`, arch: "x86_64" },
        },
        triage: {
          bucket: "universal" as const,
          prevalence: { count: 1, total: 1 },
        },
      });

      const { unmount } = render(
        <AggregateItemRow
          item={item}
          isDecisionSection={true}
          onToggle={vi.fn()}
          ack={defaultAck}
        />,
      );

      expect(screen.queryByTestId("attention-badge")).not.toBeInTheDocument();
      unmount();
    }
  });

  it("does not render attention badge for none level", () => {
    const item = makeItem({
      item_id: { kind: "Package", key: { name: "httpd", arch: "x86_64" } },
      triage: {
        bucket: "universal" as const,
        prevalence: { count: 1, total: 1 },
      },
    });

    render(
      <AggregateItemRow
        item={item}
        isDecisionSection={true}
        onToggle={vi.fn()}
        ack={defaultAck}
      />,
    );

    expect(screen.queryByTestId("attention-badge")).not.toBeInTheDocument();
  });

  // --- Section-aware metadata rendering ---

  describe("language_packages section metadata", () => {
    const langItem = makeItem({
      item_id: {
        kind: "LanguageEnv",
        key: { ecosystem: "pip", path: "/app/requirements.txt" },
      },
      section_metadata: {
        ecosystem: "pip",
        confidence: "high",
        package_count: 12,
        manifest_basis: "requirements.txt",
        packages: [
          { name: "flask", version: "2.3.0" },
          { name: "requests", version: "2.31.0" },
        ],
      },
    });

    it("renders ecosystem label for language package items", () => {
      render(
        <AggregateItemRow
          item={langItem}
          isDecisionSection={true}
          onToggle={vi.fn()}
          ack={defaultAck}
          sectionId="language_packages"
        />,
      );

      expect(screen.getByTestId("section-meta-ecosystem")).toHaveTextContent(
        "pip",
      );
    });

    it("renders green confidence badge for high confidence", () => {
      render(
        <AggregateItemRow
          item={langItem}
          isDecisionSection={true}
          onToggle={vi.fn()}
          ack={defaultAck}
          sectionId="language_packages"
        />,
      );

      const badge = screen.getByTestId("section-meta-confidence");
      expect(badge).toHaveTextContent("high");
      expect(badge).toHaveClass("aggregate-item-row__confidence--high");
    });

    it("renders orange confidence badge for medium confidence", () => {
      const medItem = makeItem({
        item_id: {
          kind: "LanguageEnv",
          key: { ecosystem: "npm", path: "/app/package.json" },
        },
        section_metadata: {
          ecosystem: "npm",
          confidence: "medium",
          package_count: 5,
          manifest_basis: null,
          packages: [],
        },
      });

      render(
        <AggregateItemRow
          item={medItem}
          isDecisionSection={true}
          onToggle={vi.fn()}
          ack={defaultAck}
          sectionId="language_packages"
        />,
      );

      const badge = screen.getByTestId("section-meta-confidence");
      expect(badge).toHaveTextContent("medium");
      expect(badge).toHaveClass("aggregate-item-row__confidence--medium");
    });

    it("renders package count badge", () => {
      render(
        <AggregateItemRow
          item={langItem}
          isDecisionSection={true}
          onToggle={vi.fn()}
          ack={defaultAck}
          sectionId="language_packages"
        />,
      );

      expect(screen.getByTestId("section-meta-pkg-count")).toHaveTextContent(
        "12 packages",
      );
    });

    it("renders manifest basis subtitle when present", () => {
      render(
        <AggregateItemRow
          item={langItem}
          isDecisionSection={true}
          onToggle={vi.fn()}
          ack={defaultAck}
          sectionId="language_packages"
        />,
      );

      expect(
        screen.getByTestId("section-meta-manifest-basis"),
      ).toHaveTextContent("requirements.txt");
    });

    it("omits manifest basis when null", () => {
      const noManifest = makeItem({
        item_id: {
          kind: "LanguageEnv",
          key: { ecosystem: "gem", path: "/app/Gemfile" },
        },
        section_metadata: {
          ecosystem: "gem",
          confidence: "low",
          package_count: 3,
          manifest_basis: null,
          packages: [],
        },
      });

      render(
        <AggregateItemRow
          item={noManifest}
          isDecisionSection={true}
          onToggle={vi.fn()}
          ack={defaultAck}
          sectionId="language_packages"
        />,
      );

      expect(
        screen.queryByTestId("section-meta-manifest-basis"),
      ).not.toBeInTheDocument();
    });

    it("does not render language metadata when sectionId is not language_packages", () => {
      render(
        <AggregateItemRow
          item={langItem}
          isDecisionSection={true}
          onToggle={vi.fn()}
          ack={defaultAck}
          sectionId="packages"
        />,
      );

      expect(
        screen.queryByTestId("section-meta-ecosystem"),
      ).not.toBeInTheDocument();
    });
  });

  describe("unmanaged_files section metadata", () => {
    const fileItem = makeItem({
      item_id: {
        kind: "UnmanagedFile",
        key: { path: "/var/lib/myapp/data.bin" },
      },
      section_metadata: {
        file_type: "elf_binary",
        size: 2400000,
        under_var: true,
        provenance: {
          last_modified: 1700000000,
          uid: 0,
          gid: 0,
          permissions: "0755",
          writable_mount: false,
          mutability: false,
          service_working_dir: false,
        },
      },
    });

    it("renders file type label for unmanaged file items", () => {
      render(
        <AggregateItemRow
          item={fileItem}
          isDecisionSection={false}
          onToggle={vi.fn()}
          ack={defaultAck}
          sectionId="unmanaged_files"
        />,
      );

      expect(screen.getByTestId("section-meta-file-type")).toHaveTextContent(
        "ELF Binary",
      );
    });

    it("renders size badge", () => {
      render(
        <AggregateItemRow
          item={fileItem}
          isDecisionSection={false}
          onToggle={vi.fn()}
          ack={defaultAck}
          sectionId="unmanaged_files"
        />,
      );

      expect(screen.getByTestId("section-meta-size")).toHaveTextContent(
        "2.3 MB",
      );
    });

    it("renders /var warning icon when under /var", () => {
      render(
        <AggregateItemRow
          item={fileItem}
          isDecisionSection={false}
          onToggle={vi.fn()}
          ack={defaultAck}
          sectionId="unmanaged_files"
        />,
      );

      expect(
        screen.getByTestId("section-meta-var-warning"),
      ).toBeInTheDocument();
    });

    it("does not render /var warning when not under /var", () => {
      const noVarItem = makeItem({
        item_id: {
          kind: "UnmanagedFile",
          key: { path: "/opt/myapp/data.bin" },
        },
        section_metadata: {
          file_type: "data",
          size: 500,
          under_var: false,
          provenance: {
            last_modified: 1700000000,
            uid: 0,
            gid: 0,
            permissions: "0644",
            writable_mount: false,
            mutability: false,
            service_working_dir: false,
          },
        },
      });

      render(
        <AggregateItemRow
          item={noVarItem}
          isDecisionSection={false}
          onToggle={vi.fn()}
          ack={defaultAck}
          sectionId="unmanaged_files"
        />,
      );

      expect(
        screen.queryByTestId("section-meta-var-warning"),
      ).not.toBeInTheDocument();
    });

    it("does not render file metadata when sectionId is not unmanaged_files", () => {
      render(
        <AggregateItemRow
          item={fileItem}
          isDecisionSection={false}
          onToggle={vi.fn()}
          ack={defaultAck}
          sectionId="packages"
        />,
      );

      expect(
        screen.queryByTestId("section-meta-file-type"),
      ).not.toBeInTheDocument();
    });
  });

  describe("formatFileType", () => {
    it("maps known file types to display names", () => {
      expect(formatFileType("elf_binary")).toBe("ELF Binary");
      expect(formatFileType("shell_script")).toBe("Shell Script");
      expect(formatFileType("data")).toBe("Data");
      expect(formatFileType("text")).toBe("Text");
      expect(formatFileType("symlink")).toBe("Symlink");
      expect(formatFileType("directory")).toBe("Directory");
    });

    it("title-cases unknown file types", () => {
      expect(formatFileType("python_script")).toBe("Python Script");
      expect(formatFileType("custom_type")).toBe("Custom Type");
    });
  });

  describe("keyboard interaction", () => {
    it("Enter expands details in decision section", async () => {
      const user = userEvent.setup();
      const onExpandVariant = vi.fn();
      const item = makeItem({
        item_id: { kind: "Config", key: { path: "/etc/httpd.conf" } },
      });

      render(
        <AggregateItemRow
          item={item}
          isDecisionSection={true}
          onToggle={vi.fn()}
          ack={defaultAck}
          onExpandVariant={onExpandVariant}
        />,
      );

      const row = screen.getByTestId("aggregate-item-row");
      row.focus();
      await user.keyboard("{Enter}");
      expect(onExpandVariant).toHaveBeenCalledWith(item.item_id);
    });

    it("Space expands details in decision section", async () => {
      const user = userEvent.setup();
      const onExpandVariant = vi.fn();
      const item = makeItem({
        item_id: { kind: "Config", key: { path: "/etc/httpd.conf" } },
      });

      render(
        <AggregateItemRow
          item={item}
          isDecisionSection={true}
          onToggle={vi.fn()}
          ack={defaultAck}
          onExpandVariant={onExpandVariant}
        />,
      );

      const row = screen.getByTestId("aggregate-item-row");
      row.focus();
      await user.keyboard(" ");
      expect(onExpandVariant).toHaveBeenCalledWith(item.item_id);
    });

    it("Escape closes expanded details and returns focus", async () => {
      const user = userEvent.setup();
      const onExpandVariant = vi.fn();
      const item = makeItem({
        item_id: { kind: "Config", key: { path: "/etc/httpd.conf" } },
      });

      render(
        <AggregateItemRow
          item={item}
          isDecisionSection={true}
          onToggle={vi.fn()}
          ack={defaultAck}
          onExpandVariant={onExpandVariant}
          isExpanded={true}
        />,
      );

      const row = screen.getByTestId("aggregate-item-row");
      row.focus();
      await user.keyboard("{Escape}");
      expect(onExpandVariant).toHaveBeenCalledWith(item.item_id);
    });
  });

  describe("formatSize", () => {
    it("formats bytes", () => {
      expect(formatSize(500)).toBe("500 B");
    });

    it("formats kilobytes", () => {
      expect(formatSize(2048)).toBe("2 KB");
    });

    it("formats megabytes", () => {
      expect(formatSize(2400000)).toBe("2.3 MB");
    });

    it("formats gigabytes", () => {
      expect(formatSize(1500000000)).toBe("1.4 GB");
    });
  });
});
