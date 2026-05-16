import { describe, it, expect, vi, afterEach } from "vitest";
import { renderHook } from "@testing-library/react";
import { fireEvent } from "@testing-library/react";
import { useKeyboard } from "../useKeyboard";
import type { UseKeyboardOptions } from "../useKeyboard";

function makeOptions(overrides: Partial<UseKeyboardOptions> = {}): UseKeyboardOptions {
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
    expect(opts.onSectionChange).toHaveBeenCalledWith("services");
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

  it("allows Ctrl-chord shortcuts even when a dialog is open", () => {
    const opts = makeOptions();
    renderHook(() => useKeyboard(opts));

    const dialog = document.createElement("div");
    dialog.setAttribute("role", "dialog");
    document.body.appendChild(dialog);

    fireEvent.keyDown(document, { key: "z", ctrlKey: true });
    expect(opts.onUndo).toHaveBeenCalledTimes(1);

    fireEvent.keyDown(document, { key: "e", ctrlKey: true });
    expect(opts.onTogglePanel).toHaveBeenCalledTimes(1);

    document.body.removeChild(dialog);
  });

  it("cleans up event listener on unmount", () => {
    const opts = makeOptions();
    const { unmount } = renderHook(() => useKeyboard(opts));

    unmount();

    fireEvent.keyDown(document, { key: "/" });
    expect(opts.onOpenSearch).not.toHaveBeenCalled();
  });
});
