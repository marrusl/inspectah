# Anaconda Gap Classifier Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Classify anaconda-sourced packages that survive baseline subtraction into four tiers: platform plumbing (locked exclude), promoted (user-intent detected), installer noise (soft exclude), and ambiguous (investigate). Collect installed dnf groups for the future rendering spec.

**Architecture:** A post-pass in `classify_packages()` reclassifies anaconda-sourced packages after the existing classification logic. Tier 1 always wins; Tiers 2-4 respect stronger existing signals. Group data is collected in the RPM inspector and stored on the snapshot but does not influence classification.

**Tech Stack:** Rust, serde, insta (snapshot testing), inspectah-core types, inspectah-refine classifier

**Spec:** `process-docs/specs/proposed/2026-06-11-anaconda-gap-classifier.md` (R6, approved)

---

### Task 1: Add `InstalledGroup` struct and `installed_groups` field to `RpmSection`

**Files:**
- Modify: `crates/core/src/types/rpm.rs`
- Test: `crates/core/src/types/rpm.rs` (inline `#[cfg(test)]`)

- [ ] **Step 1: Write the failing test**

Add to the existing `mod tests` block in `crates/core/src/types/rpm.rs`:

```rust
#[test]
fn test_installed_group_roundtrip() {
    let group = InstalledGroup {
        name: "Container Management".into(),
        packages: vec!["podman".into(), "buildah".into(), "skopeo".into()],
    };
    let json = serde_json::to_string(&group).unwrap();
    let parsed: InstalledGroup = serde_json::from_str(&json).unwrap();
    assert_eq!(group, parsed);
}

#[test]
fn test_rpm_section_installed_groups_none_vs_empty() {
    let section_none = RpmSection {
        ..Default::default()
    };
    let json_none = serde_json::to_string(&section_none).unwrap();
    let parsed_none: RpmSection = serde_json::from_str(&json_none).unwrap();
    assert!(parsed_none.installed_groups.is_none());

    let section_empty = RpmSection {
        installed_groups: Some(vec![]),
        ..Default::default()
    };
    let json_empty = serde_json::to_string(&section_empty).unwrap();
    let parsed_empty: RpmSection = serde_json::from_str(&json_empty).unwrap();
    assert_eq!(parsed_empty.installed_groups, Some(vec![]));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p inspectah-core test_installed_group_roundtrip test_rpm_section_installed_groups_none_vs_empty -- --nocapture`
Expected: FAIL — `InstalledGroup` not defined, `installed_groups` not a field on `RpmSection`.

- [ ] **Step 3: Write the struct and add the field**

Add the `InstalledGroup` struct before `RpmSection` in `crates/core/src/types/rpm.rs`:

```rust
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct InstalledGroup {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub packages: Vec<String>,
}
```

Add the field to `RpmSection`:

```rust
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installed_groups: Option<Vec<InstalledGroup>>,
```

Place it after the `file_ownership` field (end of the struct, before the closing brace).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p inspectah-core test_installed_group_roundtrip test_rpm_section_installed_groups_none_vs_empty -- --nocapture`
Expected: PASS

- [ ] **Step 5: Run full core crate tests and clippy**

Run: `cargo test -p inspectah-core && cargo clippy -p inspectah-core -- -W clippy::all`
Expected: All pass, zero warnings.

- [ ] **Step 6: Commit**

```bash
git add crates/core/src/types/rpm.rs
git commit -m "feat(core): add InstalledGroup struct and installed_groups field to RpmSection"
```

---

### Task 2: Add new `TriageReason` variants and display strings

**Files:**
- Modify: `crates/refine/src/types.rs`

- [ ] **Step 1: Add 6 new variants to `TriageReason` enum**

In `crates/refine/src/types.rs`, add these variants to the `TriageReason` enum (after `SensitivePath` and before `Custom(String)`):

```rust
    PackagePlatformPlumbing,
    PackageInstallerDefault,
    PackageInstallerPromotedService,
    PackageInstallerPromotedConfig,
    PackageInstallerAmbiguous,
    PackageInstallerEvidenceUnavailable,
```

- [ ] **Step 2: Add display strings to the `display_string()` impl**

In the `impl TriageReason` block's `display_string()` method, add arms before the `Custom` catch-all:

```rust
            Self::PackagePlatformPlumbing => "Platform plumbing — excluded by boot chain",
            Self::PackageInstallerDefault => "Installed by Anaconda, no active customization detected",
            Self::PackageInstallerPromotedService => "Installer package with active service and config",
            Self::PackageInstallerPromotedConfig => "Installer package with modified configuration",
            Self::PackageInstallerAmbiguous => "Installed by Anaconda — review for user intent",
            Self::PackageInstallerEvidenceUnavailable => "Installer package — evidence unavailable",
```

- [ ] **Step 3: Run refine crate tests and clippy**

Run: `cargo test -p inspectah-refine && cargo clippy -p inspectah-refine -- -W clippy::all`
Expected: All pass, zero warnings. Exhaustive match ensures all existing consumers handle new variants.

- [ ] **Step 4: Fix any exhaustive match failures**

If any `match` on `TriageReason` in other files fails, add the new arms. Check `crates/web/ui/src/api/types.ts` for the TypeScript union — it will need the six new snake_case strings added. Check `crates/web/src/fleet_handlers.rs` and `crates/web/ui/src/components/attentionUtils.ts` for any reason-to-display mapping.

- [ ] **Step 5: Run full workspace build**

Run: `cargo build --workspace`
Expected: Clean build, no errors.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat(refine): add anaconda classifier TriageReason variants"
```

---

### Task 3: Serialization regression tests for new reason variants

**Files:**
- Modify: `crates/refine/src/types.rs` (add to existing test module)

- [ ] **Step 1: Write serialization round-trip tests**

Add to the `#[cfg(test)]` module in `crates/refine/src/types.rs`:

```rust
#[test]
fn test_anaconda_reason_serialization() {
    let cases = vec![
        (TriageReason::PackagePlatformPlumbing, "\"package_platform_plumbing\""),
        (TriageReason::PackageInstallerDefault, "\"package_installer_default\""),
        (TriageReason::PackageInstallerPromotedService, "\"package_installer_promoted_service\""),
        (TriageReason::PackageInstallerPromotedConfig, "\"package_installer_promoted_config\""),
        (TriageReason::PackageInstallerAmbiguous, "\"package_installer_ambiguous\""),
        (TriageReason::PackageInstallerEvidenceUnavailable, "\"package_installer_evidence_unavailable\""),
    ];
    for (reason, expected_json) in cases {
        let serialized = serde_json::to_string(&reason).unwrap();
        assert_eq!(serialized, expected_json, "serialization mismatch for {:?}", reason);
        let deserialized: TriageReason = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized, reason, "deserialization mismatch for {:?}", reason);
    }
}
```

- [ ] **Step 2: Run the test**

Run: `cargo test -p inspectah-refine test_anaconda_reason_serialization -- --nocapture`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add crates/refine/src/types.rs
git commit -m "test(refine): add serialization regression tests for anaconda classifier reasons"
```

---

### Task 4: Add classifier constants

**Files:**
- Modify: `crates/refine/src/classify.rs`

- [ ] **Step 1: Add the three constant lists**

Add after the existing `OS_DEFAULT_SENSITIVE_EXACT` constant in `crates/refine/src/classify.rs`:

```rust
/// Boot-chain packages that conflict with bootc's bootloader management.
/// These are unconditionally excluded and locked.
const PLATFORM_PLUMBING_PREFIXES: &[&str] = &[
    "grub2-",
    "grubby",
    "shim-",
    "efibootmgr",
];

/// High-confidence installer noise that would never be intentionally
/// selected via group-install or kickstart.
const INSTALLER_NOISE_PATTERNS: &[&str] = &[
    "-fonts",
    "-fonts-common",
    "fonts-filesystem",
    "default-fonts-",
    "lshw",
    "lsscsi",
    "libsysfs",
    "initscripts-",
    "prefixdevname",
    "rootfiles",
    "kernel-tools",
    "dracut-config-rescue",
    "mtools",
    "biosdevname",
];

/// Packages that can promote on config-modified signal alone
/// (no service signal required).
const CONFIG_ONLY_PROMOTION: &[&str] = &[
    "sudo",
    "logrotate",
    "chrony",
    "sssd",
    "pam",
];

fn is_platform_plumbing(name: &str) -> bool {
    PLATFORM_PLUMBING_PREFIXES
        .iter()
        .any(|p| name.starts_with(p) || name == *p)
}

fn is_installer_noise(name: &str) -> bool {
    INSTALLER_NOISE_PATTERNS.iter().any(|pattern| {
        if pattern.starts_with('-') {
            // suffix match: "-fonts" matches "google-noto-sans-vf-fonts"
            name.ends_with(pattern)
        } else if pattern.ends_with('-') {
            // prefix match: "initscripts-" matches "initscripts-service"
            name.starts_with(pattern)
        } else {
            // exact or prefix match: "kernel-tools" matches "kernel-tools" and "kernel-tools-libs"
            name == *pattern || name.starts_with(&format!("{}-", pattern))
        }
    })
}

fn is_config_only_promotable(name: &str) -> bool {
    CONFIG_ONLY_PROMOTION.contains(&name)
}
```

- [ ] **Step 2: Run clippy**

Run: `cargo clippy -p inspectah-refine -- -W clippy::all`
Expected: Zero warnings (unused function warnings are acceptable at this step — they'll be used in Task 5).

- [ ] **Step 3: Commit**

```bash
git add crates/refine/src/classify.rs
git commit -m "feat(refine): add anaconda classifier constants and matching functions"
```

---

### Task 5: Implement anaconda classifier post-pass

**Files:**
- Modify: `crates/refine/src/classify.rs`

- [ ] **Step 1: Write the anaconda classifier function**

Add after the helper functions from Task 4:

```rust
use inspectah_core::types::services::{PresetDefault, ServiceUnitState};

/// Reclassify anaconda-sourced packages that survived baseline subtraction.
/// Runs as a post-pass after the main classify_packages logic.
fn apply_anaconda_classification(
    packages: &mut [RefinedPackage],
    snap: &InspectionSnapshot,
) {
    // Build evidence lookups. These return empty sets (not errors) when
    // the underlying snapshot sections are missing — the per-package
    // evidence check below handles the distinction.
    let user_enabled_service_packages = build_user_enabled_service_set(snap);
    let modified_config_packages = build_modified_config_set(snap);
    let has_services = snap.services.is_some();
    let has_config = snap.config.is_some()
        && snap.rpm.as_ref().map_or(false, |r| !r.file_ownership.is_empty());

    for pkg in packages.iter_mut() {
        if pkg.entry.source_repo != "anaconda" {
            continue;
        }

        let name = &pkg.entry.name;

        // Tier 1: platform plumbing — always wins, even over stronger signals
        if is_platform_plumbing(name) {
            pkg.entry.include = false;
            pkg.entry.locked = true;
            pkg.triage = TriageTag {
                triage: Triage::SingleHost(TriageBucket::Baseline),
                primary_reason: TriageReason::PackagePlatformPlumbing,
                annotations: vec![],
            };
            continue;
        }

        // Precedence check: skip if existing classification is stronger
        let dominated_reason = matches!(
            pkg.triage.primary_reason,
            TriageReason::PackageUserAdded | TriageReason::PackageProvenanceUnavailable
        );
        if !dominated_reason {
            continue;
        }

        // Evidence availability: if service or config sections are missing,
        // or file_ownership is empty (needed for config-to-package joins),
        // we cannot evaluate promotion. Preserve the existing classification
        // (PackageUserAdded or PackageProvenanceUnavailable) — do NOT
        // reclassify, do NOT fall through to Tiers 2-4.
        if !has_services || !has_config {
            continue;
        }

        // Per-package evidence: the service and config lookups above are
        // conservative — a package whose owning_package is None simply
        // won't appear in user_enabled_service_packages. That means it
        // can't match Path A (dual-signal), which is correct: we don't
        // have enough evidence to promote. It still reaches Tier 3/4:
        // - Tier 3 (noise) is safe because noise-pattern packages
        //   (fonts, lshw, etc.) don't have user-configurable services.
        // - Tier 4 (ambiguous) defaults to include:true — the safe
        //   fallback for packages we can't confidently classify.
        //
        // This means EvidenceUnavailable is only used when an explicit
        // per-package evidence failure is detected that would otherwise
        // cause a false-negative exclusion. Currently the lookup
        // structure handles degradation implicitly via conservative
        // set membership, so EvidenceUnavailable is reserved for future
        // cases where a specific join failure needs to be surfaced.

        // Tier 2 Path A: dual-signal promotion (user-enabled service + modified config)
        if user_enabled_service_packages.contains(name.as_str())
            && modified_config_packages.contains(name.as_str())
        {
            pkg.triage = TriageTag {
                triage: Triage::SingleHost(TriageBucket::Site),
                primary_reason: TriageReason::PackageInstallerPromotedService,
                annotations: vec![],
            };
            continue;
        }

        // Tier 2 Path B: config-only promotion (curated list)
        if is_config_only_promotable(name)
            && modified_config_packages.contains(name.as_str())
        {
            pkg.triage = TriageTag {
                triage: Triage::SingleHost(TriageBucket::Site),
                primary_reason: TriageReason::PackageInstallerPromotedConfig,
                annotations: vec![],
            };
            continue;
        }

        // Tier 3: installer noise
        if is_installer_noise(name) {
            pkg.entry.include = false;
            pkg.triage = TriageTag {
                triage: Triage::SingleHost(TriageBucket::Baseline),
                primary_reason: TriageReason::PackageInstallerDefault,
                annotations: vec![],
            };
            continue;
        }

        // Tier 4: ambiguous — may be group-install or kickstart intent
        pkg.triage = TriageTag {
            triage: Triage::SingleHost(TriageBucket::Investigate),
            primary_reason: TriageReason::PackageInstallerAmbiguous,
            annotations: vec![],
        };
        // include stays true (default) — safer to include ambiguous packages
    }
}

fn build_user_enabled_service_set(snap: &InspectionSnapshot) -> std::collections::HashSet<&str> {
    let mut set = std::collections::HashSet::new();
    if let Some(services) = &snap.services {
        for svc in &services.state_changes {
            if svc.current_state == ServiceUnitState::Enabled
                && svc.default_state != Some(PresetDefault::Enable)
            {
                if let Some(pkg) = &svc.owning_package {
                    set.insert(pkg.as_str());
                }
            }
        }
    }
    set
}

fn build_modified_config_set(snap: &InspectionSnapshot) -> std::collections::HashSet<&str> {
    let mut set = std::collections::HashSet::new();
    if let Some(config) = &snap.config {
        if let Some(rpm) = &snap.rpm {
            for ownership in &rpm.file_ownership {
                for config_file in &config.files {
                    if config_file.kind == ConfigFileKind::RpmOwnedModified
                        && ownership.files.contains(&config_file.path)
                    {
                        set.insert(ownership.package.as_str());
                    }
                }
            }
        }
    }
    set
}
```

- [ ] **Step 2: Call the post-pass from `classify_packages()`**

At the end of `classify_packages()`, just before the `return` statement (or at the end of the function body before the vec is returned), add:

```rust
    apply_anaconda_classification(&mut result, snap);
```

Where `result` is the `Vec<RefinedPackage>` being built. Find the variable name used in the actual function and use that.

- [ ] **Step 3: Add the missing import**

Add `PresetDefault` and `ServiceUnitState` to the imports at the top of `classify.rs` if not already imported:

```rust
use inspectah_core::types::services::{PresetDefault, ServiceStateChange, ServiceUnitState, SystemdDropIn};
```

Also check if `FileOwnershipEntry` needs to be imported from `inspectah_core::types::rpm`.

- [ ] **Step 4: Run full workspace build**

Run: `cargo build --workspace`
Expected: Clean build.

- [ ] **Step 5: Run clippy**

Run: `cargo clippy --workspace -- -W clippy::all`
Expected: Zero warnings.

- [ ] **Step 6: Commit**

```bash
git add crates/refine/src/classify.rs
git commit -m "feat(refine): implement anaconda gap classifier post-pass"
```

---

### Task 6: Classifier unit tests

**Files:**
- Modify: `crates/refine/src/classify.rs` (add to existing `mod tests`)

This task adds tests for each tier, precedence rules, and missing-signal fallback. All tests use the existing `snap_with_baseline()` and `pkg()` helper pattern already in the test module.

- [ ] **Step 1: Add test helpers**

Add to the existing `mod tests` in `classify.rs`. First, an extended snapshot builder that includes services and config data:

```rust
    fn snap_with_anaconda(
        baseline_names: Option<Vec<String>>,
        packages: Vec<PackageEntry>,
        services: Option<ServiceSection>,
        config: Option<ConfigSection>,
        file_ownership: Vec<FileOwnershipEntry>,
    ) -> InspectionSnapshot {
        let baseline = baseline_names.map(|names| {
            let pkgs = names
                .into_iter()
                .map(|n| {
                    let key = format!("{}.x86_64", n);
                    let entry = BaselinePackageEntry {
                        name: n,
                        epoch: None,
                        version: "1.0".into(),
                        release: "1.el10".into(),
                        arch: "x86_64".into(),
                    };
                    (key, entry)
                })
                .collect();
            BaselineData {
                image_digest: "sha256:test".to_string(),
                packages: pkgs,
                extracted_at: "2026-01-01T00:00:00Z".to_string(),
            }
        });
        InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            rpm: Some(RpmSection {
                packages_added: packages,
                file_ownership,
                ..Default::default()
            }),
            services,
            config,
            baseline,
            ..Default::default()
        }
    }

    fn anaconda_pkg(name: &str) -> PackageEntry {
        PackageEntry {
            name: name.into(),
            arch: "x86_64".into(),
            state: PackageState::Added,
            source_repo: "anaconda".into(),
            ..Default::default()
        }
    }
```

Add the necessary imports at the top of the test module:

```rust
    use inspectah_core::types::config::{ConfigSection, ConfigFileEntry, ConfigFileKind, ConfigCategory};
    use inspectah_core::types::services::{ServiceSection, ServiceStateChange, ServiceUnitState, PresetDefault, SystemdDropIn};
    use inspectah_core::types::rpm::FileOwnershipEntry;
```

- [ ] **Step 2: Run to verify helpers compile**

Run: `cargo test -p inspectah-refine --no-run`
Expected: Compiles.

- [ ] **Step 3: Write Tier 1 test — platform plumbing hard exclude**

```rust
    #[test]
    fn anaconda_tier1_platform_plumbing_hard_excluded() {
        let snap = snap_with_anaconda(
            Some(vec!["glibc".into()]),
            vec![anaconda_pkg("grub2-efi-aa64-cdboot"), anaconda_pkg("httpd")],
            Some(ServiceSection { state_changes: vec![], enabled_units: vec![], disabled_units: vec![], drop_ins: vec![], preset_matched_units: vec![] }),
            Some(ConfigSection { files: vec![] }),
            vec![],
        );
        let result = classify_packages(&snap);
        let grub = result.iter().find(|p| p.entry.name == "grub2-efi-aa64-cdboot").unwrap();
        assert_bucket(&grub.triage, TriageBucket::Baseline);
        assert_eq!(grub.triage.primary_reason, TriageReason::PackagePlatformPlumbing);
        assert!(!grub.entry.include);
        assert!(grub.entry.locked);
    }
```

- [ ] **Step 4: Write Tier 1 precedence test — platform plumbing overrides version change**

This test proves that a platform-plumbing package with a version change
(a stronger existing signal) is STILL hard-excluded. The version change
creates a `PackageVersionChanged` classification that the anaconda
post-pass must override for Tier 1 packages.

```rust
    #[test]
    fn anaconda_tier1_overrides_version_changed() {
        // grub2-tools-extra with a version change from baseline — the
        // existing classifier assigns PackageVersionChanged (stronger),
        // but Tier 1 must still win and hard-exclude.
        let mut grub = anaconda_pkg("grub2-tools-extra");
        grub.state = PackageState::Modified;
        let snap = snap_with_anaconda_and_vc(
            Some(vec!["glibc".into(), "grub2-tools-extra".into()]),
            vec![grub],
            vec![VersionChange {
                name: "grub2-tools-extra".into(),
                arch: "x86_64".into(),
                host_version: "2.06".into(),
                base_version: "2.04".into(),
                host_epoch: "1".into(),
                base_epoch: "1".into(),
                direction: Some(VersionChangeDirection::Upgrade),
                ..Default::default()
            }],
            Some(ServiceSection { state_changes: vec![], enabled_units: vec![], disabled_units: vec![], drop_ins: vec![], preset_matched_units: vec![] }),
            Some(ConfigSection { files: vec![] }),
            vec![],
        );
        let result = classify_packages(&snap);
        let grub = result.iter().find(|p| p.entry.name == "grub2-tools-extra").unwrap();
        // Tier 1 must override the PackageVersionChanged signal
        assert_eq!(grub.triage.primary_reason, TriageReason::PackagePlatformPlumbing,
            "Tier 1 must override stronger signals for boot-chain packages");
        assert!(grub.entry.locked);
        assert!(!grub.entry.include);
    }
```

- [ ] **Step 5: Write precedence tests — stronger signals preserved**

Two tests: one for `PackageVersionChanged`, one for `PackageLocalInstall`. Both must prove the anaconda classifier does not override the existing signal.

```rust
    #[test]
    fn anaconda_precedence_preserves_version_changed() {
        // A package with anaconda source_repo AND a version change from
        // baseline. The existing classifier must assign PackageVersionChanged
        // (a stronger signal), and the anaconda post-pass must NOT override.
        let mut pkg = anaconda_pkg("tzdata");
        pkg.state = PackageState::Modified;  // Modified state triggers version-change path
        let snap = snap_with_anaconda_and_vc(
            Some(vec!["glibc".into(), "tzdata".into()]),
            vec![pkg],
            vec![VersionChange {
                name: "tzdata".into(),
                arch: "x86_64".into(),
                host_version: "2026b".into(),
                base_version: "2026a".into(),
                host_epoch: "0".into(),
                base_epoch: "0".into(),
                direction: Some(VersionChangeDirection::Upgrade),
                ..Default::default()
            }],
            Some(ServiceSection { state_changes: vec![], enabled_units: vec![], disabled_units: vec![], drop_ins: vec![], preset_matched_units: vec![] }),
            Some(ConfigSection { files: vec![] }),
            vec![],
        );
        let result = classify_packages(&snap);
        let tz = result.iter().find(|p| p.entry.name == "tzdata").unwrap();
        // Must be PackageVersionChanged — NOT reclassified by anaconda post-pass.
        // The exact bucket (Site for upgrades, Investigate for downgrades)
        // depends on direction — assert the reason, not the bucket.
        assert_eq!(tz.triage.primary_reason, TriageReason::PackageVersionChanged,
            "anaconda post-pass must not override PackageVersionChanged");
    }

    #[test]
    fn anaconda_precedence_preserves_local_install() {
        let mut local = anaconda_pkg("custom-rpm");
        local.state = PackageState::LocalInstall;
        let snap = snap_with_anaconda(
            Some(vec!["glibc".into()]),
            vec![local],
            Some(ServiceSection { state_changes: vec![], enabled_units: vec![], disabled_units: vec![], drop_ins: vec![], preset_matched_units: vec![] }),
            Some(ConfigSection { files: vec![] }),
            vec![],
        );
        let result = classify_packages(&snap);
        let custom = result.iter().find(|p| p.entry.name == "custom-rpm").unwrap();
        assert_eq!(custom.triage.primary_reason, TriageReason::PackageLocalInstall);
    }
```

Note: `snap_with_anaconda_and_vc` is an extended helper that also accepts `version_changes`. Add it alongside `snap_with_anaconda` in Step 1:

```rust
    fn snap_with_anaconda_and_vc(
        baseline_names: Option<Vec<String>>,
        packages: Vec<PackageEntry>,
        version_changes: Vec<VersionChange>,
        services: Option<ServiceSection>,
        config: Option<ConfigSection>,
        file_ownership: Vec<FileOwnershipEntry>,
    ) -> InspectionSnapshot {
        let mut snap = snap_with_anaconda(baseline_names, packages, services, config, file_ownership);
        if let Some(rpm) = &mut snap.rpm {
            rpm.version_changes = version_changes;
        }
        snap
    }
```

- [ ] **Step 6: Write Tier 2 Path A test — dual-signal promotion**

```rust
    #[test]
    fn anaconda_tier2_dual_signal_promotes_to_site() {
        let snap = snap_with_anaconda(
            Some(vec!["glibc".into()]),
            vec![anaconda_pkg("firewalld")],
            Some(ServiceSection {
                state_changes: vec![ServiceStateChange {
                    unit: "firewalld.service".into(),
                    current_state: ServiceUnitState::Enabled,
                    default_state: Some(PresetDefault::Disable),
                    include: true,
                    owning_package: Some("firewalld".into()),
                    ..Default::default()
                }],
                enabled_units: vec![], disabled_units: vec![], drop_ins: vec![], preset_matched_units: vec![],
            }),
            Some(ConfigSection {
                files: vec![ConfigFileEntry {
                    path: "/etc/firewalld/zones/custom.xml".into(),
                    kind: ConfigFileKind::RpmOwnedModified,
                    category: ConfigCategory::Other,
                    content: String::new(),
                    include: true,
                    ..Default::default()
                }],
            }),
            vec![FileOwnershipEntry {
                package: "firewalld".into(),
                files: vec!["/etc/firewalld/zones/custom.xml".into()],
            }],
        );
        let result = classify_packages(&snap);
        let fw = result.iter().find(|p| p.entry.name == "firewalld").unwrap();
        assert_bucket(&fw.triage, TriageBucket::Site);
        assert_eq!(fw.triage.primary_reason, TriageReason::PackageInstallerPromotedService);
        assert!(fw.entry.include);
    }
```

- [ ] **Step 7: Write Tier 2 Path B test — config-only promotion**

```rust
    #[test]
    fn anaconda_tier2_config_only_promotes_curated_package() {
        let snap = snap_with_anaconda(
            Some(vec!["glibc".into()]),
            vec![anaconda_pkg("sudo")],
            Some(ServiceSection { state_changes: vec![], enabled_units: vec![], disabled_units: vec![], drop_ins: vec![], preset_matched_units: vec![] }),
            Some(ConfigSection {
                files: vec![ConfigFileEntry {
                    path: "/etc/sudoers".into(),
                    kind: ConfigFileKind::RpmOwnedModified,
                    category: ConfigCategory::Other,
                    content: String::new(),
                    include: true,
                    ..Default::default()
                }],
            }),
            vec![FileOwnershipEntry {
                package: "sudo".into(),
                files: vec!["/etc/sudoers".into()],
            }],
        );
        let result = classify_packages(&snap);
        let sudo = result.iter().find(|p| p.entry.name == "sudo").unwrap();
        assert_bucket(&sudo.triage, TriageBucket::Site);
        assert_eq!(sudo.triage.primary_reason, TriageReason::PackageInstallerPromotedConfig);
    }
```

- [ ] **Step 8: Write Tier 3 test — installer noise soft exclude**

```rust
    #[test]
    fn anaconda_tier3_installer_noise_soft_excluded() {
        let snap = snap_with_anaconda(
            Some(vec!["glibc".into()]),
            vec![
                anaconda_pkg("google-noto-sans-vf-fonts"),
                anaconda_pkg("lshw"),
                anaconda_pkg("kernel-tools"),
                anaconda_pkg("biosdevname"),
            ],
            Some(ServiceSection { state_changes: vec![], enabled_units: vec![], disabled_units: vec![], drop_ins: vec![], preset_matched_units: vec![] }),
            Some(ConfigSection { files: vec![] }),
            vec![],
        );
        let result = classify_packages(&snap);
        for name in &["google-noto-sans-vf-fonts", "lshw", "kernel-tools", "biosdevname"] {
            let pkg = result.iter().find(|p| p.entry.name == *name).unwrap();
            assert_bucket(&pkg.triage, TriageBucket::Baseline);
            assert_eq!(pkg.triage.primary_reason, TriageReason::PackageInstallerDefault, "wrong reason for {}", name);
            assert!(!pkg.entry.include, "{} should be excluded", name);
            assert!(!pkg.entry.locked, "{} should not be locked", name);
        }
    }
```

- [ ] **Step 9: Write Tier 4 test — ambiguous anaconda**

```rust
    #[test]
    fn anaconda_tier4_ambiguous_defaults_to_investigate_included() {
        let snap = snap_with_anaconda(
            Some(vec!["glibc".into()]),
            vec![anaconda_pkg("cronie"), anaconda_pkg("audit")],
            Some(ServiceSection { state_changes: vec![], enabled_units: vec![], disabled_units: vec![], drop_ins: vec![], preset_matched_units: vec![] }),
            Some(ConfigSection { files: vec![] }),
            vec![],
        );
        let result = classify_packages(&snap);
        for name in &["cronie", "audit"] {
            let pkg = result.iter().find(|p| p.entry.name == *name).unwrap();
            assert_bucket(&pkg.triage, TriageBucket::Investigate);
            assert_eq!(pkg.triage.primary_reason, TriageReason::PackageInstallerAmbiguous);
            assert!(pkg.entry.include, "{} should be included by default", name);
        }
    }
```

- [ ] **Step 10: Write missing-signal fallback test**

```rust
    #[test]
    fn anaconda_missing_evidence_falls_to_investigate() {
        let snap = snap_with_anaconda(
            Some(vec!["glibc".into()]),
            vec![anaconda_pkg("firewalld")],
            None,  // services missing
            None,  // config missing
            vec![],
        );
        let result = classify_packages(&snap);
        let fw = result.iter().find(|p| p.entry.name == "firewalld").unwrap();
        assert_bucket(&fw.triage, TriageBucket::Investigate);
        assert_eq!(fw.triage.primary_reason, TriageReason::PackageInstallerEvidenceUnavailable);
    }
```

- [ ] **Step 11: Write non-anaconda package unaffected test**

```rust
    #[test]
    fn non_anaconda_package_unaffected_by_classifier() {
        let mut httpd = anaconda_pkg("grub2-tools-extra");
        httpd.source_repo = "appstream".into();  // NOT anaconda
        let snap = snap_with_anaconda(
            Some(vec!["glibc".into()]),
            vec![httpd],
            Some(ServiceSection { state_changes: vec![], enabled_units: vec![], disabled_units: vec![], drop_ins: vec![], preset_matched_units: vec![] }),
            Some(ConfigSection { files: vec![] }),
            vec![],
        );
        let result = classify_packages(&snap);
        let pkg = result.iter().find(|p| p.entry.name == "grub2-tools-extra").unwrap();
        // Should NOT be platform plumbing — source_repo is not "anaconda"
        assert_ne!(pkg.triage.primary_reason, TriageReason::PackagePlatformPlumbing);
    }
```

- [ ] **Step 12: Run all tests**

Run: `cargo test -p inspectah-refine -- --nocapture`
Expected: All pass.

- [ ] **Step 13: Commit**

```bash
git add crates/refine/src/classify.rs
git commit -m "test(refine): add anaconda gap classifier unit tests for all four tiers"
```

---

### Task 7: Group-install collection in RPM inspector

**Files:**
- Modify: `crates/collect/src/inspectors/rpm/mod.rs`

- [ ] **Step 1: Add a group collection function**

Add after the existing `query_user_installed` function:

```rust
fn collect_installed_groups(exec: &dyn Executor) -> Option<Vec<InstalledGroup>> {
    // Force C locale for deterministic parsing of dnf output headings
    // ("Installed Groups:", "Mandatory Packages:", etc.)
    let result = exec.run("env", &["LC_ALL=C", "dnf", "group", "list", "--installed"]);
    if result.exit_code != 0 {
        return None;
    }

    let mut group_names = Vec::new();
    let mut in_installed = false;
    for line in result.stdout.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("Installed") {
            in_installed = true;
            continue;
        }
        if trimmed.starts_with("Available") || trimmed.is_empty() {
            if in_installed {
                break;
            }
            continue;
        }
        if in_installed && !trimmed.is_empty() {
            group_names.push(trimmed.to_string());
        }
    }

    let mut groups = Vec::new();
    for group_name in &group_names {
        let info_result = exec.run("env", &["LC_ALL=C", "dnf", "group", "info", group_name]);
        if info_result.exit_code != 0 {
            // Individual group info failure: skip this group, keep others.
            // Partial data is better than None — None means "collection
            // failed entirely" and disables group awareness.
            continue;
        }
        let packages = parse_group_info_packages(&info_result.stdout);
        groups.push(InstalledGroup {
            name: group_name.clone(),
            packages,
        });
    }

    Some(groups)
}

fn parse_group_info_packages(stdout: &str) -> Vec<String> {
    let mut packages = Vec::new();
    let mut in_package_section = false;
    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("Mandatory Packages:")
            || trimmed.starts_with("Default Packages:")
            || trimmed.starts_with("Optional Packages:")
        {
            in_package_section = true;
            continue;
        }
        if trimmed.is_empty() || trimmed.ends_with(':') {
            in_package_section = false;
            continue;
        }
        if in_package_section {
            let name = trimmed.trim_start_matches("  ");
            if !name.is_empty() {
                packages.push(name.to_string());
            }
        }
    }
    packages.sort();
    packages.dedup();
    packages
}
```

- [ ] **Step 2: Add the import**

Add `InstalledGroup` to the imports from `inspectah_core::types::rpm`:

```rust
use inspectah_core::types::rpm::{..., InstalledGroup};
```

- [ ] **Step 3: Call group collection in the `inspect()` method**

In the `inspect()` method, before the "9. Build RpmSection" comment, add:

```rust
        // 8b. Collect installed dnf groups
        let installed_groups = collect_installed_groups(exec);
```

Then in the `RpmSection` struct literal, add the field:

```rust
            installed_groups,
```

- [ ] **Step 4: Run full workspace build**

Run: `cargo build --workspace`
Expected: Clean build.

- [ ] **Step 5: Run clippy**

Run: `cargo clippy --workspace -- -W clippy::all`
Expected: Zero warnings.

- [ ] **Step 6: Commit**

```bash
git add crates/collect/src/inspectors/rpm/mod.rs
git commit -m "feat(collect): add dnf group-install collection to RPM inspector"
```

---

### Task 8: Group collection tests and classification-neutral regression

**Files:**
- Modify: `crates/collect/src/inspectors/rpm/mod.rs` (parser tests)
- Modify: `crates/refine/src/classify.rs` (regression test)

- [ ] **Step 1: Write group info parser test**

Add to the test module in `crates/collect/src/inspectors/rpm/mod.rs`:

```rust
    #[test]
    fn test_parse_group_info_packages() {
        let stdout = "\
Group: Container Management
 Description: Tools for managing Linux containers
 Mandatory Packages:
   podman
   buildah
   skopeo
 Default Packages:
   containernetworking-plugins
   crun
 Optional Packages:
   toolbox
   udica
";
        let packages = parse_group_info_packages(stdout);
        assert_eq!(packages, vec![
            "buildah",
            "containernetworking-plugins",
            "crun",
            "podman",
            "skopeo",
            "toolbox",
            "udica",
        ]);
    }

    #[test]
    fn test_parse_group_info_empty() {
        let packages = parse_group_info_packages("");
        assert!(packages.is_empty());
    }
```

- [ ] **Step 2: Write classification-neutral regression test**

Add to the test module in `crates/refine/src/classify.rs`:

```rust
    #[test]
    fn anaconda_classification_neutral_with_installed_groups() {
        let packages = vec![
            anaconda_pkg("grub2-efi-aa64-cdboot"),
            anaconda_pkg("google-noto-sans-vf-fonts"),
            anaconda_pkg("cronie"),
        ];

        // Run with installed_groups = None
        let snap_none = snap_with_anaconda(
            Some(vec!["glibc".into()]),
            packages.clone(),
            Some(ServiceSection { state_changes: vec![], enabled_units: vec![], disabled_units: vec![], drop_ins: vec![], preset_matched_units: vec![] }),
            Some(ConfigSection { files: vec![] }),
            vec![],
        );
        let result_none = classify_packages(&snap_none);

        // Run with installed_groups = Some([]) (no groups)
        let mut snap_empty = snap_with_anaconda(
            Some(vec!["glibc".into()]),
            packages.clone(),
            Some(ServiceSection { state_changes: vec![], enabled_units: vec![], disabled_units: vec![], drop_ins: vec![], preset_matched_units: vec![] }),
            Some(ConfigSection { files: vec![] }),
            vec![],
        );
        if let Some(rpm) = &mut snap_empty.rpm {
            rpm.installed_groups = Some(vec![]);
        }
        let result_empty = classify_packages(&snap_empty);

        // Run with installed_groups = Some([group with cronie])
        let mut snap_groups = snap_with_anaconda(
            Some(vec!["glibc".into()]),
            packages.clone(),
            Some(ServiceSection { state_changes: vec![], enabled_units: vec![], disabled_units: vec![], drop_ins: vec![], preset_matched_units: vec![] }),
            Some(ConfigSection { files: vec![] }),
            vec![],
        );
        if let Some(rpm) = &mut snap_groups.rpm {
            rpm.installed_groups = Some(vec![InstalledGroup {
                name: "Base".into(),
                packages: vec!["cronie".into()],
            }]);
        }
        let result_groups = classify_packages(&snap_groups);

        // All three must produce identical classification outcomes
        for (name, expected_reason) in &[
            ("grub2-efi-aa64-cdboot", TriageReason::PackagePlatformPlumbing),
            ("google-noto-sans-vf-fonts", TriageReason::PackageInstallerDefault),
            ("cronie", TriageReason::PackageInstallerAmbiguous),
        ] {
            let r_none = result_none.iter().find(|p| p.entry.name == *name).unwrap();
            let r_empty = result_empty.iter().find(|p| p.entry.name == *name).unwrap();
            let r_groups = result_groups.iter().find(|p| p.entry.name == *name).unwrap();
            assert_eq!(r_none.triage.primary_reason, *expected_reason, "None: {}", name);
            assert_eq!(r_empty.triage.primary_reason, *expected_reason, "Empty: {}", name);
            assert_eq!(r_groups.triage.primary_reason, *expected_reason, "Groups: {}", name);
            assert_eq!(r_none.entry.include, r_empty.entry.include, "include mismatch for {}", name);
            assert_eq!(r_none.entry.include, r_groups.entry.include, "include mismatch for {}", name);
        }
    }
```

- [ ] **Step 3: Run all tests**

Run: `cargo test --workspace -- --nocapture`
Expected: All pass.

- [ ] **Step 4: Run clippy**

Run: `cargo clippy --workspace -- -W clippy::all`
Expected: Zero warnings.

- [ ] **Step 5: Commit**

```bash
git add crates/collect/src/inspectors/rpm/mod.rs crates/refine/src/classify.rs
git commit -m "test: add group collection parser tests and classification-neutral regression"
```

---

### Task 9: Locking contract test at session boundary

**Files:**
- Modify: `crates/refine/src/session.rs` (add to existing test module)

- [ ] **Step 1: Write session-level locking test**

Add to the `#[cfg(test)]` module in `session.rs`. This test builds a snapshot with an anaconda-sourced platform-plumbing package (locked=true), creates a refine session, attempts to SetInclude(true), and verifies the operation is a no-op:

```rust
    #[test]
    fn locked_platform_plumbing_package_rejects_set_include() {
        let mut snap = InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            rpm: Some(RpmSection {
                packages_added: vec![PackageEntry {
                    name: "grub2-efi-aa64-cdboot".into(),
                    arch: "aarch64".into(),
                    state: PackageState::Added,
                    source_repo: "anaconda".into(),
                    include: false,
                    locked: true,
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        let session = RefineSession::new(snap);
        // Apply SetInclude(true) — should be a silent no-op
        let _result = session.apply(RefinementOp::SetInclude {
            item_id: ItemId::Package("grub2-efi-aa64-cdboot.aarch64".into()),
            include: true,
        });

        // Verify the op did not change the view state (not just export)
        let view = session.view();
        let view_pkg = view.rpm_packages().iter()
            .find(|p| p.entry.name == "grub2-efi-aa64-cdboot")
            .unwrap();
        assert!(!view_pkg.entry.include, "view: locked package must stay excluded");
        assert!(view_pkg.entry.locked, "view: locked flag must be preserved");

        // Verify the op did not land in the op stack
        // (adapt to however the session exposes op count or op list)

        // Verify export also reflects the no-op
        let exported = session.export();
        let exp_pkg = exported.rpm.unwrap().packages_added
            .iter()
            .find(|p| p.name == "grub2-efi-aa64-cdboot")
            .unwrap();
        assert!(!exp_pkg.include, "export: locked package must stay excluded");
        assert!(exp_pkg.locked, "export: locked flag must be preserved");
    }
```

Note: adapt the test to match the actual `RefineSession::new()` and `session.apply()` signatures in the codebase. The existing tests in session.rs (around line 2804+) show the pattern.

- [ ] **Step 2: Run the session test**

Run: `cargo test -p inspectah-refine locked_platform_plumbing_package -- --nocapture`
Expected: PASS

- [ ] **Step 3: Write web API boundary lock test**

Add a test in `crates/web/src/` (find the existing API test file — likely `crates/web/src/handlers.rs` or a test module). This test sends a `POST /api/op` with a `SetInclude(true)` for a locked package and verifies the response shows the package still excluded:

```rust
    #[tokio::test]
    async fn locked_package_set_include_no_op_via_api() {
        // Build a snapshot with a locked platform-plumbing package
        // Start the refine server with this snapshot
        // POST /api/op with SetInclude { item_id: Package("grub2-efi-aa64-cdboot.aarch64"), include: true }
        // GET /api/view and verify the package is still include: false, locked: true
        // GET /api/export and verify the package is not in the Containerfile
    }
```

Note: adapt to the actual web test harness in the codebase. Both the session-layer and API-layer tests are required — the API test is not optional defense-in-depth, it is a mandatory contract proof. If no web API test harness exists yet, create one for this test — the locked-item contract at the API boundary is a spec requirement.

- [ ] **Step 4: Run all tests**

Run: `cargo test --workspace -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/refine/src/session.rs crates/web/src/
git commit -m "test(refine): verify locked platform-plumbing rejects SetInclude at session and API layers"
```

---

### Task 10: Thorn checkpoint

**Pause for review before proceeding.** At this point all implementation tasks are complete. The checkpoint verifies:

- [ ] **Step 1: Run full test suite**

Run: `cargo test --workspace`
Expected: All pass.

- [ ] **Step 2: Run clippy**

Run: `cargo clippy --workspace -- -W clippy::all`
Expected: Zero warnings.

- [ ] **Step 3: Run `cargo fmt --check`**

Run: `cargo fmt --check`
Expected: No formatting issues.

- [ ] **Step 4: Verify Containerfile output with existing tarballs**

Run inspectah refine against one of the test tarballs and verify that platform-plumbing packages no longer appear in the Containerfile, installer-noise packages are excluded, and ambiguous packages are included:

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
cargo run -- refine /Users/mrussell/Work/bootc-migration/tarz/web-01-20260610-194748.tar.gz
```

In the refine UI, check:
- `grub2-efi-aa64-cdboot` is Baseline/locked (not toggleable)
- `google-noto-sans-vf-fonts` is Baseline (toggleable, excluded by default)
- `cronie` is Investigate (included by default)

Export and verify:
- Containerfile does NOT contain `grub2-efi-aa64-cdboot` or font packages (excluded)
- Containerfile DOES contain `cronie` and any other Tier 4 ambiguous packages (included by default)
- If firewalld has custom config on the test tarball, verify it appears in the Containerfile (promoted)
- The exported tarball round-trips cleanly: re-import into refine, verify classifications persist

- [ ] **Step 5: Verify TS types are updated**

Check `crates/web/ui/src/api/types.ts` for the `TriageReason` union type. Confirm all six new snake_case strings are present:
- `package_platform_plumbing`
- `package_installer_default`
- `package_installer_promoted_service`
- `package_installer_promoted_config`
- `package_installer_ambiguous`
- `package_installer_evidence_unavailable`

If the TS union is auto-generated, verify the generation. If hand-maintained, confirm the update was included in Task 2 Step 4.

- [ ] **Step 6: Review git log**

Run: `git log --oneline -10`
Verify commits are clean and follow conventional format.
