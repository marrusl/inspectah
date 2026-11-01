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

**Know what the marker proves.** It answers "did `scrub_url_auth` fire?"
It does **not** answer "is this content now credential-free." A scrubber
that locates the wrong span still fires, still emits the marker, and
still raises a high-confidence finding over a file that kept its
credential. `scrub_compose_secrets` has the same property with its own
`<REDACTED>` marker. The finding means a scrub ran, not that the file is
clean.

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
