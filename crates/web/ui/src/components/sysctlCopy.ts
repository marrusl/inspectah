/**
 * The sysctl panel's copy and literal-text rule.
 *
 * The constants mirror `crates/pipeline/src/render/sysctl_report.rs`
 * character for character, so the panel says what the audit and the HTML
 * report say. Change them only together with that file. A template's
 * Markdown code spans (`` `x` ``) render as `<code>`, as the HTML report's
 * `push_copy_runs` does, and its `{hole}`s take host values.
 */

/** Item 1 of the explanation. */
export const TRUST_LINE =
  "These values are trusted host configuration. The files that set them are carried as they were, not rewritten.";

/** Stands in for the table and the drift lines when the explanation is `Unknown`. */
export const UNKNOWN_INTRO =
  "Values and drift are not stated for this host, because inspectah does not know every input below as the host had it. One unknown line can set, override or cancel any key.";

/** Shown once above the drift lines. */
export const DRIFT_INTRO =
  "These keys run at a value other than the one the files request. A tuned profile, another service, a hand-run `sysctl -w`, or a module that created the key after systemd-sysctl ran at early boot can each cause this, and some of them reapply at every boot, so the value after a reboot cannot be read from this scan. The image gets the files, not the running values.";

/** The whole sysctl explanation on a fleet aggregate. */
export const AGGREGATE_LINE =
  "Sysctl explanation is per host and is not available across a fleet yet; each host's own scan has it. Sysctl files are carried as Configuration Files, with a variant per differing content.";

/** Ends the line shown when the explanation is unavailable. */
export const UNAVAILABLE_TAIL =
  "Sysctl files are still carried as Configuration Files.";

/** Precedes {@link UNAVAILABLE_TAIL} when the explanation is unavailable. */
export const UNAVAILABLE_LINE = "Sysctl explanation unavailable: {reason}.";

/** Item 2, `Unknown`: a surviving outside link. */
export const OUTSIDE_LINK_LINE =
  "{path} links to {target}, outside `/etc` and the sysctl directories, and inspectah did not read it.";
/** Item 2, `Unknown`: a surviving file redaction changed. */
export const REDACTED_FILE_LINE =
  "{path} held text that redaction replaced, so what it sets is not known.";
/** Item 2, `Unknown`: a machine-state file the scan did not read. */
export const WITHHELD_FILE_LINE =
  "{path} is a machine-state file whose contents were not captured, so what it sets is not known.";
/** Item 2, `Unknown`: a runtime value redaction removed. */
export const REDACTED_RUNTIME_LINE =
  "The running value of {key} held text that redaction removed.";

/** Item 2 table: Runtime value when the kernel runs the value written. */
export const RUNTIME_SAME = "same";
/** Item 2 table: Runtime value when `sysctl -a` did not list the key. */
export const RUNTIME_NOT_EXPOSED = "not exposed";
/** Item 2 table: File value when the last entry for the key is an exclusion. */
export const FILE_NOT_WRITTEN = "not written";
/** Item 2 table: follows the Source when the file was read through a link. */
export const LINK_NOTE = "(link to {target})";

/** Item 3's label. */
export const DRIFT_LABEL = "Runtime drift";
/** Item 3: one drifted key. */
export const DRIFT_LINE =
  "{key} runs at {runtime}; the files request {file_value} ({source}).";
/** Item 3: added to {@link DRIFT_INTRO} when a tuned profile is active. */
export const TUNED_ACTIVE_LINE =
  "tuned profile {profile} is active on this host.";

/** Item 4's label. */
export const VENDOR_LABEL = "Vendor files replaced";
/** Item 4's verb for a file that shadows a vendor file. */
export const VENDOR_VERB_REPLACES = "replaces";
/** Item 4's verb for a mask. */
export const VENDOR_VERB_DISABLES = "disables";
/** Item 4, the admin file is carried. */
export const VENDOR_CARRIED_LINE =
  "{admin} {verb} {vendor} entirely, on the target as well, so keys the target's version of that vendor file sets will not apply from it. Compare it with the target image's copy.";
/** Item 4, the admin file's config entry is excluded, advisory or refused by the export. */
export const VENDOR_NOT_CARRIED_LINE =
  "{admin} {verb} {vendor} on this host. It is not carried, so on the target the vendor file applies.";
/** Item 4, the admin file has no config entry. */
export const VENDOR_NO_CONFIG_ENTRY_LINE =
  "{admin} {verb} {vendor} on this host. It has no Configuration Files row, so it reaches the target only if a package there ships it.";

/** Item 5's label. */
export const NOT_CARRIED_LABEL = "Not carried";
/** Item 5, a file in `/run/sysctl.d`. */
export const NOT_CARRIED_RUNTIME_DIR_LINE = "{path}: generated at runtime.";
/** Item 5, a file in `/usr/local/lib/sysctl.d`. */
export const NOT_CARRIED_USR_LOCAL_LIB_LINE =
  "{path}: carried only as unmanaged files under `--include-unmanaged`.";
/** Item 5, an `/etc` file whose own config entry is excluded. */
export const NOT_CARRIED_EXCLUDED_LINE =
  "{path}: its Configuration Files row is excluded.";
/** Item 5, an `/etc` file carried through another path's entry, which is excluded. */
export const NOT_CARRIED_EXCLUDED_VIA_LINE =
  "{path}: the Configuration Files row that carries it, {config_path}, is excluded.";
/** Item 5, an `/etc` file whose own config entry is an advisory. */
export const NOT_CARRIED_ADVISORY_LINE =
  "{path}: its Configuration Files row is an advisory.";
/** Item 5, an `/etc` file carried through another path's entry, which is an advisory. */
export const NOT_CARRIED_ADVISORY_VIA_LINE =
  "{path}: the Configuration Files row that carries it, {config_path}, is an advisory.";
/**
 * Item 5, an `/etc` file whose own config entry is included at a path the
 * export refuses; `{reason}` is the refusal.
 */
export const NOT_CARRIED_REFUSED_LINE =
  "{path}: its Configuration Files row is included, but the export refuses the path ({reason}) and does not write the file.";
/**
 * Item 5, an `/etc` file carried through another path's entry, which is
 * included at a path the export refuses.
 */
export const NOT_CARRIED_REFUSED_VIA_LINE =
  "{path}: the Configuration Files row that carries it, {config_path}, is included, but the export refuses that path ({reason}) and does not write the file.";
/** Item 5, an `/etc` file with no config entry. */
export const NOT_CARRIED_NO_CONFIG_ENTRY_LINE =
  "{path}: it has no Configuration Files row.";

/** Item 6's label. */
export const AS_WRITTEN_LABEL = "As written";
/** Item 6, a glob assignment. */
export const AS_WRITTEN_GLOB_ASSIGN_LINE =
  "{at}: glob {pattern} set to {value}, not expanded.";
/** Item 6, a glob exclusion. */
export const AS_WRITTEN_GLOB_EXCLUDE_LINE =
  "{at}: glob exclusion {pattern}, not expanded.";
/** Item 6, a line the parser could not read. */
export const AS_WRITTEN_UNPARSED_LINE = "{at}: unparsed line {text}.";

/** What an empty value renders as (`md_literal::EMPTY`). */
const EMPTY = "(empty)";

/**
 * A host value as literal text, the rule every surface shares
 * (`md_literal::literal_text`): control characters other than tab, and
 * U+2028 and U+2029, as `\u{..}` escapes, and `(empty)` for "".
 */
export function literalText(value: string): string {
  if (value === "") return EMPTY;
  let out = "";
  for (const ch of value) {
    const cp = ch.codePointAt(0)!;
    // Rust's `char::is_control` is the Cc category: U+0000-U+001F and
    // U+007F-U+009F.
    const isControl = cp <= 0x1f || (cp >= 0x7f && cp <= 0x9f);
    if ((isControl && ch !== "\t") || cp === 0x2028 || cp === 0x2029) {
      out += `\\u{${cp.toString(16)}}`;
    } else {
      out += ch;
    }
  }
  return out;
}

/** One run of panel text: fixed copy, or a host value. */
export interface SysctlRun {
  text: string;
  /** Renders in `<code>`: a copy code span or a non-empty host value. */
  code: boolean;
  /** The Configuration Files row this value links to. */
  configPath?: string;
}

/** What fills a `{hole}` of a copy template. */
export type SysctlHole =
  | { value: string; configPath?: string }
  | { copy: string };

/** A host value as a run, as the HTML report's `value_run` builds it. */
export function valueRun(value: string, configPath?: string): SysctlRun {
  const run: SysctlRun = { text: literalText(value), code: value !== "" };
  if (configPath !== undefined) run.configPath = configPath;
  return run;
}

/** Copy text as runs: its code spans become code runs, the rest plain runs. */
function copyRuns(copy: string): SysctlRun[] {
  return copy
    .split("`")
    .map((text, i) => ({ text, code: i % 2 === 1 }))
    .filter((run) => run.text !== "");
}

/**
 * A copy template with its `{hole}`s filled, in one pass, so a value that
 * itself holds `{name}` text is never substituted again. Throws on a hole
 * with no value: the templates are this module's constants.
 */
export function fillRuns(
  template: string,
  holes: Record<string, SysctlHole>,
): SysctlRun[] {
  const runs: SysctlRun[] = [];
  let rest = template;
  for (let open = rest.indexOf("{"); open !== -1; open = rest.indexOf("{")) {
    runs.push(...copyRuns(rest.slice(0, open)));
    const after = rest.slice(open + 1);
    const close = after.indexOf("}");
    if (close === -1) throw new Error("unclosed hole in a sysctl copy template");
    const name = after.slice(0, close);
    const hole = holes[name];
    if (!hole) throw new Error(`sysctl copy template hole \`${name}\` has no value`);
    runs.push(...("copy" in hole ? copyRuns(hole.copy) : [valueRun(hole.value, hole.configPath)]));
    rest = after.slice(close + 1);
  }
  runs.push(...copyRuns(rest));
  return runs;
}
