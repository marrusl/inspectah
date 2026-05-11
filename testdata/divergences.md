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

---

# Section-Level Divergences

## Services Section

### owning_package (Rust-only nullable field)
- Go: field absent
- Rust: `"owning_package": null`
- Path: `$.services.state_changes[*].owning_package`
- Reason: Rust struct includes owning_package as Option<String> for future package ownership tracking. Go inspector did not populate this field.
- Disposition: permanent — Rust-era enhancement

### fleet (Rust-only nullable field)
- Go: field absent
- Rust: `"fleet": null`
- Path: `$.services.state_changes[*].fleet`
- Reason: Rust struct includes fleet as Option<FleetPrevalence> for fleet-mode data. Go inspector did not populate this field.
- Disposition: permanent — Rust-era enhancement

### fleet on drop_ins (Rust-only nullable field)
- Go: field absent
- Rust: `"fleet": null`
- Path: `$.services.drop_ins[*].fleet`
- Reason: Same as above, fleet field on SystemdDropIn.
- Disposition: permanent — Rust-era enhancement

## Storage Section

### include on fstab_entries (Rust serialization difference)
- Go: `"include": true` or `"include": false`
- Rust: field omitted when None (skip_serializing_if)
- Path: `$.storage.fstab_entries[*].include`
- Reason: Rust uses Option<bool> with skip_serializing_if for include field. Go always serializes it.
- Disposition: permanent — Rust uses idiomatic Option serialization

### acknowledged on fstab_entries (Rust serialization difference)
- Go: `"acknowledged": false`
- Rust: field omitted when false (skip_serializing_if)
- Path: `$.storage.fstab_entries[*].acknowledged`
- Reason: Rust uses is_false skip_serializing_if. Go always serializes it.
- Disposition: permanent — Rust uses idiomatic default-skip serialization

### fleet on fstab_entries (Rust-only nullable field)
- Go: field absent
- Rust: `"fleet": null` or omitted
- Path: `$.storage.fstab_entries[*].fleet`
- Reason: Rust struct includes fleet as Option<FleetPrevalence>. Go inspector did not populate this field.
- Disposition: permanent — Rust-era enhancement

## Kernel Boot Section

No known divergences. The Rust KernelBootSection struct was modeled
directly from the Go output format. Provisional golden files use
Rust-generated output as the reference.

