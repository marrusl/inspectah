import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { PackageDetail } from "../PackageDetail";
import type { RefinedPackage, VersionChangeEntry } from "../../api/types";

function makePkg(overrides: Partial<RefinedPackage["entry"]> = {}): RefinedPackage {
  return {
    entry: {
      name: "test-pkg",
      epoch: "0",
      version: "1.0",
      release: "1.el9",
      arch: "x86_64",
      state: "added",
      include: true,
      source_repo: "baseos",
      fleet: null,
      ...overrides,
    },
    attention: [],
    triage: { triage: { mode: "single_host" as const, baseline: null }, primary_reason: "package_baseline_match" as const, annotations: [] },
  };
}

describe("PackageDetail version change", () => {
  it("shows version change info for modified package", () => {
    const vc: VersionChangeEntry = {
      name: "bash",
      arch: "x86_64",
      host_version: "5.2.26-3.el9",
      base_version: "5.2.26-4.el9",
      host_epoch: "",
      base_epoch: "",
      direction: "downgrade",
    };
    render(
      <PackageDetail
        pkg={makePkg({
          name: "bash",
          arch: "x86_64",
          version: "5.2.26",
          release: "3.el9",
          epoch: "0",
          state: "modified",
        })}
        versionChange={vc}
      />,
    );
    expect(screen.getByText("Version Change")).toBeInTheDocument();
    // host_version appears in both NEVRA and version change, so use getAllByText
    expect(screen.getAllByText(/5\.2\.26-3\.el9/).length).toBeGreaterThanOrEqual(1);
    expect(screen.getByText(/5\.2\.26-4\.el9/)).toBeInTheDocument();
    expect(screen.getByText(/downgrade/i)).toBeInTheDocument();
  });

  it("shows both epoch prefixes for epoch-only same-EVR change", () => {
    const vc: VersionChangeEntry = {
      name: "glibc",
      arch: "x86_64",
      host_version: "2.34-100.el9",
      base_version: "2.34-100.el9",
      host_epoch: "2",
      base_epoch: "1",
      direction: "upgrade",
    };
    render(
      <PackageDetail
        pkg={makePkg({
          name: "glibc",
          arch: "x86_64",
          version: "2.34",
          release: "100.el9",
          epoch: "2",
          state: "modified",
        })}
        versionChange={vc}
      />,
    );
    expect(screen.getByText("Version Change")).toBeInTheDocument();
    // base epoch (1:) only appears in version change display
    expect(screen.getByText(/1:2\.34-100\.el9/)).toBeInTheDocument();
    // host epoch (2:) appears in both NEVRA and version change, so use getAllByText
    expect(screen.getAllByText(/2:2\.34-100\.el9/).length).toBeGreaterThanOrEqual(1);
  });

  it("does not show version change when null", () => {
    render(
      <PackageDetail
        pkg={makePkg({
          name: "httpd",
          arch: "x86_64",
          version: "2.4.57",
          release: "5.el9",
          epoch: "0",
          state: "added",
        })}
        versionChange={null}
      />,
    );
    expect(screen.queryByText("Version Change")).not.toBeInTheDocument();
  });

  it("does not show version change when undefined (prop omitted)", () => {
    render(
      <PackageDetail
        pkg={makePkg({
          name: "curl",
          arch: "x86_64",
          version: "8.0",
          release: "1.el9",
        })}
      />,
    );
    expect(screen.queryByText("Version Change")).not.toBeInTheDocument();
  });
});
