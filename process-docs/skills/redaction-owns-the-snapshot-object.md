# Redaction belongs on the snapshot object, not at the writer

`inspection-snapshot.json` is `serde_json::to_string_pretty(snap)`. Any
field that holds raw host content and is not scrubbed **on the snapshot
object** ships in the clear, no matter how carefully the sidecar copy of
the same content was scrubbed on its way out.

This is the redaction form of the two-export-paths problem
([export-paths-stage-different-trees](export-paths-stage-different-trees.md)),
and it bites harder, because the symptom is silence rather than a failed
build.

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

## The scrubbers reformat as well as scrub

Do **not** use `scrubbed != original` as the signal that a credential was
removed. Every scrubber in `render/language_packages.rs` rebuilds its
content with `content.lines().map(...).collect::<Vec<_>>().join("\n")`,
which **drops a trailing newline** whether or not anything was scrubbed,
and `scrub_requirements_txt` additionally re-emits its line from the
trimmed form, so indentation and interior whitespace on an
`--index-url` line are normalized away. Both change the string while
removing nothing. A naive inequality check reports a credential in a
`requirements.txt` that only ever pointed at `pypi.org`.

`scrub_url_auth` is the only operation in any of the five scrubbers that
removes a credential, and it always leaves `://REDACTED@` behind. Detect
on a rise in that marker's count:

```rust
if scrubbed.matches(URL_AUTH_REDACTED_MARKER).count()
    > content.matches(URL_AUTH_REDACTED_MARKER).count()
```

The count comparison rather than a `contains` keeps a second `redact()`
pass over an already-scrubbed snapshot from inventing a finding, and
keeps a credential-free manifest byte-identical through the pass.

## A scrub that emits no finding is a silent one

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
keeps `FullyRedacted` reachable and honest rather than merely unchanged.

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

## Still open

`containers.compose_files[].raw_content` has the same gap, measured:
`redact()` does not visit it, the scan path writes no compose sidecar at
all, and the refine export path scrubs it in that same local projection.
A `POSTGRES_PASSWORD` in a collected compose file reaches
`inspection-snapshot.json` in the clear. Unlike the manifest case the
compose file usually carries an inspector hint, which keeps the state at
`PartiallyRedacted`, but the content still ships.
