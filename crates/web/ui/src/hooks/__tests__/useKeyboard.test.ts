import { describe, it, expect, vi, afterEach } from "vitest";
import { renderHook } from "@testing-library/react";
import { fireEvent } from "@testing-library/react";
import { useKeyboard } from "../useKeyboard";
import type { UseKeyboardOptions } from "../useKeyboard";

function makeOptions(
  overrides: Partial<UseKeyboardOptions> = {},
): UseKeyboardOptions {
  return {
    onUndo: vi.fn(),
    onRedo: vi.fn(),
    onTogglePanel: vi.fn(),
    onExport: vi.fn(),
    onSectionChange: vi.fn(),
    onOpenSearch: vi.fn(),
    onOpenGlobalSearch: vi.fn(),
    onOpenShortcuts: vi.fn(),
    ...overrides,
  };
}

describe("useKeyboard", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("calls onUndo on Ctrl+Z", () => {
    const opts = makeOptions();
    renderHook(() => useKeyboard(opts));

    fireEvent.keyDown(document, { key: "z", ctrlKey: true });
    expect(opts.onUndo).toHaveBeenCalledTimes(1);
  });

  it("calls onRedo on Ctrl+Shift+Z", () => {
    const opts = makeOptions();
    renderHook(() => useKeyboard(opts));

    fireEvent.keyDown(document, { key: "z", ctrlKey: true, shiftKey: true });
    expect(opts.onRedo).toHaveBeenCalledTimes(1);
  });

  it("calls onTogglePanel on Ctrl+E", () => {
    const opts = makeOptions();
    renderHook(() => useKeyboard(opts));

    fireEvent.keyDown(document, { key: "e", ctrlKey: true });
    expect(opts.onTogglePanel).toHaveBeenCalledTimes(1);
  });

  it("calls onExport on Ctrl+Shift+E", () => {
    const opts = makeOptions();
    renderHook(() => useKeyboard(opts));

    fireEvent.keyDown(document, { key: "e", ctrlKey: true, shiftKey: true });
    expect(opts.onExport).toHaveBeenCalledTimes(1);
  });

  it("calls onOpenGlobalSearch on Ctrl+K", () => {
    const opts = makeOptions();
    renderHook(() => useKeyboard(opts));

    fireEvent.keyDown(document, { key: "k", ctrlKey: true });
    expect(opts.onOpenGlobalSearch).toHaveBeenCalledTimes(1);
  });

  it("calls onOpenSearch on /", () => {
    const opts = makeOptions();
    renderHook(() => useKeyboard(opts));

    fireEvent.keyDown(document, { key: "/" });
    expect(opts.onOpenSearch).toHaveBeenCalledTimes(1);
  });

  it("calls onOpenShortcuts on ?", () => {
    const opts = makeOptions();
    renderHook(() => useKeyboard(opts));

    fireEvent.keyDown(document, { key: "?" });
    expect(opts.onOpenShortcuts).toHaveBeenCalledTimes(1);
  });

  it("calls onSectionChange with correct section on 1-9", () => {
    const opts = makeOptions();
    renderHook(() => useKeyboard(opts));

    fireEvent.keyDown(document, { key: "1" });
    expect(opts.onSectionChange).toHaveBeenCalledWith("packages");

    fireEvent.keyDown(document, { key: "2" });
    expect(opts.onSectionChange).toHaveBeenCalledWith("configs");

    fireEvent.keyDown(document, { key: "3" });
    expect(opts.onSectionChange).toHaveBeenCalledWith("users_groups");
  });

  it("maps key 4 to services", () => {
    const opts = makeOptions();
    renderHook(() => useKeyboard(opts));

    fireEvent.keyDown(document, { key: "4" });
    expect(opts.onSectionChange).toHaveBeenCalledWith("services");
  });

  it("maps key 5 to containers", () => {
    const opts = makeOptions();
    renderHook(() => useKeyboard(opts));

    fireEvent.keyDown(document, { key: "5" });
    expect(opts.onSectionChange).toHaveBeenCalledWith("containers");
  });

  it("maps key 6 to language_packages", () => {
    const opts = makeOptions();
    renderHook(() => useKeyboard(opts));

    fireEvent.keyDown(document, { key: "6" });
    expect(opts.onSectionChange).toHaveBeenCalledWith("language_packages");
  });

  it("maps key 7 to unmanaged_files", () => {
    const opts = makeOptions();
    renderHook(() => useKeyboard(opts));

    fireEvent.keyDown(document, { key: "7" });
    expect(opts.onSectionChange).toHaveBeenCalledWith("unmanaged_files");
  });

  it("maps key 8 to system_tuning", () => {
    const opts = makeOptions();
    renderHook(() => useKeyboard(opts));

    fireEvent.keyDown(document, { key: "8" });
    expect(opts.onSectionChange).toHaveBeenCalledWith("system_tuning");
  });

  it("maps key 9 to version_changes", () => {
    const opts = makeOptions();
    renderHook(() => useKeyboard(opts));

    fireEvent.keyDown(document, { key: "9" });
    expect(opts.onSectionChange).toHaveBeenCalledWith("version_changes");
  });

  it("suppresses single-key shortcuts when focus is in an input", () => {
    const opts = makeOptions();
    renderHook(() => useKeyboard(opts));

    const input = document.createElement("input");
    document.body.appendChild(input);
    input.focus();

    fireEvent.keyDown(input, { key: "/" });
    expect(opts.onOpenSearch).not.toHaveBeenCalled();

    fireEvent.keyDown(input, { key: "?" });
    expect(opts.onOpenShortcuts).not.toHaveBeenCalled();

    fireEvent.keyDown(input, { key: "1" });
    expect(opts.onSectionChange).not.toHaveBeenCalled();

    document.body.removeChild(input);
  });

  it("allows Ctrl-chord shortcuts even in text inputs", () => {
    const opts = makeOptions();
    renderHook(() => useKeyboard(opts));

    const input = document.createElement("input");
    document.body.appendChild(input);
    input.focus();

    fireEvent.keyDown(input, { key: "z", ctrlKey: true });
    expect(opts.onUndo).toHaveBeenCalledTimes(1);

    fireEvent.keyDown(input, { key: "k", ctrlKey: true });
    expect(opts.onOpenGlobalSearch).toHaveBeenCalledTimes(1);

    document.body.removeChild(input);
  });

  it("suppresses single-key shortcuts when a dialog is open", () => {
    const opts = makeOptions();
    renderHook(() => useKeyboard(opts));

    // Simulate a dialog being open in the DOM
    const dialog = document.createElement("div");
    dialog.setAttribute("role", "dialog");
    document.body.appendChild(dialog);

    fireEvent.keyDown(document, { key: "/" });
    expect(opts.onOpenSearch).not.toHaveBeenCalled();

    fireEvent.keyDown(document, { key: "?" });
    expect(opts.onOpenShortcuts).not.toHaveBeenCalled();

    fireEvent.keyDown(document, { key: "1" });
    expect(opts.onSectionChange).not.toHaveBeenCalled();

    document.body.removeChild(dialog);
  });

  it("suppresses Ctrl-chord shortcuts when a dialog is open", () => {
    const opts = makeOptions();
    renderHook(() => useKeyboard(opts));

    const dialog = document.createElement("div");
    dialog.setAttribute("role", "dialog");
    document.body.appendChild(dialog);

    fireEvent.keyDown(document, { key: "z", ctrlKey: true });
    expect(opts.onUndo).not.toHaveBeenCalled();

    fireEvent.keyDown(document, { key: "e", ctrlKey: true });
    expect(opts.onTogglePanel).not.toHaveBeenCalled();

    fireEvent.keyDown(document, { key: "k", ctrlKey: true });
    expect(opts.onOpenGlobalSearch).not.toHaveBeenCalled();

    document.body.removeChild(dialog);
  });

  it("cleans up event listener on unmount", () => {
    const opts = makeOptions();
    const { unmount } = renderHook(() => useKeyboard(opts));

    unmount();

    fireEvent.keyDown(document, { key: "/" });
    expect(opts.onOpenSearch).not.toHaveBeenCalled();
  });

  describe("group-based navigation (1-8)", () => {
    const MOCK_GROUPS = [
      {
        slug: "packages-group",
        label: "Packages",
        sections: [{ id: "packages", label: "Packages", is_triage: true }],
        has_actionable_sections: true,
      },
      {
        slug: "system-config",
        label: "System Configuration",
        sections: [
          { id: "config", label: "Configuration Files", is_triage: true },
          { id: "kernel_boot", label: "Kernel & Boot", is_triage: false },
          { id: "selinux", label: "Security & Access Control", is_triage: false },
        ],
        has_actionable_sections: true,
      },
      {
        slug: "services-scheduling",
        label: "Services & Scheduling",
        sections: [
          { id: "services", label: "Services", is_triage: true },
          { id: "scheduled_tasks", label: "Scheduled Tasks", is_triage: false },
          { id: "containers", label: "Containers", is_triage: true },
        ],
        has_actionable_sections: true,
      },
      {
        slug: "identity",
        label: "Users & Identity",
        sections: [
          { id: "users_groups", label: "Users & Groups", is_triage: true },
        ],
        has_actionable_sections: true,
      },
      {
        slug: "network-group",
        label: "Network",
        sections: [{ id: "network", label: "Network", is_triage: false }],
        has_actionable_sections: false,
      },
      {
        slug: "storage-group",
        label: "Storage",
        sections: [{ id: "storage", label: "Storage", is_triage: false }],
        has_actionable_sections: false,
      },
      {
        slug: "software",
        label: "Software & Files",
        sections: [
          { id: "non_rpm_software", label: "Non-RPM Software", is_triage: true },
          { id: "unmanaged_files", label: "Unmanaged Files", is_triage: true },
        ],
        has_actionable_sections: true,
      },
      {
        slug: "secrets",
        label: "Secrets & Subscription",
        sections: [
          { id: "secrets", label: "Secrets", is_triage: false },
          { id: "subscription", label: "Subscription", is_triage: false },
        ],
        has_actionable_sections: false,
      },
    ];

    it("key 1 navigates to singleton group section", () => {
      const opts = makeOptions({ groups: MOCK_GROUPS });
      renderHook(() => useKeyboard(opts));

      fireEvent.keyDown(document, { key: "1" });
      expect(opts.onSectionChange).toHaveBeenCalledWith("packages");
    });

    it("key 2 navigates to first triage section in multi-section group", () => {
      const opts = makeOptions({ groups: MOCK_GROUPS });
      renderHook(() => useKeyboard(opts));

      fireEvent.keyDown(document, { key: "2" });
      // system-config has config (triage), kernel_boot, selinux
      expect(opts.onSectionChange).toHaveBeenCalledWith("config");
    });

    it("key 3 navigates to first triage section (services)", () => {
      const opts = makeOptions({ groups: MOCK_GROUPS });
      renderHook(() => useKeyboard(opts));

      fireEvent.keyDown(document, { key: "3" });
      expect(opts.onSectionChange).toHaveBeenCalledWith("services");
    });

    it("key 4 navigates to singleton group (users_groups)", () => {
      const opts = makeOptions({ groups: MOCK_GROUPS });
      renderHook(() => useKeyboard(opts));

      fireEvent.keyDown(document, { key: "4" });
      expect(opts.onSectionChange).toHaveBeenCalledWith("users_groups");
    });

    it("key 5 navigates to singleton reference group (network)", () => {
      const opts = makeOptions({ groups: MOCK_GROUPS });
      renderHook(() => useKeyboard(opts));

      fireEvent.keyDown(document, { key: "5" });
      expect(opts.onSectionChange).toHaveBeenCalledWith("network");
    });

    it("key 8 navigates to first section when no triage in group", () => {
      const opts = makeOptions({ groups: MOCK_GROUPS });
      renderHook(() => useKeyboard(opts));

      fireEvent.keyDown(document, { key: "8" });
      // secrets group has no triage sections, falls back to first child
      expect(opts.onSectionChange).toHaveBeenCalledWith("secrets");
    });

    it("ignores key 9 when groups provided (only 1-8 supported)", () => {
      const opts = makeOptions({ groups: MOCK_GROUPS });
      renderHook(() => useKeyboard(opts));

      fireEvent.keyDown(document, { key: "9" });
      expect(opts.onSectionChange).not.toHaveBeenCalled();
    });

    it("falls back to sectionIds when groups is undefined", () => {
      const opts = makeOptions();
      renderHook(() => useKeyboard(opts));

      fireEvent.keyDown(document, { key: "1" });
      // Without groups, uses SINGLE_HOST_SECTION_IDS[0] = "packages"
      expect(opts.onSectionChange).toHaveBeenCalledWith("packages");

      fireEvent.keyDown(document, { key: "9" });
      // Key 9 works in flat mode
      expect(opts.onSectionChange).toHaveBeenCalledWith("version_changes");
    });

    it("falls back to sectionIds when groups is empty array", () => {
      const opts = makeOptions({ groups: [] });
      renderHook(() => useKeyboard(opts));

      fireEvent.keyDown(document, { key: "1" });
      expect(opts.onSectionChange).toHaveBeenCalledWith("packages");
    });

    it("suppresses group shortcuts when in text input", () => {
      const opts = makeOptions({ groups: MOCK_GROUPS });
      renderHook(() => useKeyboard(opts));

      const input = document.createElement("input");
      document.body.appendChild(input);
      input.focus();

      fireEvent.keyDown(input, { key: "1" });
      expect(opts.onSectionChange).not.toHaveBeenCalled();

      document.body.removeChild(input);
    });

    it("suppresses group shortcuts when dialog is open", () => {
      const opts = makeOptions({ groups: MOCK_GROUPS });
      renderHook(() => useKeyboard(opts));

      const dialog = document.createElement("div");
      dialog.setAttribute("role", "dialog");
      document.body.appendChild(dialog);

      fireEvent.keyDown(document, { key: "1" });
      expect(opts.onSectionChange).not.toHaveBeenCalled();

      document.body.removeChild(dialog);
    });
  });
});
