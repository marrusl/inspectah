# Redaction belongs on the snapshot object, not at the writer

`inspection-snapshot.json` is `serde_json::to_string_pretty(snap)`. Any
field that holds raw host content and is not scrubbed **on the snapshot
object** ships in the clear, no matter how carefully the sidecar copy of
the same content was scrubbed on its way out.

This is the redaction form of the two-export-paths problem
([export-paths-stage-different-trees](export-paths-stage-different-trees.md)),
and it bites harder, because the symptom is silence rather than a failed
build.

**Read the last section first.** Four fields had this gap, and the
principle written down here did not prevent the second, third and fourth
of them. What prevents them is the pair of tests at the end.

## The instance

`redact()` (`crates/pipeline/src/redaction/engine.rs`) walked
`non_rpm_software.env_files` and `items[].git_remote` and never
`items[].manifest_files`. The only thing that scrubbed a manifest was
`scrub_manifest_auth`, called by `write_language_package_manifests` as it
wrote the sidecar. So in one export, measured on a real host:

```
language-packages/npm/<hash>/package.json   "registry": "https://REDACTED@npm.corp.example.com/"
inspection-snapshot.json                    "registry": "https://deploy:<credential>@npm.corp.example.com/"
```

The refine export path had noticed this and patched it locally, with a
projection over `manifest_files` just before its own
`to_string_pretty`. The scan path never got the equivalent. Two paths,
one of them fixed, is the standing failure mode here.

## Where to put the scrub

**In `redact()`.** It is the one place that owns the snapshot object
before either path serializes it, so a scrub there cannot be inherited
by one path and not the other. A scrub at a writer is, by construction,
a scrub one path can skip.

Corollary: a sidecar-time scrub does not even cover its own path
completely. `write_language_package_manifests` routes by method string
and recognises `python venv`, `pip dist-info`, `npm lockfile` and
`gem lockfile`; a `package.json`-only project (`npm manifest`) hits the
`else { continue }` and is never staged. For those projects the snapshot
JSON is the *only* copy of the manifest in the export, so a writer-side
scrub reaches none of them.

**A warning is never scrubbed.** `redact()` scrubs file contents and the
fields it knows; `snapshot.warnings` serializes as written, and the README
and refine's Services section print each `message`. So a warning names
where a host value is (the unit, the key, the file and line), never the
value: the units collector's unknown-needs warning once copied a whole
`ExecStart=` line, token and all, into `message` and `extra.value`. A
test for a new warning puts a sentinel credential in the host value and
asserts it is absent from every file of both exports and from refine's
view and sections.

## Search forward from the key, never from the start of the line

This was the single defect behind eight separate credential leaks
across six functions, and it always looked the same: the scrubber found
the right **line** and then took the wrong **span**.

```
"publishConfig": { "registry": "https://deploy:s3cret@host/" }
                ^ line.find(':') stops here, so "registry" is read as the value
```

The caller already knows which key routed the line to the scrubber.
Give that to [`value_span_after_key`](../../crates/core/src/redaction.rs)
and let it search forward from the key's end. It has four named
syntaxes — `JsonMember`, `QuotedArgument`, `OptionArgument`,
`Assignment` — for the four forms the callers actually parse, and it is
the only place in the codebase that decides where a value starts.

Three rules it enforces that each cost a leak to learn:

- **Only whitespace may sit between the key and its separator.**
  Anything else means the separator belongs to a different key.
- **A quoted value must open at the first non-whitespace character
  after the separator, and close on the same quote character.** The old
  Gemfile loop tried `"` before `'` and broke on whichever appeared
  first anywhere on the line, so a quoted word in a trailing comment
  won over the source URL.
- **The character right after the key must be the separator**, so a
  prefix match against a longer option cannot take that option's
  argument.

Reconstruct through the located span, never through `replace`: the
Gemfile scrubber used `trimmed.replace(url, &scrubbed)` and rewrote
every occurrence of the matched text on the line, not the one it had
found.

For "every URL on this line" — `Gemfile.lock` — tokenize and replace in
**descending offset order**, so an earlier rewrite cannot invalidate a
later span. Same discipline the regex engine uses.

## Detect on the marker, not on string inequality

A scrubber rebuilds the lines it handles, so the output string can
differ from the input without anything having been removed. Use the
redaction marker's count instead:

```rust
if scrubbed.matches(URL_AUTH_REDACTED_MARKER).count()
    > content.matches(URL_AUTH_REDACTED_MARKER).count()
```

The count comparison rather than a `contains` keeps a second `redact()`
pass over an already-scrubbed snapshot from inventing a finding, and
keeps a credential-free manifest byte-identical through the pass.

**Know what the marker proves.** It answers "did `scrub_url_auth`
fire?" It does **not** answer "is this content now credential-free."
The marker says a scrub ran; the unlocatable report below is what
covers the rest.

## A scrubber that cannot find its value has to say so

This is the half that makes the span fixes durable rather than five
more patches.

The scrubbers are rewriters, not detectors. When one runs, or fails to
run, it emits nothing into `snapshot.redactions`, and three things
follow at once: `secrets-review.md` says nothing, `README.md` prints a
`Secrets redacted` count that does not include it (or omits the row
entirely, since `readme.rs` only prints it when the count is non-zero),
and the state stays eligible for `FullyRedacted` because nothing
unresolved weighs against it. `FullyRedacted` is the one state
`crates/refine/src/tarball.rs` trusts to skip re-redaction on import, so
a snapshot mislabelled that way stays unscrubbed through a round trip.

So: anything added to `redact()` that rewrites content must also push a
`RedactionFinding`. A high-confidence finding is a *resolved* one, which
keeps `FullyRedacted` reachable.

And the inverse, which is the part that was missing. A per-line
scrubber returns one of three answers, not two:

| answer | meaning | reported |
|---|---|---|
| `NotMine` | no key matched; nothing was routed here | no |
| `Rewritten` | the value was located; it may or may not have held a credential | only if the marker count rose |
| `Unlocatable` | **a key matched and the value could not be found** | yes, as an unresolved finding |

The third is the one that used to be folded into the first. A line a
scrubber was asked to handle and could not is reported at
`Confidence::Low`, which the state machine counts as unresolved, which
downgrades the snapshot out of `FullyRedacted`. That is deliberate: the
label the import path trusts must stop being applied over content
nothing examined.

Two constraints on that report:

- **It carries no line text.** The findings are serialized into the
  export and the premise is that the line may hold a credential nothing
  stripped. File, line number and the key that routed it, nothing more.
- **It must not fire on well-formed input.** A located value that
  simply had no credential is silent, and a line no key matched is
  silent. Otherwise every ordinary manifest downgrades the state and
  the signal means nothing. `scrub_compose_secrets` has no unlocatable
  state at all now, because its key locator verifies the separator
  itself — say that rather than inventing a signal to be symmetrical.

## Comment exemptions follow the file's parser

Redaction skips a line that starts with a comment marker, so each marker
is a promise about the parser that reads the file. `#` holds everywhere.
`;` and `//` hold only where that file's parser says so
(`comment_markers` in `redaction/engine.rs`); elsewhere a line such as
`//srv/share/password=x` is data and must be scanned. Decide from the
parser's documentation, not the extension: `.conf` covers both
`smb.conf` (`;` is a comment) and `sshd_config`-style files (it is not).
systemd is the case that needs the path, not the name: `systemd.syntax(7)`
gives units, drop-ins and the daemons' own `.conf` files `#` and `;`
comments, so the test is a `systemd` directory component plus a unit
suffix, a `*.d/*.conf` drop-in, or a `.conf` directly in the `systemd`
directory. Getting it wrong in the narrow direction costs false findings;
in the wide direction it leaks, so widen only with a table row in
`test_non_hash_markers_exempt_only_where_they_are_comment_syntax`.

## A `key = value` pattern stays on its line

In `redaction/patterns.rs`, whitespace around a delimiter is `[ \t]*`,
never `\s*`. `\s` matches a newline, so after an empty value the match
runs on into the next line: the stock RHEL `rhsm.conf` has
`proxy_password =` followed by a blank line and a comment, and the
generic pattern took the key, the blank line and the comment's `#`,
leaving a line with no `=` that `configparser` rejects. With `[ \t]*`
the value class (`\S+`) must start on the key's own line, so an empty
value does not match at all. A test that only checks the secret is gone
passes over this; check that the redacted file still parses
(`test_redact_leaves_stock_rhsm_conf_parseable`).

## Content parsed after redaction is unknown when the pass changed it

Some content is parsed *after* `redact()` has run: the sysctl
explanation reads `kernel_boot.sysctl` files and runtime values to say
what each key resolves to and whether it drifts. Parsing scrubbed text
gives a confident wrong answer. A replacement token stands for a whole
match, not for a value split off its key, so a match that spans a key,
the `=` or several lines leaves text that parses as a different setting,
or as none, and a token compared with a host value can read as equal or
as drift.

The rule: **when the pass changed a piece of content, what it says is
unknown, and nothing is parsed from the scrubbed text.** The pass records
the change on the object, and the consumer reads the record rather than
the text:

- `SysctlFileBody::Read.redacted` is set when the content changed.
  While that file survives shadowing (no same-named file in a
  higher-priority directory replaces it), `SysctlSection::effective()`
  returns `Unknown` with an input naming the file. Its lines contribute
  nothing to the as-written list either, because line numbers after a
  multi-line replacement are not the host's.
- A runtime value with any match is removed from `runtime` and its key
  added to `redacted_runtime`. No scrubbed remainder is stored, because
  nothing would read it, and the explanation names the key as unknown.
- A link target is a path, not a line systemd-sysctl parses, so a change
  there scrubs it without setting `redacted`.

The cost is accepted: a host whose sysctl files hold a secret-bearing
key gets no values or drift for any key (Mark, 2026-11-11). The
alternative is an explanation built from text the host never had.

The pass is the config pass reused, not a new scrubber
(`redact_sysctl_section` in `redaction/engine.rs`): the same patterns,
eligibility and comment exemptions under the field's path (runtime
values under `/proc/sys`), the same whole-match replacement, the
snapshot's one counter registry, and findings into `all_findings` like
any other, one per match per path. An `/etc` sysctl file that is also a
config entry is scrubbed in both copies, but the config pass has already
recorded its matches, so the sysctl pass drops a content finding equal
to one already recorded at that path. It does so only when the two
copies held byte-identical text before scrubbing: a finding carries no
matched text, so two different secrets on the same line read as equal
findings, and a file that changed between the two reads must keep both. Recording both listed the file
twice in `secrets-review.md` and counted each match twice wherever
`redactions` is counted. A new consumer that parses content after redaction follows
the same rule: give the pass a flag to set, and read the flag.

## Testing it

Assert over the **whole export directory**, not over one named file:
walk every file under the output dir and assert none contains the
fixture credential
(`render::tests::no_export_artifact_carries_a_manifest_credential`). The
leak above lived for five releases under a suite that checked the
sidecar. A test naming one file cannot see the copy two directories up.

Pair it with a credential-free fixture asserting the content comes back
**byte-identical**, which is what catches the reformat-as-scrub
confusion above.

## The class, and the two tests that close it

Four fields had this gap, found one at a time:

| field | also reached | found by |
|---|---|---|
| `non_rpm_software.items[].manifest_files` | the tarball, `aggregate` | audit |
| `containers.compose_files[].raw_content` | — | fixing the first |
| `containers.quadlet_units[].content` | **the built image** | review of the first fix |
| `scheduled_tasks.*.timer_content` | `config/etc/systemd/system/` | review of the first fix |
| `scheduled_tasks.systemd_timers[].description` | the audit report | the sweep test below |

`SystemdTimer` has carried no file content since phase 3 of the units
plan: a local timer's file is a config entry or a drop-in row, scrubbed
by those passes. `timer_content` and `service_content` remain on
`generated_timer_units` only.

The sysctl section (`kernel_boot.sysctl`) was classified when it was
added, not found later: `SysctlSection.runtime` values,
`SysctlFileBody::Read.content` and `link_target`, and
`SysctlFileBody::OutsideLink.target` are scrubbed;
`SysctlFile.dir` and `name`, `Read.redacted` and `redacted_runtime` are
exempt. No sysctl type has a `Default`, so its patterns sit in
never-called functions the compiler still checks for completeness.

None of these was a parsing bug. Each was a content-bearing field nobody
wired into `redact()`. The quadlet one is the sharpest: a
`COPY quadlet/ /etc/containers/systemd/` put an unscrubbed
`Environment=DB_PASSWORD=` line into the built image, at a path systemd
reads at boot, with the snapshot stamped `FullyRedacted` and no finding
raised — and the identical content *is* scrubbed when it arrives as
`services.drop_ins[].content`.

Two tests in `crates/pipeline/src/render/mod.rs` now hold it shut, and
they need each other:

1. **`every_field_of_a_content_bearing_type_is_classified`** destructures
   every content-bearing snapshot struct with no `..` in the pattern, so
   **adding a field stops the build** until someone writes it into the
   scrubbed or exempt group with a reason. A compile error, not a
   convention.
2. **`exactly_the_exempt_fields_survive_the_export`** plants a distinct
   sentinel in every one of those fields, runs `redact()` and
   `render_all()`, and asserts that the set surviving anywhere under the
   export directory is **exactly** the declared exempt set.

Assert only that scrubbed fields vanish and you get an untested half.
That is precisely how `systemd_timers[].description` got through: it was
classified exempt on the reasoning that a `Description=` is prose rather
than a value, nothing planted a secret in it, and a real scan then put
one in the snapshot and in the rendered audit report. **An exemption
nothing exercises is an assumption, not a decision** — so the exempt
fields are planted too, and the test fails if one of them stops
surviving, which means someone wired it and the classification is stale.

When adding a field to a snapshot type: the compile error tells you to
classify it; if you classify it scrubbed, add a sentinel and wire the
field in `redact()`; if you classify it exempt, add a sentinel anyway
and say why in the comment beside it.
