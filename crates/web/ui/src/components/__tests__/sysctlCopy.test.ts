import { describe, it, expect } from "vitest";
import { fillRuns, literalText } from "../sysctlCopy";

describe("literalText", () => {
  // The cases of `md_literal::literal_text_escapes_line_breaks_only` in
  // crates/pipeline/src/render/md_literal.rs, so the panel shows a value
  // exactly as the audit and the HTML report do.
  it.each([
    ["plain", "vm.swappiness", "vm.swappiness"],
    ["empty", "", "(empty)"],
    ["newline", "a\nb", "a\\u{a}b"],
    ["line separator", "a b", "a\\u{2028}b"],
    ["paragraph separator", "a b", "a\\u{2029}b"],
    ["tab kept", "32768\t60999", "32768\t60999"],
    ["backticks kept", "a```b", "a```b"],
    ["pipe kept", "a|b", "a|b"],
    ["html stays as written", "<img src=x>", "<img src=x>"],
    ["nul", "a\u0000b", "a\\u{0}b"],
    ["delete", "a\u007fb", "a\\u{7f}b"],
    ["C1 control", "a\u0085b", "a\\u{85}b"],
    ["astral kept", "a\u{1f600}b", "a\u{1f600}b"],
  ])("%s", (_name, value, expected) => {
    expect(literalText(value)).toBe(expected);
  });
});

describe("fillRuns", () => {
  it("splits copy code spans and fills each hole once", () => {
    expect(
      fillRuns("{path} links to {target}, outside `/etc`.", {
        path: { value: "{target}" },
        target: { value: "" },
      }),
    ).toEqual([
      { text: "{target}", code: true },
      { text: " links to ", code: false },
      { text: "(empty)", code: false },
      { text: ", outside ", code: false },
      { text: "/etc", code: true },
      { text: ".", code: false },
    ]);
  });

  it("keeps a hole's config row and fills copy holes as copy", () => {
    expect(
      fillRuns("{admin} {verb} it", {
        admin: { value: "/etc/sysctl.d/a.conf", configPath: "/etc/sysctl.d/a.conf" },
        verb: { copy: "replaces" },
      }),
    ).toEqual([
      { text: "/etc/sysctl.d/a.conf", code: true, configPath: "/etc/sysctl.d/a.conf" },
      { text: " ", code: false },
      { text: "replaces", code: false },
      { text: " it", code: false },
    ]);
  });

  it("refuses a hole with no value", () => {
    expect(() => fillRuns("{missing}", {})).toThrow(/missing/);
  });
});

