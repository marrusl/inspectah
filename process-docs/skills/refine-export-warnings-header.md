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
  `ExportWarningKind` (`crates/refine/src/export_warning.rs`).
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
`notice()`, so an entry without a kind does not compile. Everything
`render_refine_export` returns is built in `crates/refine/src/session.rs`:

- `not_carried_notice`: one notice, first in the list, when the export
  leaves out not-carried packages that own a unit enabled on the source
  host. It reads `not_carried_packages`, the list the `# Not carried`
  block renders, and filters to entries with enabled units. See
  `not-carried-auto-packages.md`.
- `not_verified_warning`: one failure per included package that claimed
  a staged upload the export could not prove.

The notice goes first because there is at most one, so the cap can only
ever drop failures behind it.

The upload-directory rescan's warnings (`"skipped {name} in the upload
directory: ..."`, from `rescan_upload_dir`) are `String`s returned by
`ensure_upload_dir` and are **not** in this header. Nothing surfaces them
yet (backlog: `inspectah-rescan-warnings-have-no-consumer`).

## Why each byte is safe in a header

Entry text can carry snapshot-chosen names, so two layers apply.
`serde_json::to_string` escapes the C0 range, which closes header
injection. It passes DEL and non-ASCII through, which `HeaderValue`
rejects (a 500 rather than a download), so every snapshot-named string
goes through `sanitize_package_id` first. The notice also bounds each
name (`NOT_CARRIED_NOTICE_NAME_MAX`) and names at most three, so it has a
fixed maximum size. `warnings_header_value_escapes_c0_and_leaves_the_rest_to_the_sanitizer`
pins the division. A new producer that puts a snapshot string in `text`
sanitizes it the same way.

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
value there.
