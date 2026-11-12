/// <reference types="vite/client" />
/**
 * Sysctl is not a decision (spec § 5.6): `ItemId` has no sysctl variant,
 * so no source file names "Sysctl" as an item kind. The pattern is built
 * from parts so this file does not match itself.
 */
import { describe, it, expect } from "vitest";

const SOURCES = import.meta.glob<string>("/src/**/*.{ts,tsx}", {
  query: "?raw",
  import: "default",
  eager: true,
});

const SYSCTL_KIND = new RegExp(["kind", ':\\s*"', "Sysctl", '"'].join(""));

describe("sysctl item kind", () => {
  it("reads the source tree", () => {
    expect(Object.keys(SOURCES).length).toBeGreaterThan(50);
  });

  it("is named by no file under src", () => {
    const offenders = Object.entries(SOURCES)
      .filter(([, text]) => SYSCTL_KIND.test(text))
      .map(([path]) => path);
    expect(offenders).toEqual([]);
  });
});
