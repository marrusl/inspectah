# Refine export warnings header

`POST /api/tarball` returns the tarball as the body, so what the export
wants to tell the operator travels in the `x-export-warnings` response
header. The constant is `EXPORT_WARNINGS_HEADER` in
`crates/web/src/handlers.rs`.

## The contract

The value is a JSON array of objects, never a bare string:

```json
[{"kind":"notice","text":"..."},{"kind":"failure","text":"..."}]
```

- `kind` is `"failure"` or `"notice"`, the serialized
  `ExportWarningKind` (`crates/pipeline/src/export_warning.rs`).
  A failure is something the operator asked for that is not in the
  export. A notice is worth knowing about an export that is otherwise as
  asked. A client styles by `kind` and never infers severity from `text`.
- `text` is display text, printable ASCII.
- At most 20 entries (`MAX_HEADER_WARNINGS`), then one more,
  `"...and N more"`, whose kind is `failure` when any entry it replaces
  is a failure, else `notice`. So the array is never longer than 21.
- `[]` when there is nothing to say. Every successful export sends the
  header.

## Producers

`ExportWarning` has private fields and two constructors, `failure()` and
`notice()`; producers build entries only through those, so every entry
names its kind (`Deserialize` exists so a test can parse the header
back). `render_refine_export` in `crates/refine/src/session.rs` returns:

- `not_carried_notice` (`crates/pipeline/src/render/not_carried.rs`):
  one notice, first in the list, when the export
  leaves out not-carried packages that own a unit enabled on the source
  host. It reads `not_carried_packages`, the list the `# Not carried`
  block renders, and filters to entries with enabled units. See
  `not-carried-auto-packages.md`.
- `not_verified_warning` (`session.rs`): one failure per included package that claimed
  a staged upload the export could not prove. Its `name.arch` is
  sanitized and cut to `NOT_VERIFIED_ID_MAX` (128) bytes.

The notice goes first because there is at most one, so the cap can only
ever drop failures behind it.

The upload-directory rescan's warnings (`"skipped {name} in the upload
directory: ..."`, from `rescan_upload_dir`) are `String`s returned by
`ensure_upload_dir` and are **not** in this header. Nothing surfaces them
yet (backlog: `inspectah-rescan-warnings-have-no-consumer`).

## Client

The refine UI reads the header in `exportTarball`
(`crates/web/ui/src/api/client.ts`), which returns `{blob, warnings}`.
Parsing is `parseExportWarnings` in `crates/web/ui/src/api/exportWarnings.ts`,
and it never throws, because the tarball is already downloaded when it runs:

- A missing header is `[]`.
- A header that is not JSON, or not an array, becomes one `failure` entry
  saying the warnings could not be read. Entries without a string `text`
  are dropped and replaced by that same entry, once, at the end.
- An unknown `kind` is shown as a `failure`, never hidden and never
  downgraded to a notice.
- A list longer than `MAX_CLIENT_WARNINGS` (21: the server's
  `MAX_HEADER_WARNINGS` of 20 plus its tail) keeps the first 20 and ends
  with one `failure`, `"...and N more not shown"`. A well-formed header
  never reaches this; it keeps the dialog bounded if one does. If the
  server's cap changes, change this constant with it.

`ExportDialog.tsx` closes on success when the list is empty. Otherwise it
stays open as "Downloaded, with warnings", with the accessible name
"Export warnings" in place of "Export tarball", and one inline alert per entry:
`failure` as `warning` (the export itself succeeded; the dialog keeps
`danger` for "Export failed"), `notice` as `info`. The text is the alert
title as a React text node. Alerts are not individually dismissable;
closing the dialog dismisses them. Every entry is shown: the planned
pre-download not-carried notice, which would let the post-download list
skip a notice already shown, does not exist yet.

The header is already in the CORS `expose_headers` list
(`crates/web/src/lib.rs`), which matters only for a cross-origin dev page;
the served UI is same-origin.

## Why each byte is safe in a header

Entry text can carry snapshot-chosen names, so two layers apply.
`serde_json::to_string` escapes the C0 range, which closes header
injection. It passes DEL and non-ASCII through, which `HeaderValue`
rejects (a 500 rather than a download), so every snapshot-named string
goes through `sanitize_package_id` first. `warnings_header_value_escapes_c0_and_leaves_the_rest_to_the_sanitizer`
pins the division.

## Why the header stays small

The cap bounds the number of entries, not their size, and a snapshot
chooses how long a package name is. So every snapshot-named string is
also length-bounded, through `sanitize_bounded` in
`crates/pipeline/src/export_warning.rs`: a
failure's id to `NOT_VERIFIED_ID_MAX` (128) bytes, each notice name to
`NOT_CARRIED_NOTICE_NAME_MAX` (64), and the notice names at most three.
`not_verified_warning_bounds_a_hostile_package_id` serializes the worst
case (the notice, nineteen bounded failures and the tail) and asserts it
under 8 KB, the size at which browsers and proxies may drop a header; it
measures about 4.2 KB. A new producer that puts a snapshot string in
`text` uses `sanitize_bounded`, and that test grows a case for it.

The notice-first order is pinned end to end by
`the_notice_leads_a_failure_from_the_same_export` in
`crates/cli/tests/not_carried_parity_test.rs`.

## The scan prints the notice too

`inspectah scan` has no refine export and no header, but it writes the
same not-carried defaults into its tarball, and an operator who builds
from that tarball never sees the Export dialog. So after the tarball is
written, `run_scan` (`crates/cli/src/commands/scan.rs`) calls the same
`not_carried_notice` on the scan's snapshot and prints it on stderr. On a
stock RHEL 10.2 host:

```
Notice: 5 packages that own a unit enabled on the source host are not carried: audit, audit-rules, cronie and 2 more. See the Not carried table in audit-report.md and the # Not carried block in the Containerfile.
```

- The line is `ExportWarning`'s `Display`: the label is the kind's
  (`Notice`, `Failure`), capitalized like the scan's `Warning:` and
  `Error:` lines on the same stream, never read from the text.
- It is the last thing printed, after the completion summary, in every
  verbosity including `--quiet`: quiet hides progress, not results.
- Nothing goes to stdout. `--inspect-only` prints nothing, because it
  writes neither the audit report nor the Containerfile the text points
  at.
- No cap (there is at most one entry) and the exit status is unchanged.
- Both pointers hold for the scan tarball: `render_all` writes the
  `### Not carried` table and the `# Not carried` block, and
  `the_scan_prints_a_notice_for_not_carried_unit_owners` asserts both
  next to the line. The command needs root and a real host, so that
  test runs the layer under it (collect, `apply_scan_package_defaults`,
  `render_all`) and the same `not_carried_notice` and `Display`.

## It is not the record

Every entry restates something the tarball already carries: a demotion
is a `# FIXME` line in the Containerfile, and the not-carried notice
summarizes the `# Not carried` block and the audit report's table.
Truncating the list loses a convenience, not a fact.

## Reading older measurements

Real-host notes from before 2026-11-12 record `x-export-warnings: []` on
stock exports. On a host with not-carried unit owners (`cronie`,
`firewalld`, `rsyslog` on the RHEL validation hosts) an unchanged export
now carries the not-carried notice, so `[]` is no longer the expected
value there. Those three are the drifted RHEL 10.2 host's owners. A
stock host also lists `audit`, plus `audit-rules` on RHEL 10.2 and
`insights-client` on RHEL 9.8, so its line names three and ends
`and 2 more`: see
[real-host-validation-runs](real-host-validation-runs.md).
