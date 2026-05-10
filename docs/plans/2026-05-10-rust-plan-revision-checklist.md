# Rust Phase 0-1 Plan — Revision Checklist

Blockers distilled from the five-lane review (Tang, Thorn, Collins, Press, Slate) plus Mark's decisions on the open questions.

**Source reviews:** `marks-inbox/reviews/2026-05-10-inspectah-rust-rewrite-phase0-1-*-review.md`

---

## Mark's decisions on open questions

| Question | Decision |
|----------|----------|
| Phase 1 includes minimal `ffi-rpm`? | **Yes.** Restore per approved spec. Feature-gated with shell fallback. |
| `--host-root` in Phase 1? | **Defer entirely.** May not support containerized deployment at all. Remove from Phase 1 CLI surface. `RealExecutor` assumes `host_root = /`. |
| `--no-redaction` in Phase 1? | **Defer entirely.** Remove from Phase 1 CLI surface. |

---

## Must-fix blockers

### 1. Typed core contracts (Tang, Thorn, Collins)

Replace `serde_json::Value` boundaries with typed contracts at the inspector/snapshot interface:

- [ ] `InspectorOutput.section` → typed `SectionData` enum (one variant per inspector section: `SectionData::Rpm(RpmSection)`, `SectionData::Config(ConfigSection)`, etc.). JSON compat lives at the serde/export edge, not the trait boundary.
- [ ] `InspectionSnapshot.redactions` → `Vec<RedactionFinding>` (typed, not `Vec<serde_json::Value>`).
- [ ] Wire `redaction_state: RedactionState` onto `InspectionSnapshot`. Phase 1 exports must carry `FullyRedacted` when all findings are resolved.
- [ ] Wire `completeness: Completeness` onto `InspectionSnapshot`. Phase 1 exports must carry `Full` or `Partial` based on inspector failure state.
- [ ] `InspectorId` → newtype or enum, not bare `String`.
- [ ] `Warning.severity` → typed enum, not `Option<String>`.
- [ ] `Renderer::render()` signature → takes `RenderRequest` (snapshot + triage + target context), not just snapshot + path. Even though Phase 1 won't use target context, the signature should be correct from the start.

### 2. Source/render context preservation (Collins)

- [ ] `InspectionContext` must carry the full `SourceSystem` (not just `SystemType` + `RpmState`). Bootc needs `booted_image`; rpm-ostree needs variant + base_image.
- [ ] The "Key Implementation Notes" section claiming SourceSystem is reconstructed from `system_type + os_release` is wrong for bootc/rpm-ostree. Fix the note or remove it — the pipeline constructs SourceSystem from richer detection, not just those two snapshot fields.
- [ ] Task 17 baseline story must reference the approved bootc rule: booted image is the sole baseline truth.

### 3. Mandatory parity gate (Press, Thorn)

- [ ] Full Go v13 golden file is **required** in Phase 1, not optional. Capture it from a real Go scan of a package-based host. If the golden doesn't exist, CI fails — not skips.
- [ ] Create `testdata/divergences.md` with an explicit allowlist of expected Go-vs-Rust differences (e.g., `schema_version: 13 → 14`). The normalized diff only passes if all divergences are documented.
- [ ] Fix `normalize()`: strip only explicitly volatile subfields (timestamps, version strings inside meta), not the entire `meta` object. Contract-bearing meta keys (`hostname`, `host_root`) must survive normalization.
- [ ] Task 25 parity assertion: "zero undocumented divergences" must be mechanically enforced via the divergences file, not left to human interpretation.

### 4. Full tarball artifact surface (Press)

- [ ] Add `report.html` renderer to Phase 1 task list. The Go renderer writes it unconditionally — Rust must too. (Can be a minimal/stub PatternFly report in Phase 1; full interactive dashboard is Phase 5.)
- [ ] Add `kickstart-suggestion.ks` renderer to Phase 1 task list. Same: Go writes it unconditionally, Rust must match.
- [ ] Task 25 E2E test must verify all 8 always-written artifacts are present: `inspection-snapshot.json`, `Containerfile`, `README.md`, `report.html`, `audit-report.md`, `secrets-review.md`, `kickstart-suggestion.ks`, `schema/snapshot.schema.json`.

### 5. Full `config/` contract (Press)

- [ ] Task 23 must implement the full `writeConfigTree()` materialization model, not just `config/etc/`. The approved contract includes: config files, repo/GPG files, firewall zones, kernel/boot snippets (modules-load.d, modprobe.d, dracut.conf.d, tuned, kargs.d), systemd drop-in mirroring, generated/local timer+service units, non-RPM env files.
- [ ] Reference `cmd/inspectah/internal/renderer/configtree.go` as the canonical source for materialization paths.

### 6. CLI surface narrowing (Tang, Thorn, Slate)

- [ ] Remove `--host-root` from Phase 1 `scan` subcommand. `RealExecutor` hardcodes `host_root = /`.
- [ ] Remove `--no-redaction` from Phase 1 `scan` subcommand.
- [ ] Remove `--target` from Phase 1 `scan` subcommand. Target/preflight is Phase 3.
- [ ] Keep `--inspect-only` (JSON-only, no tarball) — this is safe and useful for debugging.
- [ ] Keep `--output` (output path) — no trust implications.
- [ ] Phase 1 CLI surface: `inspectah scan [--inspect-only] [--output PATH]` and `inspectah version`. Nothing else.

### 7. Restore Phase 1 `ffi-rpm` (Tang)

- [ ] Phase 1 RPM inspector includes minimal `librpm` FFI wrapper, feature-gated behind `ffi-rpm`.
- [ ] Shell-based fallback when `ffi-rpm` feature is disabled (for development/CI without librpm-devel).
- [ ] The `ffi-rpm` wrapper validates the dynamic-linking strategy from the approved spec.
- [ ] Add `librpm-sys` or equivalent `-sys` crate to `inspectah-collect/Cargo.toml` under the feature gate.
- [ ] CI runs both minimal (no FFI) and full (with FFI) test profiles.

---

## Should-fix (not blocking, but revise if touching the area)

### 8. Expand Tasks 17-25 into explicit TDD steps (Thorn)

- [ ] Package classification: named negative-path cases (unknown state, empty baseline, duplicate NEVRA).
- [ ] Redaction: explicit `PartiallyRedacted` fixture + assertion. Disabled/empty shadow markers. Tarball-wide secret absence check across all emitted artifacts.
- [ ] Renderer safety: malformed path rejection tests, shell metacharacter escaping, HTML escaping in report output.
- [ ] Tarball: path traversal rejection, symlink containment, NUL byte rejection.
- [ ] Each late-phase task should have review checkpoints, not just "implement and commit."

---

## Not changing

- Crate split (5 crates) — all reviewers accepted this.
- Phase sequencing (P0 → P1 → P2...) — accepted.
- Section type definitions (Tasks 4-8 struct definitions) — accepted, just need the typed `SectionData` wrapper.
- Pipeline typestate model — accepted.
- Schema migration approach (v12/v13 compat via `serde(default)`) — accepted.
- `insta` snapshot testing strategy — accepted.
