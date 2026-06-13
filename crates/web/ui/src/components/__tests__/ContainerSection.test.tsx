import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ContainerSection } from "../ContainerSection";
import type {
  QuadletDecisionDto,
  FlatpakDecisionDto,
  ViewResponse,
} from "../../api/types";

// Mock the applyOp client
vi.mock("../../api/client", () => ({
  applyOp: vi.fn(),
}));

import { applyOp } from "../../api/client";
const mockApplyOp = vi.mocked(applyOp);

function makeQuadlet(
  overrides: Partial<QuadletDecisionDto> = {},
): QuadletDecisionDto {
  return {
    path: "/etc/containers/systemd/myapp.container",
    name: "myapp.container",
    image: "quay.io/org/myapp:latest",
    triage: {
      triage: { mode: "single_host", baseline: null },
      primary_reason: "quadlet_user_deployed",
      annotations: [],
    },
    include: true,
    ...overrides,
  };
}

function makeFlatpak(
  overrides: Partial<FlatpakDecisionDto> = {},
): FlatpakDecisionDto {
  return {
    app_id: "org.mozilla.firefox",
    remote: "flathub",
    branch: "stable",
    triage: {
      triage: { mode: "single_host", baseline: null },
      primary_reason: "flatpak_provisioned_on_first_boot",
      annotations: [],
    },
    include: true,
    lifecycle: "first_boot",
    ...overrides,
  };
}

describe("ContainerSection", () => {
  const onViewUpdate = vi.fn();
  const onMutationError = vi.fn();

  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders empty state when no quadlets or flatpaks", () => {
    render(
      <ContainerSection
        quadlets={[]}
        flatpaks={[]}
        onViewUpdate={onViewUpdate}
        onMutationError={onMutationError}
      />,
    );
    expect(
      screen.getByText(/No quadlet or flatpak items detected/),
    ).toBeInTheDocument();
  });

  it("renders quadlet with lifecycle badge", () => {
    render(
      <ContainerSection
        quadlets={[makeQuadlet()]}
        flatpaks={[]}
        onViewUpdate={onViewUpdate}
        onMutationError={onMutationError}
      />,
    );
    expect(screen.getByText("myapp.container")).toBeInTheDocument();
    expect(screen.getByText("Quadlet")).toBeInTheDocument();
    expect(screen.getByText("Image content")).toBeInTheDocument();
    expect(screen.getByText("quay.io/org/myapp:latest")).toBeInTheDocument();
  });

  it("renders flatpak with 'First boot' badge", () => {
    render(
      <ContainerSection
        quadlets={[]}
        flatpaks={[makeFlatpak()]}
        onViewUpdate={onViewUpdate}
        onMutationError={onMutationError}
      />,
    );
    expect(screen.getByText("org.mozilla.firefox")).toBeInTheDocument();
    expect(screen.getByText("Flatpak")).toBeInTheDocument();
    expect(screen.getByText("First boot")).toBeInTheDocument();
    expect(screen.getByText("flathub/stable")).toBeInTheDocument();
  });

  it("toggle sends correct SetInclude with Quadlet ItemId", async () => {
    const updatedView = {} as ViewResponse;
    mockApplyOp.mockResolvedValueOnce(updatedView);

    render(
      <ContainerSection
        quadlets={[makeQuadlet({ include: true })]}
        flatpaks={[]}
        onViewUpdate={onViewUpdate}
        onMutationError={onMutationError}
      />,
    );

    const checkbox = screen.getByRole("checkbox", {
      name: /Toggle myapp\.container/,
    });
    await userEvent.click(checkbox);

    expect(mockApplyOp).toHaveBeenCalledWith({
      op: "SetInclude",
      target: {
        item_id: {
          kind: "Quadlet",
          key: { path: "/etc/containers/systemd/myapp.container" },
        },
        include: false,
      },
    });
  });

  it("toggle sends correct SetInclude with Flatpak ItemId", async () => {
    const updatedView = {} as ViewResponse;
    mockApplyOp.mockResolvedValueOnce(updatedView);

    render(
      <ContainerSection
        quadlets={[]}
        flatpaks={[makeFlatpak({ include: true })]}
        onViewUpdate={onViewUpdate}
        onMutationError={onMutationError}
      />,
    );

    const checkbox = screen.getByRole("checkbox", {
      name: /Toggle org\.mozilla\.firefox/,
    });
    await userEvent.click(checkbox);

    expect(mockApplyOp).toHaveBeenCalledWith({
      op: "SetInclude",
      target: {
        item_id: {
          kind: "Flatpak",
          key: {
            app_id: "org.mozilla.firefox",
            remote: "flathub",
            branch: "stable",
          },
        },
        include: false,
      },
    });
  });

  it("excluded quadlet shows unchecked state", () => {
    render(
      <ContainerSection
        quadlets={[makeQuadlet({ include: false })]}
        flatpaks={[]}
        onViewUpdate={onViewUpdate}
        onMutationError={onMutationError}
      />,
    );

    const checkbox = screen.getByRole("checkbox", {
      name: /Toggle myapp\.container/,
    });
    expect(checkbox).not.toBeChecked();
  });

  it("excluded flatpak shows unchecked state", () => {
    render(
      <ContainerSection
        quadlets={[]}
        flatpaks={[makeFlatpak({ include: false })]}
        onViewUpdate={onViewUpdate}
        onMutationError={onMutationError}
      />,
    );

    const checkbox = screen.getByRole("checkbox", {
      name: /Toggle org\.mozilla\.firefox/,
    });
    expect(checkbox).not.toBeChecked();
  });

  it("renders both quadlets and flatpaks together", () => {
    render(
      <ContainerSection
        quadlets={[makeQuadlet()]}
        flatpaks={[makeFlatpak()]}
        onViewUpdate={onViewUpdate}
        onMutationError={onMutationError}
      />,
    );

    expect(screen.getByText("myapp.container")).toBeInTheDocument();
    expect(screen.getByText("org.mozilla.firefox")).toBeInTheDocument();
    expect(screen.getByText("Quadlet")).toBeInTheDocument();
    expect(screen.getByText("Flatpak")).toBeInTheDocument();
  });

  it("expands quadlet row to show unit file content", async () => {
    const unitContent =
      "[Container]\nImage=quay.io/org/myapp:latest\nPublishPort=8080:80";
    render(
      <ContainerSection
        quadlets={[makeQuadlet({ content: unitContent })]}
        flatpaks={[]}
        onViewUpdate={onViewUpdate}
        onMutationError={onMutationError}
      />,
    );

    // Content should not be visible initially
    expect(screen.queryByTestId("quadlet-content")).not.toBeInTheDocument();

    // Click the expand button
    const expandBtn = screen.getByRole("button", {
      name: /Show unit file/,
    });
    await userEvent.click(expandBtn);

    // Content should now be visible in a <pre> block
    const contentBlock = screen.getByTestId("quadlet-content");
    expect(contentBlock).toBeInTheDocument();
    expect(contentBlock.tagName).toBe("PRE");
    expect(contentBlock).toHaveTextContent(
      "[Container] Image=quay.io/org/myapp:latest PublishPort=8080:80",
    );
  });

  it("does not show expand button when quadlet has no content", () => {
    render(
      <ContainerSection
        quadlets={[makeQuadlet()]}
        flatpaks={[]}
        onViewUpdate={onViewUpdate}
        onMutationError={onMutationError}
      />,
    );

    expect(
      screen.queryByRole("button", { name: /Show unit file/ }),
    ).not.toBeInTheDocument();
  });

  it("collapses quadlet content on second click", async () => {
    const unitContent = "[Container]\nImage=test:latest";
    render(
      <ContainerSection
        quadlets={[makeQuadlet({ content: unitContent })]}
        flatpaks={[]}
        onViewUpdate={onViewUpdate}
        onMutationError={onMutationError}
      />,
    );

    const expandBtn = screen.getByRole("button", {
      name: /Show unit file/,
    });
    await userEvent.click(expandBtn);
    expect(screen.getByTestId("quadlet-content")).toBeInTheDocument();

    // Click again to collapse
    const collapseBtn = screen.getByRole("button", {
      name: /Hide unit file/,
    });
    await userEvent.click(collapseBtn);
    expect(screen.queryByTestId("quadlet-content")).not.toBeInTheDocument();
  });
});
