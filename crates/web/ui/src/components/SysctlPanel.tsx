import { useId, type ReactNode } from "react";
import { Content } from "@patternfly/react-core";
import { Table, Thead, Tbody, Tr, Th, Td } from "@patternfly/react-table";
import type {
  AsWrittenLineDto,
  CarriageDto,
  NotCarriedDto,
  SysctlFileDto,
  SysctlHostView,
  SysctlLineRefDto,
  SysctlSettingDto,
  SysctlViewDto,
  UnknownInputDto,
  VendorReplacementDto,
} from "../api/types";
import {
  AGGREGATE_LINE,
  AS_WRITTEN_GLOB_ASSIGN_LINE,
  AS_WRITTEN_GLOB_EXCLUDE_LINE,
  AS_WRITTEN_LABEL,
  AS_WRITTEN_UNPARSED_LINE,
  DRIFT_INTRO,
  DRIFT_LABEL,
  DRIFT_LINE,
  FILE_NOT_WRITTEN,
  LINK_NOTE,
  NOT_CARRIED_ADVISORY_LINE,
  NOT_CARRIED_ADVISORY_VIA_LINE,
  NOT_CARRIED_EXCLUDED_LINE,
  NOT_CARRIED_EXCLUDED_VIA_LINE,
  NOT_CARRIED_LABEL,
  NOT_CARRIED_NO_CONFIG_ENTRY_LINE,
  NOT_CARRIED_REFUSED_LINE,
  NOT_CARRIED_REFUSED_VIA_LINE,
  NOT_CARRIED_RUNTIME_DIR_LINE,
  NOT_CARRIED_USR_LOCAL_LIB_LINE,
  OUTSIDE_LINK_LINE,
  REDACTED_FILE_LINE,
  REDACTED_RUNTIME_LINE,
  WITHHELD_FILE_LINE,
  RUNTIME_NOT_EXPOSED,
  RUNTIME_SAME,
  TRUST_LINE,
  TUNED_ACTIVE_LINE,
  UNAVAILABLE_LINE,
  UNAVAILABLE_TAIL,
  UNKNOWN_INTRO,
  VENDOR_CARRIED_LINE,
  VENDOR_LABEL,
  VENDOR_NO_CONFIG_ENTRY_LINE,
  VENDOR_NOT_CARRIED_LINE,
  VENDOR_VERB_DISABLES,
  VENDOR_VERB_REPLACES,
  fillRuns,
  valueRun,
  type SysctlHole,
  type SysctlRun,
} from "./sysctlCopy";

export interface SysctlPanelProps {
  sysctl: SysctlViewDto;
  /**
   * Opens the Configuration Files row at `configPath`. Without it, paths
   * render as text.
   */
  onNavigateToConfig?: (configPath: string) => void;
}

/** Panel-only copy: the files table, which the audit does not have. */
const FILES_LABEL = "Files";
const FILE_READ = "read";
const FILE_READ_THROUGH_LINK = "read through a link to {target}";
const FILE_MASK = "mask";
const FILE_OUTSIDE_LINK =
  "link outside the sysctl directories to {target}, not read";
const FILE_WITHHELD = "machine-state file, contents not captured";
const NO_CONFIG_ROW = "none";

/** `<path>:<line>` of a sysctl line, as the audit writes it. */
function lineRefText(at: SysctlLineRefDto): string {
  return `${at.path}:${at.line}`;
}

/**
 * The read-only sysctl panel: the audit's explanation, worded the same.
 *
 * It holds no decision. Each `/etc` sysctl file is decided by its
 * Configuration Files row, which the panel links to. Every host value is
 * React text, escaped as the audit escapes it.
 */
export function SysctlPanel({ sysctl, onNavigateToConfig }: SysctlPanelProps) {
  const headingId = useId();

  const renderRuns = (runs: SysctlRun[]): ReactNode =>
    runs.map((run, i) => {
      const text = run.code ? <code>{run.text}</code> : run.text;
      if (run.configPath === undefined || !onNavigateToConfig) {
        return <span key={i}>{text}</span>;
      }
      const configPath = run.configPath;
      return (
        <a
          key={i}
          href={`#configs:${configPath}`}
          onClick={(e) => {
            e.preventDefault();
            onNavigateToConfig(configPath);
          }}
        >
          {text}
        </a>
      );
    });

  const line = (runs: SysctlRun[], key?: number) => (
    <p key={key}>{renderRuns(runs)}</p>
  );

  let body: ReactNode;
  switch (sysctl.state) {
    case "aggregate":
      body = line(fillRuns(AGGREGATE_LINE, {}));
      break;
    case "unavailable":
      body = line(
        fillRuns(`${UNAVAILABLE_LINE} ${UNAVAILABLE_TAIL}`, {
          reason: { value: sysctl.reason },
        }),
      );
      break;
    case "host":
      body = <HostBody host={sysctl} line={line} renderRuns={renderRuns} />;
      break;
  }

  return (
    <section
      aria-labelledby={headingId}
      className="inspectah-sysctl-panel"
      data-testid="sysctl-panel"
      style={{ padding: "var(--pf-t--global--spacer--md)" }}
    >
      <Content>
        <h3 id={headingId}>Sysctl</h3>
        {body}
      </Content>
    </section>
  );
}

interface HostBodyProps {
  host: SysctlHostView;
  line: (runs: SysctlRun[], key?: number) => ReactNode;
  renderRuns: (runs: SysctlRun[]) => ReactNode;
}

function HostBody({ host, line, renderRuns }: HostBodyProps) {
  // The Configuration Files row for each collected file, by its path.
  const configRows = new Map(
    host.files
      .filter((f) => f.config_path !== null)
      .map((f) => [f.path, f.config_path as string]),
  );
  const pathHole = (path: string) => ({ value: path, configPath: configRows.get(path) });
  const label = (text: string) => (
    <p>
      <strong>{text}</strong>
    </p>
  );
  const explanation = host.explanation;

  return (
    <>
      {line(fillRuns(TRUST_LINE, {}))}
      {explanation.state === "resolved" ? (
        <>
          <SettingsTable settings={explanation.settings} renderRuns={renderRuns} />
          {explanation.drift.length > 0 && (
            <>
              {label(DRIFT_LABEL)}
              {line([
                ...fillRuns(DRIFT_INTRO, {}),
                ...(host.tuned_active !== null
                  ? [
                      { text: " ", code: false },
                      ...fillRuns(TUNED_ACTIVE_LINE, {
                        profile: { value: host.tuned_active },
                      }),
                    ]
                  : []),
              ])}
              {explanation.drift.map((d, i) =>
                line(
                  fillRuns(DRIFT_LINE, {
                    key: { value: d.key },
                    runtime: { value: d.runtime },
                    file_value: { value: d.file_value },
                    source: { value: lineRefText(d.from) },
                  }),
                  i,
                ),
              )}
            </>
          )}
        </>
      ) : (
        <>
          {line(fillRuns(UNKNOWN_INTRO, {}))}
          {explanation.inputs.map((input, i) =>
            line(unknownInputRuns(input, pathHole), i),
          )}
        </>
      )}
      {host.vendor_replacements.length > 0 && (
        <>
          {label(VENDOR_LABEL)}
          {host.vendor_replacements.map((v, i) => line(vendorRuns(v), i))}
        </>
      )}
      {host.not_carried.length > 0 && (
        <>
          {label(NOT_CARRIED_LABEL)}
          {host.not_carried.map((n, i) => line(notCarriedRuns(n), i))}
        </>
      )}
      {host.as_written.length > 0 && (
        <>
          {label(AS_WRITTEN_LABEL)}
          {host.as_written.map((a, i) => line(asWrittenRuns(a), i))}
        </>
      )}
      {host.files.length > 0 && (
        <>
          {label(FILES_LABEL)}
          <FilesTable files={host.files} renderRuns={renderRuns} />
        </>
      )}
    </>
  );
}

/** Item 2's table. Rendered with no rows when no key is set, so a
 *  mask-only or glob-only host still shows it (spec § 5.6). */
function SettingsTable({
  settings,
  renderRuns,
}: {
  settings: SysctlSettingDto[];
  renderRuns: (runs: SysctlRun[]) => ReactNode;
}) {
  return (
    <Table aria-label="Sysctl settings" role="table" variant="compact">
      <Thead>
        <Tr>
          <Th>Key</Th>
          <Th>File value</Th>
          <Th>Runtime value</Th>
          <Th>Source</Th>
        </Tr>
      </Thead>
      <Tbody>
        {settings.map((s) => {
          const { fileValue, runtime, from } = settingCells(s);
          const source: SysctlRun[] = [valueRun(lineRefText(from))];
          if (from.link_target !== null) {
            source.push(
              { text: " ", code: false },
              ...fillRuns(LINK_NOTE, { target: { value: from.link_target } }),
            );
          }
          return (
            <Tr key={s.key}>
              <Td dataLabel="Key">{renderRuns([valueRun(s.key)])}</Td>
              <Td dataLabel="File value">{renderRuns(fileValue)}</Td>
              <Td dataLabel="Runtime value">{renderRuns(runtime)}</Td>
              <Td dataLabel="Source">{renderRuns(source)}</Td>
            </Tr>
          );
        })}
      </Tbody>
    </Table>
  );
}

function settingCells(s: SysctlSettingDto): {
  fileValue: SysctlRun[];
  runtime: SysctlRun[];
  from: SysctlLineRefDto;
} {
  const plain = (text: string): SysctlRun[] => [{ text, code: false }];
  const outcome = s.outcome;
  if (outcome.kind === "writes") {
    const runtime =
      outcome.runtime.kind === "matches"
        ? plain(RUNTIME_SAME)
        : outcome.runtime.kind === "not_exposed"
          ? plain(RUNTIME_NOT_EXPOSED)
          : [valueRun(outcome.runtime.runtime)];
    return { fileValue: [valueRun(outcome.value)], runtime, from: outcome.from };
  }
  return {
    fileValue: plain(FILE_NOT_WRITTEN),
    runtime:
      outcome.runtime === null
        ? plain(RUNTIME_NOT_EXPOSED)
        : [valueRun(outcome.runtime)],
    from: outcome.by,
  };
}

function unknownInputRuns(
  input: UnknownInputDto,
  pathHole: (path: string) => { value: string; configPath?: string },
): SysctlRun[] {
  switch (input.kind) {
    case "outside_link":
      return fillRuns(OUTSIDE_LINK_LINE, {
        path: pathHole(input.path),
        target: { value: input.target },
      });
    case "redacted_file":
      return fillRuns(REDACTED_FILE_LINE, { path: pathHole(input.path) });
    case "withheld_file":
      return fillRuns(WITHHELD_FILE_LINE, { path: pathHole(input.path) });
    case "redacted_runtime":
      return fillRuns(REDACTED_RUNTIME_LINE, { key: { value: input.key } });
  }
}

/**
 * Item 4's template for a carriage state. A file the export refuses is not
 * carried, so it takes the not-carried line, as the audit words it.
 */
function vendorTemplate(carriage: CarriageDto): string {
  switch (carriage.kind) {
    case "carried":
      return VENDOR_CARRIED_LINE;
    case "excluded":
    case "advisory":
    case "refused":
      return VENDOR_NOT_CARRIED_LINE;
    case "no_config_entry":
      return VENDOR_NO_CONFIG_ENTRY_LINE;
  }
}

/** Item 4, worded by how the admin file reaches the image. */
function vendorRuns(v: VendorReplacementDto): SysctlRun[] {
  const verb = v.action === "masks" ? VENDOR_VERB_DISABLES : VENDOR_VERB_REPLACES;
  const template = vendorTemplate(v.carriage);
  const configPath =
    v.carriage.kind === "no_config_entry" ? undefined : v.carriage.config_path;
  return fillRuns(template, {
    admin: { value: v.path, configPath },
    verb: { copy: verb },
    vendor: { value: v.vendor_path },
  });
}

/** Item 5, worded as the audit words it. */
function notCarriedRuns(n: NotCarriedDto): SysctlRun[] {
  const reason = n.reason;
  switch (reason.kind) {
    case "runtime_dir":
      return fillRuns(NOT_CARRIED_RUNTIME_DIR_LINE, { path: { value: n.path } });
    case "usr_local_lib":
      return fillRuns(NOT_CARRIED_USR_LOCAL_LIB_LINE, { path: { value: n.path } });
    case "no_config_entry":
      return fillRuns(NOT_CARRIED_NO_CONFIG_ENTRY_LINE, { path: { value: n.path } });
    case "excluded":
      return entryRuns(
        n.path,
        reason.config_path,
        NOT_CARRIED_EXCLUDED_LINE,
        NOT_CARRIED_EXCLUDED_VIA_LINE,
        {},
      );
    case "advisory":
      return entryRuns(
        n.path,
        reason.config_path,
        NOT_CARRIED_ADVISORY_LINE,
        NOT_CARRIED_ADVISORY_VIA_LINE,
        {},
      );
    case "refused":
      // The refusal is fixed copy (`PathError`'s text), not a host value.
      return entryRuns(
        n.path,
        reason.config_path,
        NOT_CARRIED_REFUSED_LINE,
        NOT_CARRIED_REFUSED_VIA_LINE,
        { reason: { copy: reason.reason } },
      );
  }
}

/**
 * Item 5 for an `/etc` file a config entry decides: the `own` line when the
 * entry is the file's own path, which then links to the row, else the `via`
 * line, whose `{config_path}` links to it.
 */
function entryRuns(
  path: string,
  configPath: string,
  own: string,
  via: string,
  holes: Record<string, SysctlHole>,
): SysctlRun[] {
  return configPath === path
    ? fillRuns(own, { ...holes, path: { value: path, configPath } })
    : fillRuns(via, {
        ...holes,
        path: { value: path },
        config_path: { value: configPath, configPath },
      });
}

/** Item 6. */
function asWrittenRuns(a: AsWrittenLineDto): SysctlRun[] {
  const at = { value: lineRefText(a.at) };
  switch (a.kind) {
    case "glob_assign":
      return fillRuns(AS_WRITTEN_GLOB_ASSIGN_LINE, {
        at,
        pattern: { value: a.pattern },
        value: { value: a.value },
      });
    case "glob_exclude":
      return fillRuns(AS_WRITTEN_GLOB_EXCLUDE_LINE, {
        at,
        pattern: { value: a.pattern },
      });
    case "unparsed":
      return fillRuns(AS_WRITTEN_UNPARSED_LINE, { at, text: { value: a.text } });
  }
}

/** Every collected file, what it is, and the row that decides it. */
function FilesTable({
  files,
  renderRuns,
}: {
  files: SysctlFileDto[];
  renderRuns: (runs: SysctlRun[]) => ReactNode;
}) {
  return (
    <Table aria-label="Sysctl files" role="table" variant="compact">
      <Thead>
        <Tr>
          <Th>File</Th>
          <Th>What it is</Th>
          <Th>Configuration Files row</Th>
        </Tr>
      </Thead>
      <Tbody>
        {files.map((f) => {
          const body = f.body;
          const kind =
            body.kind === "mask"
              ? fillRuns(FILE_MASK, {})
              : body.kind === "outside_link"
                ? fillRuns(FILE_OUTSIDE_LINK, { target: { value: body.target } })
                : body.kind === "withheld"
                  ? fillRuns(FILE_WITHHELD, {})
                  : body.link_target !== null
                    ? fillRuns(FILE_READ_THROUGH_LINK, {
                        target: { value: body.link_target },
                      })
                    : fillRuns(FILE_READ, {});
          const row =
            f.config_path === null
              ? fillRuns(NO_CONFIG_ROW, {})
              : [valueRun(f.config_path, f.config_path)];
          return (
            <Tr key={f.path}>
              <Td dataLabel="File">{renderRuns([valueRun(f.path)])}</Td>
              <Td dataLabel="What it is">{renderRuns(kind)}</Td>
              <Td dataLabel="Configuration Files row">{renderRuns(row)}</Td>
            </Tr>
          );
        })}
      </Tbody>
    </Table>
  );
}
