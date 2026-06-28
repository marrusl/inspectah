import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { VersionChangesTable } from "../VersionChangesTable";
import type { VersionChangeEntry } from "../../api/types";

const downgrade: VersionChangeEntry = {
  name: "httpd",
  arch: "x86_64",
  host_version: "2.4.57",
  base_version: "2.4.51",
  host_epoch: "",
  base_epoch: "",
  direction: "downgrade",
};
const upgrade: VersionChangeEntry = {
  name: "podman",
  arch: "x86_64",
  host_version: "4.6.1",
  base_version: "4.9.0",
  host_epoch: "",
  base_epoch: "",
  direction: "upgrade",
};

describe("VersionChangesTable", () => {
  it("renders downgrades before upgrades", () => {
    render(<VersionChangesTable entries={[upgrade, downgrade]} />);
    const rows = screen.getAllByTestId(/^context-item-/);
    expect(rows[0]).toHaveAttribute("data-testid", "context-item-httpd.x86_64");
    expect(rows[1]).toHaveAttribute(
      "data-testid",
      "context-item-podman.x86_64",
    );
  });

  it("shows group headers with counts", () => {
    render(<VersionChangesTable entries={[downgrade, upgrade]} />);
    expect(screen.getByText(/Downgrades \(1\)/)).toBeInTheDocument();
    expect(screen.getByText(/Upgrades \(1\)/)).toBeInTheDocument();
  });

  it("omits empty groups", () => {
    render(<VersionChangesTable entries={[upgrade]} />);
    expect(screen.queryByText(/Downgrades/)).not.toBeInTheDocument();
    expect(screen.getByText(/Upgrades \(1\)/)).toBeInTheDocument();
  });

  it("renders data_unavailable empty state", () => {
    render(<VersionChangesTable entries={[]} emptyReason="data_unavailable" />);
    expect(screen.getByText(/not available/i)).toBeInTheDocument();
  });

  it("renders zero_drift empty state", () => {
    render(<VersionChangesTable entries={[]} emptyReason="zero_drift" />);
    expect(screen.getByText(/match the target baseline/i)).toBeInTheDocument();
  });

  it("renders default empty state when no reason", () => {
    render(<VersionChangesTable entries={[]} />);
    expect(screen.getByText(/No Version Changes/i)).toBeInTheDocument();
  });

  it("applies pairwise EVR formatting", () => {
    const epochEntry: VersionChangeEntry = {
      name: "pkg",
      arch: "x86_64",
      host_version: "1.0",
      base_version: "2.0",
      host_epoch: "1",
      base_epoch: "0",
      direction: "downgrade",
    };
    render(<VersionChangesTable entries={[epochEntry]} />);
    expect(screen.getByText("1:1.0")).toBeInTheDocument();
    expect(screen.getByText("0:2.0")).toBeInTheDocument();
  });

  it("data rows have focusable context-item testids", () => {
    render(<VersionChangesTable entries={[downgrade]} />);
    const row = screen.getByTestId("context-item-httpd.x86_64");
    expect(row).toHaveAttribute("tabindex", "-1");
  });
});
