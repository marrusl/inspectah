import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ShortcutOverlay } from "../ShortcutOverlay";

describe("ShortcutOverlay", () => {
  it("renders nothing when closed", () => {
    const { container } = render(
      <ShortcutOverlay isOpen={false} onClose={vi.fn()} />,
    );

    expect(container.innerHTML).toBe("");
  });

  it("renders modal with shortcut groups when open", () => {
    render(<ShortcutOverlay isOpen={true} onClose={vi.fn()} />);

    expect(screen.getByTestId("shortcut-overlay")).toBeInTheDocument();
    expect(screen.getByText("Keyboard Shortcuts")).toBeInTheDocument();
  });

  it("renders all three shortcut groups", () => {
    render(<ShortcutOverlay isOpen={true} onClose={vi.fn()} />);

    expect(screen.getByTestId("shortcuts-navigation")).toBeInTheDocument();
    expect(screen.getByTestId("shortcuts-actions")).toBeInTheDocument();
    expect(screen.getByTestId("shortcuts-global")).toBeInTheDocument();
  });

  it("shows navigation shortcuts", () => {
    render(<ShortcutOverlay isOpen={true} onClose={vi.fn()} />);

    expect(screen.getByText("Next item")).toBeInTheDocument();
    expect(screen.getByText("Previous item")).toBeInTheDocument();
    expect(screen.getByText("First item")).toBeInTheDocument();
    expect(screen.getByText("Last item")).toBeInTheDocument();
    expect(screen.getByText("Jump to section by index")).toBeInTheDocument();
  });

  it("shows action shortcuts", () => {
    render(<ShortcutOverlay isOpen={true} onClose={vi.fn()} />);

    expect(screen.getByText("Toggle include/exclude")).toBeInTheDocument();
    expect(screen.getByText("Expand/collapse detail")).toBeInTheDocument();
  });

  it("shows global shortcuts", () => {
    render(<ShortcutOverlay isOpen={true} onClose={vi.fn()} />);

    expect(screen.getByText("Open section search")).toBeInTheDocument();
    expect(screen.getByText("Undo")).toBeInTheDocument();
    expect(screen.getByText("Redo")).toBeInTheDocument();
    expect(screen.getByText("Toggle Containerfile panel")).toBeInTheDocument();
    expect(screen.getByText("Export")).toBeInTheDocument();
    expect(screen.getByText("Show keyboard shortcuts")).toBeInTheDocument();
    expect(screen.getByText("Close search / overlay")).toBeInTheDocument();
  });

  it("calls onClose when modal close button is clicked", async () => {
    const onClose = vi.fn();
    render(<ShortcutOverlay isOpen={true} onClose={onClose} />);

    // PatternFly Modal has a close button
    const closeButton = screen.getByLabelText("Close");
    await userEvent.click(closeButton);
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
