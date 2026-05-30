import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect, vi } from "vitest";
import { DependencyModal } from "../DependencyModal";

describe("DependencyModal", () => {
  const deps = ["glibc.x86_64", "ncurses-libs.x86_64", "apr.x86_64"];

  it("renders sorted dependency list", () => {
    render(
      <DependencyModal
        packageId="httpd.x86_64"
        dependencies={deps}
        isOpen={true}
        onClose={vi.fn()}
      />,
    );
    expect(screen.getByText("Dependencies: httpd.x86_64")).toBeInTheDocument();
    expect(screen.getByText("(3 dependencies)")).toBeInTheDocument();
    const items = screen.getAllByRole("listitem");
    expect(items[0]).toHaveTextContent("apr.x86_64");
    expect(items[1]).toHaveTextContent("glibc.x86_64");
    expect(items[2]).toHaveTextContent("ncurses-libs.x86_64");
  });

  it("has distinct ARIA labels for dialog and list", () => {
    render(
      <DependencyModal
        packageId="httpd.x86_64"
        dependencies={deps}
        isOpen={true}
        onClose={vi.fn()}
      />,
    );
    expect(
      screen.getByRole("dialog", { name: /dependencies.*httpd/i }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("list", { name: /dependency list.*httpd/i }),
    ).toBeInTheDocument();
  });

  it("calls onClose when close button clicked", async () => {
    const onClose = vi.fn();
    render(
      <DependencyModal
        packageId="httpd.x86_64"
        dependencies={deps}
        isOpen={true}
        onClose={onClose}
      />,
    );
    await userEvent.click(screen.getByLabelText("Close"));
    expect(onClose).toHaveBeenCalled();
  });

  it("renders nothing when not open", () => {
    const { container } = render(
      <DependencyModal
        packageId="httpd.x86_64"
        dependencies={deps}
        isOpen={false}
        onClose={vi.fn()}
      />,
    );
    expect(container).toBeEmptyDOMElement();
  });

  it("closes on Escape key", async () => {
    const onClose = vi.fn();
    render(
      <DependencyModal
        packageId="httpd.x86_64"
        dependencies={deps}
        isOpen={true}
        onClose={onClose}
      />,
    );
    await userEvent.keyboard("{Escape}");
    expect(onClose).toHaveBeenCalled();
  });

  it("scrolls long dependency lists", () => {
    const longDeps = Array.from(
      { length: 60 },
      (_, i) => `dep-${String(i).padStart(3, "0")}.x86_64`,
    );
    render(
      <DependencyModal
        packageId="httpd.x86_64"
        dependencies={longDeps}
        isOpen={true}
        onClose={vi.fn()}
      />,
    );
    expect(screen.getByText("(60 dependencies)")).toBeInTheDocument();
    const list = screen.getByRole("list");
    expect(list).toHaveStyle({ overflowY: "auto", maxHeight: "60vh" });
  });

  it("dependency list is keyboard-focusable for scrolling", () => {
    render(
      <DependencyModal
        packageId="httpd.x86_64"
        dependencies={deps}
        isOpen={true}
        onClose={vi.fn()}
      />,
    );
    const list = screen.getByRole("list");
    expect(list).toHaveAttribute("tabindex", "0");
  });
});
