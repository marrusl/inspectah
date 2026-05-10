# Expected Go-vs-Rust Divergences

Divergences listed here are expected and excluded from the parity gate.
Any difference NOT listed here fails CI.

## null-vs-empty convention (handled automatically)
Go's `encoding/json` emits `null` for nil slices and nil string pointers.
Rust deserializes these as `[]` / `""` and re-serializes accordingly.
The diff engine treats `null == []` and `null == ""` as equivalent
automatically — no per-field allowlist entry needed.

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

