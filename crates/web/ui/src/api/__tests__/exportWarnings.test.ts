import { describe, it, expect } from "vitest";
import {
  MAX_CLIENT_WARNINGS,
  parseExportWarnings,
  UNREADABLE_WARNINGS_TEXT,
  withoutShownNotice,
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

// The view's `not_carried_notice` is the header's notice entry byte for byte
// (not-carried-auto-packages.md, Refine projection contract). The Export
// dialog shows it before download, so the post-download list skips it.
describe("withoutShownNotice", () => {
  const NOTICE = {
    kind: "notice" as const,
    text: "1 package that owns a unit enabled on the source host is not carried: firewalld. See the Not carried table in audit-report.md and the # Not carried block in the Containerfile.",
  };
  const FAILURE = {
    kind: "failure" as const,
    text: "epel-release.noarch: staged RPM not verified, not exported",
  };

  it("skips the header entry equal to the notice already shown", () => {
    const parsed = parseExportWarnings(JSON.stringify([NOTICE, FAILURE]));
    expect(withoutShownNotice(parsed, [NOTICE])).toEqual([FAILURE]);
  });

  it("leaves nothing when the notice was the only entry", () => {
    const parsed = parseExportWarnings(JSON.stringify([NOTICE]));
    expect(withoutShownNotice(parsed, [NOTICE])).toEqual([]);
  });

  it("keeps a header notice that differs from the one shown", () => {
    // Drift between the view and the export: the operator has not seen
    // this text, so it stays.
    const drifted = { kind: "notice" as const, text: NOTICE.text.replace("1 package", "2 packages") };
    const parsed = parseExportWarnings(JSON.stringify([drifted, FAILURE]));
    expect(withoutShownNotice(parsed, [NOTICE])).toEqual([drifted, FAILURE]);
  });

  it("matches on kind as well as text", () => {
    const sameTextFailure = { kind: "failure" as const, text: NOTICE.text };
    expect(withoutShownNotice([sameTextFailure], [NOTICE])).toEqual([
      sameTextFailure,
    ]);
  });

  it("does not match on a text pattern", () => {
    const prefix = { kind: "notice" as const, text: NOTICE.text.slice(0, 40) };
    expect(withoutShownNotice([prefix], [NOTICE])).toEqual([prefix]);
  });

  it("keeps everything when nothing was shown", () => {
    const parsed = parseExportWarnings(JSON.stringify([NOTICE, FAILURE]));
    expect(withoutShownNotice(parsed, [])).toEqual([NOTICE, FAILURE]);
  });

  it("skips every entry equal to one of several shown", () => {
    // The not-carried notice and the machine-state notices are all on
    // screen before download.
    const LEFT_OUT = {
      kind: "notice" as const,
      text: "6 machine-state files left out. At provisioning: realm join.",
    };
    const IDENTITY = {
      kind: "notice" as const,
      text: "Host identity included in the image: /etc/iscsi/initiatorname.iscsi",
    };
    const parsed = parseExportWarnings(
      JSON.stringify([NOTICE, LEFT_OUT, IDENTITY, FAILURE]),
    );
    expect(withoutShownNotice(parsed, [NOTICE, LEFT_OUT, IDENTITY])).toEqual([
      FAILURE,
    ]);
  });
});
