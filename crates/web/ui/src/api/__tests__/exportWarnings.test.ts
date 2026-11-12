import { describe, it, expect } from "vitest";
import {
  MAX_CLIENT_WARNINGS,
  parseExportWarnings,
  UNREADABLE_WARNINGS_TEXT,
} from "../exportWarnings";

// The header contract lives in process-docs/skills/refine-export-warnings-header.md.
// A header that cannot be read must never break the download, and must
// never hide that the server had something to say.

describe("parseExportWarnings", () => {
  it("parses notice and failure entries in order", () => {
    const raw = JSON.stringify([
      { kind: "notice", text: "2 packages that own a unit enabled on the source host are not carried: firewalld, rhc." },
      { kind: "failure", text: "epel-release.noarch: staged RPM not verified, not exported" },
    ]);
    expect(parseExportWarnings(raw)).toEqual([
      { kind: "notice", text: "2 packages that own a unit enabled on the source host are not carried: firewalld, rhc." },
      { kind: "failure", text: "epel-release.noarch: staged RPM not verified, not exported" },
    ]);
  });

  it("keeps the tail entry as an ordinary entry", () => {
    const raw = JSON.stringify([
      { kind: "failure", text: "a.noarch: staged RPM not verified, not exported" },
      { kind: "failure", text: "...and 4 more" },
    ]);
    expect(parseExportWarnings(raw)).toEqual([
      { kind: "failure", text: "a.noarch: staged RPM not verified, not exported" },
      { kind: "failure", text: "...and 4 more" },
    ]);
  });

  it("returns an empty list for []", () => {
    expect(parseExportWarnings("[]")).toEqual([]);
  });

  it("returns an empty list when the header is missing", () => {
    expect(parseExportWarnings(null)).toEqual([]);
  });

  it("turns malformed JSON into one failure instead of throwing", () => {
    expect(parseExportWarnings("[{not json")).toEqual([
      { kind: "failure", text: UNREADABLE_WARNINGS_TEXT },
    ]);
  });

  it("turns a non-array value into one failure", () => {
    expect(parseExportWarnings('"a bare string"')).toEqual([
      { kind: "failure", text: UNREADABLE_WARNINGS_TEXT },
    ]);
  });

  it("renders an unknown kind as a failure, never hides it", () => {
    const raw = JSON.stringify([{ kind: "catastrophe", text: "something new" }]);
    expect(parseExportWarnings(raw)).toEqual([
      { kind: "failure", text: "something new" },
    ]);
  });

  it("replaces entries without string text with one trailing failure", () => {
    const raw = JSON.stringify([
      { kind: "notice", text: "kept" },
      { kind: "failure" },
      42,
    ]);
    expect(parseExportWarnings(raw)).toEqual([
      { kind: "notice", text: "kept" },
      { kind: "failure", text: UNREADABLE_WARNINGS_TEXT },
    ]);
  });

  it("passes a list at the server's ceiling through unchanged", () => {
    const entries = Array.from({ length: MAX_CLIENT_WARNINGS }, (_, i) => ({
      kind: "failure",
      text: `entry ${i}`,
    }));
    expect(parseExportWarnings(JSON.stringify(entries))).toEqual(entries);
  });

  it("caps a longer list and says how many entries were dropped", () => {
    const entries = Array.from({ length: 30 }, (_, i) => ({
      kind: "notice",
      text: `entry ${i}`,
    }));
    const parsed = parseExportWarnings(JSON.stringify(entries));
    expect(parsed).toHaveLength(MAX_CLIENT_WARNINGS);
    expect(parsed.slice(0, MAX_CLIENT_WARNINGS - 1)).toEqual(
      entries.slice(0, MAX_CLIENT_WARNINGS - 1),
    );
    expect(parsed[MAX_CLIENT_WARNINGS - 1]).toEqual({
      kind: "failure",
      text: "...and 10 more not shown",
    });
  });
});
