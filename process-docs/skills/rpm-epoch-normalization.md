---
name: rpm-epoch-normalization
description: RPM epoch comparison requires explicit normalization -- empty string and "0" must be treated as equivalent.
---

# RPM Epoch Normalization

The RPM classifier in `inspectah-collect/src/inspectors/rpm/classifier.rs`
compares host packages against baseline packages using `rpmvercmp()` for
epoch, version, and release independently. Epoch has a non-obvious
normalization requirement.

### The Problem

RPM's `--queryformat %{EPOCH}` returns the literal string `(none)` or
`0` for packages without an explicit epoch. After parsing, some code
paths store this as an empty string, others as `"0"`. The baseline
extraction (which runs `rpm -qa` inside a container) may produce a
different representation than the host-side query.

If you compare `""` against `"0"` with `rpmvercmp`, they are **not**
equal -- `rpmvercmp` treats them as different version strings. This
would make every package with epoch=0 appear as "modified" in the
classification output.

### The Fix

The classifier uses `norm_epoch()` to canonicalize before comparison:

```rust
fn norm_epoch(e: &str) -> &str {
    if e.is_empty() { "0" } else { e }
}

// Usage in classify_packages:
let epoch_cmp = rpmvercmp(norm_epoch(&pkg.epoch), norm_epoch(&base.epoch));
```

### What This Means for New Code

Any code that compares RPM epochs across serialization boundaries (host
vs. baseline, snapshot vs. snapshot, fleet aggregation) must normalize
first. The raw `epoch` field on `PackageEntry` can be either `""` or
`"0"` -- both mean "no epoch." Do not use `==` on raw epoch strings.

The baseline lookup key format is `"{name}.{arch}"` (e.g.,
`bash.x86_64`). If you build keys differently, the `HashMap::get()`
in `classify_packages` will miss matches and classify everything as
`PackageState::Added`.

### Same-EVR Packages Are "Added"

A package with identical epoch, version, and release to the baseline
is classified as `PackageState::Added`, not as a new "Unchanged" state.
This is intentional -- the attention model (downstream in refine)
handles baseline-match visibility via `PackageBaselineMatch`, not the
classifier. Do not add an "Unchanged" variant to `PackageState`.

## Why This Matters

Epoch mismatch was the root cause of a performance regression where
the baseline filter failed to filter anything. Every package appeared
modified, producing a full diff on every scan. The fix was mechanical
(one function) but the bug was invisible without comparing actual
serialized epoch values from both sides.

## See Also

- `inspectah-collect/src/inspectors/rpm/classifier.rs` -- `norm_epoch`, `classify_packages`
- `inspectah-collect/src/inspectors/rpm/parser.rs` -- `rpmvercmp` implementation
- `inspectah-collect/src/baseline.rs` -- baseline extraction (container-side RPM query)
- `inspectah-core/src/baseline.rs` -- `BaselineData`, `BaselinePackageEntry`
