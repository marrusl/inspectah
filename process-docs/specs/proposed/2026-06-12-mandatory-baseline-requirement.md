# Mandatory Baseline Requirement

**Status:** Proposed
**Date:** 2026-06-12
**Authors:** Mark Russell, with input from Fern (UX), Ember (product strategy), Tang (code scoping)

## Summary

Remove the `--no-baseline` code path from inspectah. Baseline extraction (pulling and inspecting the target container image) becomes a hard requirement. If the pull fails, the scan exits with a classified error message and specific remediation guidance. No pull policy flag is added.

## Motivation

inspectah's value proposition is the delta — classifying what the user added to the base OS. A scan without baseline data cannot perform this classification. The current `--no-baseline` path produces output that looks complete but isn't actionable: no package classification, no proper `FROM` line in the Containerfile, limited audit utility.

Removing this path:
- **Eliminates misleading output.** Users get actionable artifacts or a clear error, never a half-baked result.
- **Simplifies the codebase.** Every renderer and classifier currently branches on "what if there's no baseline?" — ~70-90 lines of production code and ~11 dedicated test functions. More importantly, the cognitive load of reasoning about two modes disappears.
- **Sharpens product positioning.** inspectah commits to being a precision migration tool, not a general-purpose system auditor (Ember).

The air-gapped objection is handled: users can pull to a connected machine (`podman save`) and load on the target (`podman load`), or use a local/mirror registry. The error UX guides them through this.

## Design

### 1. Behavioral Change

**Current:** `--no-baseline` flag lets scans proceed without pulling the target image. Output is degraded but scan "succeeds."

**New:** Baseline extraction is mandatory. If the pull fails, the scan exits with a non-zero exit code and a guided error message. The `--no-baseline` flag is removed.

**Scope:** Single-host scan. Fleet inherits this naturally — every tarball in a fleet manifest will contain baseline data because every scan required it.

**What doesn't change:**
- `--base-image` override stays (lets users correct auto-resolution — more important than ever as the escape hatch)
- `Completeness`/degraded system is untouched (handles inspector execution failures, not baseline absence)
- `InspectionContext.baseline_data` stays as `Option` internally (non-RPM inspectors structurally pass `None` because they don't use it — see Implementation Notes)
- Fleet's `baseline_provisional` stays (separate concept — flags when fleet hosts had different baselines)

### 2. Error Classification and UX

When baseline extraction fails, the CLI classifies the failure by pattern-matching podman's stderr into five categories:

| Category | Detection signals (substring, case-insensitive) | Cause |
|---|---|---|
| Registry unreachable | `dial tcp`, `no such host`, `connection refused`, `i/o timeout`, `network is unreachable` | DNS or network path to registry is down |
| Auth required | `unauthorized`, `authentication required`, `403`, `login` | Registry exists but credentials missing or wrong |
| Image not found | `manifest unknown`, `404`, `not found`, `name unknown` | Registry up, creds fine, image:tag doesn't exist |
| TLS/cert error | `certificate`, `x509`, `tls`, `insecure` | Self-signed or expired certificate |
| Unknown | Anything else | Catch-all for unrecognized podman output |

**Error format** (consistent structure across all categories):

```
Error: cannot pull baseline image

  Image:  quay.io/centos-bootc/centos-bootc:stream10
  Cause:  <one-line plain-English diagnosis>

  <2-4 specific remediation steps>

  Disconnected? You can load images from a tarball:
    podman save -o rhel-bootc.tar quay.io/centos-bootc/centos-bootc:stream10
    podman load -i rhel-bootc.tar
```

**Per-category remediation:**

**Registry unreachable:**
```
  Cause:  cannot reach registry (registry.redhat.io)

  Check network connectivity to the registry:
    curl -s https://registry.redhat.io/v2/ || echo "unreachable"
  If behind a proxy, configure podman:
    Edit /etc/containers/registries.conf or set HTTP_PROXY/HTTPS_PROXY
```

**Auth required:**
```
  Cause:  authentication required

  Verify the image reference is correct (a wrong registry can look like an auth error):
    inspectah scan --base-image <correct-registry>/<image>:<tag>
  If the reference is correct, log in to the registry:
    podman login <registry>
  For Red Hat registries (e.g., registry.redhat.io/rhel10/rhel-bootc:10.2),
  use your Red Hat account or a service account token.
```

**Image not found:**
```
  Cause:  image or tag not found

  Verify the image reference is correct:
    podman search <registry>/<image>
    skopeo list-tags docker://<registry>/<image>
  If your image is at a different registry or tag, use:
    inspectah scan --base-image <correct-registry>/<image>:<tag>
```

**TLS/cert error:**
```
  Cause:  TLS certificate error

  Verify the image reference is correct (a wrong registry can cause TLS errors):
    inspectah scan --base-image <correct-registry>/<image>:<tag>
  If using a private registry with self-signed certificates:
    sudo cp ca.crt /etc/pki/ca-trust/source/anchors/ && sudo update-ca-trust
  Or configure podman to trust the registry:
    Edit /etc/containers/registries.conf.d/ to add [[registry]] with insecure=true
```

**Unknown:**
```
  Cause:  pull failed

  podman reported:
    <first 3 lines of raw stderr, clipped at sentence/line boundaries>

  Try pulling the image manually to diagnose:
    podman pull <image-ref>
```

**Design decisions:**
- The header `Error: cannot pull baseline image` is always the same string — scriptable, grepable.
- The `Image:` line echoes back the resolved reference so users can verify it's correct.
- The disconnected hint appears on every failure type (a sysadmin behind a firewall hitting an auth error may still need the offline path).
- "Not found" leads with "verify the ref/tag exist" before suggesting `--base-image` (typos are the common case).
- Unknown failures preserve podman's raw stderr clipped at sentence/line boundaries (not mid-word).
- **Live stderr sanitization (required).** `pull_progress.rs` surfaces raw podman stderr during the pull (TTY viewport or `pull:` prefix lines). This is a trust boundary — podman stderr must not leak credentials, tokens, or auth headers to the terminal. This is a required product behavior, not advisory.
  - **Acceptance criteria:** (1) Audit podman pull stderr output across all five failure categories for credential/token content. (2) If podman can emit credential material in stderr (e.g., auth negotiation headers, token fragments), add a sanitization pass in `strip_ansi()` or the viewport/non-TTY callbacks that redacts recognized credential patterns before display. (3) The final classified error message (Unknown category) must also sanitize its 3-line stderr excerpt. (4) Add a test that verifies a stderr string containing a bearer token or basic auth header is redacted in both the live progress output and the final error message.
- Auth and TLS remediation messages should surface `--base-image` as an option before nudging users to log in or weaken TLS trust settings — the resolved image ref may be wrong, and logging in to a wrong registry is wasted effort.
- No ANSI color. Sysadmins paste error output into tickets and chat.
- **Exit code:** Pull failures exit with code 3. Exit code 1 = general error, exit code 2 = incomplete scan (already used by `ScanOutcome::Incomplete` and the no-subcommand case in `main.rs`), exit code 3 = baseline pull failure (new), exit code 130 = interrupted (SIGINT). This preserves the existing exit code contract while giving automation a distinct signal for pull failures.
- **Target-image resolution failure** (inspectah cannot determine what image to pull — e.g., no os-release, no bootc status, ambiguous result) is a separate error from pull failure. Resolution failures should exit with code 1 (general error) and a message suggesting `--base-image <ref>`. This is not a new error path — it already exists — but the spec should be explicit that resolution failure and pull failure are distinct UX flows.
- **Classifier precedence:** When podman stderr matches multiple categories (e.g., contains both `unauthorized` and `connection refused`), the classifier uses a deterministic priority order: TLS/cert > Auth > Registry unreachable > Image not found > Unknown. First match in priority order wins. Rationale: TLS errors can cascade into auth-looking failures; showing the root cause is more useful.
- Classification is a pure function (`classify_pull_failure(stderr: &str) -> PullFailureKind`) at the CLI display layer. The collect crate's `ExtractionError::PullFailed` stays as-is — the `#[error(...)]` attribute remains a short internal message. User-facing formatting lives in the CLI crate.

### 3. Code Changes

**Remove:**

| Crate | File | What | Est. lines |
|---|---|---|---|
| `cli` | `crates/cli/src/commands/scan.rs` | `--no-baseline` CLI arg, flag validation, `Err(e) if args.no_baseline` match arm, `snapshot.no_baseline` assignment | ~15 |
| `core` | `crates/core/src/snapshot.rs` | `pub no_baseline: bool` field + serde attrs | ~5 |
| `core` | `crates/core/src/types/rpm.rs` | `pub no_baseline: bool` field on `RpmSection` | ~2 |
| `collect` | `crates/collect/src/inspectors/rpm/mod.rs` | `no_baseline` derivation from `ctx.baseline_data.is_none()`, warning push, field assignment | ~10 |
| `pipeline` | `crates/pipeline/src/render/baseline_fmt.rs` | `if snap.no_baseline` / `else` branch | ~5 |
| `pipeline` | `crates/pipeline/src/render/readme.rs` | `no_baseline` local, `if no_baseline` branch for label | ~6 |
| `pipeline` | `crates/pipeline/src/render/service_intent.rs` | Simplify compound condition (remove `rpm.no_baseline` and `snap.no_baseline` terms) | ~2 |
| `pipeline` | `crates/pipeline/src/render/report.rs` | No-baseline panel logic | ~2 |
| `pipeline` | `crates/pipeline/src/render/containerfile.rs` | No-baseline test assertions | ~3 |
| `core` | `crates/core/src/fleet/merge.rs` | `no_baseline` merging logic | ~4 |
| `core` | `crates/core/src/fleet/mod.rs` | `merged.no_baseline = merged.baseline.is_none()` assignment | ~1 |
| `refine` | `crates/refine/src/projection/reference.rs` | `EmptyReason::NoBaseline` variant + test | ~5 |
| `refine` | `crates/refine/src/session.rs` | `baseline_available` derivation (always true) | ~3 |
| `web` | `crates/web/src/adapter.rs` | `EmptyReason::NoBaseline => "no_baseline"` mapping | ~1 |
| `web` | Frontend JS/TS | Remove any `no_baseline` empty-state rendering, type definitions, or conditional UI paths that reference the `no_baseline` field or `EmptyReason::NoBaseline` string. This includes the single-host packages-view degraded banner ("Baseline unavailable ... NeedsReview") and its frontend test. After removal, decide: delete entirely (baseline is always present) or retain as a defensive impossible-state guard with a "this should never happen" assertion. | TBD |

**Total removal estimate:** ~70-90 lines production Rust source, plus frontend cleanup (scope TBD — depends on how many UI components handle the no-baseline empty state).

**Add:**

| Crate | What |
|---|---|
| `cli` | `PullFailureKind` enum: `RegistryUnreachable`, `AuthRequired`, `ImageNotFound`, `TlsCertError`, `Unknown` |
| `cli` | `classify_pull_failure(stderr: &str) -> PullFailureKind` pure function |
| `cli` | `format_pull_error(kind: PullFailureKind, image_ref: &str, raw_stderr: &str) -> String` |
| `cli` | Exit code 3 on `ExtractionError::PullFailed` |
| `core` | Bump `SCHEMA_VERSION` from 18 to 19 in `crates/core/src/snapshot.rs` |

**Modify:**

| Crate | File | What |
|---|---|---|
| `cli` | `scan.rs` | Baseline extraction failure becomes hard error (remove degraded-mode fallback) |
| `pipeline` | `baseline_fmt.rs` | Remove "skipped (--no-baseline)" / "unavailable" distinction |

**Don't touch:**

| Item | Reason |
|---|---|
| `InspectionContext.baseline_data: Option` | Non-RPM inspectors structurally pass `None`. The `Option` is correct — it means "this field is relevant to some inspectors but not others." See Implementation Notes. |
| `Completeness`/degraded system | Orthogonal — handles inspector execution failures |
| Fleet `baseline_provisional` | Separate concept (flags mixed baselines across fleet hosts) |
| `--base-image` override flag | Stays, more important than ever |

**Test changes:**

- Delete ~11 dedicated test functions for no-baseline behavior across `baseline_fmt.rs`, `readme.rs`, `containerfile.rs`, `service_intent_test.rs`, `fleet_merge_test.rs`, `snapshot.rs`, `attention_test.rs`, `reference.rs`
- Remove `no_baseline: true/false` field from ~25 test struct literals
- Add tests for `classify_pull_failure` (one per category + edge cases for substring matching, mixed signals, empty stderr)
- Add integration test: scan with unreachable image exits 3 with correct error format
- Add test for classifier precedence when stderr matches multiple categories

### 4. Migration and Compatibility

**This is a clean break. No legacy support.**

**CLI breaking change:** Removing `--no-baseline` is a breaking CLI change. Scripts that pass this flag will get an "unknown argument" error. Exit code 3 is new.

**Snapshot schema break:** This change bumps `SCHEMA_VERSION` from 18 to 19 in `crates/core/src/snapshot.rs`. The `no_baseline` field is removed from the snapshot struct. Old tarballs (schema version 18 or earlier) are rejected by the existing version gate in `crates/pipeline/src/validate.rs` (`schema_version != SCHEMA_VERSION` → `UnsupportedVersion` error) and `crates/core/src/snapshot.rs` (`MIN_SCHEMA` check). This applies to refine, web, fleet, and any other consumer that loads snapshots. Users with old tarballs should re-scan to produce valid schema-19 snapshots.

**Rationale:** inspectah is pre-1.0 alpha. The number of no-baseline tarballs in the wild is near zero. The existing version gate provides the enforcement mechanism — no new compatibility code needed.

**Documentation updates:**
- CHANGELOG.md: document under "Removed" (breaking)
- README: add "requires access to your target base image" as a prerequisite, front-load the air-gapped workaround (`podman save`/`podman load`, local registry)
- CLI help text: remove `--no-baseline` references
- Release notes: why it was removed, what to do instead, the disconnected workaround
- Verify no existing tutorials, demo scripts, or docs reference `--no-baseline` or `no_baseline`

## Acceptance Criteria

1. `--no-baseline` flag is removed; passing it produces a clap "unknown argument" error.
2. `no_baseline` field is removed from `InspectionSnapshot` and `RpmSection`.
3. `SCHEMA_VERSION` is 19; loading a schema-18 tarball in refine/web/fleet produces `UnsupportedVersion` error.
4. Baseline pull failure exits with code 3 and displays the classified error message matching the format in section 2.
5. All five error categories produce correct remediation text (verified by unit tests on `classify_pull_failure` and `format_pull_error`).
6. Auth and TLS error messages lead with `--base-image` verification before registry-side remediation.
7. Mixed-signal stderr is classified deterministically per the priority order (TLS > Auth > Registry > Not found > Unknown), verified by test.
8. Target-image resolution failure (can't determine what to pull) exits with code 1 and suggests `--base-image`.
9. Live pull progress output (`pull_progress.rs`) does not leak credential/token material — verified by audit and test.
10. Unknown category stderr excerpt does not leak credential/token material — verified by test.
11. Web UI / frontend no longer references `no_baseline` or renders a no-baseline empty state.
12. `cargo clippy -- -D warnings` passes. All existing tests pass (minus deleted no-baseline tests).
13. README documents baseline as a prerequisite with air-gapped workaround.
14. CHANGELOG.md documents the removal under "Removed" (breaking).

## Implementation Notes

**`InspectionContext.baseline_data: Option` stays as `Option`.** The `InspectionContext` struct is shared across all inspectors, but only the RPM inspector uses baseline data. The other ~10 inspectors pass `None` structurally. Changing to non-optional would force threading a real `BaselineData` through every inspector context, which would be worse — it would pretend all inspectors need baseline when they don't. Tang should evaluate during implementation whether a split-context-type approach (RPM-specific context vs. general context) is cleaner than the current single struct with an `Option`.

**No pull policy flag.** The current "just pull" behavior is the simplest correct behavior. Podman already handles the "image is local and fresh" case — `podman pull` on a locally-present image checks the remote manifest and succeeds fast. A `--pull=always|never|missing` flag is deferred until CI/fleet use cases create concrete demand (Ember's recommendation).

**Preloaded local images (podman load).** The disconnected workaround relies on `podman load` making the image available in the local store, then `podman pull` succeeding against it. Implementation should verify this works end-to-end: `podman save` on a connected machine, `podman load` on the target, then `inspectah scan` succeeds without network access. If `podman pull` still attempts a remote check after `podman load`, the pull will fail despite the image being local. In that case, the error message should suggest the local registry alternative or a future `--pull=never` flag. This is a verification task, not a design change — the behavior depends on podman's pull semantics for locally-loaded images.

## Future Work (out of scope)

- **`inspectah assess` subcommand:** Lightweight migration complexity estimate without baseline. First-class feature with its own output format, not a degraded scan. Filed as backlog item.
- **Enriched baseline extraction:** Currently baseline only captures the RPM package list. Expanding to capture service state, config file defaults, etc. from the target image would enable richer comparison across more inspectors. Filed on nit list.
- **Pull policy flag:** `--pull=always|never|missing` for CI pipelines with pre-staged images. Add when there's demand.
- **Migration tool landscape research:** Deep dive into AWS Migration Hub, Google Migrate to Containers — what they collect, what inspectah can learn. Filed as backlog item.
