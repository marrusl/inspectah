use std::collections::HashSet;

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::rpm::VersionChangeDirection;
use inspectah_pipeline::render::service_intent::render_service_intent;

use super::types::{
    EmptyReason, RefDropInItem, RefOmittedService, RefServiceAdvisory, RefServiceItem,
    RefServiceWarning, RefServices, RefVersionChangeItem, RefVersionChanges,
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
}
