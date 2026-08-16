# Skills Index

Non-obvious patterns and correctness requirements for working in this
codebase. Each skill documents a real problem that surfaced during
development or review.

| Skill | Summary |
|-------|---------|
| [codebase-layout](codebase-layout.md) | Workspace structure, crate organization, where to find commands/types/inspectors/renderers/tests/docs |
| [two-wave-collection](two-wave-collection.md) | Inspector dispatch ordering, RPM runs first in Wave 1, Wave 2 gets `RpmState`; None vs Some(empty) contract |
| [mock-executor-key-format](mock-executor-key-format.md) | Test infrastructure: command key is `cmd + " " + args.join(" ")`; mismatches silently return exit 127 |
| [snapshot-schema-versioning](snapshot-schema-versioning.md) | Snapshot JSON schema version gating, serde annotation requirements, no backward compat |
| [rpm-epoch-normalization](rpm-epoch-normalization.md) | RPM epoch empty-string vs "0" equivalence across serialization boundaries |
| [serde-include-default-ambiguity](serde-include-default-ambiguity.md) | `include` field deserialization requires pre-patch via `load_for_refine()` to distinguish absent from explicit-false |
| [finding-disposition-serde-defaults](finding-disposition-serde-defaults.md) | `disposition: FindingKind` fields must name their serde default; field-level `serde(default)` ignores the struct's `Default` impl |
| [web-disposition-contract](web-disposition-contract.md) | Web API sends `FindingKind` as a tagged union, not a bool; TS branches on `kind`; config advisories restore from the original snapshot, not the projection; section stats partition into included/excluded/advisory |
| [package-identity-is-name-dot-arch](package-identity-is-name-dot-arch.md) | Package identity is `name.arch` everywhere; bare names cause multiarch collisions |
| [nonrpm-stored-path-normalization](nonrpm-stored-path-normalization.md) | Non-RPM paths store with `trim_start_matches('/')` and render back through `absolute_path()`; `strip_prefix('/')` diverges on doubled roots |
| [aggregate-vs-single-host-behavioral-split](aggregate-vs-single-host-behavioral-split.md) | Aggregate and single-host modes diverge on leaf filtering, redaction state, and rendering |
| [subscription-preserve-flow](subscription-preserve-flow.md) | Subscription PEM collection pipeline, X.509 cert expiry parsing with `x509-parser`, display thresholds, symlink safety |
| [anaconda-classifier-flow](anaconda-classifier-flow.md) | Anaconda gap classifier pipeline: four tiers, locked plumbing, promoted-via-service/config, user-op preservation after reclassification |
| [release-build-configuration](release-build-configuration.md) | Workspace Cargo.toml layout, zigbuild cross-compile, musl static binaries, missing `[profile.release]` tuning |
| [release-process](release-process.md) | Full release checklist: version bump locations, RPM tilde convention, changelog flow, build targets, binary naming, GH release, Homebrew formula |
| [rpm-ownership-vs-name-heuristic](rpm-ownership-vs-name-heuristic.md) | Use `rpm -qf <path>` for ownership proof, not `python3-<name>` heuristic; `RpmState.owned_paths` is /etc-only |
| [rpm-repo-name-mismatch](rpm-repo-name-mismatch.md) | Install-time short names vs full repo IDs require case-insensitive substring matching; method constant registry in `util.rs` |
| [containerfile-value-quoting](containerfile-value-quoting.md) | Quote vs. sanitize vs. reject per interpolation context in the Containerfile renderer; quoting must land before any predicate narrowing |
| [el8-platform-compatibility](el8-platform-compatibility.md) | EL8 platform considerations: no bootc image exists, map up to EL9 floor tag (9.6); defaults must stay version-pinned or migration-kind detection breaks |
