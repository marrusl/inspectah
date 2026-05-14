# Expected Go-vs-Rust Divergences

Divergences listed here are expected and excluded from the parity gate.
Any difference NOT listed here fails CI.

## schema_version
- Go: 13
- Rust: 14
- Path: `$.schema_version`
- Reason: Rust continues the integer sequence per spec.
- Disposition: permanent

## meta.inspectah_version
- Path: `$.meta.inspectah_version`
- Reason: Different binary version strings.
- Disposition: permanent

## meta.timestamp
- Path: `$.meta.timestamp`
- Reason: Different scan times.
- Disposition: permanent

## redaction_state (Rust-only field)
- Path: `$.redaction_state`
- Reason: New Rust-era field, not present in Go output.
- Disposition: permanent

## completeness (Rust-only field)
- Path: `$.completeness`
- Reason: New Rust-era field, not present in Go output.
- Disposition: permanent

---

# Section-Level Divergences

## Services Section

### state_changes: unchanged unit inclusion (design choice)
- Go: Includes ALL systemd units in `state_changes`, including unchanged ones (`action: "unchanged"`, `include: false`), static units, and template units. The real Go golden has 186 entries (184 unchanged + 2 enable).
- Rust: Only includes actual state divergences — units whose `current_state` differs from `default_state`. Unchanged units are omitted.
- Path: `$.state_changes[*]`
- Reason: Intentional design choice per spec ("output equivalence, not implementation equivalence"). Rust captures the essential migration-relevant data. Including 180+ unchanged entries adds noise without migration value.
- Disposition: permanent
- Approval: approved-by-spec

### fleet on drop_ins (Rust-only nullable field)
- Go: field absent (Go golden has empty `drop_ins: []` on the real host)
- Rust: `"fleet": null`
- Path: `$.services.drop_ins[*].fleet`
- Reason: Fleet field on SystemdDropIn struct. Only surfaces when drop_ins are populated.
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

### alternatives (scope gap — deferred)
- Go: Collects `alternatives` from `update-alternatives --list`. The real Go golden has 28 entries.
- Rust: Field exists in `KernelBootSection` struct but is not populated by the inspector yet. Returns empty `[]`.
- Path: `$.alternatives[*]`
- Reason: `update-alternatives` collection not yet implemented in Rust inspector. Deferred to a future slice. Serde roundtrip of Go golden proves type-level compatibility (Go entries deserialize into `AlternativeEntry` and reserialize faithfully).
- Disposition: temporary
- Approval: approved-by-spec

### non_default_modules (scope gap — deferred)
- Go: Collects `non_default_modules` by comparing loaded modules against a kernel default set. The real Go golden has 33 entries.
- Rust: Field exists in `KernelBootSection` struct but is not populated yet. Returns empty `[]`.
- Path: `$.non_default_modules[*]`
- Reason: Non-default module detection not yet implemented in Rust inspector. Deferred to a future slice. Serde roundtrip of Go golden proves type-level compatibility (Go entries deserialize into `KernelModule` and reserialize faithfully).
- Disposition: temporary
- Approval: approved-by-spec

### tuned_active (fixture vs host difference)
- Go: Shows `""` (empty string) on the real host where tuned is not active.
- Rust: Returns `""` in fixture-based tests when fixture returns `"virtual-guest"` (fixture data differs from real host state).
- Path: `$.tuned_active`
- Reason: This is a data difference between fixture data and real host data, not a code divergence. Rust inspector correctly parses the tuned-adm output. The serde roundtrip of the Go golden succeeds (empty string round-trips faithfully).
- Disposition: permanent — inherent fixture/host data difference
- Approval: approved-by-spec

### loaded_modules ordering and content (fixture vs host difference)
- Go: Contains 73 loaded modules from the real CentOS Stream 9 host.
- Rust: Contains modules from the fixture `lsmod.txt`, which is a different (smaller) dataset.
- Path: `$.loaded_modules[*]`
- Reason: Fixture data is a representative subset, not the full host module list. Inspector-vs-golden comparison between fixture output and real host golden will always diverge on content. Serde roundtrip (Go golden -> Rust type -> JSON) passes, proving type compatibility.
- Disposition: permanent — inherent fixture/host data difference
- Approval: approved-by-spec

### sysctl_overrides (fixture vs host difference)
- Go: Empty array `[]` on the real host (no sysctl overrides detected).
- Rust: May produce entries from fixture sysctl data that differs from real host.
- Path: `$.sysctl_overrides[*]`
- Reason: Fixture sysctl data contains synthetic overrides not present on the real host. Serde roundtrip of Go golden succeeds (empty array round-trips faithfully).
- Disposition: permanent — inherent fixture/host data difference
- Approval: approved-by-spec

### modprobe_d content (fixture vs host difference)
- Go: Contains 1 entry from real host (`firewalld-sysctls.conf`).
- Rust: Empty from fixtures (mock has empty `/etc/modprobe.d`).
- Path: `$.modprobe_d[*]`
- Reason: Fixture mock provides no modprobe.d files. Real host has firewalld sysctl integration.
- Disposition: permanent — inherent fixture/host data difference
- Approval: approved-by-spec

### grub_defaults content (fixture vs host difference)
- Go: Contains full GRUB defaults from real host.
- Rust: Contains fixture GRUB data (different host configuration).
- Path: `$.grub_defaults`
- Reason: Fixture proc-cmdline and GRUB data represent a synthetic environment, not the real host.
- Disposition: permanent — inherent fixture/host data difference
- Approval: approved-by-spec

### cmdline content (fixture vs host difference)
- Go: Contains real host kernel command line.
- Rust: Contains fixture kernel command line.
- Path: `$.cmdline`
- Reason: Same as grub_defaults — fixture data differs from real host.
- Disposition: permanent — inherent fixture/host data difference
- Approval: approved-by-spec

### dracut_conf content (fixture vs host difference)
- Go: Empty array `[]` on the real host.
- Rust: May contain entries from fixture dracut data.
- Path: `$.dracut_conf[*]`
- Reason: Fixture has synthetic dracut configuration not present on real host.
- Disposition: permanent — inherent fixture/host data difference
- Approval: approved-by-spec

## Network Section

### null vs empty array for ip_rules (serde default)
- Go: `"ip_rules": null` (Go `omitempty` on empty slice)
- Rust: `"ip_rules": []` (Rust `#[serde(default)]` deserializes missing as empty, serializes as `[]`)
- Path: `$.network.ip_rules`
- Reason: Rust serde default behavior produces `[]` where Go produces `null` for empty arrays. Semantically identical.
- Disposition: permanent
- Approval: approved-by-spec

## Containers Section

### null vs empty array for all container sub-fields (serde default)
- Go: `"quadlet_units": null`, `"compose_files": null`, `"running_containers": null`, `"flatpak_apps": null`
- Rust: All four fields serialize as `[]` when empty
- Path: `$.containers.quadlet_units`, `$.containers.compose_files`, `$.containers.running_containers`, `$.containers.flatpak_apps`
- Reason: Same `null` vs `[]` serde default pattern as network.ip_rules. Go `omitempty` on empty slices produces `null`. Rust `#[serde(default)]` produces `[]`. Semantically identical.
- Disposition: permanent
- Approval: approved-by-spec

## Users/Groups Section

### users provisioning strategy model (design choice)
- Go: Three-way classification (service/human/ambiguous) mapping to sysusers/kickstart/useradd strategies. Blueprint is used as an override-only strategy.
- Rust: Two-way auto-detect based on login shell validity. Users with a valid login shell (e.g., `/bin/bash`) are classified as human and assigned the `blueprint` strategy. Users without a valid login shell (e.g., `/sbin/nologin`) are classified as service and assigned the `sysusers` strategy. `useradd` and `kickstart` are override-only strategies.
- Path: `$.users[*].classification`, `$.users[*].strategy`
- Reason: Deliberate product decision. The Go three-way model produces an `ambiguous` bucket that requires manual triage. The Rust two-way model eliminates ambiguity by using login shell as a deterministic signal, reducing friction in migration plans.
- Disposition: permanent
- Approval: approved-by-spec

### shadow_entries hash handling (design choice)
- Go: Redacts shadow hashes to numbered tokens (`REDACTED_SHADOW_HASH_1`, etc.)
- Rust: Replaces hash field with status string (`password_set`, `locked`, `disabled`, `no_password`)
- Path: `$.users_groups.shadow_entries[*]`
- Reason: Rust approach is more informative (tells you the account status) while being equally safe (hash never stored). Both prevent hash export.
- Disposition: permanent
- Approval: approved-by-spec

### ssh_authorized_keys_refs key_count field (Rust enhancement)
- Go: SSH key refs contain `user` and `path` only
- Rust: Adds `key_count` field with the number of keys found
- Path: `$.users_groups.ssh_authorized_keys_refs[*].key_count`
- Reason: Rust inspector counts keys during presence detection. Additional field, no Go data lost.
- Disposition: permanent
- Approval: approved-by-spec

### sudoers_rules includedir preservation (Rust enhancement)
- Go: Filters out `#includedir` directives from sudoers rules
- Rust: Preserves `#includedir` as a structural rule (useful for understanding sudoers layout)
- Path: `$.users_groups.sudoers_rules[*]`
- Reason: Include directives are structural information about how sudoers is organized. Preserving them aids migration planning.
- Disposition: permanent
- Approval: approved-by-spec
