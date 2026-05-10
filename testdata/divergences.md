# Expected Go-vs-Rust Divergences

Divergences listed here are expected and excluded from the parity gate.
Any difference NOT listed here fails CI.

## schema_version
- Go: 13
- Rust: 14
- Path: `$.schema_version`
- Reason: Rust continues the integer sequence per spec.

## meta.inspectah_version
- Path: `$.meta.inspectah_version`
- Reason: Different binary version strings.

## meta.timestamp
- Path: `$.meta.timestamp`
- Reason: Different scan times.

## redaction_state (Rust-only field)
- Path: `$.redaction_state`
- Reason: New Rust-era field, not present in Go output.

## completeness (Rust-only field)
- Path: `$.completeness`
- Reason: New Rust-era field, not present in Go output.

## system_type default
- Path: `$.system_type`
- Reason: Rust `InspectionSnapshot::new()` defaults to `"unknown"` (no host scanned). Go fixture contains `"package-mode"` (a scanned host). This diverges only in synthetic/empty snapshots, not real scans.

## preflight.status default
- Path: `$.preflight.status`
- Reason: Rust `PreflightResult::default()` uses empty string. Go fixture contains `"ok"`. Real scans populate this from actual preflight results.
