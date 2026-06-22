# Skills Index

Non-obvious patterns and correctness requirements for working in this
codebase. Each skill documents a real problem that surfaced during
development or review.

| Skill | Summary |
|-------|---------|
| [two-wave-collection](two-wave-collection.md) | Inspector dispatch ordering, RPM runs first in Wave 1, Wave 2 gets `RpmState`; None vs Some(empty) contract |
| [mock-executor-key-format](mock-executor-key-format.md) | Test infrastructure: command key is `cmd + " " + args.join(" ")`; mismatches silently return exit 127 |
| [snapshot-schema-versioning](snapshot-schema-versioning.md) | Snapshot JSON schema version gating, serde annotation requirements, no backward compat |
| [rpm-epoch-normalization](rpm-epoch-normalization.md) | RPM epoch empty-string vs "0" equivalence across serialization boundaries |
| [serde-include-default-ambiguity](serde-include-default-ambiguity.md) | `include` field deserialization requires pre-patch via `load_for_refine()` to distinguish absent from explicit-false |
| [package-identity-is-name-dot-arch](package-identity-is-name-dot-arch.md) | Package identity is `name.arch` everywhere; bare names cause multiarch collisions |
| [aggregate-vs-single-host-behavioral-split](aggregate-vs-single-host-behavioral-split.md) | Aggregate and single-host modes diverge on leaf filtering, redaction state, and rendering |
| [subscription-preserve-flow](subscription-preserve-flow.md) | Subscription PEM collection pipeline, X.509 cert expiry parsing with `x509-parser`, display thresholds, symlink safety |
