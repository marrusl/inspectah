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
