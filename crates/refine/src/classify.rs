use crate::types::{
    RefinedConfig, RefinedDropIn, RefinedFlatpak, RefinedPackage, RefinedQuadlet,
    RefinedServiceState, RefinedSysctl, RefinedTunedSelection, Triage, TriageAnnotation,
    TriageBucket, TriageReason, TriageTag,
};
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::config::ConfigFileKind;
use inspectah_core::types::kernelboot::SysctlOverride;
use inspectah_core::types::redaction::RedactionState;
use inspectah_core::types::rpm::{
    PackageEntry, PackageState, VersionChange, VersionChangeDirection,
};
use inspectah_core::types::services::{
    PresetDefault, ServiceStateChange, ServiceUnitState, SystemdDropIn,
};

const SENSITIVE_PATHS: &[&str] = &[
    "/etc/shadow",
    "/etc/gshadow",
    "/etc/ssh/",
    "/etc/pki/",
    "/etc/ssl/",
    "/etc/security/",
];

fn is_sensitive_path(path: &str) -> bool {
    SENSITIVE_PATHS.iter().any(|s| path.starts_with(s))
}

/// Well-known OS-default path prefixes in sensitive directories.
///
/// Files at these paths are delivered by standard RPM packages (ca-certificates,
/// openssh-server, pam, openssl, fwupd, nss-tools, etc.) as regular files, not
/// `%config`. The config inspector classifies them as `Unowned` because they
/// aren't tracked by `rpm -Vc`, but on a stock system they are package defaults.
///
/// Suppresses the SensitivePath annotation so these stock files don't add noise.
const OS_DEFAULT_SENSITIVE_PREFIXES: &[&str] = &[
    // ca-certificates, p11-kit-trust
    "/etc/pki/ca-trust/",
    // centos-stream-release, redhat-release
    "/etc/pki/rpm-gpg/",
    // fwupd
    "/etc/pki/fwupd/",
    "/etc/pki/fwupd-metadata/",
    // nss-softokn, nss-tools
    "/etc/pki/nssdb/",
    // openssl
    "/etc/ssl/",
    // pam
    "/etc/security/",
    // openssh-server, openssh-clients
    "/etc/ssh/ssh_config.d/",
    "/etc/ssh/sshd_config.d/",
];

/// Exact paths that are OS defaults in sensitive directories.
const OS_DEFAULT_SENSITIVE_EXACT: &[&str] = &["/etc/ssh/moduli", "/etc/ssh/ssh_config"];

/// Returns true when the path is a well-known OS default inside a sensitive
/// directory. These files should NOT get a SensitivePath annotation.
fn is_os_default_sensitive(path: &str) -> bool {
    OS_DEFAULT_SENSITIVE_PREFIXES
        .iter()
        .any(|p| path.starts_with(p))
        || OS_DEFAULT_SENSITIVE_EXACT.contains(&path)
}

/// Boot-chain packages that conflict with bootc's bootloader management.
/// These are unconditionally excluded and locked.
const PLATFORM_PLUMBING_PREFIXES: &[&str] = &["grub2-", "grubby", "shim-", "efibootmgr"];

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
const CONFIG_ONLY_PROMOTION: &[&str] = &["sudo", "logrotate", "chrony", "sssd", "pam"];

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

/// Reclassify anaconda-sourced packages that survived baseline subtraction.
/// Runs as a post-pass after the main classify_packages logic.
fn apply_anaconda_classification(packages: &mut [RefinedPackage], snap: &InspectionSnapshot) {
    let user_enabled_service_packages = build_user_enabled_service_set(snap);
    let modified_config_packages = build_modified_config_set(snap);
    let has_services = snap.services.is_some();
    let has_config = snap.config.is_some()
        && snap
            .rpm
            .as_ref()
            .is_some_and(|r| !r.file_ownership.is_empty());

    // Detect per-package evidence degradation: services with missing
    // owning_package or modified configs without a file_ownership join.
    // When degraded, Tier 3 (noise exclusion) is unsafe because we
    // can't rule out that the package has a promotable service/config
    // we simply failed to attribute.
    let has_unattributed_services = snap
        .services
        .as_ref()
        .is_some_and(|svc| svc.state_changes.iter().any(|s| s.owning_package.is_none()));
    let has_orphaned_modified_configs = has_config
        && snap.config.as_ref().is_some_and(|config| {
            let owned_paths: std::collections::HashSet<&str> = snap
                .rpm
                .as_ref()
                .map(|r| {
                    r.file_ownership
                        .iter()
                        .flat_map(|o| o.paths.iter().map(|p| p.as_str()))
                        .collect()
                })
                .unwrap_or_default();
            config.files.iter().any(|f| {
                f.kind == ConfigFileKind::RpmOwnedModified && !owned_paths.contains(f.path.as_str())
            })
        });
    let evidence_degraded = has_unattributed_services || has_orphaned_modified_configs;

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

        // Tier 2 Path A: dual-signal promotion (user-enabled service + modified config)
        if user_enabled_service_packages.contains(name.as_str())
            && modified_config_packages.contains(name.as_str())
        {
            pkg.entry.include = true;
            pkg.triage = TriageTag {
                triage: Triage::SingleHost(TriageBucket::Site),
                primary_reason: TriageReason::PackageInstallerPromotedService,
                annotations: vec![],
            };
            continue;
        }

        // Tier 2 Path B: config-only promotion (curated list)
        if is_config_only_promotable(name) && modified_config_packages.contains(name.as_str()) {
            pkg.entry.include = true;
            pkg.triage = TriageTag {
                triage: Triage::SingleHost(TriageBucket::Site),
                primary_reason: TriageReason::PackageInstallerPromotedConfig,
                annotations: vec![],
            };
            continue;
        }

        // Tier 3: installer noise — only safe when evidence is complete.
        // When evidence is degraded (missing owning_package or orphaned
        // modified configs), we can't rule out that this package has a
        // promotable service/config we failed to attribute. Fall to
        // Tier 4 (include=true) instead of excluding.
        if is_installer_noise(name) && !evidence_degraded {
            pkg.entry.include = false;
            pkg.triage = TriageTag {
                triage: Triage::SingleHost(TriageBucket::Baseline),
                primary_reason: TriageReason::PackageInstallerDefault,
                annotations: vec![],
            };
            continue;
        }

        // Tier 4: ambiguous — may be group-install or kickstart intent.
        // Also catches noise-pattern packages when evidence is degraded.
        pkg.entry.include = true;
        pkg.triage = TriageTag {
            triage: Triage::SingleHost(TriageBucket::Investigate),
            primary_reason: if evidence_degraded && is_installer_noise(name) {
                TriageReason::PackageInstallerEvidenceUnavailable
            } else {
                TriageReason::PackageInstallerAmbiguous
            },
            annotations: vec![],
        };
    }
}

fn build_user_enabled_service_set(snap: &InspectionSnapshot) -> std::collections::HashSet<&str> {
    let mut set = std::collections::HashSet::new();
    if let Some(services) = &snap.services {
        for svc in &services.state_changes {
            if svc.current_state == ServiceUnitState::Enabled
                && svc.default_state != Some(PresetDefault::Enable)
                && let Some(pkg) = &svc.owning_package
            {
                set.insert(pkg.as_str());
            }
        }
    }
    set
}

fn build_modified_config_set(snap: &InspectionSnapshot) -> std::collections::HashSet<&str> {
    let mut set = std::collections::HashSet::new();
    if let Some(config) = &snap.config
        && let Some(rpm) = &snap.rpm
    {
        for ownership in &rpm.file_ownership {
            for config_file in &config.files {
                if config_file.kind == ConfigFileKind::RpmOwnedModified
                    && ownership.paths.contains(&config_file.path)
                {
                    set.insert(ownership.package_name.as_str());
                }
            }
        }
    }
    set
}

pub fn classify_packages(snap: &InspectionSnapshot) -> Vec<RefinedPackage> {
    let rpm = match &snap.rpm {
        Some(r) => r,
        None => return Vec::new(),
    };

    let baseline_names: Option<Vec<String>> = snap
        .baseline
        .as_ref()
        .map(|b| b.packages.keys().cloned().collect());
    let baseline: Option<&[String]> = baseline_names.as_deref();

    // Build baseline_suppressed set for fast lookup
    let suppressed_set: std::collections::HashSet<&str> = rpm
        .baseline_suppressed
        .as_ref()
        .map(|v| v.iter().map(|s| s.as_str()).collect())
        .unwrap_or_default();

    // Build config path set for @commandline auto-exclude check.
    let config_paths: std::collections::HashSet<&str> = snap
        .config
        .as_ref()
        .map(|c| c.files.iter().map(|f| f.path.as_str()).collect())
        .unwrap_or_default();

    // Build package -> owned paths map from file_ownership.
    let ownership_map: std::collections::HashMap<&str, &[String]> = rpm
        .file_ownership
        .iter()
        .map(|e| (e.package_name.as_str(), e.paths.as_slice()))
        .collect();

    let mut result: Vec<RefinedPackage> = rpm
        .packages_added
        .iter()
        .map(|entry| {
            let canonical_id = format!("{}.{}", entry.name, entry.arch);

            if suppressed_set.contains(canonical_id.as_str()) {
                return RefinedPackage {
                    entry: entry.clone(),
                    triage: TriageTag {
                        triage: Triage::SingleHost(TriageBucket::Baseline),
                        primary_reason: TriageReason::PackageBaselineMatch,
                        annotations: Vec::new(),
                    },
                };
            }

            let mut tag = classify_package(entry, baseline, &rpm.version_changes);

            if is_sensitive_path(&entry.name) {
                tag.annotations.push(TriageAnnotation::SensitivePath);
            }

            let mut refined = RefinedPackage {
                entry: entry.clone(),
                triage: tag,
            };

            // Auto-exclude @commandline packages whose files are all config-captured.
            if entry.source_repo.eq_ignore_ascii_case("@commandline")
                && let Some(owned_paths) = ownership_map.get(entry.name.as_str())
                && !owned_paths.is_empty()
                && owned_paths
                    .iter()
                    .all(|p| p.starts_with("/etc/") && config_paths.contains(p.as_str()))
            {
                refined.entry.include = false;
                refined.triage = TriageTag {
                    triage: Triage::SingleHost(TriageBucket::Site),
                    primary_reason: TriageReason::PackageConfigCaptured,
                    annotations: refined.triage.annotations,
                };
            }

            refined
        })
        .collect();

    apply_anaconda_classification(&mut result, snap);

    result
}

fn classify_package(
    entry: &PackageEntry,
    baseline: Option<&[String]>,
    version_changes: &[VersionChange],
) -> TriageTag {
    // LocalInstall and NoRepo are always Investigate, regardless of baseline or repo.
    match entry.state {
        PackageState::LocalInstall => {
            return TriageTag {
                triage: Triage::SingleHost(TriageBucket::Investigate),
                primary_reason: TriageReason::PackageLocalInstall,
                annotations: Vec::new(),
            };
        }
        PackageState::NoRepo => {
            return TriageTag {
                triage: Triage::SingleHost(TriageBucket::Investigate),
                primary_reason: TriageReason::PackageNoRepoSource,
                annotations: Vec::new(),
            };
        }
        _ => {}
    }

    // Empty source_repo or @commandline means unknown provenance — always Investigate.
    if entry.source_repo.is_empty() || entry.source_repo.eq_ignore_ascii_case("@commandline") {
        return TriageTag {
            triage: Triage::SingleHost(TriageBucket::Investigate),
            primary_reason: TriageReason::PackageNoRepoSource,
            annotations: Vec::new(),
        };
    }

    // Modified packages: check version change direction.
    // Upgrades are normal maintenance (Site). Downgrades need investigation.
    if entry.state == PackageState::Modified {
        return match baseline {
            Some(_) => {
                let is_downgrade = version_changes.iter().any(|vc| {
                    vc.name == entry.name
                        && vc.arch == entry.arch
                        && vc.direction == VersionChangeDirection::Downgrade
                });
                if is_downgrade {
                    TriageTag {
                        triage: Triage::SingleHost(TriageBucket::Investigate),
                        primary_reason: TriageReason::PackageVersionChanged,
                        annotations: Vec::new(),
                    }
                } else {
                    TriageTag {
                        triage: Triage::SingleHost(TriageBucket::Site),
                        primary_reason: TriageReason::PackageVersionChanged,
                        annotations: Vec::new(),
                    }
                }
            }
            None => TriageTag {
                triage: Triage::SingleHost(TriageBucket::Investigate),
                primary_reason: TriageReason::PackageProvenanceUnavailable,
                annotations: Vec::new(),
            },
        };
    }

    // Classify based on baseline availability and membership (Added/BaseImageOnly only).
    match baseline {
        Some(names)
            if names.iter().any(|n| {
                let entry_key = format!("{}.{}", entry.name, entry.arch);
                n == &entry_key
            }) =>
        {
            // In baseline with known repo — expected package, Baseline.
            TriageTag {
                triage: Triage::SingleHost(TriageBucket::Baseline),
                primary_reason: TriageReason::PackageBaselineMatch,
                annotations: Vec::new(),
            }
        }
        Some(_) => {
            // Not in baseline but has a known repo — user-added, Site.
            TriageTag {
                triage: Triage::SingleHost(TriageBucket::Site),
                primary_reason: TriageReason::PackageUserAdded,
                annotations: Vec::new(),
            }
        }
        None => {
            // No baseline available — can't determine provenance, Investigate.
            TriageTag {
                triage: Triage::SingleHost(TriageBucket::Investigate),
                primary_reason: TriageReason::PackageProvenanceUnavailable,
                annotations: Vec::new(),
            }
        }
    }
}

pub fn classify_configs(snap: &InspectionSnapshot) -> Vec<RefinedConfig> {
    let config = match &snap.config {
        Some(c) => c,
        None => return Vec::new(),
    };

    let mut configs: Vec<RefinedConfig> = config
        .files
        .iter()
        .map(|entry| {
            let (bucket, reason) = match entry.kind {
                ConfigFileKind::RpmOwnedDefault => {
                    (TriageBucket::Baseline, TriageReason::ConfigDefault)
                }
                ConfigFileKind::BaselineMatch => {
                    (TriageBucket::Baseline, TriageReason::ConfigBaselineMatch)
                }
                ConfigFileKind::Unowned => (TriageBucket::Site, TriageReason::ConfigUnowned),
                ConfigFileKind::RpmOwnedModified => {
                    (TriageBucket::Site, TriageReason::ConfigModified)
                }
                ConfigFileKind::Orphaned => (TriageBucket::Site, TriageReason::ConfigOrphaned),
            };

            let mut annotations = Vec::new();
            // Sensitive path annotation: add to items in sensitive directories.
            // OS-default files in sensitive directories are NOT annotated —
            // they are stock package contents, not meaningful user customizations.
            if is_sensitive_path(&entry.path) && !is_os_default_sensitive(&entry.path) {
                annotations.push(TriageAnnotation::SensitivePath);
            }

            RefinedConfig {
                entry: entry.clone(),
                triage: TriageTag {
                    triage: Triage::SingleHost(bucket),
                    primary_reason: reason,
                    annotations,
                },
            }
        })
        .collect();

    // Surface unresolved redaction hints as needs-investigation on matching
    // config files. Applies to PartiallyRedacted and SensitiveRetained.
    if let Some(
        RedactionState::PartiallyRedacted {
            ref unresolved_hints,
            ..
        }
        | RedactionState::SensitiveRetained {
            ref unresolved_hints,
            ..
        },
    ) = snap.redaction_state
    {
        for hint in unresolved_hints {
            if let Some(cfg) = configs.iter_mut().find(|c| c.entry.path == hint.path) {
                cfg.triage = TriageTag {
                    triage: Triage::SingleHost(TriageBucket::Investigate),
                    primary_reason: TriageReason::Custom("unresolved redaction hint".into()),
                    annotations: cfg.triage.annotations.clone(),
                };
            }
        }
    }

    configs
}

/// Classify service state changes and drop-ins into triage buckets.
///
/// All known services classify as Site (baseline data for non-package
/// items does not exist yet). Services without an owning package
/// classify as Investigate.
pub fn classify_services(
    snap: &InspectionSnapshot,
) -> (Vec<RefinedServiceState>, Vec<RefinedDropIn>) {
    let services = match &snap.services {
        Some(s) => s,
        None => return (Vec::new(), Vec::new()),
    };
    let states: Vec<RefinedServiceState> = services
        .state_changes
        .iter()
        .map(|change| RefinedServiceState {
            entry: change.clone(),
            triage: classify_service(change),
        })
        .collect();
    let dropins: Vec<RefinedDropIn> = services
        .drop_ins
        .iter()
        .map(|dropin| RefinedDropIn {
            entry: dropin.clone(),
            triage: classify_dropin(dropin),
        })
        .collect();
    (states, dropins)
}

fn classify_service(change: &ServiceStateChange) -> TriageTag {
    if change.owning_package.is_none() {
        return TriageTag {
            triage: Triage::SingleHost(TriageBucket::Investigate),
            primary_reason: TriageReason::ServiceUnknownOrigin,
            annotations: vec![],
        };
    }
    TriageTag {
        triage: Triage::SingleHost(TriageBucket::Site),
        primary_reason: TriageReason::ServiceNonDefaultState,
        annotations: vec![],
    }
}

fn classify_dropin(_dropin: &SystemdDropIn) -> TriageTag {
    TriageTag {
        triage: Triage::SingleHost(TriageBucket::Site),
        primary_reason: TriageReason::ServiceDropInPresent,
        annotations: vec![],
    }
}

/// Classify quadlet units and flatpak apps into triage buckets.
///
/// Quadlets are always Site (user-deployed container workloads).
/// Flatpaks with complete provenance (non-empty remote and branch) are Site
/// with a FirstBootProvisioned annotation. Flatpaks missing remote or branch
/// are Investigate with FlatpakIncompleteProvenance.
pub fn classify_containers(
    snap: &InspectionSnapshot,
) -> (Vec<RefinedQuadlet>, Vec<RefinedFlatpak>) {
    let containers = match &snap.containers {
        Some(c) => c,
        None => return (Vec::new(), Vec::new()),
    };

    let quadlets: Vec<RefinedQuadlet> = containers
        .quadlet_units
        .iter()
        .map(|unit| RefinedQuadlet {
            entry: unit.clone(),
            triage: TriageTag {
                triage: Triage::SingleHost(TriageBucket::Site),
                primary_reason: TriageReason::QuadletUserDeployed,
                annotations: vec![],
            },
        })
        .collect();

    let flatpaks: Vec<RefinedFlatpak> = containers
        .flatpak_apps
        .iter()
        .map(|app| {
            if app.remote.is_empty() || app.branch.is_empty() {
                RefinedFlatpak {
                    entry: app.clone(),
                    triage: TriageTag {
                        triage: Triage::SingleHost(TriageBucket::Investigate),
                        primary_reason: TriageReason::FlatpakIncompleteProvenance,
                        annotations: vec![],
                    },
                }
            } else {
                RefinedFlatpak {
                    entry: app.clone(),
                    triage: TriageTag {
                        triage: Triage::SingleHost(TriageBucket::Site),
                        primary_reason: TriageReason::FlatpakProvisionedOnFirstBoot,
                        annotations: vec![TriageAnnotation::FirstBootProvisioned],
                    },
                }
            }
        })
        .collect();

    (quadlets, flatpaks)
}

/// Sysctl keys that produce transient or non-reproducible effects and should
/// be excluded from migration output.
const SYSCTL_DENY_LIST: &[&str] = &["vm.drop_caches", "vm.compact_memory", "kernel.sysrq"];

/// Classify sysctl overrides into triage buckets.
///
/// Only file-backed overrides (source under `/etc/sysctl.d/` or `/etc/sysctl.conf`)
/// are promoted to Site. Runtime-only observations also get Site classification
/// but carry a `RuntimeOnlyObservation` annotation so the frontend can present
/// them differently. Deny-listed keys are excluded entirely.
pub fn classify_sysctls(snap: &InspectionSnapshot) -> Vec<RefinedSysctl> {
    let kernel_boot = match &snap.kernel_boot {
        Some(kb) => kb,
        None => return Vec::new(),
    };

    kernel_boot
        .sysctl_overrides
        .iter()
        .filter(|s| !SYSCTL_DENY_LIST.contains(&s.key.as_str()))
        .map(|s| RefinedSysctl {
            entry: s.clone(),
            triage: classify_single_sysctl(s),
        })
        .collect()
}

fn is_file_backed(source: &str) -> bool {
    source.starts_with("/etc/sysctl.d/") || source == "/etc/sysctl.conf"
}

fn classify_single_sysctl(s: &SysctlOverride) -> TriageTag {
    let file_backed = is_file_backed(&s.source);

    let reason = if file_backed {
        TriageReason::SysctlFileBackedOverride
    } else {
        TriageReason::SysctlNoBaseline
    };

    let annotations = if file_backed {
        vec![]
    } else {
        vec![TriageAnnotation::RuntimeOnlyObservation]
    };

    TriageTag {
        triage: Triage::SingleHost(TriageBucket::Site),
        primary_reason: reason,
        annotations,
    }
}

/// Classify tuned profile selection into triage buckets.
///
/// Produces at most one `RefinedTunedSelection` per host, bundling the active
/// profile name and any custom profile file paths. Returns empty if tuned is
/// not active (empty `tuned_active` field).
///
/// Classification rules:
/// - Non-default profile or custom profiles present → **Site**
/// - Tuned active but package missing from RPM list → **Investigate** +
///   `RequiresProjectedPackage { name: "tuned" }` annotation
/// - Default profile with no custom profiles → **Site** (baseline deferred)
pub fn classify_tuned(snap: &InspectionSnapshot) -> Vec<RefinedTunedSelection> {
    let kernel_boot = match &snap.kernel_boot {
        Some(kb) => kb,
        None => return Vec::new(),
    };

    let active = &kernel_boot.tuned_active;
    if active.is_empty() {
        return Vec::new();
    }

    let custom_paths: Vec<String> = kernel_boot
        .tuned_custom_profiles
        .iter()
        .map(|c| c.path.clone())
        .collect();

    // Check whether the tuned RPM is installed.
    let tuned_pkg_installed = snap
        .rpm
        .as_ref()
        .map(|rpm| rpm.packages_added.iter().any(|p| p.name == "tuned"))
        .unwrap_or(false);

    let (reason, bucket, annotations) = if !tuned_pkg_installed {
        // Tuned service active but package not in the RPM list — unusual state.
        (
            TriageReason::TunedUnusualState,
            TriageBucket::Investigate,
            vec![TriageAnnotation::RequiresProjectedPackage {
                name: "tuned".to_string(),
            }],
        )
    } else if !custom_paths.is_empty() {
        (TriageReason::TunedCustomProfile, TriageBucket::Site, vec![])
    } else if active != "virtual-guest" && active != "balanced" {
        // Non-default profile.
        (
            TriageReason::TunedNonDefaultProfile,
            TriageBucket::Site,
            vec![],
        )
    } else {
        // Default profile, no custom profiles — Site until baseline comparison
        // is implemented.
        (
            TriageReason::TunedNonDefaultProfile,
            TriageBucket::Site,
            vec![],
        )
    };

    vec![RefinedTunedSelection {
        active_profile: active.clone(),
        custom_profiles: custom_paths,
        triage: TriageTag {
            triage: Triage::SingleHost(bucket),
            primary_reason: reason,
            annotations,
        },
        include: kernel_boot.tuned_include,
    }]
}

#[cfg(test)]
#[allow(clippy::needless_update)]
mod tests {
    use super::*;
    use inspectah_core::baseline::{BaselineData, BaselinePackageEntry};
    use inspectah_core::types::rpm::{RpmSection, VersionChange, VersionChangeDirection};

    fn pkg(name: &str, state: PackageState, source_repo: &str) -> PackageEntry {
        PackageEntry {
            name: name.to_string(),
            arch: "x86_64".to_string(),
            state,
            source_repo: source_repo.to_string(),
            ..Default::default()
        }
    }

    fn vc(name: &str, direction: VersionChangeDirection) -> VersionChange {
        VersionChange {
            name: name.to_string(),
            arch: "x86_64".to_string(),
            direction,
            ..Default::default()
        }
    }

    fn vc_arch(name: &str, arch: &str, direction: VersionChangeDirection) -> VersionChange {
        VersionChange {
            name: name.to_string(),
            arch: arch.to_string(),
            direction,
            ..Default::default()
        }
    }

    fn pkg_arch(name: &str, arch: &str, state: PackageState, source_repo: &str) -> PackageEntry {
        PackageEntry {
            name: name.to_string(),
            arch: arch.to_string(),
            state,
            source_repo: source_repo.to_string(),
            ..Default::default()
        }
    }

    fn snap_with_baseline(
        baseline_names: Option<Vec<String>>,
        packages: Vec<PackageEntry>,
    ) -> InspectionSnapshot {
        snap_with_baseline_and_vc(baseline_names, packages, vec![])
    }

    fn snap_with_baseline_and_vc(
        baseline_names: Option<Vec<String>>,
        packages: Vec<PackageEntry>,
        version_changes: Vec<VersionChange>,
    ) -> InspectionSnapshot {
        let baseline = baseline_names.map(|names| {
            let pkgs = names
                .into_iter()
                .map(|n| {
                    let key = format!("{}.x86_64", n);
                    let entry = BaselinePackageEntry {
                        name: n,
                        epoch: Some("0".to_string()),
                        version: "1.0".to_string(),
                        release: "1.el9".to_string(),
                        arch: "x86_64".to_string(),
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
                version_changes,
                ..Default::default()
            }),
            baseline,
            ..Default::default()
        }
    }

    fn assert_bucket(tag: &TriageTag, expected: TriageBucket) {
        match &tag.triage {
            Triage::SingleHost(b) => assert_eq!(*b, expected, "bucket mismatch"),
            Triage::Aggregate(_) => panic!("expected SingleHost, got Aggregate"),
        }
    }

    #[test]
    fn verified_added_in_baseline_is_baseline() {
        let snap = snap_with_baseline(
            Some(vec!["glibc".into()]),
            vec![pkg("glibc", PackageState::Added, "rhel-9-baseos")],
        );
        let result = classify_packages(&snap);
        assert_eq!(result.len(), 1);
        assert_bucket(&result[0].triage, TriageBucket::Baseline);
        assert_eq!(
            result[0].triage.primary_reason,
            TriageReason::PackageBaselineMatch
        );
    }

    #[test]
    fn verified_added_not_in_baseline_recognized_repo_is_site() {
        let snap = snap_with_baseline(
            Some(vec!["glibc".into()]),
            vec![pkg("httpd", PackageState::Added, "rhel-9-appstream")],
        );
        let result = classify_packages(&snap);
        assert_eq!(result.len(), 1);
        assert_bucket(&result[0].triage, TriageBucket::Site);
        assert_eq!(
            result[0].triage.primary_reason,
            TriageReason::PackageUserAdded
        );
    }

    #[test]
    fn verified_added_no_repo_is_investigate() {
        let snap = snap_with_baseline(
            Some(vec!["glibc".into()]),
            vec![pkg("mystery", PackageState::Added, "")],
        );
        let result = classify_packages(&snap);
        assert_eq!(result.len(), 1);
        assert_bucket(&result[0].triage, TriageBucket::Investigate);
        assert_eq!(
            result[0].triage.primary_reason,
            TriageReason::PackageNoRepoSource
        );
    }

    #[test]
    fn verified_modified_upgrade_is_site() {
        let snap = snap_with_baseline_and_vc(
            Some(vec!["kernel".into()]),
            vec![pkg("kernel", PackageState::Modified, "rhel-9-baseos")],
            vec![vc("kernel", VersionChangeDirection::Upgrade)],
        );
        let result = classify_packages(&snap);
        assert_eq!(result.len(), 1);
        assert_bucket(&result[0].triage, TriageBucket::Site);
        assert_eq!(
            result[0].triage.primary_reason,
            TriageReason::PackageVersionChanged
        );
    }

    #[test]
    fn verified_modified_downgrade_is_investigate() {
        let snap = snap_with_baseline_and_vc(
            Some(vec!["kernel".into()]),
            vec![pkg("kernel", PackageState::Modified, "rhel-9-baseos")],
            vec![vc("kernel", VersionChangeDirection::Downgrade)],
        );
        let result = classify_packages(&snap);
        assert_eq!(result.len(), 1);
        assert_bucket(&result[0].triage, TriageBucket::Investigate);
        assert_eq!(
            result[0].triage.primary_reason,
            TriageReason::PackageVersionChanged
        );
    }

    #[test]
    fn verified_modified_no_version_change_entry_defaults_to_site() {
        let snap = snap_with_baseline(
            Some(vec!["kernel".into()]),
            vec![pkg("kernel", PackageState::Modified, "rhel-9-baseos")],
        );
        let result = classify_packages(&snap);
        assert_eq!(result.len(), 1);
        assert_bucket(&result[0].triage, TriageBucket::Site);
        assert_eq!(
            result[0].triage.primary_reason,
            TriageReason::PackageVersionChanged
        );
    }

    #[test]
    fn verified_modified_not_in_baseline_upgrade_is_site() {
        let snap = snap_with_baseline_and_vc(
            Some(vec!["glibc".into()]),
            vec![pkg("kernel", PackageState::Modified, "rhel-9-baseos")],
            vec![vc("kernel", VersionChangeDirection::Upgrade)],
        );
        let result = classify_packages(&snap);
        assert_eq!(result.len(), 1);
        assert_bucket(&result[0].triage, TriageBucket::Site);
        assert_eq!(
            result[0].triage.primary_reason,
            TriageReason::PackageVersionChanged
        );
    }

    #[test]
    fn verified_modified_no_repo_is_investigate() {
        let snap = snap_with_baseline(
            Some(vec!["glibc".into()]),
            vec![pkg("kernel", PackageState::Modified, "")],
        );
        let result = classify_packages(&snap);
        assert_eq!(result.len(), 1);
        assert_bucket(&result[0].triage, TriageBucket::Investigate);
        assert_eq!(
            result[0].triage.primary_reason,
            TriageReason::PackageNoRepoSource
        );
    }

    #[test]
    fn verified_local_install_is_investigate() {
        let snap = snap_with_baseline(
            Some(vec!["glibc".into()]),
            vec![pkg("custom-tool", PackageState::LocalInstall, "")],
        );
        let result = classify_packages(&snap);
        assert_eq!(result.len(), 1);
        assert_bucket(&result[0].triage, TriageBucket::Investigate);
        assert_eq!(
            result[0].triage.primary_reason,
            TriageReason::PackageLocalInstall
        );
    }

    #[test]
    fn verified_no_repo_state_is_investigate() {
        let snap = snap_with_baseline(
            Some(vec!["glibc".into()]),
            vec![pkg("orphan-pkg", PackageState::NoRepo, "some-repo")],
        );
        let result = classify_packages(&snap);
        assert_eq!(result.len(), 1);
        assert_bucket(&result[0].triage, TriageBucket::Investigate);
        assert_eq!(
            result[0].triage.primary_reason,
            TriageReason::PackageNoRepoSource
        );
    }

    #[test]
    fn snap_baseline_field_drives_verified_mode() {
        use std::collections::HashMap;
        let mut pkgs = HashMap::new();
        pkgs.insert(
            "glibc.x86_64".to_string(),
            BaselinePackageEntry {
                name: "glibc".to_string(),
                epoch: Some("0".to_string()),
                version: "2.34".to_string(),
                release: "83.el9".to_string(),
                arch: "x86_64".to_string(),
            },
        );
        let snap = InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            rpm: Some(RpmSection {
                packages_added: vec![pkg("glibc", PackageState::Added, "rhel-9-baseos")],
                ..Default::default()
            }),
            baseline: Some(BaselineData {
                image_digest: "sha256:abc".to_string(),
                packages: pkgs,
                extracted_at: "2026-01-01T00:00:00Z".to_string(),
            }),
            ..Default::default()
        };
        let result = classify_packages(&snap);
        assert_eq!(result.len(), 1);
        assert_bucket(&result[0].triage, TriageBucket::Baseline);
        assert_eq!(
            result[0].triage.primary_reason,
            TriageReason::PackageBaselineMatch
        );
    }

    #[test]
    fn degraded_added_is_investigate_provenance_unavailable() {
        let snap = snap_with_baseline(
            None,
            vec![pkg("httpd", PackageState::Added, "rhel-9-appstream")],
        );
        let result = classify_packages(&snap);
        assert_eq!(result.len(), 1);
        assert_bucket(&result[0].triage, TriageBucket::Investigate);
        assert_eq!(
            result[0].triage.primary_reason,
            TriageReason::PackageProvenanceUnavailable
        );
    }

    #[test]
    fn degraded_local_install_still_investigate() {
        let snap = snap_with_baseline(
            None,
            vec![pkg("custom-tool", PackageState::LocalInstall, "")],
        );
        let result = classify_packages(&snap);
        assert_eq!(result.len(), 1);
        assert_bucket(&result[0].triage, TriageBucket::Investigate);
        assert_eq!(
            result[0].triage.primary_reason,
            TriageReason::PackageLocalInstall
        );
    }

    #[test]
    fn degraded_no_repo_state_still_investigate() {
        let snap = snap_with_baseline(None, vec![pkg("orphan", PackageState::NoRepo, "some-repo")]);
        let result = classify_packages(&snap);
        assert_eq!(result.len(), 1);
        assert_bucket(&result[0].triage, TriageBucket::Investigate);
        assert_eq!(
            result[0].triage.primary_reason,
            TriageReason::PackageNoRepoSource
        );
    }

    #[test]
    fn commandline_source_repo_is_investigate() {
        let snap = snap_with_baseline(
            Some(vec!["glibc".into()]),
            vec![pkg(
                "rpmfusion-free-release",
                PackageState::Added,
                "@commandline",
            )],
        );
        let result = classify_packages(&snap);
        assert_eq!(result.len(), 1);
        assert_bucket(&result[0].triage, TriageBucket::Investigate);
        assert_eq!(
            result[0].triage.primary_reason,
            TriageReason::PackageNoRepoSource
        );
    }

    #[test]
    fn commandline_config_captured_auto_excludes() {
        use inspectah_core::types::config::{ConfigFileEntry as CfgEntry, ConfigSection};
        use inspectah_core::types::rpm::{FileOwnershipEntry, RpmSection};

        let snap = InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            rpm: Some(RpmSection {
                packages_added: vec![pkg(
                    "rpmfusion-free-release",
                    PackageState::Added,
                    "@commandline",
                )],
                file_ownership: vec![FileOwnershipEntry {
                    package_name: "rpmfusion-free-release".into(),
                    paths: vec![
                        "/etc/yum.repos.d/rpmfusion-free.repo".into(),
                        "/etc/pki/rpm-gpg/RPM-GPG-KEY-rpmfusion-free-el-9".into(),
                    ],
                }],
                ..Default::default()
            }),
            config: Some(ConfigSection {
                files: vec![
                    CfgEntry {
                        path: "/etc/yum.repos.d/rpmfusion-free.repo".into(),
                        include: true,
                        locked: false,
                        ..Default::default()
                    },
                    CfgEntry {
                        path: "/etc/pki/rpm-gpg/RPM-GPG-KEY-rpmfusion-free-el-9".into(),
                        include: true,
                        locked: false,
                        ..Default::default()
                    },
                ],
            }),
            ..Default::default()
        };

        let result = classify_packages(&snap);
        assert_eq!(result.len(), 1);
        assert!(!result[0].entry.include, "should be auto-excluded");
        assert_eq!(
            result[0].triage.primary_reason,
            TriageReason::PackageConfigCaptured,
            "should have PackageConfigCaptured reason"
        );
    }

    #[test]
    fn commandline_with_non_etc_file_not_auto_excluded() {
        use inspectah_core::types::config::{ConfigFileEntry as CfgEntry, ConfigSection};
        use inspectah_core::types::rpm::{FileOwnershipEntry, RpmSection};

        let snap = InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            rpm: Some(RpmSection {
                packages_added: vec![pkg("epel-release", PackageState::Added, "@commandline")],
                file_ownership: vec![FileOwnershipEntry {
                    package_name: "epel-release".into(),
                    paths: vec!["/etc/yum.repos.d/epel.repo".into(), "/usr/bin/crb".into()],
                }],
                ..Default::default()
            }),
            config: Some(ConfigSection {
                files: vec![CfgEntry {
                    path: "/etc/yum.repos.d/epel.repo".into(),
                    include: true,
                    locked: false,
                    ..Default::default()
                }],
            }),
            ..Default::default()
        };

        let result = classify_packages(&snap);
        assert_eq!(result.len(), 1);
        assert_ne!(
            result[0].triage.primary_reason,
            TriageReason::PackageConfigCaptured,
            "should not be config-captured"
        );
    }

    #[test]
    fn commandline_source_repo_degraded_is_investigate() {
        let snap = snap_with_baseline(
            None,
            vec![pkg("custom-tool", PackageState::Added, "@commandline")],
        );
        let result = classify_packages(&snap);
        assert_eq!(result.len(), 1);
        assert_bucket(&result[0].triage, TriageBucket::Investigate);
        assert_eq!(
            result[0].triage.primary_reason,
            TriageReason::PackageNoRepoSource
        );
    }

    #[test]
    fn degraded_empty_source_repo_is_investigate() {
        let snap = snap_with_baseline(None, vec![pkg("mystery", PackageState::Added, "")]);
        let result = classify_packages(&snap);
        assert_eq!(result.len(), 1);
        assert_bucket(&result[0].triage, TriageBucket::Investigate);
        assert_eq!(
            result[0].triage.primary_reason,
            TriageReason::PackageNoRepoSource
        );
    }

    #[test]
    fn verified_mixed_packages_classification() {
        let snap = snap_with_baseline(
            Some(vec!["glibc".into(), "bash".into()]),
            vec![
                pkg("glibc", PackageState::Added, "rhel-9-baseos"),
                pkg("httpd", PackageState::Added, "rhel-9-appstream"),
                pkg("custom", PackageState::LocalInstall, ""),
                pkg("unknown", PackageState::Added, ""),
            ],
        );
        let result = classify_packages(&snap);
        assert_eq!(result.len(), 4);

        assert_bucket(&result[0].triage, TriageBucket::Baseline);
        assert_eq!(
            result[0].triage.primary_reason,
            TriageReason::PackageBaselineMatch
        );

        assert_bucket(&result[1].triage, TriageBucket::Site);
        assert_eq!(
            result[1].triage.primary_reason,
            TriageReason::PackageUserAdded
        );

        assert_bucket(&result[2].triage, TriageBucket::Investigate);
        assert_eq!(
            result[2].triage.primary_reason,
            TriageReason::PackageLocalInstall
        );

        assert_bucket(&result[3].triage, TriageBucket::Investigate);
        assert_eq!(
            result[3].triage.primary_reason,
            TriageReason::PackageNoRepoSource
        );
    }

    #[test]
    fn multiarch_downgrade_only_affects_matching_arch() {
        let snap = snap_with_baseline_and_vc(
            Some(vec!["openssl".into()]),
            vec![
                pkg_arch("openssl", "x86_64", PackageState::Modified, "rhel-9-baseos"),
                pkg_arch("openssl", "i686", PackageState::Modified, "rhel-9-baseos"),
            ],
            vec![
                vc_arch("openssl", "x86_64", VersionChangeDirection::Upgrade),
                vc_arch("openssl", "i686", VersionChangeDirection::Downgrade),
            ],
        );
        let result = classify_packages(&snap);
        assert_eq!(result.len(), 2);

        assert_bucket(&result[0].triage, TriageBucket::Site);
        assert_eq!(
            result[0].triage.primary_reason,
            TriageReason::PackageVersionChanged
        );

        assert_bucket(&result[1].triage, TriageBucket::Investigate);
        assert_eq!(
            result[1].triage.primary_reason,
            TriageReason::PackageVersionChanged
        );
    }

    use inspectah_core::types::config::{ConfigCategory, ConfigFileEntry, ConfigSection};

    fn config_entry(path: &str, kind: ConfigFileKind) -> ConfigFileEntry {
        ConfigFileEntry {
            path: path.to_string(),
            kind,
            category: ConfigCategory::default(),
            include: true,
            locked: false,
            ..Default::default()
        }
    }

    #[test]
    fn os_default_pki_rpm_gpg_no_annotation() {
        let snap = InspectionSnapshot {
            config: Some(ConfigSection {
                files: vec![config_entry(
                    "/etc/pki/rpm-gpg/RPM-GPG-KEY-centosofficial",
                    ConfigFileKind::Unowned,
                )],
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = classify_configs(&snap);
        assert_eq!(result.len(), 1);
        assert!(
            result[0].triage.annotations.is_empty(),
            "no SensitivePath annotation"
        );
        assert_bucket(&result[0].triage, TriageBucket::Site);
        assert_eq!(result[0].triage.primary_reason, TriageReason::ConfigUnowned);
    }

    #[test]
    fn os_default_security_pam_no_annotation() {
        let snap = InspectionSnapshot {
            config: Some(ConfigSection {
                files: vec![config_entry(
                    "/etc/security/limits.conf",
                    ConfigFileKind::Unowned,
                )],
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = classify_configs(&snap);
        assert_eq!(result.len(), 1);
        assert!(
            result[0].triage.annotations.is_empty(),
            "no SensitivePath annotation"
        );
    }

    #[test]
    fn os_default_ssl_no_annotation() {
        let snap = InspectionSnapshot {
            config: Some(ConfigSection {
                files: vec![config_entry(
                    "/etc/ssl/openssl.cnf",
                    ConfigFileKind::Unowned,
                )],
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = classify_configs(&snap);
        assert_eq!(result.len(), 1);
        assert!(
            result[0].triage.annotations.is_empty(),
            "no SensitivePath annotation"
        );
    }

    #[test]
    fn os_default_ssh_moduli_no_annotation() {
        let snap = InspectionSnapshot {
            config: Some(ConfigSection {
                files: vec![config_entry("/etc/ssh/moduli", ConfigFileKind::Unowned)],
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = classify_configs(&snap);
        assert_eq!(result.len(), 1);
        assert!(
            result[0].triage.annotations.is_empty(),
            "no SensitivePath annotation"
        );
    }

    #[test]
    fn non_default_sensitive_path_gets_annotation() {
        let snap = InspectionSnapshot {
            config: Some(ConfigSection {
                files: vec![config_entry("/etc/shadow", ConfigFileKind::Unowned)],
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = classify_configs(&snap);
        assert_eq!(result.len(), 1);
        assert!(
            result[0]
                .triage
                .annotations
                .contains(&TriageAnnotation::SensitivePath),
            "SensitivePath annotation expected"
        );
    }

    #[test]
    fn rpm_modified_in_sensitive_path_gets_annotation() {
        let snap = InspectionSnapshot {
            config: Some(ConfigSection {
                files: vec![config_entry(
                    "/etc/ssh/sshd_config",
                    ConfigFileKind::RpmOwnedModified,
                )],
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = classify_configs(&snap);
        assert_eq!(result.len(), 1);
        assert_bucket(&result[0].triage, TriageBucket::Site);
        assert_eq!(
            result[0].triage.primary_reason,
            TriageReason::ConfigModified
        );
        assert!(
            result[0]
                .triage
                .annotations
                .contains(&TriageAnnotation::SensitivePath),
            "SensitivePath annotation expected for modified sensitive path"
        );
    }

    #[test]
    fn unknown_pki_subdir_gets_annotation() {
        let snap = InspectionSnapshot {
            config: Some(ConfigSection {
                files: vec![config_entry(
                    "/etc/pki/tls/custom.pem",
                    ConfigFileKind::Unowned,
                )],
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = classify_configs(&snap);
        assert_eq!(result.len(), 1);
        assert!(
            result[0]
                .triage
                .annotations
                .contains(&TriageAnnotation::SensitivePath),
            "SensitivePath annotation expected"
        );
    }

    #[test]
    fn test_baseline_suppressed_package_gets_baseline_not_investigate() {
        let snap = InspectionSnapshot {
            rpm: Some(RpmSection {
                packages_added: vec![PackageEntry {
                    name: "bash".into(),
                    arch: "x86_64".into(),
                    version: "5.2.26".into(),
                    release: "3.el9".into(),
                    epoch: String::new(),
                    state: PackageState::Modified,
                    include: true,
                    locked: false,
                    source_repo: "baseos".into(),
                    ..Default::default()
                }],
                version_changes: vec![VersionChange {
                    name: "bash".into(),
                    arch: "x86_64".into(),
                    host_version: "5.2.26-3.el9".into(),
                    base_version: "5.2.26-4.el9".into(),
                    host_epoch: String::new(),
                    base_epoch: String::new(),
                    direction: VersionChangeDirection::Downgrade,
                }],
                baseline_suppressed: Some(vec!["bash.x86_64".into()]),
                ..Default::default()
            }),
            ..Default::default()
        };

        let result = classify_packages(&snap);
        let bash = result.iter().find(|p| p.entry.name == "bash").unwrap();
        assert_bucket(&bash.triage, TriageBucket::Baseline);
        assert_eq!(
            bash.triage.primary_reason,
            TriageReason::PackageBaselineMatch
        );
    }

    #[test]
    fn test_non_suppressed_downgrade_still_gets_investigate() {
        let snap = InspectionSnapshot {
            rpm: Some(RpmSection {
                packages_added: vec![PackageEntry {
                    name: "httpd".into(),
                    arch: "x86_64".into(),
                    version: "2.4.57".into(),
                    release: "4.el9".into(),
                    epoch: String::new(),
                    state: PackageState::Modified,
                    include: true,
                    locked: false,
                    source_repo: "appstream".into(),
                    ..Default::default()
                }],
                version_changes: vec![VersionChange {
                    name: "httpd".into(),
                    arch: "x86_64".into(),
                    host_version: "2.4.57-4.el9".into(),
                    base_version: "2.4.57-5.el9".into(),
                    host_epoch: String::new(),
                    base_epoch: String::new(),
                    direction: VersionChangeDirection::Downgrade,
                }],
                baseline_suppressed: Some(Vec::new()),
                ..Default::default()
            }),
            baseline: Some(BaselineData {
                image_digest: "sha256:test".into(),
                packages: std::collections::HashMap::new(),
                extracted_at: "2026-01-01T00:00:00Z".into(),
            }),
            ..Default::default()
        };

        let result = classify_packages(&snap);
        let httpd = result.iter().find(|p| p.entry.name == "httpd").unwrap();
        assert_bucket(&httpd.triage, TriageBucket::Investigate);
    }

    #[test]
    fn sensitive_retained_surfaces_unresolved_hints() {
        use inspectah_core::types::redaction::RedactionHint;

        let snap = InspectionSnapshot {
            redaction_state: Some(RedactionState::SensitiveRetained {
                redacted_by: "inspectah 0.8.0".into(),
                config_hash: "abc".into(),
                unresolved_count: 1,
                unresolved_hints: vec![RedactionHint {
                    path: "/etc/httpd/conf/httpd.conf".into(),
                    reason: "possible credential".into(),
                    confidence: None,
                }],
            }),
            config: Some(ConfigSection {
                files: vec![ConfigFileEntry {
                    path: "/etc/httpd/conf/httpd.conf".into(),
                    include: true,
                    locked: false,
                    ..Default::default()
                }],
            }),
            ..Default::default()
        };

        let result = classify_configs(&snap);
        assert_bucket(&result[0].triage, TriageBucket::Investigate);
        assert!(
            matches!(result[0].triage.primary_reason, TriageReason::Custom(_)),
            "SensitiveRetained with unresolved hints must surface Investigate"
        );
    }
}

#[cfg(test)]
#[allow(clippy::needless_update)]
mod anaconda_classification {
    use super::*;
    use inspectah_core::baseline::{BaselineData, BaselinePackageEntry};
    use inspectah_core::types::config::{
        ConfigCategory, ConfigFileEntry, ConfigFileKind, ConfigSection,
    };
    use inspectah_core::types::rpm::{
        FileOwnershipEntry, RpmSection, VersionChange, VersionChangeDirection,
    };
    use inspectah_core::types::services::{
        PresetDefault, ServiceSection, ServiceStateChange, ServiceUnitState,
    };

    fn assert_bucket(tag: &TriageTag, expected: TriageBucket) {
        match &tag.triage {
            Triage::SingleHost(b) => assert_eq!(*b, expected, "bucket mismatch"),
            Triage::Aggregate(_) => panic!("expected SingleHost, got Aggregate"),
        }
    }

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

    fn snap_with_anaconda_and_vc(
        baseline_names: Option<Vec<String>>,
        packages: Vec<PackageEntry>,
        version_changes: Vec<VersionChange>,
        services: Option<ServiceSection>,
        config: Option<ConfigSection>,
        file_ownership: Vec<FileOwnershipEntry>,
    ) -> InspectionSnapshot {
        let mut snap =
            snap_with_anaconda(baseline_names, packages, services, config, file_ownership);
        if let Some(rpm) = &mut snap.rpm {
            rpm.version_changes = version_changes;
        }
        snap
    }

    fn anaconda_pkg(name: &str) -> PackageEntry {
        PackageEntry {
            name: name.into(),
            arch: "x86_64".into(),
            state: PackageState::Added,
            source_repo: "anaconda".into(),
            include: true, // serde default_true doesn't apply to Default::default()
            ..Default::default()
        }
    }

    fn empty_service_section() -> ServiceSection {
        ServiceSection {
            state_changes: vec![],
            enabled_units: vec![],
            disabled_units: vec![],
            drop_ins: vec![],
            preset_matched_units: vec![],
        }
    }

    // --- Tier 1: platform plumbing hard exclude ---

    #[test]
    fn anaconda_tier1_platform_plumbing_hard_excluded() {
        let snap = snap_with_anaconda(
            Some(vec!["glibc".into()]),
            vec![anaconda_pkg("grub2-efi-aa64-cdboot"), anaconda_pkg("httpd")],
            Some(empty_service_section()),
            Some(ConfigSection { files: vec![] }),
            vec![],
        );
        let result = classify_packages(&snap);
        let grub = result
            .iter()
            .find(|p| p.entry.name == "grub2-efi-aa64-cdboot")
            .unwrap();
        assert_bucket(&grub.triage, TriageBucket::Baseline);
        assert_eq!(
            grub.triage.primary_reason,
            TriageReason::PackagePlatformPlumbing
        );
        assert!(!grub.entry.include);
        assert!(grub.entry.locked);
    }

    #[test]
    fn anaconda_tier1_overrides_version_changed() {
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
                direction: VersionChangeDirection::Upgrade,
                ..Default::default()
            }],
            Some(empty_service_section()),
            Some(ConfigSection { files: vec![] }),
            vec![],
        );
        let result = classify_packages(&snap);
        let grub = result
            .iter()
            .find(|p| p.entry.name == "grub2-tools-extra")
            .unwrap();
        assert_eq!(
            grub.triage.primary_reason,
            TriageReason::PackagePlatformPlumbing,
            "Tier 1 must override stronger signals for boot-chain packages"
        );
        assert!(grub.entry.locked);
        assert!(!grub.entry.include);
    }

    // --- Precedence: anaconda post-pass must not override version-changed or local-install ---

    #[test]
    fn anaconda_precedence_preserves_version_changed() {
        let mut pkg = anaconda_pkg("tzdata");
        pkg.state = PackageState::Modified;
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
                direction: VersionChangeDirection::Upgrade,
                ..Default::default()
            }],
            Some(empty_service_section()),
            Some(ConfigSection { files: vec![] }),
            vec![],
        );
        let result = classify_packages(&snap);
        let tz = result.iter().find(|p| p.entry.name == "tzdata").unwrap();
        assert_eq!(
            tz.triage.primary_reason,
            TriageReason::PackageVersionChanged,
            "anaconda post-pass must not override PackageVersionChanged"
        );
    }

    #[test]
    fn anaconda_precedence_preserves_local_install() {
        let mut local = anaconda_pkg("custom-rpm");
        local.state = PackageState::LocalInstall;
        let snap = snap_with_anaconda(
            Some(vec!["glibc".into()]),
            vec![local],
            Some(empty_service_section()),
            Some(ConfigSection { files: vec![] }),
            vec![],
        );
        let result = classify_packages(&snap);
        let custom = result
            .iter()
            .find(|p| p.entry.name == "custom-rpm")
            .unwrap();
        assert_eq!(
            custom.triage.primary_reason,
            TriageReason::PackageLocalInstall
        );
    }

    // --- Tier 2 Path A: dual-signal promotion (service + config) ---

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
                    locked: false,
                    aggregate: None,
                    attention_reason: None,
                }],
                enabled_units: vec![],
                disabled_units: vec![],
                drop_ins: vec![],
                preset_matched_units: vec![],
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
                package_name: "firewalld".into(),
                paths: vec!["/etc/firewalld/zones/custom.xml".into()],
            }],
        );
        let result = classify_packages(&snap);
        let fw = result.iter().find(|p| p.entry.name == "firewalld").unwrap();
        assert_bucket(&fw.triage, TriageBucket::Site);
        assert_eq!(
            fw.triage.primary_reason,
            TriageReason::PackageInstallerPromotedService
        );
        assert!(fw.entry.include);
    }

    // --- Tier 2 Path B: config-only promotion ---

    #[test]
    fn anaconda_tier2_config_only_promotes_curated_package() {
        let snap = snap_with_anaconda(
            Some(vec!["glibc".into()]),
            vec![anaconda_pkg("sudo")],
            Some(empty_service_section()),
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
                package_name: "sudo".into(),
                paths: vec!["/etc/sudoers".into()],
            }],
        );
        let result = classify_packages(&snap);
        let sudo = result.iter().find(|p| p.entry.name == "sudo").unwrap();
        assert_bucket(&sudo.triage, TriageBucket::Site);
        assert_eq!(
            sudo.triage.primary_reason,
            TriageReason::PackageInstallerPromotedConfig
        );
    }

    // --- Tier 3: installer noise (soft exclude) ---

    #[test]
    fn anaconda_tier3_installer_noise_soft_excluded() {
        // file_ownership must be non-empty for has_config to be true
        let snap = snap_with_anaconda(
            Some(vec!["glibc".into()]),
            vec![
                anaconda_pkg("google-noto-sans-vf-fonts"),
                anaconda_pkg("lshw"),
                anaconda_pkg("kernel-tools"),
                anaconda_pkg("biosdevname"),
            ],
            Some(empty_service_section()),
            Some(ConfigSection { files: vec![] }),
            vec![FileOwnershipEntry {
                package_name: "dummy".into(),
                paths: vec!["/etc/dummy".into()],
            }],
        );
        let result = classify_packages(&snap);
        for name in &[
            "google-noto-sans-vf-fonts",
            "lshw",
            "kernel-tools",
            "biosdevname",
        ] {
            let pkg = result.iter().find(|p| p.entry.name == *name).unwrap();
            assert_bucket(&pkg.triage, TriageBucket::Baseline);
            assert_eq!(
                pkg.triage.primary_reason,
                TriageReason::PackageInstallerDefault,
                "wrong reason for {}",
                name
            );
            assert!(!pkg.entry.include, "{} should be excluded", name);
            assert!(!pkg.entry.locked, "{} should not be locked", name);
        }
    }

    // --- Tier 4: ambiguous (defaults to investigate, included) ---

    #[test]
    fn anaconda_tier4_ambiguous_defaults_to_investigate_included() {
        // file_ownership must be non-empty for has_config to be true
        let snap = snap_with_anaconda(
            Some(vec!["glibc".into()]),
            vec![anaconda_pkg("cronie"), anaconda_pkg("audit")],
            Some(empty_service_section()),
            Some(ConfigSection { files: vec![] }),
            vec![FileOwnershipEntry {
                package_name: "dummy".into(),
                paths: vec!["/etc/dummy".into()],
            }],
        );
        let result = classify_packages(&snap);
        for name in &["cronie", "audit"] {
            let pkg = result.iter().find(|p| p.entry.name == *name).unwrap();
            assert_bucket(&pkg.triage, TriageBucket::Investigate);
            assert_eq!(
                pkg.triage.primary_reason,
                TriageReason::PackageInstallerAmbiguous
            );
            assert!(pkg.entry.include, "{} should be included by default", name);
        }
    }

    // --- Missing evidence: preserves existing classification ---

    #[test]
    fn anaconda_missing_evidence_preserves_existing_classification() {
        // No services, no config → missing evidence
        let snap = snap_with_anaconda(
            Some(vec!["glibc".into()]),
            vec![anaconda_pkg("firewalld")],
            None,
            None,
            vec![],
        );
        let result = classify_packages(&snap);
        let fw = result.iter().find(|p| p.entry.name == "firewalld").unwrap();
        assert_eq!(
            fw.triage.primary_reason,
            TriageReason::PackageUserAdded,
            "missing evidence must preserve existing classification, not reclassify"
        );
        assert!(fw.entry.include);
    }

    #[test]
    fn anaconda_missing_file_ownership_preserves_existing() {
        // Services and config present but file_ownership empty → missing evidence
        let snap = snap_with_anaconda(
            Some(vec!["glibc".into()]),
            vec![anaconda_pkg("firewalld")],
            Some(empty_service_section()),
            Some(ConfigSection { files: vec![] }),
            vec![], // empty file_ownership
        );
        let result = classify_packages(&snap);
        let fw = result.iter().find(|p| p.entry.name == "firewalld").unwrap();
        assert_eq!(
            fw.triage.primary_reason,
            TriageReason::PackageUserAdded,
            "missing file_ownership must preserve existing classification"
        );
    }

    // --- Non-anaconda packages unaffected ---

    #[test]
    fn non_anaconda_package_unaffected_by_classifier() {
        let mut httpd = anaconda_pkg("grub2-tools-extra");
        httpd.source_repo = "appstream".into();
        let snap = snap_with_anaconda(
            Some(vec!["glibc".into()]),
            vec![httpd],
            Some(empty_service_section()),
            Some(ConfigSection { files: vec![] }),
            vec![],
        );
        let result = classify_packages(&snap);
        let pkg = result
            .iter()
            .find(|p| p.entry.name == "grub2-tools-extra")
            .unwrap();
        assert_ne!(
            pkg.triage.primary_reason,
            TriageReason::PackagePlatformPlumbing
        );
    }

    #[test]
    fn anaconda_classification_neutral_with_installed_groups() {
        use inspectah_core::types::rpm::InstalledGroup;

        let packages = vec![
            anaconda_pkg("grub2-efi-aa64-cdboot"),
            anaconda_pkg("google-noto-sans-vf-fonts"),
            anaconda_pkg("cronie"),
        ];

        // Run with installed_groups = None
        let snap_none = snap_with_anaconda(
            Some(vec!["glibc".into()]),
            packages.clone(),
            Some(empty_service_section()),
            Some(ConfigSection { files: vec![] }),
            vec![FileOwnershipEntry {
                package_name: "dummy".into(),
                paths: vec!["/dummy".into()],
            }],
        );
        let result_none = classify_packages(&snap_none);

        // Run with installed_groups = Some([]) (no groups)
        let mut snap_empty = snap_with_anaconda(
            Some(vec!["glibc".into()]),
            packages.clone(),
            Some(empty_service_section()),
            Some(ConfigSection { files: vec![] }),
            vec![FileOwnershipEntry {
                package_name: "dummy".into(),
                paths: vec!["/dummy".into()],
            }],
        );
        if let Some(rpm) = &mut snap_empty.rpm {
            rpm.installed_groups = Some(vec![]);
        }
        let result_empty = classify_packages(&snap_empty);

        // Run with installed_groups = Some([group with cronie])
        let mut snap_groups = snap_with_anaconda(
            Some(vec!["glibc".into()]),
            packages.clone(),
            Some(empty_service_section()),
            Some(ConfigSection { files: vec![] }),
            vec![FileOwnershipEntry {
                package_name: "dummy".into(),
                paths: vec!["/dummy".into()],
            }],
        );
        if let Some(rpm) = &mut snap_groups.rpm {
            rpm.installed_groups = Some(vec![InstalledGroup {
                name: "Base".into(),
                members: vec!["cronie".into()],
                ..Default::default()
            }]);
        }
        let result_groups = classify_packages(&snap_groups);

        // All three must produce identical classification outcomes
        for (name, expected_reason) in &[
            (
                "grub2-efi-aa64-cdboot",
                TriageReason::PackagePlatformPlumbing,
            ),
            (
                "google-noto-sans-vf-fonts",
                TriageReason::PackageInstallerDefault,
            ),
            ("cronie", TriageReason::PackageInstallerAmbiguous),
        ] {
            let r_none = result_none.iter().find(|p| p.entry.name == *name).unwrap();
            let r_empty = result_empty.iter().find(|p| p.entry.name == *name).unwrap();
            let r_groups = result_groups
                .iter()
                .find(|p| p.entry.name == *name)
                .unwrap();
            assert_eq!(
                r_none.triage.primary_reason, *expected_reason,
                "None: {}",
                name
            );
            assert_eq!(
                r_empty.triage.primary_reason, *expected_reason,
                "Empty: {}",
                name
            );
            assert_eq!(
                r_groups.triage.primary_reason, *expected_reason,
                "Groups: {}",
                name
            );
            assert_eq!(
                r_none.entry.include, r_empty.entry.include,
                "include mismatch for {}",
                name
            );
            assert_eq!(
                r_none.entry.include, r_groups.entry.include,
                "include mismatch for {}",
                name
            );
        }
    }

    #[test]
    fn anaconda_degraded_evidence_blocks_tier3_noise_exclusion() {
        // When a service has missing owning_package, evidence is degraded.
        // Tier 3 noise packages must NOT be soft-excluded — they should
        // fall to Tier 4 (include=true) with EvidenceUnavailable reason.
        let snap = snap_with_anaconda(
            Some(vec!["glibc".into()]),
            vec![anaconda_pkg("kernel-tools"), anaconda_pkg("cronie")],
            Some(ServiceSection {
                state_changes: vec![ServiceStateChange {
                    unit: "mystery.service".into(),
                    current_state: ServiceUnitState::Enabled,
                    default_state: Some(PresetDefault::Disable),
                    include: true,
                    owning_package: None, // degraded — missing attribution
                    locked: false,
                    aggregate: None,
                    attention_reason: None,
                }],
                enabled_units: vec![],
                disabled_units: vec![],
                drop_ins: vec![],
                preset_matched_units: vec![],
            }),
            Some(ConfigSection { files: vec![] }),
            vec![FileOwnershipEntry {
                package_name: "dummy".into(),
                paths: vec!["/dummy".into()],
            }],
        );
        let result = classify_packages(&snap);

        // kernel-tools matches INSTALLER_NOISE_PATTERNS but evidence is
        // degraded — must NOT be Tier 3 excluded. Should get
        // EvidenceUnavailable and include=true.
        let kt = result
            .iter()
            .find(|p| p.entry.name == "kernel-tools")
            .unwrap();
        assert_eq!(
            kt.triage.primary_reason,
            TriageReason::PackageInstallerEvidenceUnavailable,
            "noise package with degraded evidence must not be soft-excluded"
        );
        assert!(
            kt.entry.include,
            "noise package with degraded evidence must be included"
        );

        // cronie is not a noise pattern — should still be Tier 4 ambiguous
        let cr = result.iter().find(|p| p.entry.name == "cronie").unwrap();
        assert_eq!(
            cr.triage.primary_reason,
            TriageReason::PackageInstallerAmbiguous
        );
        assert!(cr.entry.include);
    }

    #[test]
    fn anaconda_clean_evidence_allows_tier3_noise_exclusion() {
        // When all services have owning_package, evidence is clean.
        // Tier 3 noise packages should be soft-excluded normally.
        let snap = snap_with_anaconda(
            Some(vec!["glibc".into()]),
            vec![anaconda_pkg("kernel-tools")],
            Some(ServiceSection {
                state_changes: vec![ServiceStateChange {
                    unit: "sshd.service".into(),
                    current_state: ServiceUnitState::Enabled,
                    default_state: Some(PresetDefault::Enable),
                    include: true,
                    owning_package: Some("openssh-server".into()),
                    locked: false,
                    aggregate: None,
                    attention_reason: None,
                }],
                enabled_units: vec![],
                disabled_units: vec![],
                drop_ins: vec![],
                preset_matched_units: vec![],
            }),
            Some(ConfigSection { files: vec![] }),
            vec![FileOwnershipEntry {
                package_name: "dummy".into(),
                paths: vec!["/dummy".into()],
            }],
        );
        let result = classify_packages(&snap);
        let kt = result
            .iter()
            .find(|p| p.entry.name == "kernel-tools")
            .unwrap();
        assert_eq!(
            kt.triage.primary_reason,
            TriageReason::PackageInstallerDefault,
            "noise package with clean evidence should be soft-excluded"
        );
        assert!(!kt.entry.include);
    }
}

#[cfg(test)]
mod service_classification {
    use super::*;
    use inspectah_core::types::services::{
        PresetDefault, ServiceSection, ServiceStateChange, ServiceUnitState, SystemdDropIn,
    };

    fn assert_bucket(tag: &TriageTag, expected: TriageBucket) {
        match &tag.triage {
            Triage::SingleHost(b) => assert_eq!(*b, expected, "bucket mismatch"),
            Triage::Aggregate(_) => panic!("expected SingleHost, got Aggregate"),
        }
    }

    #[test]
    fn service_matching_preset_default_is_site_until_baseline() {
        let snap = InspectionSnapshot {
            services: Some(ServiceSection {
                state_changes: vec![ServiceStateChange {
                    unit: "sshd.service".into(),
                    current_state: ServiceUnitState::Enabled,
                    default_state: Some(PresetDefault::Enable),
                    include: true,
                    locked: false,
                    owning_package: Some("openssh-server".into()),
                    aggregate: None,
                    attention_reason: None,
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        let (states, dropins) = classify_services(&snap);
        assert_eq!(states.len(), 1);
        assert_eq!(dropins.len(), 0);
        // Until baseline exists, even preset-matching services are Site
        assert_bucket(&states[0].triage, TriageBucket::Site);
        assert_eq!(
            states[0].triage.primary_reason,
            TriageReason::ServiceNonDefaultState
        );
    }

    #[test]
    fn service_differing_from_preset_is_site() {
        let snap = InspectionSnapshot {
            services: Some(ServiceSection {
                state_changes: vec![ServiceStateChange {
                    unit: "firewalld.service".into(),
                    current_state: ServiceUnitState::Enabled,
                    default_state: Some(PresetDefault::Disable),
                    include: true,
                    locked: false,
                    owning_package: Some("firewalld".into()),
                    aggregate: None,
                    attention_reason: None,
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        let (states, _) = classify_services(&snap);
        assert_eq!(states.len(), 1);
        assert_bucket(&states[0].triage, TriageBucket::Site);
        assert_eq!(
            states[0].triage.primary_reason,
            TriageReason::ServiceNonDefaultState
        );
    }

    #[test]
    fn service_without_owning_package_is_investigate() {
        let snap = InspectionSnapshot {
            services: Some(ServiceSection {
                state_changes: vec![ServiceStateChange {
                    unit: "mystery.service".into(),
                    current_state: ServiceUnitState::Enabled,
                    default_state: None,
                    include: true,
                    locked: false,
                    owning_package: None,
                    aggregate: None,
                    attention_reason: None,
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        let (states, _) = classify_services(&snap);
        assert_eq!(states.len(), 1);
        assert_bucket(&states[0].triage, TriageBucket::Investigate);
        assert_eq!(
            states[0].triage.primary_reason,
            TriageReason::ServiceUnknownOrigin
        );
    }

    #[test]
    fn dropin_is_always_site() {
        let snap = InspectionSnapshot {
            services: Some(ServiceSection {
                drop_ins: vec![SystemdDropIn {
                    unit: "sshd.service".into(),
                    path: "/etc/systemd/system/sshd.service.d/override.conf".into(),
                    content: "[Service]\nTimeoutStartSec=90".into(),
                    include: true,
                    locked: false,
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        let (states, dropins) = classify_services(&snap);
        assert_eq!(states.len(), 0);
        assert_eq!(dropins.len(), 1);
        assert_bucket(&dropins[0].triage, TriageBucket::Site);
        assert_eq!(
            dropins[0].triage.primary_reason,
            TriageReason::ServiceDropInPresent
        );
    }

    #[test]
    fn no_services_returns_empty() {
        let snap = InspectionSnapshot::default();
        let (states, dropins) = classify_services(&snap);
        assert!(states.is_empty());
        assert!(dropins.is_empty());
    }
}

#[cfg(test)]
mod container_classification {
    use super::*;
    use inspectah_core::types::containers::{ContainerSection, FlatpakApp, QuadletUnit};

    fn assert_bucket(tag: &TriageTag, expected: TriageBucket) {
        match &tag.triage {
            Triage::SingleHost(b) => assert_eq!(*b, expected, "bucket mismatch"),
            Triage::Aggregate(_) => panic!("expected SingleHost, got Aggregate"),
        }
    }

    #[test]
    fn quadlet_is_site() {
        let snap = InspectionSnapshot {
            containers: Some(ContainerSection {
                quadlet_units: vec![QuadletUnit {
                    path: "/etc/containers/systemd/myapp.container".into(),
                    name: "myapp.container".into(),
                    image: "quay.io/myorg/myapp:latest".into(),
                    include: true,
                    locked: false,
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        let (quadlets, flatpaks) = classify_containers(&snap);
        assert_eq!(quadlets.len(), 1);
        assert_eq!(flatpaks.len(), 0);
        assert_bucket(&quadlets[0].triage, TriageBucket::Site);
        assert_eq!(
            quadlets[0].triage.primary_reason,
            TriageReason::QuadletUserDeployed
        );
    }

    #[test]
    fn flatpak_with_full_provenance_is_site() {
        let snap = InspectionSnapshot {
            containers: Some(ContainerSection {
                flatpak_apps: vec![FlatpakApp {
                    app_id: "org.mozilla.Firefox".into(),
                    remote: "flathub".into(),
                    branch: "stable".into(),
                    include: true,
                    locked: false,
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        let (quadlets, flatpaks) = classify_containers(&snap);
        assert_eq!(quadlets.len(), 0);
        assert_eq!(flatpaks.len(), 1);
        assert_bucket(&flatpaks[0].triage, TriageBucket::Site);
        assert_eq!(
            flatpaks[0].triage.primary_reason,
            TriageReason::FlatpakProvisionedOnFirstBoot
        );
        assert!(
            flatpaks[0]
                .triage
                .annotations
                .contains(&TriageAnnotation::FirstBootProvisioned),
            "expected FirstBootProvisioned annotation"
        );
    }

    #[test]
    fn flatpak_missing_remote_is_investigate() {
        let snap = InspectionSnapshot {
            containers: Some(ContainerSection {
                flatpak_apps: vec![FlatpakApp {
                    app_id: "org.gnome.Calculator".into(),
                    remote: String::new(),
                    branch: "stable".into(),
                    include: true,
                    locked: false,
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        let (_, flatpaks) = classify_containers(&snap);
        assert_eq!(flatpaks.len(), 1);
        assert_bucket(&flatpaks[0].triage, TriageBucket::Investigate);
        assert_eq!(
            flatpaks[0].triage.primary_reason,
            TriageReason::FlatpakIncompleteProvenance
        );
    }

    #[test]
    fn flatpak_missing_branch_is_investigate() {
        let snap = InspectionSnapshot {
            containers: Some(ContainerSection {
                flatpak_apps: vec![FlatpakApp {
                    app_id: "org.gnome.Calculator".into(),
                    remote: "flathub".into(),
                    branch: String::new(),
                    include: true,
                    locked: false,
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        let (_, flatpaks) = classify_containers(&snap);
        assert_eq!(flatpaks.len(), 1);
        assert_bucket(&flatpaks[0].triage, TriageBucket::Investigate);
        assert_eq!(
            flatpaks[0].triage.primary_reason,
            TriageReason::FlatpakIncompleteProvenance
        );
    }

    #[test]
    fn no_containers_returns_empty() {
        let snap = InspectionSnapshot::default();
        let (quadlets, flatpaks) = classify_containers(&snap);
        assert!(quadlets.is_empty());
        assert!(flatpaks.is_empty());
    }
}

#[cfg(test)]
mod sysctl_classification {
    use super::*;
    use inspectah_core::types::kernelboot::{KernelBootSection, SysctlOverride};

    fn assert_bucket(tag: &TriageTag, expected: TriageBucket) {
        match &tag.triage {
            Triage::SingleHost(b) => assert_eq!(*b, expected, "bucket mismatch"),
            Triage::Aggregate(_) => panic!("expected SingleHost, got Aggregate"),
        }
    }

    #[test]
    fn sysctl_file_backed_is_site() {
        let snap = InspectionSnapshot {
            kernel_boot: Some(KernelBootSection {
                sysctl_overrides: vec![SysctlOverride {
                    key: "net.ipv4.ip_forward".into(),
                    runtime: "1".into(),
                    default: "0".into(),
                    source: "/etc/sysctl.d/99-custom.conf".into(),
                    include: true,
                    locked: false,
                    aggregate: None,
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = classify_sysctls(&snap);
        assert_eq!(result.len(), 1);
        assert_bucket(&result[0].triage, TriageBucket::Site);
        assert_eq!(
            result[0].triage.primary_reason,
            TriageReason::SysctlFileBackedOverride
        );
        assert!(
            result[0].triage.annotations.is_empty(),
            "file-backed sysctl should have no annotations"
        );
    }

    #[test]
    fn sysctl_sysctl_conf_is_site() {
        let snap = InspectionSnapshot {
            kernel_boot: Some(KernelBootSection {
                sysctl_overrides: vec![SysctlOverride {
                    key: "net.core.somaxconn".into(),
                    runtime: "4096".into(),
                    default: "128".into(),
                    source: "/etc/sysctl.conf".into(),
                    include: true,
                    locked: false,
                    aggregate: None,
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = classify_sysctls(&snap);
        assert_eq!(result.len(), 1);
        assert_bucket(&result[0].triage, TriageBucket::Site);
        assert_eq!(
            result[0].triage.primary_reason,
            TriageReason::SysctlFileBackedOverride
        );
    }

    #[test]
    fn sysctl_runtime_only_has_annotation() {
        let snap = InspectionSnapshot {
            kernel_boot: Some(KernelBootSection {
                sysctl_overrides: vec![SysctlOverride {
                    key: "net.ipv4.ip_forward".into(),
                    runtime: "1".into(),
                    default: "0".into(),
                    source: "runtime".into(),
                    include: true,
                    locked: false,
                    aggregate: None,
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = classify_sysctls(&snap);
        assert_eq!(result.len(), 1);
        assert_bucket(&result[0].triage, TriageBucket::Site);
        assert_eq!(
            result[0].triage.primary_reason,
            TriageReason::SysctlNoBaseline
        );
        assert!(
            result[0]
                .triage
                .annotations
                .contains(&TriageAnnotation::RuntimeOnlyObservation),
            "runtime-only sysctl must have RuntimeOnlyObservation annotation"
        );
    }

    #[test]
    fn sysctl_deny_list_excluded() {
        let snap = InspectionSnapshot {
            kernel_boot: Some(KernelBootSection {
                sysctl_overrides: vec![
                    SysctlOverride {
                        key: "vm.drop_caches".into(),
                        runtime: "3".into(),
                        default: "0".into(),
                        source: "/etc/sysctl.d/99-custom.conf".into(),
                        include: true,
                        locked: false,
                        aggregate: None,
                    },
                    SysctlOverride {
                        key: "vm.compact_memory".into(),
                        runtime: "1".into(),
                        default: "0".into(),
                        source: "runtime".into(),
                        include: true,
                        locked: false,
                        aggregate: None,
                    },
                    SysctlOverride {
                        key: "kernel.sysrq".into(),
                        runtime: "16".into(),
                        default: "0".into(),
                        source: "/etc/sysctl.d/10-sysrq.conf".into(),
                        include: true,
                        locked: false,
                        aggregate: None,
                    },
                ],
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = classify_sysctls(&snap);
        assert!(
            result.is_empty(),
            "all deny-listed sysctls should be excluded, got {}",
            result.len()
        );
    }

    #[test]
    fn no_sysctls_returns_empty() {
        let snap = InspectionSnapshot::default();
        let result = classify_sysctls(&snap);
        assert!(result.is_empty());
    }
}

#[cfg(test)]
mod tuned_classification {
    use super::*;
    use inspectah_core::types::kernelboot::{ConfigSnippet, KernelBootSection};
    use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection};

    fn assert_bucket(tag: &TriageTag, expected: TriageBucket) {
        match &tag.triage {
            Triage::SingleHost(b) => assert_eq!(*b, expected, "bucket mismatch"),
            Triage::Aggregate(_) => panic!("expected SingleHost, got Aggregate"),
        }
    }

    fn tuned_pkg() -> PackageEntry {
        PackageEntry {
            name: "tuned".to_string(),
            arch: "noarch".to_string(),
            state: PackageState::Added,
            source_repo: "rhel-9-baseos".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn tuned_non_default_profile_is_site() {
        let snap = InspectionSnapshot {
            kernel_boot: Some(KernelBootSection {
                tuned_active: "throughput-performance".to_string(),
                ..Default::default()
            }),
            rpm: Some(RpmSection {
                packages_added: vec![tuned_pkg()],
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = classify_tuned(&snap);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].active_profile, "throughput-performance");
        assert_bucket(&result[0].triage, TriageBucket::Site);
        assert_eq!(
            result[0].triage.primary_reason,
            TriageReason::TunedNonDefaultProfile
        );
    }

    #[test]
    fn tuned_package_missing_is_investigate() {
        let snap = InspectionSnapshot {
            kernel_boot: Some(KernelBootSection {
                tuned_active: "throughput-performance".to_string(),
                ..Default::default()
            }),
            // No RPM section — tuned package not present.
            ..Default::default()
        };
        let result = classify_tuned(&snap);
        assert_eq!(result.len(), 1);
        assert_bucket(&result[0].triage, TriageBucket::Investigate);
        assert_eq!(
            result[0].triage.primary_reason,
            TriageReason::TunedUnusualState
        );
        assert!(
            result[0]
                .triage
                .annotations
                .contains(&TriageAnnotation::RequiresProjectedPackage {
                    name: "tuned".to_string()
                }),
            "expected RequiresProjectedPackage annotation"
        );
    }

    #[test]
    fn tuned_default_profile_is_site_until_baseline() {
        let snap = InspectionSnapshot {
            kernel_boot: Some(KernelBootSection {
                tuned_active: "virtual-guest".to_string(),
                ..Default::default()
            }),
            rpm: Some(RpmSection {
                packages_added: vec![tuned_pkg()],
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = classify_tuned(&snap);
        assert_eq!(result.len(), 1);
        assert_bucket(&result[0].triage, TriageBucket::Site);
    }

    #[test]
    fn no_tuned_returns_empty() {
        let snap = InspectionSnapshot::default();
        let result = classify_tuned(&snap);
        assert!(result.is_empty());
    }

    #[test]
    fn tuned_custom_profiles_are_captured() {
        let snap = InspectionSnapshot {
            kernel_boot: Some(KernelBootSection {
                tuned_active: "my-custom-profile".to_string(),
                tuned_custom_profiles: vec![ConfigSnippet {
                    path: "/etc/tuned/my-custom-profile/tuned.conf".to_string(),
                    content: "[main]\nsummary=Custom profile".to_string(),
                }],
                ..Default::default()
            }),
            rpm: Some(RpmSection {
                packages_added: vec![tuned_pkg()],
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = classify_tuned(&snap);
        assert_eq!(result.len(), 1);
        assert_eq!(
            result[0].custom_profiles,
            vec!["/etc/tuned/my-custom-profile/tuned.conf"]
        );
        assert_bucket(&result[0].triage, TriageBucket::Site);
        assert_eq!(
            result[0].triage.primary_reason,
            TriageReason::TunedCustomProfile
        );
    }

    #[test]
    fn tuned_empty_active_returns_empty() {
        let snap = InspectionSnapshot {
            kernel_boot: Some(KernelBootSection {
                tuned_active: String::new(),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = classify_tuned(&snap);
        assert!(result.is_empty());
    }
}
