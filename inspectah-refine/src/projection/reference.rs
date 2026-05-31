use std::collections::HashSet;

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::rpm::VersionChangeDirection;
use inspectah_pipeline::render::service_intent::render_service_intent;

use super::types::{
    ContainerMount, EmptyReason, RefAlternativeEntry, RefComposeItem, RefConfigSnippet,
    RefContainers, RefDropInItem, RefFlatpakRefItem, RefKernelBoot, RefKernelModule,
    RefOmittedService, RefQuadletItem, RefRunningContainerItem, RefServiceAdvisory, RefServiceItem,
    RefServiceWarning, RefServices, RefSysctlOverride, RefVersionChangeItem, RefVersionChanges,
};

/// Project version changes from snapshot into reference format.
///
/// Logic follows the three-state empty reason pattern from handlers:
/// - `DataUnavailable`: No rpm section exists
/// - `NoBaseline`: Rpm section exists but no baseline data
/// - `ZeroDrift`: Baseline exists but no version changes detected
pub fn project_ref_version_changes(snap: &InspectionSnapshot) -> RefVersionChanges {
    let rpm = match &snap.rpm {
        None => {
            return RefVersionChanges {
                downgrades: Vec::new(),
                upgrades: Vec::new(),
                empty_reason: Some(EmptyReason::DataUnavailable),
            };
        }
        Some(rpm) => rpm,
    };

    if rpm.version_changes.is_empty() {
        let reason = if snap.baseline.is_some() {
            EmptyReason::ZeroDrift
        } else {
            EmptyReason::NoBaseline
        };
        return RefVersionChanges {
            downgrades: Vec::new(),
            upgrades: Vec::new(),
            empty_reason: Some(reason),
        };
    }

    // Partition by direction
    let mut downgrades = Vec::new();
    let mut upgrades = Vec::new();

    for vc in &rpm.version_changes {
        let item = RefVersionChangeItem {
            name: vc.name.clone(),
            arch: vc.arch.clone(),
            host_version: vc.host_version.clone(),
            base_version: vc.base_version.clone(),
            host_epoch: vc.host_epoch.clone(),
            base_epoch: vc.base_epoch.clone(),
            direction: vc.direction.clone(),
        };

        match vc.direction {
            VersionChangeDirection::Downgrade => downgrades.push(item),
            VersionChangeDirection::Upgrade => upgrades.push(item),
        }
    }

    RefVersionChanges {
        downgrades,
        upgrades,
        empty_reason: None,
    }
}

/// Project service data from snapshot into reference format.
///
/// Categorization mirrors `normalize_services` in handlers.rs:
/// 1. **Divergent** — units in `state_changes` whose current state differs from preset default
/// 2. **Preset-matched with drop-ins** — units matching preset but carrying drop-in overrides
/// 3. **Preset-unknown enabled** — enabled units not in divergent or matched sets
/// 4. **Preset-unknown disabled** — disabled units not in divergent or matched sets
/// 5. **Standalone drop-ins** — drop-ins for units not covered by any of the above
///
/// Omitted/advisory data comes from `render_service_intent` in the pipeline crate.
/// Warnings are filtered from `snap.warnings` where `inspector == "services"`.
pub fn project_ref_services(snap: &InspectionSnapshot) -> RefServices {
    let svc = match &snap.services {
        Some(s) => s,
        None => return RefServices::default(),
    };

    let render_plan = render_service_intent(snap);

    let matched_set: HashSet<&str> = svc
        .preset_matched_units
        .iter()
        .map(|s| s.as_str())
        .collect();
    let divergent_set: HashSet<&str> = svc
        .state_changes
        .iter()
        .map(|sc| sc.unit.as_str())
        .collect();
    let enabled_set: HashSet<&str> = svc.enabled_units.iter().map(|s| s.as_str()).collect();
    let disabled_set: HashSet<&str> = svc.disabled_units.iter().map(|s| s.as_str()).collect();

    // Build drop-in lookup: units covered by any category get their drop-ins
    // folded in; everything else is standalone.
    let mut dropin_by_unit: std::collections::HashMap<&str, Vec<String>> =
        std::collections::HashMap::new();
    let mut standalone_dropins = Vec::new();
    for d in &svc.drop_ins {
        if divergent_set.contains(d.unit.as_str())
            || matched_set.contains(d.unit.as_str())
            || enabled_set.contains(d.unit.as_str())
            || disabled_set.contains(d.unit.as_str())
        {
            dropin_by_unit
                .entry(d.unit.as_str())
                .or_default()
                .push(d.content.clone());
        } else {
            standalone_dropins.push(RefDropInItem {
                unit: d.unit.clone(),
                path: d.path.clone(),
                content: d.content.clone(),
            });
        }
    }

    // Collect omitted unit names so they are excluded from the main list.
    let omitted_units: HashSet<&str> = render_plan
        .omissions
        .iter()
        .map(|o| o.unit.as_str())
        .collect();

    // 1. Divergent items (from state_changes)
    let mut divergent = Vec::new();
    for sc in &svc.state_changes {
        if omitted_units.contains(sc.unit.as_str()) {
            continue;
        }
        divergent.push(RefServiceItem {
            unit: sc.unit.clone(),
            current_state: sc.current_state,
            default_state: sc.default_state,
            owning_package: sc.owning_package.clone(),
            dropin_contents: dropin_by_unit
                .get(sc.unit.as_str())
                .cloned()
                .unwrap_or_default(),
        });
    }

    // 2. Preset-matched with drop-in (without drop-in are suppressed entirely)
    let mut preset_matched_with_dropins = Vec::new();
    for unit_name in &svc.preset_matched_units {
        if let Some(contents) = dropin_by_unit.get(unit_name.as_str()) {
            let current_state = if enabled_set.contains(unit_name.as_str()) {
                inspectah_core::types::services::ServiceUnitState::Enabled
            } else {
                inspectah_core::types::services::ServiceUnitState::Disabled
            };
            preset_matched_with_dropins.push(RefServiceItem {
                unit: unit_name.clone(),
                current_state,
                default_state: None, // matched means current == default; no divergence to record
                owning_package: None,
                dropin_contents: contents.clone(),
            });
        }
    }

    // 3. Preset-unknown enabled units (not divergent, not matched)
    let mut preset_unknown_enabled = Vec::new();
    for unit_name in &svc.enabled_units {
        if !divergent_set.contains(unit_name.as_str())
            && !matched_set.contains(unit_name.as_str())
        {
            preset_unknown_enabled.push(RefServiceItem {
                unit: unit_name.clone(),
                current_state: inspectah_core::types::services::ServiceUnitState::Enabled,
                default_state: None,
                owning_package: None,
                dropin_contents: dropin_by_unit
                    .get(unit_name.as_str())
                    .cloned()
                    .unwrap_or_default(),
            });
        }
    }

    // 4. Preset-unknown disabled units (not divergent, not matched)
    let mut preset_unknown_disabled = Vec::new();
    for unit_name in &svc.disabled_units {
        if !divergent_set.contains(unit_name.as_str())
            && !matched_set.contains(unit_name.as_str())
        {
            preset_unknown_disabled.push(RefServiceItem {
                unit: unit_name.clone(),
                current_state: inspectah_core::types::services::ServiceUnitState::Disabled,
                default_state: None,
                owning_package: None,
                dropin_contents: Vec::new(),
            });
        }
    }

    // Omitted services (package proven absent)
    let omitted: Vec<RefOmittedService> = render_plan
        .omissions
        .iter()
        .map(|o| RefOmittedService {
            unit: o.unit.clone(),
            package: o.owning_package.clone(),
            reason: format!("package '{}' not in target image", o.owning_package),
        })
        .collect();

    // Service advisories (presence uncertain)
    let advisories: Vec<RefServiceAdvisory> = render_plan
        .advisories
        .iter()
        .map(|a| RefServiceAdvisory {
            unit: a.unit.clone(),
            owning_package: a.owning_package.clone(),
            reasons: a.reasons.clone(),
        })
        .collect();

    // Service warnings (from collector)
    let warnings: Vec<RefServiceWarning> = snap
        .warnings
        .iter()
        .filter(|w| w.inspector == "services")
        .map(|w| {
            let unit = w
                .extra
                .get("unit")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            RefServiceWarning {
                unit,
                message: w.message.clone(),
            }
        })
        .collect();

    RefServices {
        divergent,
        preset_matched_with_dropins,
        preset_unknown_enabled,
        preset_unknown_disabled,
        standalone_dropins,
        omitted,
        advisories,
        warnings,
    }
}

/// Project container data from snapshot into reference format.
///
/// Extracts four container subsections:
/// - **Quadlets** — systemd-managed container units
/// - **Compose files** — docker-compose/podman-compose definitions
/// - **Running containers** — live container state at scan time
/// - **Flatpaks** — installed Flatpak applications
pub fn project_ref_containers(snap: &InspectionSnapshot) -> RefContainers {
    let ct = match &snap.containers {
        Some(c) => c,
        None => return RefContainers::default(),
    };

    let quadlets = ct
        .quadlet_units
        .iter()
        .map(|q| RefQuadletItem {
            name: q.name.clone(),
            image: q.image.clone(),
            path: q.path.clone(),
            content: q.content.clone(),
            ports: q.ports.clone(),
            volumes: q.volumes.clone(),
        })
        .collect();

    let compose_files = ct
        .compose_files
        .iter()
        .map(|cf| RefComposeItem {
            path: cf.path.clone(),
            services: cf.images.clone(),
            include: cf.include,
        })
        .collect();

    let running_containers = ct
        .running_containers
        .iter()
        .map(|rc| RefRunningContainerItem {
            id: rc.id.clone(),
            name: rc.name.clone(),
            image: rc.image.clone(),
            status: rc.status.clone(),
            env: rc.env.clone(),
            mounts: rc
                .mounts
                .iter()
                .map(|m| ContainerMount {
                    mount_type: m.mount_type.clone(),
                    source: m.source.clone(),
                    destination: m.destination.clone(),
                })
                .collect(),
            restart_policy: rc.restart_policy.clone(),
        })
        .collect();

    let flatpaks = ct
        .flatpak_apps
        .iter()
        .map(|f| RefFlatpakRefItem {
            app_id: f.app_id.clone(),
            origin: f.origin.clone(),
            branch: f.branch.clone(),
            remote: f.remote.clone(),
            remote_url: f.remote_url.clone(),
        })
        .collect();

    RefContainers {
        quadlets,
        compose_files,
        running_containers,
        flatpaks,
    }
}

/// Project kernel/boot data from snapshot into reference format.
///
/// Extracts kernel command line, GRUB defaults, tuned profile, locale/timezone,
/// sysctl overrides, non-default modules, and config snippets (modules-load.d,
/// modprobe.d, dracut.conf.d, custom tuned profiles, alternatives).
///
/// String fields use `Option<String>` with `None` for empty values so the web
/// adapter can distinguish "not collected" from "collected but empty".
pub fn project_ref_kernel_boot(snap: &InspectionSnapshot) -> RefKernelBoot {
    let kb = match &snap.kernel_boot {
        Some(k) => k,
        None => return RefKernelBoot::default(),
    };

    let non_empty = |s: &str| -> Option<String> {
        if s.is_empty() {
            None
        } else {
            Some(s.to_string())
        }
    };

    let sysctl_overrides = kb
        .sysctl_overrides
        .iter()
        .map(|s| RefSysctlOverride {
            key: s.key.clone(),
            runtime: s.runtime.clone(),
            default: s.default.clone(),
            source: s.source.clone(),
        })
        .collect();

    let non_default_modules = kb
        .non_default_modules
        .iter()
        .map(|m| RefKernelModule {
            name: m.name.clone(),
            size: m.size.clone(),
            used_by: m.used_by.clone(),
        })
        .collect();

    let map_snippets = |snippets: &[inspectah_core::types::kernelboot::ConfigSnippet]| -> Vec<RefConfigSnippet> {
        snippets
            .iter()
            .map(|s| RefConfigSnippet {
                path: s.path.clone(),
                content: s.content.clone(),
            })
            .collect()
    };

    RefKernelBoot {
        cmdline: non_empty(&kb.cmdline),
        grub_defaults: non_empty(&kb.grub_defaults),
        tuned_active: non_empty(&kb.tuned_active),
        locale: kb.locale.clone(),
        timezone: kb.timezone.clone(),
        sysctl_overrides,
        non_default_modules,
        modules_load_d: map_snippets(&kb.modules_load_d),
        modprobe_d: map_snippets(&kb.modprobe_d),
        dracut_conf: map_snippets(&kb.dracut_conf),
        custom_tuned_profiles: map_snippets(&kb.tuned_custom_profiles),
        alternatives: kb
            .alternatives
            .iter()
            .map(|a| RefAlternativeEntry {
                name: a.name.clone(),
                path: a.path.clone(),
                status: a.status.clone(),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::baseline::BaselineData;
    use inspectah_core::types::rpm::{RpmSection, VersionChange};
    use std::collections::HashMap;

    #[test]
    fn test_empty_snapshot_returns_data_unavailable() {
        let snap = InspectionSnapshot {
            rpm: None,
            ..Default::default()
        };

        let result = project_ref_version_changes(&snap);

        assert!(result.downgrades.is_empty());
        assert!(result.upgrades.is_empty());
        assert_eq!(result.empty_reason, Some(EmptyReason::DataUnavailable));
    }

    #[test]
    fn test_no_baseline_returns_no_baseline() {
        let snap = InspectionSnapshot {
            rpm: Some(RpmSection {
                version_changes: Vec::new(),
                ..Default::default()
            }),
            baseline: None,
            ..Default::default()
        };

        let result = project_ref_version_changes(&snap);

        assert!(result.downgrades.is_empty());
        assert!(result.upgrades.is_empty());
        assert_eq!(result.empty_reason, Some(EmptyReason::NoBaseline));
    }

    #[test]
    fn test_baseline_with_no_changes_returns_zero_drift() {
        let snap = InspectionSnapshot {
            rpm: Some(RpmSection {
                version_changes: Vec::new(),
                ..Default::default()
            }),
            baseline: Some(BaselineData {
                image_digest: "sha256:test".to_string(),
                packages: HashMap::new(),
                extracted_at: "2024-01-01T00:00:00Z".to_string(),
            }),
            ..Default::default()
        };

        let result = project_ref_version_changes(&snap);

        assert!(result.downgrades.is_empty());
        assert!(result.upgrades.is_empty());
        assert_eq!(result.empty_reason, Some(EmptyReason::ZeroDrift));
    }

    #[test]
    fn test_partition_by_direction() {
        let snap = InspectionSnapshot {
            rpm: Some(RpmSection {
                version_changes: vec![
                    VersionChange {
                        name: "pkg1".to_string(),
                        arch: "x86_64".to_string(),
                        host_version: "2.0".to_string(),
                        base_version: "1.0".to_string(),
                        host_epoch: "0".to_string(),
                        base_epoch: "0".to_string(),
                        direction: VersionChangeDirection::Upgrade,
                    },
                    VersionChange {
                        name: "pkg2".to_string(),
                        arch: "x86_64".to_string(),
                        host_version: "1.0".to_string(),
                        base_version: "2.0".to_string(),
                        host_epoch: "0".to_string(),
                        base_epoch: "0".to_string(),
                        direction: VersionChangeDirection::Downgrade,
                    },
                    VersionChange {
                        name: "pkg3".to_string(),
                        arch: "aarch64".to_string(),
                        host_version: "3.0".to_string(),
                        base_version: "2.5".to_string(),
                        host_epoch: "1".to_string(),
                        base_epoch: "0".to_string(),
                        direction: VersionChangeDirection::Upgrade,
                    },
                ],
                ..Default::default()
            }),
            baseline: Some(BaselineData {
                image_digest: "sha256:test".to_string(),
                packages: HashMap::new(),
                extracted_at: "2024-01-01T00:00:00Z".to_string(),
            }),
            ..Default::default()
        };

        let result = project_ref_version_changes(&snap);

        assert_eq!(result.downgrades.len(), 1);
        assert_eq!(result.upgrades.len(), 2);
        assert_eq!(result.empty_reason, None);

        assert_eq!(result.downgrades[0].name, "pkg2");
        assert_eq!(result.upgrades[0].name, "pkg1");
        assert_eq!(result.upgrades[1].name, "pkg3");
    }

    // ── project_ref_services tests ──────────────────────────────────

    use inspectah_core::types::services::{
        PresetDefault, ServiceSection, ServiceStateChange, ServiceUnitState, SystemdDropIn,
    };
    use inspectah_core::types::warnings::Warning;

    #[test]
    fn test_no_services_returns_default() {
        let snap = InspectionSnapshot {
            services: None,
            ..Default::default()
        };

        let result = project_ref_services(&snap);

        assert!(result.divergent.is_empty());
        assert!(result.preset_matched_with_dropins.is_empty());
        assert!(result.preset_unknown_enabled.is_empty());
        assert!(result.preset_unknown_disabled.is_empty());
        assert!(result.standalone_dropins.is_empty());
        assert!(result.omitted.is_empty());
        assert!(result.advisories.is_empty());
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn test_divergent_service_categorized() {
        let snap = InspectionSnapshot {
            services: Some(ServiceSection {
                state_changes: vec![ServiceStateChange {
                    unit: "firewalld.service".into(),
                    current_state: ServiceUnitState::Enabled,
                    default_state: Some(PresetDefault::Disable),
                    include: true,
                    owning_package: Some("firewalld".into()),
                    fleet: None,
                    attention_reason: None,
                }],
                enabled_units: vec!["firewalld.service".into()],
                disabled_units: Vec::new(),
                drop_ins: Vec::new(),
                preset_matched_units: Vec::new(),
            }),
            ..Default::default()
        };

        let result = project_ref_services(&snap);

        assert_eq!(result.divergent.len(), 1);
        assert_eq!(result.divergent[0].unit, "firewalld.service");
        assert_eq!(result.divergent[0].current_state, ServiceUnitState::Enabled);
        assert_eq!(
            result.divergent[0].default_state,
            Some(PresetDefault::Disable)
        );
        assert_eq!(
            result.divergent[0].owning_package,
            Some("firewalld".into())
        );
        // firewalld is in enabled_units AND divergent_set, so it should NOT
        // also appear in preset_unknown_enabled.
        assert!(result.preset_unknown_enabled.is_empty());
    }

    #[test]
    fn test_preset_matched_with_dropin_visible() {
        let snap = InspectionSnapshot {
            services: Some(ServiceSection {
                state_changes: Vec::new(),
                enabled_units: vec!["chronyd.service".into()],
                disabled_units: Vec::new(),
                drop_ins: vec![SystemdDropIn {
                    unit: "chronyd.service".into(),
                    path: "/etc/systemd/system/chronyd.service.d/override.conf".into(),
                    content: "[Service]\nExecStart=".into(),
                    include: true,
                    ..Default::default()
                }],
                preset_matched_units: vec!["chronyd.service".into()],
            }),
            ..Default::default()
        };

        let result = project_ref_services(&snap);

        assert_eq!(result.preset_matched_with_dropins.len(), 1);
        assert_eq!(
            result.preset_matched_with_dropins[0].unit,
            "chronyd.service"
        );
        assert_eq!(
            result.preset_matched_with_dropins[0].current_state,
            ServiceUnitState::Enabled
        );
        assert_eq!(result.preset_matched_with_dropins[0].dropin_contents.len(), 1);
        // Should NOT appear in preset_unknown_enabled since it's matched.
        assert!(result.preset_unknown_enabled.is_empty());
    }

    #[test]
    fn test_preset_matched_without_dropin_suppressed() {
        let snap = InspectionSnapshot {
            services: Some(ServiceSection {
                state_changes: Vec::new(),
                enabled_units: vec!["chronyd.service".into()],
                disabled_units: Vec::new(),
                drop_ins: Vec::new(),
                preset_matched_units: vec!["chronyd.service".into()],
            }),
            ..Default::default()
        };

        let result = project_ref_services(&snap);

        // No drop-in means matched unit is suppressed entirely.
        assert!(result.preset_matched_with_dropins.is_empty());
        // And it should NOT appear in preset_unknown_enabled either.
        assert!(result.preset_unknown_enabled.is_empty());
    }

    #[test]
    fn test_unknown_enabled_and_disabled_categorized() {
        let snap = InspectionSnapshot {
            services: Some(ServiceSection {
                state_changes: Vec::new(),
                enabled_units: vec!["custom-agent.service".into()],
                disabled_units: vec!["unused.service".into()],
                drop_ins: Vec::new(),
                preset_matched_units: Vec::new(),
            }),
            ..Default::default()
        };

        let result = project_ref_services(&snap);

        assert_eq!(result.preset_unknown_enabled.len(), 1);
        assert_eq!(result.preset_unknown_enabled[0].unit, "custom-agent.service");
        assert_eq!(
            result.preset_unknown_enabled[0].current_state,
            ServiceUnitState::Enabled
        );

        assert_eq!(result.preset_unknown_disabled.len(), 1);
        assert_eq!(result.preset_unknown_disabled[0].unit, "unused.service");
        assert_eq!(
            result.preset_unknown_disabled[0].current_state,
            ServiceUnitState::Disabled
        );
    }

    #[test]
    fn test_standalone_dropin_for_uncovered_unit() {
        let snap = InspectionSnapshot {
            services: Some(ServiceSection {
                state_changes: Vec::new(),
                enabled_units: Vec::new(),
                disabled_units: Vec::new(),
                drop_ins: vec![SystemdDropIn {
                    unit: "phantom.service".into(),
                    path: "/etc/systemd/system/phantom.service.d/10-custom.conf".into(),
                    content: "[Service]\nRestart=always".into(),
                    include: true,
                    ..Default::default()
                }],
                preset_matched_units: Vec::new(),
            }),
            ..Default::default()
        };

        let result = project_ref_services(&snap);

        assert_eq!(result.standalone_dropins.len(), 1);
        assert_eq!(result.standalone_dropins[0].unit, "phantom.service");
        assert_eq!(
            result.standalone_dropins[0].path,
            "/etc/systemd/system/phantom.service.d/10-custom.conf"
        );
        assert_eq!(
            result.standalone_dropins[0].content,
            "[Service]\nRestart=always"
        );
    }

    #[test]
    fn test_dropin_folded_into_divergent_item() {
        let snap = InspectionSnapshot {
            services: Some(ServiceSection {
                state_changes: vec![ServiceStateChange {
                    unit: "httpd.service".into(),
                    current_state: ServiceUnitState::Enabled,
                    default_state: Some(PresetDefault::Disable),
                    include: true,
                    owning_package: Some("httpd".into()),
                    fleet: None,
                    attention_reason: None,
                }],
                enabled_units: vec!["httpd.service".into()],
                disabled_units: Vec::new(),
                drop_ins: vec![SystemdDropIn {
                    unit: "httpd.service".into(),
                    path: "/etc/systemd/system/httpd.service.d/override.conf".into(),
                    content: "[Service]\nLimitNOFILE=65536".into(),
                    include: true,
                    ..Default::default()
                }],
                preset_matched_units: Vec::new(),
            }),
            ..Default::default()
        };

        let result = project_ref_services(&snap);

        assert_eq!(result.divergent.len(), 1);
        assert_eq!(result.divergent[0].dropin_contents.len(), 1);
        assert_eq!(
            result.divergent[0].dropin_contents[0],
            "[Service]\nLimitNOFILE=65536"
        );
        // Should NOT also appear as standalone.
        assert!(result.standalone_dropins.is_empty());
    }

    #[test]
    fn test_three_way_split() {
        let snap = InspectionSnapshot {
            services: Some(ServiceSection {
                state_changes: vec![ServiceStateChange {
                    unit: "firewalld.service".into(),
                    current_state: ServiceUnitState::Enabled,
                    default_state: Some(PresetDefault::Disable),
                    include: true,
                    owning_package: Some("firewalld".into()),
                    fleet: None,
                    attention_reason: None,
                }],
                enabled_units: vec![
                    "firewalld.service".into(),
                    "custom.service".into(),
                ],
                disabled_units: vec!["unused.service".into()],
                drop_ins: Vec::new(),
                preset_matched_units: Vec::new(),
            }),
            ..Default::default()
        };

        let result = project_ref_services(&snap);

        // firewalld is divergent
        assert_eq!(result.divergent.len(), 1);
        assert_eq!(result.divergent[0].unit, "firewalld.service");
        // custom.service is enabled but not divergent or matched
        assert_eq!(result.preset_unknown_enabled.len(), 1);
        assert_eq!(result.preset_unknown_enabled[0].unit, "custom.service");
        // unused.service is disabled but not divergent or matched
        assert_eq!(result.preset_unknown_disabled.len(), 1);
        assert_eq!(result.preset_unknown_disabled[0].unit, "unused.service");
    }

    #[test]
    fn test_service_warnings_extracted() {
        let snap = InspectionSnapshot {
            services: Some(ServiceSection {
                state_changes: Vec::new(),
                enabled_units: Vec::new(),
                disabled_units: Vec::new(),
                drop_ins: Vec::new(),
                preset_matched_units: Vec::new(),
            }),
            warnings: vec![
                Warning {
                    inspector: "services".into(),
                    message: "unit file not found".into(),
                    extra: [("unit".to_string(), serde_json::json!("ghost.service"))]
                        .into_iter()
                        .collect(),
                    ..Default::default()
                },
                Warning {
                    inspector: "network".into(),
                    message: "unrelated warning".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };

        let result = project_ref_services(&snap);

        assert_eq!(result.warnings.len(), 1);
        assert_eq!(result.warnings[0].unit, "ghost.service");
        assert_eq!(result.warnings[0].message, "unit file not found");
    }

    #[test]
    fn test_masked_service_in_divergent() {
        let snap = InspectionSnapshot {
            services: Some(ServiceSection {
                state_changes: vec![ServiceStateChange {
                    unit: "cups.service".into(),
                    current_state: ServiceUnitState::Masked,
                    default_state: None,
                    include: true,
                    owning_package: Some("cups".into()),
                    fleet: None,
                    attention_reason: None,
                }],
                enabled_units: Vec::new(),
                disabled_units: Vec::new(),
                drop_ins: Vec::new(),
                preset_matched_units: Vec::new(),
            }),
            ..Default::default()
        };

        let result = project_ref_services(&snap);

        assert_eq!(result.divergent.len(), 1);
        assert_eq!(result.divergent[0].current_state, ServiceUnitState::Masked);
        assert_eq!(result.divergent[0].default_state, None);
    }

    // ── project_ref_containers tests ────────────────────────────────

    use inspectah_core::types::containers::{
        ComposeFile, ComposeService, ContainerSection, FlatpakApp, QuadletUnit, RunningContainer,
    };

    #[test]
    fn test_no_containers_returns_default() {
        let snap = InspectionSnapshot {
            containers: None,
            ..Default::default()
        };

        let result = project_ref_containers(&snap);

        assert!(result.quadlets.is_empty());
        assert!(result.compose_files.is_empty());
        assert!(result.running_containers.is_empty());
        assert!(result.flatpaks.is_empty());
    }

    #[test]
    fn test_empty_container_section_returns_default() {
        let snap = InspectionSnapshot {
            containers: Some(ContainerSection::default()),
            ..Default::default()
        };

        let result = project_ref_containers(&snap);

        assert!(result.quadlets.is_empty());
        assert!(result.compose_files.is_empty());
        assert!(result.running_containers.is_empty());
        assert!(result.flatpaks.is_empty());
    }

    #[test]
    fn test_quadlet_extraction() {
        let snap = InspectionSnapshot {
            containers: Some(ContainerSection {
                quadlet_units: vec![QuadletUnit {
                    name: "myapp.container".into(),
                    image: "quay.io/myorg/myapp:latest".into(),
                    path: "/etc/containers/systemd/myapp.container".into(),
                    content: "[Container]\nImage=quay.io/myorg/myapp:latest".into(),
                    ports: vec!["8080:80".into()],
                    volumes: vec!["/data:/app/data".into()],
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };

        let result = project_ref_containers(&snap);

        assert_eq!(result.quadlets.len(), 1);
        assert_eq!(result.quadlets[0].name, "myapp.container");
        assert_eq!(result.quadlets[0].image, "quay.io/myorg/myapp:latest");
        assert_eq!(
            result.quadlets[0].path,
            "/etc/containers/systemd/myapp.container"
        );
        assert_eq!(result.quadlets[0].ports, vec!["8080:80"]);
        assert_eq!(result.quadlets[0].volumes, vec!["/data:/app/data"]);
        assert!(!result.quadlets[0].content.is_empty());
    }

    #[test]
    fn test_compose_file_extraction() {
        let snap = InspectionSnapshot {
            containers: Some(ContainerSection {
                compose_files: vec![ComposeFile {
                    path: "/opt/app/docker-compose.yml".into(),
                    images: vec![
                        ComposeService {
                            service: "web".into(),
                            image: "nginx:latest".into(),
                        },
                        ComposeService {
                            service: "db".into(),
                            image: "postgres:16".into(),
                        },
                    ],
                    include: true,
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };

        let result = project_ref_containers(&snap);

        assert_eq!(result.compose_files.len(), 1);
        assert_eq!(result.compose_files[0].path, "/opt/app/docker-compose.yml");
        assert_eq!(result.compose_files[0].services.len(), 2);
        assert_eq!(result.compose_files[0].services[0].service, "web");
        assert_eq!(result.compose_files[0].services[1].image, "postgres:16");
        assert!(result.compose_files[0].include);
    }

    #[test]
    fn test_running_container_extraction() {
        let snap = InspectionSnapshot {
            containers: Some(ContainerSection {
                running_containers: vec![RunningContainer {
                    id: "abc123".into(),
                    name: "my-nginx".into(),
                    image: "nginx:1.25".into(),
                    status: "Up 3 hours".into(),
                    env: vec!["NGINX_PORT=80".into()],
                    mounts: vec![inspectah_core::types::containers::ContainerMount {
                        mount_type: "bind".into(),
                        source: "/host/data".into(),
                        destination: "/data".into(),
                        mode: "rw".into(),
                        rw: true,
                    }],
                    restart_policy: "always".into(),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };

        let result = project_ref_containers(&snap);

        assert_eq!(result.running_containers.len(), 1);
        let rc = &result.running_containers[0];
        assert_eq!(rc.id, "abc123");
        assert_eq!(rc.name, "my-nginx");
        assert_eq!(rc.image, "nginx:1.25");
        assert_eq!(rc.status, "Up 3 hours");
        assert_eq!(rc.env, vec!["NGINX_PORT=80"]);
        assert_eq!(rc.restart_policy, "always");
        assert_eq!(rc.mounts.len(), 1);
        assert_eq!(rc.mounts[0].mount_type, "bind");
        assert_eq!(rc.mounts[0].source, "/host/data");
        assert_eq!(rc.mounts[0].destination, "/data");
    }

    #[test]
    fn test_flatpak_extraction() {
        let snap = InspectionSnapshot {
            containers: Some(ContainerSection {
                flatpak_apps: vec![FlatpakApp {
                    app_id: "org.gnome.Calculator".into(),
                    origin: "flathub".into(),
                    branch: "stable".into(),
                    remote: "flathub".into(),
                    remote_url: "https://dl.flathub.org/repo/".into(),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };

        let result = project_ref_containers(&snap);

        assert_eq!(result.flatpaks.len(), 1);
        assert_eq!(result.flatpaks[0].app_id, "org.gnome.Calculator");
        assert_eq!(result.flatpaks[0].origin, "flathub");
        assert_eq!(result.flatpaks[0].branch, "stable");
        assert_eq!(result.flatpaks[0].remote, "flathub");
        assert_eq!(
            result.flatpaks[0].remote_url,
            "https://dl.flathub.org/repo/"
        );
    }

    #[test]
    fn test_all_container_subsections_together() {
        let snap = InspectionSnapshot {
            containers: Some(ContainerSection {
                quadlet_units: vec![QuadletUnit {
                    name: "app.container".into(),
                    image: "img:1".into(),
                    ..Default::default()
                }],
                compose_files: vec![ComposeFile {
                    path: "/compose.yml".into(),
                    ..Default::default()
                }],
                running_containers: vec![RunningContainer {
                    id: "r1".into(),
                    name: "run1".into(),
                    ..Default::default()
                }],
                flatpak_apps: vec![FlatpakApp {
                    app_id: "com.example.App".into(),
                    ..Default::default()
                }],
            }),
            ..Default::default()
        };

        let result = project_ref_containers(&snap);

        assert_eq!(result.quadlets.len(), 1);
        assert_eq!(result.compose_files.len(), 1);
        assert_eq!(result.running_containers.len(), 1);
        assert_eq!(result.flatpaks.len(), 1);
    }

    // ── project_ref_kernel_boot tests ───────────────────────────────

    use inspectah_core::types::kernelboot::{
        AlternativeEntry, ConfigSnippet, KernelBootSection, KernelModule, SysctlOverride,
    };

    #[test]
    fn test_no_kernel_boot_returns_default() {
        let snap = InspectionSnapshot {
            kernel_boot: None,
            ..Default::default()
        };

        let result = project_ref_kernel_boot(&snap);

        assert!(result.cmdline.is_none());
        assert!(result.grub_defaults.is_none());
        assert!(result.tuned_active.is_none());
        assert!(result.locale.is_none());
        assert!(result.timezone.is_none());
        assert!(result.sysctl_overrides.is_empty());
        assert!(result.non_default_modules.is_empty());
        assert!(result.modules_load_d.is_empty());
        assert!(result.modprobe_d.is_empty());
        assert!(result.dracut_conf.is_empty());
        assert!(result.custom_tuned_profiles.is_empty());
        assert!(result.alternatives.is_empty());
    }

    #[test]
    fn test_empty_kernel_boot_returns_none_strings() {
        let snap = InspectionSnapshot {
            kernel_boot: Some(KernelBootSection::default()),
            ..Default::default()
        };

        let result = project_ref_kernel_boot(&snap);

        // Empty strings become None
        assert!(result.cmdline.is_none());
        assert!(result.grub_defaults.is_none());
        assert!(result.tuned_active.is_none());
    }

    #[test]
    fn test_cmdline_preserved_untruncated() {
        let long_cmdline =
            "BOOT_IMAGE=(hd0,msdos1)/vmlinuz root=/dev/mapper/rhel-root ro crashkernel=auto \
             rd.lvm.lv=rhel/root rd.lvm.lv=rhel/swap rhgb quiet net.ifnames=0 biosdevname=0";
        let snap = InspectionSnapshot {
            kernel_boot: Some(KernelBootSection {
                cmdline: long_cmdline.to_string(),
                ..Default::default()
            }),
            ..Default::default()
        };

        let result = project_ref_kernel_boot(&snap);

        assert_eq!(result.cmdline.as_deref(), Some(long_cmdline));
    }

    #[test]
    fn test_sysctl_overrides_extracted() {
        let snap = InspectionSnapshot {
            kernel_boot: Some(KernelBootSection {
                sysctl_overrides: vec![
                    SysctlOverride {
                        key: "kernel.sysrq".into(),
                        runtime: "16".into(),
                        default: "0".into(),
                        source: "/etc/sysctl.d/99-custom.conf".into(),
                        ..Default::default()
                    },
                    SysctlOverride {
                        key: "net.ipv4.ip_forward".into(),
                        runtime: "1".into(),
                        default: "0".into(),
                        source: "/etc/sysctl.d/k8s.conf".into(),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            }),
            ..Default::default()
        };

        let result = project_ref_kernel_boot(&snap);

        assert_eq!(result.sysctl_overrides.len(), 2);
        assert_eq!(result.sysctl_overrides[0].key, "kernel.sysrq");
        assert_eq!(result.sysctl_overrides[0].runtime, "16");
        assert_eq!(
            result.sysctl_overrides[0].source,
            "/etc/sysctl.d/99-custom.conf"
        );
        assert_eq!(result.sysctl_overrides[1].key, "net.ipv4.ip_forward");
    }

    #[test]
    fn test_non_default_modules_extracted() {
        let snap = InspectionSnapshot {
            kernel_boot: Some(KernelBootSection {
                non_default_modules: vec![KernelModule {
                    name: "overlay".into(),
                    size: "151552".into(),
                    used_by: "".into(),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };

        let result = project_ref_kernel_boot(&snap);

        assert_eq!(result.non_default_modules.len(), 1);
        assert_eq!(result.non_default_modules[0].name, "overlay");
        assert_eq!(result.non_default_modules[0].size, "151552");
    }

    #[test]
    fn test_config_snippets_extracted() {
        let snap = InspectionSnapshot {
            kernel_boot: Some(KernelBootSection {
                modules_load_d: vec![ConfigSnippet {
                    path: "/etc/modules-load.d/br_netfilter.conf".into(),
                    content: "br_netfilter".into(),
                }],
                modprobe_d: vec![ConfigSnippet {
                    path: "/etc/modprobe.d/blacklist.conf".into(),
                    content: "blacklist nouveau".into(),
                }],
                dracut_conf: vec![ConfigSnippet {
                    path: "/etc/dracut.conf.d/custom.conf".into(),
                    content: "add_drivers+=\" iscsi \"".into(),
                }],
                tuned_custom_profiles: vec![ConfigSnippet {
                    path: "/etc/tuned/myprofile/tuned.conf".into(),
                    content: "[main]\ninclude=throughput-performance".into(),
                }],
                ..Default::default()
            }),
            ..Default::default()
        };

        let result = project_ref_kernel_boot(&snap);

        assert_eq!(result.modules_load_d.len(), 1);
        assert_eq!(
            result.modules_load_d[0].path,
            "/etc/modules-load.d/br_netfilter.conf"
        );
        assert_eq!(result.modules_load_d[0].content, "br_netfilter");

        assert_eq!(result.modprobe_d.len(), 1);
        assert_eq!(result.modprobe_d[0].content, "blacklist nouveau");

        assert_eq!(result.dracut_conf.len(), 1);
        assert_eq!(
            result.dracut_conf[0].content,
            "add_drivers+=\" iscsi \""
        );

        assert_eq!(result.custom_tuned_profiles.len(), 1);
        assert_eq!(
            result.custom_tuned_profiles[0].path,
            "/etc/tuned/myprofile/tuned.conf"
        );
    }

    #[test]
    fn test_locale_timezone_alternatives() {
        let snap = InspectionSnapshot {
            kernel_boot: Some(KernelBootSection {
                locale: Some("en_US.UTF-8".into()),
                timezone: Some("America/New_York".into()),
                alternatives: vec![AlternativeEntry {
                    name: "python3".into(),
                    path: "/usr/bin/python3.11".into(),
                    status: "auto".into(),
                }],
                ..Default::default()
            }),
            ..Default::default()
        };

        let result = project_ref_kernel_boot(&snap);

        assert_eq!(result.locale.as_deref(), Some("en_US.UTF-8"));
        assert_eq!(result.timezone.as_deref(), Some("America/New_York"));
        assert_eq!(result.alternatives.len(), 1);
        assert_eq!(result.alternatives[0].name, "python3");
        assert_eq!(result.alternatives[0].path, "/usr/bin/python3.11");
        assert_eq!(result.alternatives[0].status, "auto");
    }

    #[test]
    fn test_full_kernel_boot_roundtrip() {
        let snap = InspectionSnapshot {
            kernel_boot: Some(KernelBootSection {
                cmdline: "quiet crashkernel=auto".into(),
                grub_defaults: "GRUB_TIMEOUT=5".into(),
                tuned_active: "virtual-guest".into(),
                locale: Some("C.UTF-8".into()),
                timezone: Some("UTC".into()),
                sysctl_overrides: vec![SysctlOverride {
                    key: "vm.swappiness".into(),
                    runtime: "10".into(),
                    default: "60".into(),
                    source: "/etc/sysctl.d/swap.conf".into(),
                    ..Default::default()
                }],
                non_default_modules: vec![KernelModule {
                    name: "vfio".into(),
                    size: "32768".into(),
                    used_by: "vfio-pci".into(),
                    ..Default::default()
                }],
                modules_load_d: vec![ConfigSnippet {
                    path: "/etc/modules-load.d/vfio.conf".into(),
                    content: "vfio\nvfio-pci".into(),
                }],
                modprobe_d: Vec::new(),
                dracut_conf: Vec::new(),
                tuned_custom_profiles: Vec::new(),
                alternatives: vec![AlternativeEntry {
                    name: "java".into(),
                    path: "/usr/lib/jvm/java-17".into(),
                    status: "manual".into(),
                }],
                ..Default::default()
            }),
            ..Default::default()
        };

        let result = project_ref_kernel_boot(&snap);

        assert_eq!(result.cmdline.as_deref(), Some("quiet crashkernel=auto"));
        assert_eq!(result.grub_defaults.as_deref(), Some("GRUB_TIMEOUT=5"));
        assert_eq!(result.tuned_active.as_deref(), Some("virtual-guest"));
        assert_eq!(result.locale.as_deref(), Some("C.UTF-8"));
        assert_eq!(result.timezone.as_deref(), Some("UTC"));
        assert_eq!(result.sysctl_overrides.len(), 1);
        assert_eq!(result.non_default_modules.len(), 1);
        assert_eq!(result.non_default_modules[0].used_by, "vfio-pci");
        assert_eq!(result.modules_load_d.len(), 1);
        assert!(result.modprobe_d.is_empty());
        assert!(result.dracut_conf.is_empty());
        assert!(result.custom_tuned_profiles.is_empty());
        assert_eq!(result.alternatives.len(), 1);
        assert_eq!(result.alternatives[0].status, "manual");
    }
}
