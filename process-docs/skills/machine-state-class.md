---
name: machine-state-class
description: Files that belong to one machine (credentials, host identity) are a class with one rule table, one read gate every inspector reads through, one admission point in the config inspector, validation at every load and write, a fleet merge that withholds every class path, and provisioning notices on both export paths.
---

# The machine-state class

Spec: `process-docs/specs/proposed/2026-11-21-machine-state-class.md`.

## The rules live in one table

`inspectah_core::machine_state::RULES` maps path patterns to members, first
match wins. `classify_path` normalizes first (`normal_path`: drop empty and
`.` components, refuse `..` and control characters), so `etc/x`, `/etc/./x`
and `/etc//x` are one path. The collector, the stager's guard and
`validate` all read this table; never add a second list of member paths.
A member is either:

- **withheld** (`RuleTarget::Withheld`): content never held. Every
  credential (`HostCredential`), and the identity files a copy always
  breaks (machine-id, Insights state, `lvm/devices`). Recorded as a
  `WithheldConfigEntry`, which has no `content` and no `include` field.
  Credentials are never overridable (Mark Russell, 2026-11-21).
- **carriable** (`RuleTarget::Carriable`): a `ConfigFileEntry` with
  `FindingKind::MachineState { member, include: false }`; overridable at
  `warn` or `plain`. `with_include` keeps the variant.

To add a member: a variant on `MachineStateMember` (or `CarriableMember`),
its `reason`, `notice` and JSON name in `types/machine_state.rs`, then a
rule. The tests fail until every variant has a rule and an answer.

The unowned walk's silent lists (`UNOWNED_EXCLUDE_*` in `walk.rs`) hold
noise only. A file that identifies a machine is a class member, recorded
visibly; never put one back on a silent list.

## Every inspector reads through the gate

`InspectionContext::executor` is a `ScreenedExecutor`. Its `read_file`:

1. screens the logical path, the resolved path (`resolve_final_target`, so
   ancestor directory links count) and the `(dev, ino)` against every file
   the withheld `Exact`, `Glob` and `Prefix` rules match on the host
   (`MachineStateScreen::build`, stat only); a hit opens nothing;
2. reads two bytes and refuses a keytab header (`05 01`, `05 02`), on a
   regular file only;
3. reads the file and refuses a whole-file private key
   (`is_whole_file_private_key`), dropping the string.

A refusal is an `io::Error` carrying the member; `machine_state::refusal`
recovers it. Rules for a collector:

- Never map a read error to empty content and keep a row with the file's
  path: validation refuses that snapshot at the scan's own write. Skip the
  file and push `machine_state::left_out_reason(path)` as a degraded
  reason (see `scheduled.rs::read_noting_refusal`, `collect_env_files`).
- sysctl records `SysctlFileBody::Withheld { member }`, which still
  shadows by name.
- The only bypass is `ScreenBypass::SubscriptionSection`; a new one is a
  new variant, reviewed as such.
- `crates/collect/clippy.toml` refuses `std::fs` opens and copy,
  `Command::new` and `RealExecutor::new` in collect. `RealExecutor` has a
  private field and no `Default`, so it cannot be built by literal either.
  `run_scan` builds the only one and wraps it at once.
- `HostRead::read_head` is not on `Executor`, and the gate does not
  implement `HostRead`: no collector can make a prefix read. The gate's
  own are the keytab sniff and `screen_payload`'s check.
- A unit file or drop-in refusal is recorded through `unit_read_failure`
  (units inspector), so it reads `machine-state file left out: <path>`
  like every other collector's; the RPM section's repo and `gpgkey=` key
  readers push the same reason and the inspector returns `Degraded`.
- The subscription inspector reads `/etc/hostname` through the gate; only
  the RHSM material goes through its bypass.

## The inode inventory follows links, or the scan does not run

`MachineStateScreen::build` lists each withheld `Glob` and `Prefix`
rule's tree from the rule's directory, following directory links and
matching the rule against the logical spelling, so a member directory
that is a link elsewhere still puts the physical files in the inode set.
It descends only where `may_match_below` says the rule can still match
(a glob is read as an NFA over the path's start), so the `/etc`-rooted
`krb5.keytab.*` glob never walks `/etc`, and it skips a directory already
on the current path, which ends link cycles. Where it cannot finish (more
than 32 directories deep, more than 65,536 entries, a name that is not
UTF-8 where a rule could match, a directory `read_dir_strict` cannot list)
it records a `ScreenGap`: the gate then withholds every path, and
`run_scan` bails with the gap's message before any inspector runs.
Never relax this to "withhold below the gap": a hard link or bind mount
elsewhere of a file the inventory never listed would read as ordinary.
In a mock, register a directory link with `with_link` and the target
directory's listing; the build lists the resolved path, so no listing is
needed at the link's own path.

## Unmanaged payloads are screened by content

The bundler copies unmanaged files byte for byte, so the walk calls
`screen_payload`, not `screen`: path, alias and inode, then one read of at
most `PAYLOAD_CHECK_LEN` + 1 bytes judged by `payload_credential`. A
keytab header wins; a file within the limit is judged whole, so bytes
that are not UTF-8 are never a PEM key (an ordinary binary is carried); a
longer file is a key unless its start rules that out. A file the check
cannot read stays an item. `bundle_unmanaged_files` lives in
`pipeline::render::unmanaged`, takes the host root (`/` in `run_scan`, a
tempdir in tests), asks `screen_payload` again for every included regular
file before copying any, and fails on a withheld answer or a read error;
it never skips a file. Test the whole route with
`unmanaged_credentials_are_withheld_on_both_export_paths`: write each
payload both to the `SysctlHost` and to a host-root tempdir.

Not covered, and caught only by review: a file-opening route not on the
lint list (a new dependency, a `libc` call) and a command whose output is a
file's bytes. `no_command_prints_an_etc_file` checks today's command names.

## The config inspector admits through one function

`ConfigCollector::admit` (`config/collector.rs`) is the only way a file
enters `config.files` or `config.withheld`: screen both spellings, then
`admit::readable`, then the read, then the class. Every branch (rpm -Va,
`%ghost`, the walk, orphans, ostree, authselect) calls it, and
`admit_advisory` for entries with no content, which never records a member
path as an advisory. `locate` and `readable` let a member through on
purpose so the gate records it as a row under the alias spelling.

## Validation at every load and write

`machine_state::validate` runs in `InspectionSnapshot::load`, in
`to_validated_json` (the only snapshot writer: both render paths,
`--inspect-only`, `aggregate --json-only`) and on refine resume. It walks
the serialized value, so a new field is covered with no list to update:
no string outside `subscription` is a credential, and no object with a
string `path` beside `content` or `raw_content` sits at a class path. A
failure is reported, never repaired. A test that builds a snapshot in
process with an ordinary entry at a class path now fails to render; use
`staged_writes` or `render_containerfile` to test the guard, and expect
`render_all` to refuse.

## Fleet

`ClassPaths::from_hosts` is built from rows before any section merges.
`merge_items` drops every observation whose `destination()` is a class
path; `destination` is required on `AggregateMergeable`. Config class
paths merge into one `WithheldConfigEntry` with every host's prevalence,
whatever each host held, so no host's identity reaches an aggregate.

## Notices

`pipeline::render::machine_state` builds every surface from the snapshot:
`provisioning_steps` (owed steps of rows not overridden, deduplicated, in
`ProvisioningStep` order), the Containerfile `Provisioning` section, the
`## Provisioning steps` Markdown (audit report and scan README), the audit
`Machine state` table, and `export_notices` for stderr and
`x-export-warnings`. All text is enum-owned or a path; nothing quotes file
content. Nothing is written when no step is owed.

## Refine

The view's `machine_state` rows are the only place class rows appear; they
leave `config_files`, so stats and bulk controls never reach them.
`item_id` is present exactly when the row can be overridden. `SetInclude`
on a withheld config or unmanaged path, and `EditVariant` holding a
credential, return `RefineError::NotCarriable` (422, `not carried:`).
`SetInclude` on a carriable row that already holds the value returns
`NoopTarget` (422, `nothing to change:`), checked in `apply` before the
generic idempotency shortcut, which would answer 200. The HTTP contract is
pinned in `crates/web/tests/machine_state_api_test.rs`.

The UI (`MachineStateGroup.tsx`) takes a row's control from `override`
alone, never from its path, member or reason: `never` shows the text
"Not carried" and no control, `plain` an Include button, `warn` an
Include… button behind a confirm dialog. The block sits at the foot of
Configuration in both modes; on a fleet every row is `never`. Single-host
Configuration renders one tree in every state: the heading, the section
search and the block keep their places, and an empty state ("all
excluded", "no match") replaces only the decision list. A separate tree
for an empty state drops the search while its query still filters the
block, and remounts the block, which collapses it. A row's item id is
`machine-state:<path>`: it is the GlobalSearch id, the `ServiceSection`
`required_config` link's reveal target and its `href` hash (MainContent
passes the one `configItemId` function to both), the pending-guard key
for include and exclude, and the `useItemFailures` key. Its `<li>`
carries `data-testid="machine-state-row-<path>"` and `tabIndex={-1}`;
reveal finds it by that exact string and focuses the row's button, or
the `<li>` when it has none: `App.tsx` on a single host,
`revealMachineStateRow` from `AggregateApp` on a fleet, which clears its
reveal once the row has focus so the same pick works again, and keeps
the section-change focus off the first ordinary row meanwhile. The
Export dialog's "Review machine state" link clears the section query
first (`AppShell` bumps `filterClearCounter`), since Configuration may
already be the active section. A 422 `not carried:` or `nothing to
change:` is shown on the row as an `info` statement; `nothing to
change:` means the view was stale, so the row also refetches it.
Anything else shows as "not saved", and the row keeps the server's
state. The UI fixtures are the recorded `machine-state`,
`machine-state-included`, `machine-state-configs-excluded` and
`machine-state-unit` sessions in `system_tuning_fixtures_test.rs` (the
last scans a unit host whose `tls-proxy@web.service` requires a private
key path); record any new case there, never by hand. The export dialog
lists `machine_state_steps` and drops the `machine_state_notices` entries
after download
([refine-export-warnings-header](refine-export-warnings-header.md)).
