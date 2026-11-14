/// <reference types="vite/client" />
/**
 * A scheduled timer row is a description with no control (spec § 8,
 * T20): `ItemId` has no timer variant, so no source file holds the
 * removed kind's name as a quoted string, neither as an item kind nor as
 * a case over one, and the UI cannot send the operation the server
 * refuses. The name is built from parts so this file does not match
 * itself.
 */
import { describe, it, expect } from "vitest";

const SOURCES = import.meta.glob<string>("/src/**/*.{ts,tsx}", {
  query: "?raw",
  import: "default",
  eager: true,
});

/** Files holding `kind` as a quoted string literal. */
function filesNaming(kind: string): string[] {
  const literal = `"${kind}"`;
  return Object.entries(SOURCES)
    .filter(([, text]) => text.includes(literal))
    .map(([path]) => path);
}

describe("systemd timer item kind", () => {
  it("finds a kind that does exist, CronJob, in the source tree", () => {
    expect(Object.keys(SOURCES).length).toBeGreaterThan(50);
    const cron = filesNaming("CronJob");
    expect(cron).toContain("/src/api/types.ts");
    expect(cron).toContain("/src/components/aggregate/AggregateItemRow.tsx");
  });

  it("is named by no file under src", () => {
    expect(filesNaming(["Systemd", "Timer"].join(""))).toEqual([]);
  });
});
