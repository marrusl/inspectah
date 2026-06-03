use std::collections::HashSet;
use std::path::Path;

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::rpm::VersionChangeDirection;
use inspectah_pipeline::render::service_intent::render_service_intent;

use super::types::{
    ContainerMount, EmptyReason, GenericRefItem, RefAlternativeEntry, RefComposeItem,
    RefConfigSnippet, RefContainers, RefCredentialRef, RefDropInItem, RefFirewallDirectRule,
    RefFirewallZone, RefFlatpakRefItem, RefFstabEntry, RefKernelBoot, RefKernelModule,
    RefLvmVolume, RefMountPoint, RefNMConnection, RefNetwork, RefOmittedService, RefProxyEnv,
    RefQuadletItem, RefRunningContainerItem, RefServiceAdvisory, RefServiceItem,
    RefServiceWarning, RefServices, RefStaticRoute, RefStorage, RefSysctlOverride,
    RefVarDirectory, RefVersionChangeItem, RefVersionChanges, ReferenceProjection,
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

/// Project network data from snapshot into reference format.
///
/// Extracts NM connections, firewall zones/direct rules, static routes,
/// ip routes/rules, resolv provenance, hosts additions, and proxy env.
pub fn project_ref_network(snap: &InspectionSnapshot) -> RefNetwork {
    let net = match &snap.network {
        Some(n) => n,
        None => return RefNetwork::default(),
    };

    let connections = net
        .connections
        .iter()
        .map(|c| RefNMConnection {
            name: c.name.clone(),
            conn_type: c.conn_type.clone(),
            method: c.method.clone(),
            path: c.path.clone(),
        })
        .collect();

    let firewall_zones = net
        .firewall_zones
        .iter()
        .map(|z| RefFirewallZone {
            name: z.name.clone(),
            path: z.path.clone(),
            content: z.content.clone(),
            services: z.services.clone(),
            ports: z.ports.clone(),
            rich_rules: z.rich_rules.clone(),
        })
        .collect();

    let firewall_direct_rules = net
        .firewall_direct_rules
        .iter()
        .map(|r| RefFirewallDirectRule {
            ipv: r.ipv.clone(),
            table: r.table.clone(),
            chain: r.chain.clone(),
            priority: r.priority.clone(),
            args: r.args.clone(),
        })
        .collect();

    let static_routes = net
        .static_routes
        .iter()
        .map(|s| RefStaticRoute {
            path: s.path.clone(),
            name: s.name.clone(),
        })
        .collect();

    let proxy_env = net
        .proxy
        .iter()
        .map(|p| RefProxyEnv {
            source: p.source.clone(),
            line: p.line.clone(),
        })
        .collect();

    RefNetwork {
        connections,
        firewall_zones,
        firewall_direct_rules,
        static_routes,
        ip_routes: net.ip_routes.clone(),
        ip_rules: net.ip_rules.clone(),
        resolv_provenance: net.resolv_provenance.clone(),
        hosts_additions: net.hosts_additions.clone(),
        proxy_env,
    }
}

/// Project storage data from snapshot into reference format.
///
/// Extracts fstab entries, mount points, LVM volumes, /var directories,
/// and credential references.
pub fn project_ref_storage(snap: &InspectionSnapshot) -> RefStorage {
    let st = match &snap.storage {
        Some(s) => s,
        None => return RefStorage::default(),
    };

    let fstab_entries = st
        .fstab_entries
        .iter()
        .map(|e| RefFstabEntry {
            device: e.device.clone(),
            mount_point: e.mount_point.clone(),
            fstype: e.fstype.clone(),
            options: e.options.clone(),
        })
        .collect();

    let mount_points = st
        .mount_points
        .iter()
        .map(|m| RefMountPoint {
            target: m.target.clone(),
            source: m.source.clone(),
            fstype: m.fstype.clone(),
            options: m.options.clone(),
        })
        .collect();

    let lvm_volumes = st
        .lvm_info
        .iter()
        .map(|v| RefLvmVolume {
            vg_name: v.vg_name.clone(),
            lv_name: v.lv_name.clone(),
            lv_size: v.lv_size.clone(),
        })
        .collect();

    let var_directories = st
        .var_directories
        .iter()
        .map(|d| RefVarDirectory {
            path: d.path.clone(),
            size_estimate: d.size_estimate.clone(),
            recommendation: d.recommendation.clone(),
        })
        .collect();

    let credential_refs = st
        .credential_refs
        .iter()
        .map(|c| RefCredentialRef {
            credential_path: c.credential_path.clone(),
            mount_point: c.mount_point.clone(),
            source: c.source.clone(),
        })
        .collect();

    RefStorage {
        fstab_entries,
        mount_points,
        lvm_volumes,
        var_directories,
        credential_refs,
    }
}

/// Project scheduled tasks from snapshot into generic reference items.
///
/// Maps four task types to `GenericRefItem`:
/// - **CronJob**: key = file basename, summary = source, tags = `["cron"]`
/// - **SystemdTimer**: key = timer name, summary = on_calendar, content = description + exec_start, tags = `["timer"]`
/// - **AtJob**: key = file name, summary = `user: command`, content = working_dir, tags = `["at"]`
/// - **GeneratedTimerUnit**: key = unit name, summary = cron_expr, content = source_path + command, tags = `["generated-timer"]`
fn project_ref_scheduled_tasks(snap: &InspectionSnapshot) -> Vec<GenericRefItem> {
    let sched = match &snap.scheduled_tasks {
        Some(s) => s,
        None => return Vec::new(),
    };

    let mut items = Vec::new();

    for cj in &sched.cron_jobs {
        let basename = Path::new(&cj.path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| cj.path.clone());
        items.push(GenericRefItem {
            id: cj.path.clone(),
            key: basename,
            summary: if cj.source.is_empty() {
                None
            } else {
                Some(cj.source.clone())
            },
            content: None,
            tags: vec!["cron".into(), cj.path.clone()],
        });
    }

    for st in &sched.systemd_timers {
        let mut content_parts = Vec::new();
        if !st.description.is_empty() {
            content_parts.push(st.description.clone());
        }
        if !st.exec_start.is_empty() {
            content_parts.push(st.exec_start.clone());
        }
        items.push(GenericRefItem {
            id: st.name.clone(),
            key: st.name.clone(),
            summary: if st.on_calendar.is_empty() {
                None
            } else {
                Some(st.on_calendar.clone())
            },
            content: if content_parts.is_empty() {
                None
            } else {
                Some(content_parts.join("\n"))
            },
            tags: vec!["timer".into()],
        });
    }

    for aj in &sched.at_jobs {
        items.push(GenericRefItem {
            id: aj.file.clone(),
            key: aj.file.clone(),
            summary: Some(format!("{}: {}", aj.user, aj.command)),
            content: if aj.working_dir.is_empty() {
                None
            } else {
                Some(aj.working_dir.clone())
            },
            tags: vec!["at".into()],
        });
    }

    for gtu in &sched.generated_timer_units {
        let mut content_parts = Vec::new();
        if !gtu.source_path.is_empty() {
            content_parts.push(gtu.source_path.clone());
        }
        if !gtu.command.is_empty() {
            content_parts.push(gtu.command.clone());
        }
        items.push(GenericRefItem {
            id: gtu.name.clone(),
            key: gtu.name.clone(),
            summary: if gtu.cron_expr.is_empty() {
                None
            } else {
                Some(gtu.cron_expr.clone())
            },
            content: if content_parts.is_empty() {
                None
            } else {
                Some(content_parts.join("\n"))
            },
            tags: vec!["generated-timer".into()],
        });
    }

    items
}

/// Project non-RPM software from snapshot into generic reference items.
///
/// Maps two item types to `GenericRefItem`:
/// - **NonRpmItem**: key = name, summary = `method (confidence)`,
///   content = path (+ pip packages if present), tags = `["non-rpm"]`
/// - **ConfigFileEntry** (env_files): key = file basename, summary = kind label,
///   content = file content, tags = `["env-file"]`
fn project_ref_non_rpm(snap: &InspectionSnapshot) -> Vec<GenericRefItem> {
    let nrpm = match &snap.non_rpm_software {
        Some(n) => n,
        None => return Vec::new(),
    };

    let mut items = Vec::new();

    for item in &nrpm.items {
        let subtitle = if item.version.is_empty() {
            format!("{} ({})", item.method, item.confidence)
        } else {
            format!("{} {} ({})", item.method, item.version, item.confidence)
        };
        let detail = if !item.packages.is_empty() {
            let pkg_list: Vec<String> = item
                .packages
                .iter()
                .map(|p| {
                    if p.version.is_empty() {
                        p.name.clone()
                    } else {
                        format!("{}=={}", p.name, p.version)
                    }
                })
                .collect();
            Some(format!("{}\n{}", item.path, pkg_list.join(", ")))
        } else if item.path.is_empty() {
            None
        } else {
            Some(item.path.clone())
        };

        let mut tags = vec!["non-rpm".into()];
        if !item.lang.is_empty() {
            tags.push(item.lang.clone());
        }

        items.push(GenericRefItem {
            id: item.name.clone(),
            key: item.name.clone(),
            summary: Some(subtitle),
            content: detail,
            tags,
        });
    }

    for ef in &nrpm.env_files {
        let basename = Path::new(&ef.path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| ef.path.clone());
        let kind_str = match ef.kind {
            inspectah_core::types::config::ConfigFileKind::RpmOwnedDefault => "rpm-default",
            inspectah_core::types::config::ConfigFileKind::RpmOwnedModified => "rpm-modified",
            inspectah_core::types::config::ConfigFileKind::Unowned => "unowned",
            inspectah_core::types::config::ConfigFileKind::Orphaned => "orphaned",
            inspectah_core::types::config::ConfigFileKind::BaselineMatch => "baseline-match",
        };
        items.push(GenericRefItem {
            id: ef.path.clone(),
            key: basename,
            summary: Some(kind_str.to_string()),
            content: if ef.content.is_empty() {
                None
            } else {
                Some(ef.content.clone())
            },
            tags: vec!["env-file".into(), ef.path.clone()],
        });
    }

    items
}

/// Project security & access control data from snapshot into generic reference items.
///
/// Maps multiple SELinux-section subtypes to `GenericRefItem`:
/// - **mode**: synthetic item with key `"SELinux mode"`, summary = mode string
/// - **FIPS mode**: always emitted, summary = `"enabled"` / `"disabled"`
/// - **SelinuxPortLabel**: key = `protocol/port`, summary = label type, tags = `["port-label"]`
/// - **boolean_overrides**: key = boolean name, summary = value/state, tags = `["boolean"]`
/// - **custom_modules**: key = module name, tags = `["module"]`
/// - **fcontext_rules**: key = rule text, tags = `["fcontext"]`
/// - **audit_rules** (CarryForwardFile): key = file basename, content = file content, tags = `["audit-rule"]`
/// - **pam_configs** (CarryForwardFile): key = file basename, content = file content, tags = `["pam"]`
fn project_ref_selinux(snap: &InspectionSnapshot) -> Vec<GenericRefItem> {
    let se = match &snap.selinux {
        Some(s) => s,
        None => return Vec::new(),
    };

    let mut items = Vec::new();

    // Mode
    if !se.mode.is_empty() {
        items.push(GenericRefItem {
            id: "selinux_mode".into(),
            key: "SELinux mode".into(),
            summary: Some(se.mode.clone()),
            content: None,
            tags: vec!["mode".into()],
        });
    }

    // FIPS mode — always emitted
    {
        let fips_label = if se.fips_mode { "enabled" } else { "disabled" };
        items.push(GenericRefItem {
            id: "fips_mode".into(),
            key: "FIPS mode".into(),
            summary: Some(fips_label.into()),
            content: None,
            tags: vec!["fips".into()],
        });
    }

    // Port labels
    for pl in &se.port_labels {
        let id = format!("{}/{}", pl.protocol, pl.port);
        items.push(GenericRefItem {
            id: id.clone(),
            key: id,
            summary: Some(pl.label_type.clone()),
            content: None,
            tags: vec!["port-label".into()],
        });
    }

    // Boolean overrides
    for bo in &se.boolean_overrides {
        let name = bo
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let value = bo
            .get("value")
            .or_else(|| bo.get("state"))
            .map(|v| v.to_string())
            .unwrap_or_default();
        items.push(GenericRefItem {
            id: name.clone(),
            key: name,
            summary: Some(value),
            content: None,
            tags: vec!["boolean".into()],
        });
    }

    // Custom modules
    for module in &se.custom_modules {
        items.push(GenericRefItem {
            id: module.clone(),
            key: module.clone(),
            summary: Some("custom module".into()),
            content: None,
            tags: vec!["module".into()],
        });
    }

    // Fcontext rules
    for rule in &se.fcontext_rules {
        items.push(GenericRefItem {
            id: rule.clone(),
            key: rule.clone(),
            summary: Some("fcontext".into()),
            content: None,
            tags: vec!["fcontext".into()],
        });
    }

    // Audit rules (CarryForwardFile)
    for cf in &se.audit_rules {
        let basename = Path::new(&cf.path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| cf.path.clone());
        items.push(GenericRefItem {
            id: cf.path.clone(),
            key: basename,
            summary: Some("audit rule".into()),
            content: if cf.content.is_empty() {
                None
            } else {
                Some(cf.content.clone())
            },
            tags: vec!["audit-rule".into(), cf.path.clone()],
        });
    }

    // PAM configs (CarryForwardFile)
    for cf in &se.pam_configs {
        let basename = Path::new(&cf.path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| cf.path.clone());
        items.push(GenericRefItem {
            id: cf.path.clone(),
            key: basename,
            summary: Some("PAM config".into()),
            content: if cf.content.is_empty() {
                None
            } else {
                Some(cf.content.clone())
            },
            tags: vec!["pam".into(), cf.path.clone()],
        });
    }

    items
}

/// Build a complete reference projection from a snapshot.
///
/// Orchestrates all per-section extractors into a single `ReferenceProjection`.
pub fn project_reference(snap: &InspectionSnapshot) -> ReferenceProjection {
    ReferenceProjection {
        services: project_ref_services(snap),
        version_changes: project_ref_version_changes(snap),
        containers: project_ref_containers(snap),
        kernel_boot: project_ref_kernel_boot(snap),
        network: project_ref_network(snap),
        storage: project_ref_storage(snap),
        scheduled_tasks: project_ref_scheduled_tasks(snap),
        non_rpm_software: project_ref_non_rpm(snap),
        selinux: project_ref_selinux(snap),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::baseline::BaselineData;
    use inspectah_core::types::config::{ConfigFileEntry, ConfigFileKind};
    use inspectah_core::types::nonrpm::{NonRpmItem, NonRpmSoftwareSection, PipPackage};
    use inspectah_core::types::rpm::{RpmSection, VersionChange};
    use inspectah_core::types::scheduled::{
        AtJob, CronJob, GeneratedTimerUnit, ScheduledTaskSection, SystemdTimer,
    };
    use inspectah_core::types::selinux::{CarryForwardFile, SelinuxPortLabel, SelinuxSection};
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
                    locked: false,
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
                    locked: false,
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
                    locked: false,
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
                    locked: false,
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
                    locked: false,
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
                    locked: false,
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
                    locked: false,
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
                    locked: false,
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

    // ── project_ref_network tests ──────────────────────────────────

    use inspectah_core::types::network::{
        FirewallDirectRule, FirewallZone, NMConnection, NetworkSection, ProxyEntry, StaticRouteFile,
    };

    #[test]
    fn test_no_network_returns_default() {
        let snap = InspectionSnapshot {
            network: None,
            ..Default::default()
        };

        let result = project_ref_network(&snap);

        assert!(result.connections.is_empty());
        assert!(result.firewall_zones.is_empty());
        assert!(result.firewall_direct_rules.is_empty());
        assert!(result.static_routes.is_empty());
        assert!(result.ip_routes.is_empty());
        assert!(result.ip_rules.is_empty());
        assert!(result.resolv_provenance.is_empty());
        assert!(result.hosts_additions.is_empty());
        assert!(result.proxy_env.is_empty());
    }

    #[test]
    fn test_empty_network_returns_default() {
        let snap = InspectionSnapshot {
            network: Some(NetworkSection::default()),
            ..Default::default()
        };

        let result = project_ref_network(&snap);

        assert!(result.connections.is_empty());
        assert!(result.firewall_zones.is_empty());
        assert!(result.firewall_direct_rules.is_empty());
        assert!(result.static_routes.is_empty());
        assert!(result.ip_routes.is_empty());
        assert!(result.ip_rules.is_empty());
        assert!(result.resolv_provenance.is_empty());
        assert!(result.hosts_additions.is_empty());
        assert!(result.proxy_env.is_empty());
    }

    #[test]
    fn test_nm_connections_extracted() {
        let snap = InspectionSnapshot {
            network: Some(NetworkSection {
                connections: vec![NMConnection {
                    name: "eth0".into(),
                    conn_type: "802-3-ethernet".into(),
                    method: "auto".into(),
                    path: "/etc/NetworkManager/system-connections/eth0.nmconnection".into(),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };

        let result = project_ref_network(&snap);

        assert_eq!(result.connections.len(), 1);
        assert_eq!(result.connections[0].name, "eth0");
        assert_eq!(result.connections[0].conn_type, "802-3-ethernet");
        assert_eq!(result.connections[0].method, "auto");
        assert_eq!(
            result.connections[0].path,
            "/etc/NetworkManager/system-connections/eth0.nmconnection"
        );
    }

    #[test]
    fn test_firewall_zones_extracted() {
        let snap = InspectionSnapshot {
            network: Some(NetworkSection {
                firewall_zones: vec![FirewallZone {
                    name: "public".into(),
                    path: "/etc/firewalld/zones/public.xml".into(),
                    content: "<zone>...</zone>".into(),
                    services: vec!["ssh".into(), "http".into()],
                    ports: vec!["8080/tcp".into()],
                    rich_rules: vec!["rule family=ipv4 accept".into()],
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };

        let result = project_ref_network(&snap);

        assert_eq!(result.firewall_zones.len(), 1);
        assert_eq!(result.firewall_zones[0].name, "public");
        assert_eq!(result.firewall_zones[0].path, "/etc/firewalld/zones/public.xml");
        assert_eq!(result.firewall_zones[0].services, vec!["ssh", "http"]);
        assert_eq!(result.firewall_zones[0].ports, vec!["8080/tcp"]);
        assert_eq!(
            result.firewall_zones[0].rich_rules,
            vec!["rule family=ipv4 accept"]
        );
    }

    #[test]
    fn test_firewall_direct_rules_extracted() {
        let snap = InspectionSnapshot {
            network: Some(NetworkSection {
                firewall_direct_rules: vec![FirewallDirectRule {
                    ipv: "ipv4".into(),
                    table: "filter".into(),
                    chain: "INPUT".into(),
                    priority: "0".into(),
                    args: "-p tcp --dport 443 -j ACCEPT".into(),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };

        let result = project_ref_network(&snap);

        assert_eq!(result.firewall_direct_rules.len(), 1);
        assert_eq!(result.firewall_direct_rules[0].ipv, "ipv4");
        assert_eq!(result.firewall_direct_rules[0].table, "filter");
        assert_eq!(result.firewall_direct_rules[0].chain, "INPUT");
        assert_eq!(result.firewall_direct_rules[0].priority, "0");
        assert_eq!(
            result.firewall_direct_rules[0].args,
            "-p tcp --dport 443 -j ACCEPT"
        );
    }

    #[test]
    fn test_static_routes_and_scalars() {
        let snap = InspectionSnapshot {
            network: Some(NetworkSection {
                static_routes: vec![StaticRouteFile {
                    path: "/etc/sysconfig/network-scripts/route-eth0".into(),
                    name: "eth0".into(),
                }],
                ip_routes: vec!["10.0.0.0/8 via 192.168.1.1".into()],
                ip_rules: vec!["from 10.0.0.0/8 lookup custom".into()],
                resolv_provenance: "NetworkManager".into(),
                hosts_additions: vec!["192.168.1.100 myhost".into()],
                ..Default::default()
            }),
            ..Default::default()
        };

        let result = project_ref_network(&snap);

        assert_eq!(result.static_routes.len(), 1);
        assert_eq!(
            result.static_routes[0].path,
            "/etc/sysconfig/network-scripts/route-eth0"
        );
        assert_eq!(result.static_routes[0].name, "eth0");
        assert_eq!(result.ip_routes, vec!["10.0.0.0/8 via 192.168.1.1"]);
        assert_eq!(result.ip_rules, vec!["from 10.0.0.0/8 lookup custom"]);
        assert_eq!(result.resolv_provenance, "NetworkManager");
        assert_eq!(result.hosts_additions, vec!["192.168.1.100 myhost"]);
    }

    #[test]
    fn test_proxy_env_extracted() {
        let snap = InspectionSnapshot {
            network: Some(NetworkSection {
                proxy: vec![ProxyEntry {
                    source: "/etc/profile.d/proxy.sh".into(),
                    line: "export HTTP_PROXY=http://proxy:3128".into(),
                }],
                ..Default::default()
            }),
            ..Default::default()
        };

        let result = project_ref_network(&snap);

        assert_eq!(result.proxy_env.len(), 1);
        assert_eq!(result.proxy_env[0].source, "/etc/profile.d/proxy.sh");
        assert_eq!(
            result.proxy_env[0].line,
            "export HTTP_PROXY=http://proxy:3128"
        );
    }

    #[test]
    fn test_full_network_roundtrip() {
        let snap = InspectionSnapshot {
            network: Some(NetworkSection {
                connections: vec![NMConnection {
                    name: "bond0".into(),
                    conn_type: "bond".into(),
                    method: "manual".into(),
                    path: "/etc/NetworkManager/system-connections/bond0.nmconnection".into(),
                    ..Default::default()
                }],
                firewall_zones: vec![FirewallZone {
                    name: "internal".into(),
                    path: "/etc/firewalld/zones/internal.xml".into(),
                    content: "<zone>internal</zone>".into(),
                    services: vec!["dns".into()],
                    ports: Vec::new(),
                    rich_rules: Vec::new(),
                    ..Default::default()
                }],
                firewall_direct_rules: vec![FirewallDirectRule {
                    ipv: "ipv6".into(),
                    table: "mangle".into(),
                    chain: "PREROUTING".into(),
                    priority: "1".into(),
                    args: "-j MARK --set-mark 1".into(),
                    ..Default::default()
                }],
                static_routes: vec![StaticRouteFile {
                    path: "/etc/sysconfig/network-scripts/route-bond0".into(),
                    name: "bond0".into(),
                }],
                ip_routes: vec!["default via 10.0.0.1".into()],
                ip_rules: vec!["from all lookup main".into()],
                resolv_provenance: "systemd-resolved".into(),
                hosts_additions: vec!["10.0.0.5 dbserver".into()],
                proxy: vec![ProxyEntry {
                    source: "/etc/environment".into(),
                    line: "HTTPS_PROXY=http://proxy:3128".into(),
                }],
            }),
            ..Default::default()
        };

        let result = project_ref_network(&snap);

        assert_eq!(result.connections.len(), 1);
        assert_eq!(result.connections[0].name, "bond0");
        assert_eq!(result.firewall_zones.len(), 1);
        assert_eq!(result.firewall_zones[0].services, vec!["dns"]);
        assert_eq!(result.firewall_direct_rules.len(), 1);
        assert_eq!(result.firewall_direct_rules[0].table, "mangle");
        assert_eq!(result.static_routes.len(), 1);
        assert_eq!(result.ip_routes.len(), 1);
        assert_eq!(result.ip_rules.len(), 1);
        assert_eq!(result.resolv_provenance, "systemd-resolved");
        assert_eq!(result.hosts_additions.len(), 1);
        assert_eq!(result.proxy_env.len(), 1);
    }

    // ── project_ref_storage tests ──────────────────────────────────

    use inspectah_core::types::storage::{
        CredentialRef, FstabEntry, LvmVolume, MountPoint, StorageSection, VarDirectory,
    };

    #[test]
    fn test_no_storage_returns_default() {
        let snap = InspectionSnapshot {
            storage: None,
            ..Default::default()
        };

        let result = project_ref_storage(&snap);

        assert!(result.fstab_entries.is_empty());
        assert!(result.mount_points.is_empty());
        assert!(result.lvm_volumes.is_empty());
        assert!(result.var_directories.is_empty());
        assert!(result.credential_refs.is_empty());
    }

    #[test]
    fn test_empty_storage_returns_default() {
        let snap = InspectionSnapshot {
            storage: Some(StorageSection::default()),
            ..Default::default()
        };

        let result = project_ref_storage(&snap);

        assert!(result.fstab_entries.is_empty());
        assert!(result.mount_points.is_empty());
        assert!(result.lvm_volumes.is_empty());
        assert!(result.var_directories.is_empty());
        assert!(result.credential_refs.is_empty());
    }

    #[test]
    fn test_fstab_entries_extracted() {
        let snap = InspectionSnapshot {
            storage: Some(StorageSection {
                fstab_entries: vec![FstabEntry {
                    device: "/dev/sda1".into(),
                    mount_point: "/boot".into(),
                    fstype: "xfs".into(),
                    options: "defaults".into(),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };

        let result = project_ref_storage(&snap);

        assert_eq!(result.fstab_entries.len(), 1);
        assert_eq!(result.fstab_entries[0].device, "/dev/sda1");
        assert_eq!(result.fstab_entries[0].mount_point, "/boot");
        assert_eq!(result.fstab_entries[0].fstype, "xfs");
        assert_eq!(result.fstab_entries[0].options, "defaults");
    }

    #[test]
    fn test_mount_points_extracted() {
        let snap = InspectionSnapshot {
            storage: Some(StorageSection {
                mount_points: vec![MountPoint {
                    target: "/var/log".into(),
                    source: "/dev/mapper/rhel-var_log".into(),
                    fstype: "xfs".into(),
                    options: "rw,relatime".into(),
                }],
                ..Default::default()
            }),
            ..Default::default()
        };

        let result = project_ref_storage(&snap);

        assert_eq!(result.mount_points.len(), 1);
        assert_eq!(result.mount_points[0].target, "/var/log");
        assert_eq!(result.mount_points[0].source, "/dev/mapper/rhel-var_log");
        assert_eq!(result.mount_points[0].fstype, "xfs");
        assert_eq!(result.mount_points[0].options, "rw,relatime");
    }

    #[test]
    fn test_lvm_volumes_extracted() {
        let snap = InspectionSnapshot {
            storage: Some(StorageSection {
                lvm_info: vec![LvmVolume {
                    vg_name: "rhel".into(),
                    lv_name: "root".into(),
                    lv_size: "50G".into(),
                }],
                ..Default::default()
            }),
            ..Default::default()
        };

        let result = project_ref_storage(&snap);

        assert_eq!(result.lvm_volumes.len(), 1);
        assert_eq!(result.lvm_volumes[0].vg_name, "rhel");
        assert_eq!(result.lvm_volumes[0].lv_name, "root");
        assert_eq!(result.lvm_volumes[0].lv_size, "50G");
    }

    #[test]
    fn test_var_directories_extracted() {
        let snap = InspectionSnapshot {
            storage: Some(StorageSection {
                var_directories: vec![VarDirectory {
                    path: "/var/lib/pgsql".into(),
                    size_estimate: "12G".into(),
                    recommendation: "mount as separate volume".into(),
                }],
                ..Default::default()
            }),
            ..Default::default()
        };

        let result = project_ref_storage(&snap);

        assert_eq!(result.var_directories.len(), 1);
        assert_eq!(result.var_directories[0].path, "/var/lib/pgsql");
        assert_eq!(result.var_directories[0].size_estimate, "12G");
        assert_eq!(
            result.var_directories[0].recommendation,
            "mount as separate volume"
        );
    }

    #[test]
    fn test_credential_refs_extracted() {
        let snap = InspectionSnapshot {
            storage: Some(StorageSection {
                credential_refs: vec![CredentialRef {
                    credential_path: "/etc/fstab.d/creds".into(),
                    mount_point: "/mnt/secure".into(),
                    source: "fstab".into(),
                }],
                ..Default::default()
            }),
            ..Default::default()
        };

        let result = project_ref_storage(&snap);

        assert_eq!(result.credential_refs.len(), 1);
        assert_eq!(result.credential_refs[0].credential_path, "/etc/fstab.d/creds");
        assert_eq!(result.credential_refs[0].mount_point, "/mnt/secure");
        assert_eq!(result.credential_refs[0].source, "fstab");
    }

    #[test]
    fn test_full_storage_roundtrip() {
        let snap = InspectionSnapshot {
            storage: Some(StorageSection {
                fstab_entries: vec![
                    FstabEntry {
                        device: "/dev/mapper/rhel-root".into(),
                        mount_point: "/".into(),
                        fstype: "xfs".into(),
                        options: "defaults".into(),
                        ..Default::default()
                    },
                    FstabEntry {
                        device: "UUID=abcd-1234".into(),
                        mount_point: "/boot/efi".into(),
                        fstype: "vfat".into(),
                        options: "umask=0077".into(),
                        ..Default::default()
                    },
                ],
                mount_points: vec![MountPoint {
                    target: "/".into(),
                    source: "/dev/mapper/rhel-root".into(),
                    fstype: "xfs".into(),
                    options: "rw,seclabel,relatime".into(),
                }],
                lvm_info: vec![LvmVolume {
                    vg_name: "rhel".into(),
                    lv_name: "swap".into(),
                    lv_size: "4G".into(),
                }],
                var_directories: vec![VarDirectory {
                    path: "/var/log".into(),
                    size_estimate: "2G".into(),
                    recommendation: "keep on root".into(),
                }],
                credential_refs: vec![CredentialRef {
                    credential_path: "/etc/cifs-creds".into(),
                    mount_point: "/mnt/share".into(),
                    source: "fstab".into(),
                }],
            }),
            ..Default::default()
        };

        let result = project_ref_storage(&snap);

        assert_eq!(result.fstab_entries.len(), 2);
        assert_eq!(result.fstab_entries[1].device, "UUID=abcd-1234");
        assert_eq!(result.mount_points.len(), 1);
        assert_eq!(result.mount_points[0].options, "rw,seclabel,relatime");
        assert_eq!(result.lvm_volumes.len(), 1);
        assert_eq!(result.lvm_volumes[0].lv_name, "swap");
        assert_eq!(result.var_directories.len(), 1);
        assert_eq!(result.var_directories[0].recommendation, "keep on root");
        assert_eq!(result.credential_refs.len(), 1);
        assert_eq!(result.credential_refs[0].credential_path, "/etc/cifs-creds");
    }

    // ── project_ref_scheduled_tasks tests ──────────────────────────

    #[test]
    fn test_no_scheduled_tasks_returns_empty() {
        let snap = InspectionSnapshot {
            scheduled_tasks: None,
            ..Default::default()
        };

        let result = project_ref_scheduled_tasks(&snap);
        assert!(result.is_empty());
    }

    #[test]
    fn test_empty_scheduled_tasks_returns_empty() {
        let snap = InspectionSnapshot {
            scheduled_tasks: Some(ScheduledTaskSection::default()),
            ..Default::default()
        };

        let result = project_ref_scheduled_tasks(&snap);
        assert!(result.is_empty());
    }

    #[test]
    fn test_cron_jobs_mapped() {
        let snap = InspectionSnapshot {
            scheduled_tasks: Some(ScheduledTaskSection {
                cron_jobs: vec![CronJob {
                    path: "/etc/cron.d/backup".into(),
                    source: "file".into(),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };

        let result = project_ref_scheduled_tasks(&snap);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "/etc/cron.d/backup");
        assert_eq!(result[0].key, "backup");
        assert_eq!(result[0].summary, Some("file".into()));
        assert_eq!(result[0].tags, vec!["cron", "/etc/cron.d/backup"]);
    }

    #[test]
    fn test_systemd_timers_mapped() {
        let snap = InspectionSnapshot {
            scheduled_tasks: Some(ScheduledTaskSection {
                systemd_timers: vec![SystemdTimer {
                    name: "logrotate.timer".into(),
                    on_calendar: "daily".into(),
                    exec_start: "/usr/sbin/logrotate".into(),
                    description: "Rotate logs daily".into(),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };

        let result = project_ref_scheduled_tasks(&snap);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "logrotate.timer");
        assert_eq!(result[0].key, "logrotate.timer");
        assert_eq!(result[0].summary, Some("daily".into()));
        assert!(result[0].content.as_ref().unwrap().contains("Rotate logs daily"));
        assert!(result[0].content.as_ref().unwrap().contains("/usr/sbin/logrotate"));
        assert_eq!(result[0].tags, vec!["timer"]);
    }

    #[test]
    fn test_at_jobs_mapped() {
        let snap = InspectionSnapshot {
            scheduled_tasks: Some(ScheduledTaskSection {
                at_jobs: vec![AtJob {
                    file: "at-job-42".into(),
                    command: "/usr/local/bin/cleanup".into(),
                    user: "root".into(),
                    working_dir: "/tmp".into(),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };

        let result = project_ref_scheduled_tasks(&snap);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "at-job-42");
        assert_eq!(result[0].key, "at-job-42");
        assert_eq!(result[0].summary, Some("root: /usr/local/bin/cleanup".into()));
        assert_eq!(result[0].content, Some("/tmp".into()));
        assert_eq!(result[0].tags, vec!["at"]);
    }

    #[test]
    fn test_generated_timer_units_mapped() {
        let snap = InspectionSnapshot {
            scheduled_tasks: Some(ScheduledTaskSection {
                generated_timer_units: vec![GeneratedTimerUnit {
                    name: "backup.timer".into(),
                    cron_expr: "0 2 * * *".into(),
                    source_path: "/etc/cron.d/backup".into(),
                    command: "/usr/local/bin/backup.sh".into(),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };

        let result = project_ref_scheduled_tasks(&snap);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "backup.timer");
        assert_eq!(result[0].key, "backup.timer");
        assert_eq!(result[0].summary, Some("0 2 * * *".into()));
        assert!(result[0].content.as_ref().unwrap().contains("/etc/cron.d/backup"));
        assert!(result[0]
            .content
            .as_ref()
            .unwrap()
            .contains("/usr/local/bin/backup.sh"));
        assert_eq!(result[0].tags, vec!["generated-timer"]);
    }

    #[test]
    fn test_mixed_scheduled_tasks() {
        let snap = InspectionSnapshot {
            scheduled_tasks: Some(ScheduledTaskSection {
                cron_jobs: vec![CronJob {
                    path: "/etc/cron.d/backup".into(),
                    source: "file".into(),
                    ..Default::default()
                }],
                systemd_timers: vec![SystemdTimer {
                    name: "logrotate.timer".into(),
                    on_calendar: "daily".into(),
                    ..Default::default()
                }],
                at_jobs: vec![AtJob {
                    file: "at-42".into(),
                    command: "echo hello".into(),
                    user: "root".into(),
                    ..Default::default()
                }],
                generated_timer_units: vec![GeneratedTimerUnit {
                    name: "gen.timer".into(),
                    cron_expr: "*/5 * * * *".into(),
                    ..Default::default()
                }],
            }),
            ..Default::default()
        };

        let result = project_ref_scheduled_tasks(&snap);
        assert_eq!(result.len(), 4);
        assert_eq!(result[0].tags, vec!["cron", "/etc/cron.d/backup"]);
        assert_eq!(result[1].tags, vec!["timer"]);
        assert_eq!(result[2].tags, vec!["at"]);
        assert_eq!(result[3].tags, vec!["generated-timer"]);
    }

    // ── project_ref_non_rpm tests ──────────────────────────────────

    #[test]
    fn test_no_non_rpm_returns_empty() {
        let snap = InspectionSnapshot {
            non_rpm_software: None,
            ..Default::default()
        };

        let result = project_ref_non_rpm(&snap);
        assert!(result.is_empty());
    }

    #[test]
    fn test_non_rpm_items_mapped() {
        let snap = InspectionSnapshot {
            non_rpm_software: Some(NonRpmSoftwareSection {
                items: vec![NonRpmItem {
                    name: "custom-app".into(),
                    path: "/opt/app/bin".into(),
                    method: "binary".into(),
                    confidence: "high".into(),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };

        let result = project_ref_non_rpm(&snap);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "custom-app");
        assert_eq!(result[0].key, "custom-app");
        assert_eq!(result[0].summary, Some("binary (high)".into()));
        assert_eq!(result[0].content, Some("/opt/app/bin".into()));
        assert_eq!(result[0].tags, vec!["non-rpm"]); // no lang set, so just non-rpm
    }

    #[test]
    fn test_non_rpm_with_pip_packages() {
        let snap = InspectionSnapshot {
            non_rpm_software: Some(NonRpmSoftwareSection {
                items: vec![NonRpmItem {
                    name: "venv".into(),
                    path: "/opt/venv".into(),
                    method: "virtualenv".into(),
                    confidence: "high".into(),
                    packages: vec![
                        PipPackage {
                            name: "requests".into(),
                            version: "2.28.0".into(),
                        },
                        PipPackage {
                            name: "flask".into(),
                            version: "".into(),
                        },
                    ],
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };

        let result = project_ref_non_rpm(&snap);

        assert_eq!(result.len(), 1);
        let content = result[0].content.as_ref().unwrap();
        assert!(content.contains("/opt/venv"));
        assert!(content.contains("requests==2.28.0"));
        assert!(content.contains("flask"));
    }

    #[test]
    fn test_non_rpm_env_files_mapped() {
        let snap = InspectionSnapshot {
            non_rpm_software: Some(NonRpmSoftwareSection {
                items: vec![],
                env_files: vec![ConfigFileEntry {
                    path: "/etc/sysconfig/myapp".into(),
                    kind: ConfigFileKind::Unowned,
                    content: "KEY=value".into(),
                    ..Default::default()
                }],
            }),
            ..Default::default()
        };

        let result = project_ref_non_rpm(&snap);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "/etc/sysconfig/myapp");
        assert_eq!(result[0].key, "myapp");
        assert_eq!(result[0].summary, Some("unowned".into()));
        assert_eq!(result[0].content, Some("KEY=value".into()));
        assert_eq!(result[0].tags, vec!["env-file", "/etc/sysconfig/myapp"]);
    }

    // ── project_ref_selinux tests ──────────────────────────────────

    #[test]
    fn test_no_selinux_returns_empty() {
        let snap = InspectionSnapshot {
            selinux: None,
            ..Default::default()
        };

        let result = project_ref_selinux(&snap);
        assert!(result.is_empty());
    }

    #[test]
    fn test_selinux_mode_and_fips() {
        let snap = InspectionSnapshot {
            selinux: Some(SelinuxSection {
                mode: "enforcing".into(),
                fips_mode: true,
                ..Default::default()
            }),
            ..Default::default()
        };

        let result = project_ref_selinux(&snap);

        // mode + fips = 2 items
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].id, "selinux_mode");
        assert_eq!(result[0].key, "SELinux mode");
        assert_eq!(result[0].summary, Some("enforcing".into()));
        assert_eq!(result[1].id, "fips_mode");
        assert_eq!(result[1].summary, Some("enabled".into()));
    }

    #[test]
    fn test_selinux_fips_disabled() {
        let snap = InspectionSnapshot {
            selinux: Some(SelinuxSection {
                mode: "permissive".into(),
                fips_mode: false,
                ..Default::default()
            }),
            ..Default::default()
        };

        let result = project_ref_selinux(&snap);

        let fips = result.iter().find(|i| i.id == "fips_mode").unwrap();
        assert_eq!(fips.summary, Some("disabled".into()));
    }

    #[test]
    fn test_selinux_port_labels() {
        let snap = InspectionSnapshot {
            selinux: Some(SelinuxSection {
                port_labels: vec![SelinuxPortLabel {
                    protocol: "tcp".into(),
                    port: "8080".into(),
                    label_type: "http_port_t".into(),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };

        let result = project_ref_selinux(&snap);

        let port = result.iter().find(|i| i.id == "tcp/8080").unwrap();
        assert_eq!(port.key, "tcp/8080");
        assert_eq!(port.summary, Some("http_port_t".into()));
        assert_eq!(port.tags, vec!["port-label"]);
    }

    #[test]
    fn test_selinux_boolean_overrides() {
        let snap = InspectionSnapshot {
            selinux: Some(SelinuxSection {
                boolean_overrides: vec![serde_json::json!({
                    "name": "httpd_can_network_connect",
                    "state": true,
                })],
                ..Default::default()
            }),
            ..Default::default()
        };

        let result = project_ref_selinux(&snap);

        let bool_item = result
            .iter()
            .find(|i| i.id == "httpd_can_network_connect")
            .unwrap();
        assert_eq!(bool_item.tags, vec!["boolean"]);
        assert!(bool_item.summary.is_some());
    }

    #[test]
    fn test_selinux_custom_modules() {
        let snap = InspectionSnapshot {
            selinux: Some(SelinuxSection {
                custom_modules: vec!["myapp".into()],
                ..Default::default()
            }),
            ..Default::default()
        };

        let result = project_ref_selinux(&snap);

        let module = result.iter().find(|i| i.id == "myapp").unwrap();
        assert_eq!(module.summary, Some("custom module".into()));
        assert_eq!(module.tags, vec!["module"]);
    }

    #[test]
    fn test_selinux_audit_rules_and_pam() {
        let snap = InspectionSnapshot {
            selinux: Some(SelinuxSection {
                audit_rules: vec![CarryForwardFile {
                    path: "etc/audit/rules.d/custom.rules".into(),
                    content: "-w /etc/shadow -p wa".into(),
                }],
                pam_configs: vec![CarryForwardFile {
                    path: "etc/pam.d/custom-sshd".into(),
                    content: "auth required pam_unix.so".into(),
                }],
                ..Default::default()
            }),
            ..Default::default()
        };

        let result = project_ref_selinux(&snap);

        let audit = result
            .iter()
            .find(|i| i.id == "etc/audit/rules.d/custom.rules")
            .unwrap();
        assert_eq!(audit.key, "custom.rules");
        assert_eq!(audit.summary, Some("audit rule".into()));
        assert_eq!(audit.content, Some("-w /etc/shadow -p wa".into()));
        assert_eq!(
            audit.tags,
            vec!["audit-rule", "etc/audit/rules.d/custom.rules"]
        );

        let pam = result
            .iter()
            .find(|i| i.id == "etc/pam.d/custom-sshd")
            .unwrap();
        assert_eq!(pam.key, "custom-sshd");
        assert_eq!(pam.summary, Some("PAM config".into()));
        assert_eq!(pam.content, Some("auth required pam_unix.so".into()));
        assert_eq!(pam.tags, vec!["pam", "etc/pam.d/custom-sshd"]);
    }

    // ── project_reference orchestrator tests ───────────────────────

    #[test]
    fn test_project_reference_empty_snapshot() {
        let snap = InspectionSnapshot::default();

        let result = project_reference(&snap);

        assert!(result.scheduled_tasks.is_empty());
        assert!(result.non_rpm_software.is_empty());
        assert!(result.selinux.is_empty());
    }

    #[test]
    fn test_project_reference_populates_all_generic_sections() {
        let snap = InspectionSnapshot {
            scheduled_tasks: Some(ScheduledTaskSection {
                cron_jobs: vec![CronJob {
                    path: "/etc/cron.d/test".into(),
                    source: "file".into(),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            non_rpm_software: Some(NonRpmSoftwareSection {
                items: vec![NonRpmItem {
                    name: "app".into(),
                    method: "binary".into(),
                    confidence: "high".into(),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            selinux: Some(SelinuxSection {
                mode: "enforcing".into(),
                ..Default::default()
            }),
            ..Default::default()
        };

        let result = project_reference(&snap);

        assert_eq!(result.scheduled_tasks.len(), 1);
        assert_eq!(result.non_rpm_software.len(), 1);
        // selinux: mode + fips = 2
        assert_eq!(result.selinux.len(), 2);
    }
}
