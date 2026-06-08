//! HTML report renderer — produces a self-contained PatternFly HTML report
//! using minijinja templates.
//!
//! The base template (`templates/report/base.html`) provides the structural
//! shell. Section templates are added incrementally in T5-T12.

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::traits::renderer::RenderContext;
use inspectah_core::types::completeness::{Completeness, InspectorId};
use inspectah_core::types::users::UserGroupDecision;
use minijinja::{context, Environment, Value};

use super::report_data::{build_filter_data, section_state, script_safe_json, SectionState};

const PF_CSS: &str = include_str!("../../assets/patternfly.min.css");
const REPORT_CSS: &str = include_str!("../../assets/report.css");
const REPORT_JS: &str = include_str!("../../assets/report.js");

/// Render a self-contained PatternFly HTML report from the snapshot.
pub fn render_report(snap: &InspectionSnapshot, _context: &RenderContext) -> String {
    let mut env = Environment::new();
    env.set_loader(minijinja::path_loader(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/templates"
    )));

    let os_name = snap
        .os_release
        .as_ref()
        .map(|o| {
            if o.pretty_name.is_empty() {
                o.name.clone()
            } else {
                o.pretty_name.clone()
            }
        })
        .unwrap_or_else(|| "Unknown System".into());

    let hostname = snap
        .meta
        .get("hostname")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    let filter_data = build_filter_data(snap);
    let filter_json = serde_json::to_string(&filter_data).unwrap_or_default();
    let safe_json = script_safe_json(&filter_json);

    // Warning count for header badge
    let warning_count = snap.warnings.len();

    // Completeness data for banner
    let (failed_sections, degraded_sections, completeness_reason) = {
        fn inspector_display(id: &InspectorId) -> (&'static str, &'static str) {
            match id {
                InspectorId::Rpm => ("Packages", "packages"),
                InspectorId::Config => ("Configuration Files", "config-files"),
                InspectorId::Services => ("Service State Changes", "services"),
                InspectorId::Storage => ("Storage", "storage"),
                InspectorId::ScheduledTasks => ("Scheduled Tasks", "scheduled-tasks"),
                InspectorId::KernelBoot => ("Kernel & Boot", "kernel-boot"),
                InspectorId::Selinux => ("Security & Access Control", "security"),
                InspectorId::NonRpmSoftware => ("Non-RPM Software", "nonrpm"),
                InspectorId::UsersGroups => ("Users & Groups", "users-groups"),
                InspectorId::Containers => ("Containers", "containers"),
                InspectorId::Network => ("Network", "network"),
                InspectorId::Subscription => ("Subscription", "subscription"),
                InspectorId::Hardware => ("Hardware", "hardware"),
                InspectorId::Ostree => ("OSTree", "ostree"),
                InspectorId::OsRelease => ("OS Release", "os-release"),
            }
        }

        fn to_section_values(ids: &[InspectorId]) -> Vec<Value> {
            ids.iter()
                .map(|id| {
                    let (name, html_id) = inspector_display(id);
                    Value::from_serialize(serde_json::json!({
                        "name": name, "id": html_id
                    }))
                })
                .collect()
        }

        match &snap.completeness {
            Completeness::Complete => (vec![], vec![], String::new()),
            Completeness::Partial {
                degraded_sections: ds,
                reason,
            } => (vec![], to_section_values(ds), reason.clone()),
            Completeness::Incomplete {
                failed_sections: fs,
                degraded_sections: ds,
                reason,
            } => (to_section_values(fs), to_section_values(ds), reason.clone()),
        }
    };

    // ── Summary cards and TOC entries ─────────────────────────────
    let (summary_cards, toc_entries) = {
        /// Map InspectorId to (display_name, html_id).
        fn inspector_display(id: InspectorId) -> (&'static str, &'static str) {
            match id {
                InspectorId::Rpm => ("Packages", "packages"),
                InspectorId::Config => ("Configuration Files", "config-files"),
                InspectorId::Services => ("Service State Changes", "services"),
                InspectorId::Storage => ("Storage", "storage"),
                InspectorId::ScheduledTasks => ("Scheduled Tasks", "scheduled-tasks"),
                InspectorId::KernelBoot => ("Kernel & Boot", "kernel-boot"),
                InspectorId::Selinux => ("Security & Access Control", "security"),
                InspectorId::NonRpmSoftware => ("Non-RPM Software", "nonrpm"),
                InspectorId::UsersGroups => ("Users & Groups", "users-groups"),
                InspectorId::Containers => ("Containers", "containers"),
                InspectorId::Network => ("Network", "network"),
                InspectorId::Subscription => ("Subscription", "subscription"),
                InspectorId::Hardware => ("Hardware", "hardware"),
                InspectorId::Ostree => ("OSTree", "ostree"),
                InspectorId::OsRelease => ("OS Release", "os-release"),
            }
        }

        /// Count items in a section. Returns `Some(count)` if the section data
        /// is present, `None` if absent.
        fn section_count(id: InspectorId, snap: &InspectionSnapshot) -> Option<usize> {
            match id {
                InspectorId::Rpm => snap
                    .rpm
                    .as_ref()
                    .map(|r| r.packages_added.len()),
                InspectorId::Config => snap
                    .config
                    .as_ref()
                    .map(|c| c.files.len()),
                InspectorId::Services => snap
                    .services
                    .as_ref()
                    .map(|s| s.state_changes.len()),
                InspectorId::Storage => snap
                    .storage
                    .as_ref()
                    .map(|s| s.fstab_entries.len()),
                InspectorId::KernelBoot => snap
                    .kernel_boot
                    .as_ref()
                    .map(|k| k.sysctl_overrides.len() + k.non_default_modules.len()),
                InspectorId::ScheduledTasks => snap
                    .scheduled_tasks
                    .as_ref()
                    .map(|s| {
                        s.cron_jobs.len()
                            + s.systemd_timers.len()
                            + s.generated_timer_units.len()
                            + s.at_jobs.len()
                    }),
                InspectorId::NonRpmSoftware => snap
                    .non_rpm_software
                    .as_ref()
                    .map(|n| n.items.len()),
                InspectorId::UsersGroups => snap
                    .users_groups
                    .as_ref()
                    .map(|u| u.users.len()),
                InspectorId::Selinux => snap
                    .selinux
                    .as_ref()
                    .map(|_| 1),
                // Redactions and warnings are not Option — always present as Vec
                _ => None,
            }
        }

        fn state_str(s: SectionState) -> &'static str {
            match s {
                SectionState::Normal => "normal",
                SectionState::Degraded => "degraded",
                SectionState::Failed => "failed",
            }
        }

        // Always-rendered sections: present even when count is 0.
        let always_rendered = [
            InspectorId::Rpm,
            InspectorId::Config,
            InspectorId::Services,
            InspectorId::Storage,
            InspectorId::KernelBoot,
            InspectorId::Selinux,
            InspectorId::ScheduledTasks,
            InspectorId::NonRpmSoftware,
            InspectorId::UsersGroups,
        ];

        let mut cards: Vec<Value> = Vec::new();
        let mut toc: Vec<Value> = Vec::new();

        for &id in &always_rendered {
            let (title, html_id) = inspector_display(id);
            let state = section_state(id, &snap.completeness);
            let count = if state == SectionState::Failed {
                "n/a".to_string()
            } else {
                section_count(id, snap).unwrap_or(0).to_string()
            };
            let entry = serde_json::json!({
                "title": title,
                "count": count,
                "state": state_str(state),
                "id": html_id,
            });
            cards.push(Value::from_serialize(&entry));
            toc.push(Value::from_serialize(&entry));
        }

        // Redactions — always rendered
        {
            let count = snap.redactions.len().to_string();
            let entry = serde_json::json!({
                "title": "Redactions",
                "count": count,
                "state": "normal",
                "id": "redactions",
            });
            cards.push(Value::from_serialize(&entry));
            toc.push(Value::from_serialize(&entry));
        }

        // Warnings — TOC only, NOT in summary cards
        if !snap.warnings.is_empty() {
            let entry = serde_json::json!({
                "title": "Warnings",
                "count": snap.warnings.len().to_string(),
                "state": "normal",
                "id": "warnings",
            });
            toc.push(Value::from_serialize(&entry));
        }

        (cards, toc)
    };

    // ── Packages section data ─────────────────────────────────────
    let packages: Vec<Value> = snap
        .rpm
        .as_ref()
        .map(|rpm| {
            rpm.packages_added
                .iter()
                .map(|p| {
                    Value::from_serialize(serde_json::json!({
                        "name": p.name,
                        "version": p.version,
                        "release": p.release,
                        "arch": p.arch,
                        "repo": p.source_repo,
                    }))
                })
                .collect()
        })
        .unwrap_or_default();

    let pkg_count = packages.len();
    let pkg_state = section_state(InspectorId::Rpm, &snap.completeness);
    let pkg_state_str = match pkg_state {
        SectionState::Normal => "normal",
        SectionState::Degraded => "degraded",
        SectionState::Failed => "failed",
    };

    // Version changes from baseline comparison
    let version_changes: Vec<Value> = snap
        .rpm
        .as_ref()
        .map(|rpm| {
            rpm.version_changes
                .iter()
                .map(|vc| {
                    let dir_str = match vc.direction {
                        inspectah_core::types::rpm::VersionChangeDirection::Upgrade => "upgrade",
                        inspectah_core::types::rpm::VersionChangeDirection::Downgrade => "downgrade",
                    };
                    Value::from_serialize(serde_json::json!({
                        "name": vc.name,
                        "arch": vc.arch,
                        "host_version": vc.host_version,
                        "base_version": vc.base_version,
                        "direction": dir_str,
                    }))
                })
                .collect()
        })
        .unwrap_or_default();

    // ── Config files section data ───────────────────────────────
    let config_files: Vec<Value> = snap
        .config
        .as_ref()
        .map(|cfg| {
            cfg.files
                .iter()
                .map(|f| {
                    let kind_str = serde_json::to_string(&f.kind)
                        .unwrap_or_default()
                        .trim_matches('"')
                        .to_string();
                    let cat_str = serde_json::to_string(&f.category)
                        .unwrap_or_default()
                        .trim_matches('"')
                        .to_string();
                    Value::from_serialize(serde_json::json!({
                        "path": f.path,
                        "kind": kind_str,
                        "category": cat_str,
                        "package": f.package.as_deref().unwrap_or(""),
                    }))
                })
                .collect()
        })
        .unwrap_or_default();

    let config_count = config_files.len();
    let config_state = section_state(InspectorId::Config, &snap.completeness);
    let config_state_str = match config_state {
        SectionState::Normal => "normal",
        SectionState::Degraded => "degraded",
        SectionState::Failed => "failed",
    };

    // Check if any config file has a package value
    let has_config_packages = snap
        .config
        .as_ref()
        .map(|cfg| cfg.files.iter().any(|f| f.package.is_some()))
        .unwrap_or(false);

    // Fleet conflict count: count config files that have fleet data with conflicts
    let config_conflict_count: usize = snap
        .config
        .as_ref()
        .and_then(|cfg| {
            if snap.fleet_meta.is_some() {
                Some(
                    cfg.files
                        .iter()
                        .filter(|f| f.fleet.is_some())
                        .count(),
                )
            } else {
                None
            }
        })
        .unwrap_or(0);

    // ── Services section data ────────────────────────────────────
    let services: Vec<Value> = snap
        .services
        .as_ref()
        .map(|svc| {
            svc.state_changes
                .iter()
                .map(|s| {
                    let default_str = s
                        .default_state
                        .as_ref()
                        .map(|d| d.to_string())
                        .unwrap_or_else(|| "n/a".to_string());
                    Value::from_serialize(serde_json::json!({
                        "unit": s.unit,
                        "current_state": s.current_state.to_string(),
                        "default_state": default_str,
                        "include": s.include,
                    }))
                })
                .collect()
        })
        .unwrap_or_default();

    let svc_count = services.len();
    let svc_state = section_state(InspectorId::Services, &snap.completeness);
    let svc_state_str = match svc_state {
        SectionState::Normal => "normal",
        SectionState::Degraded => "degraded",
        SectionState::Failed => "failed",
    };

    // Build extra badge with enabled/masked sub-counts
    let svc_extra_badge = snap
        .services
        .as_ref()
        .map(|svc| {
            use inspectah_core::types::services::ServiceUnitState;
            let enabled = svc
                .state_changes
                .iter()
                .filter(|s| s.current_state == ServiceUnitState::Enabled)
                .count();
            let masked = svc
                .state_changes
                .iter()
                .filter(|s| s.current_state == ServiceUnitState::Masked)
                .count();
            let mut parts = Vec::new();
            if enabled > 0 {
                parts.push(format!("{enabled} enabled"));
            }
            if masked > 0 {
                parts.push(format!("{masked} masked"));
            }
            parts.join(", ")
        })
        .unwrap_or_default();

    // ── Storage section data (conditional) ────────────────────────
    let has_storage = snap.storage.is_some();
    let storage_items: Vec<Value> = snap
        .storage
        .as_ref()
        .map(|st| {
            st.fstab_entries
                .iter()
                .map(|e| {
                    Value::from_serialize(serde_json::json!({
                        "device": e.device,
                        "mount_point": e.mount_point,
                        "fstype": e.fstype,
                        "options": e.options,
                    }))
                })
                .collect()
        })
        .unwrap_or_default();
    let storage_count = storage_items.len();
    let storage_state = section_state(InspectorId::Storage, &snap.completeness);
    let storage_state_str = match storage_state {
        SectionState::Normal => "normal",
        SectionState::Degraded => "degraded",
        SectionState::Failed => "failed",
    };

    // ── Kernel & Boot section data (conditional) ────────────────
    let has_kernelboot = snap.kernel_boot.is_some();
    let kernel_cmdline = snap
        .kernel_boot
        .as_ref()
        .map(|k| k.cmdline.clone())
        .unwrap_or_default();
    let sysctl_overrides: Vec<Value> = snap
        .kernel_boot
        .as_ref()
        .map(|k| {
            k.sysctl_overrides
                .iter()
                .map(|s| {
                    Value::from_serialize(serde_json::json!({
                        "key": s.key,
                        "runtime": s.runtime,
                        "default": s.default,
                        "source": s.source,
                    }))
                })
                .collect()
        })
        .unwrap_or_default();
    let modules_load_d: Vec<Value> = snap
        .kernel_boot
        .as_ref()
        .map(|k| {
            k.modules_load_d
                .iter()
                .map(|m| {
                    Value::from_serialize(serde_json::json!({
                        "path": m.path,
                    }))
                })
                .collect()
        })
        .unwrap_or_default();
    let modprobe_d: Vec<Value> = snap
        .kernel_boot
        .as_ref()
        .map(|k| {
            k.modprobe_d
                .iter()
                .map(|m| {
                    Value::from_serialize(serde_json::json!({
                        "path": m.path,
                    }))
                })
                .collect()
        })
        .unwrap_or_default();
    let kernelboot_count = sysctl_overrides.len() + modules_load_d.len() + modprobe_d.len();
    let kernelboot_state = section_state(InspectorId::KernelBoot, &snap.completeness);
    let kernelboot_state_str = match kernelboot_state {
        SectionState::Normal => "normal",
        SectionState::Degraded => "degraded",
        SectionState::Failed => "failed",
    };

    // ── Security & Access Control section data (conditional) ────
    let has_security = snap.selinux.is_some();
    let selinux_mode = snap
        .selinux
        .as_ref()
        .map(|s| s.mode.clone())
        .unwrap_or_default();
    let fips_enabled = snap
        .selinux
        .as_ref()
        .map(|s| s.fips_mode)
        .unwrap_or(false);
    let selinux_modules: Vec<Value> = snap
        .selinux
        .as_ref()
        .map(|s| {
            s.custom_modules
                .iter()
                .map(|m| Value::from(m.as_str()))
                .collect()
        })
        .unwrap_or_default();
    let selinux_booleans: Vec<Value> = snap
        .selinux
        .as_ref()
        .map(|s| {
            s.boolean_overrides
                .iter()
                .map(Value::from_serialize)
                .collect()
        })
        .unwrap_or_default();
    let selinux_fcontexts: Vec<Value> = snap
        .selinux
        .as_ref()
        .map(|s| {
            s.fcontext_rules
                .iter()
                .map(|f| Value::from(f.as_str()))
                .collect()
        })
        .unwrap_or_default();
    let security_count = selinux_modules.len()
        + selinux_booleans.len()
        + selinux_fcontexts.len();
    let security_state = section_state(InspectorId::Selinux, &snap.completeness);
    let security_state_str = match security_state {
        SectionState::Normal => "normal",
        SectionState::Degraded => "degraded",
        SectionState::Failed => "failed",
    };
    let security_extra_badge = snap
        .selinux
        .as_ref()
        .map(|s| s.mode.clone())
        .unwrap_or_default();

    // ── Scheduled Tasks section data (conditional) ──────────────
    let has_scheduled = snap.scheduled_tasks.is_some();
    let cron_jobs: Vec<Value> = snap
        .scheduled_tasks
        .as_ref()
        .map(|st| {
            st.cron_jobs
                .iter()
                .map(|j| {
                    Value::from_serialize(serde_json::json!({
                        "path": j.path,
                        "source": j.source,
                    }))
                })
                .collect()
        })
        .unwrap_or_default();
    let systemd_timers: Vec<Value> = snap
        .scheduled_tasks
        .as_ref()
        .map(|st| {
            st.systemd_timers
                .iter()
                .map(|t| {
                    Value::from_serialize(serde_json::json!({
                        "name": t.name,
                        "on_calendar": t.on_calendar,
                        "description": t.description,
                    }))
                })
                .collect()
        })
        .unwrap_or_default();
    let generated_timers: Vec<Value> = snap
        .scheduled_tasks
        .as_ref()
        .map(|st| {
            st.generated_timer_units
                .iter()
                .map(|g| {
                    Value::from_serialize(serde_json::json!({
                        "name": g.name,
                        "cron_expr": g.cron_expr,
                    }))
                })
                .collect()
        })
        .unwrap_or_default();
    let at_jobs: Vec<Value> = snap
        .scheduled_tasks
        .as_ref()
        .map(|st| {
            st.at_jobs
                .iter()
                .map(|a| {
                    Value::from_serialize(serde_json::json!({
                        "file": a.file,
                        "command": a.command,
                        "user": a.user,
                    }))
                })
                .collect()
        })
        .unwrap_or_default();
    let sched_count = cron_jobs.len()
        + systemd_timers.len()
        + generated_timers.len()
        + at_jobs.len();
    let sched_state = section_state(InspectorId::ScheduledTasks, &snap.completeness);
    let sched_state_str = match sched_state {
        SectionState::Normal => "normal",
        SectionState::Degraded => "degraded",
        SectionState::Failed => "failed",
    };

    // ── Non-RPM Software section data (conditional) ───────────
    let has_nonrpm = snap.non_rpm_software.is_some();
    let nonrpm_items: Vec<Value> = snap
        .non_rpm_software
        .as_ref()
        .map(|n| {
            n.items
                .iter()
                .map(|item| {
                    Value::from_serialize(serde_json::json!({
                        "name": item.name,
                        "path": item.path,
                        "method": item.method,
                        "confidence": item.confidence,
                        "lang": item.lang,
                        "version": item.version,
                    }))
                })
                .collect()
        })
        .unwrap_or_default();
    let nonrpm_count = nonrpm_items.len();
    let nonrpm_state = section_state(InspectorId::NonRpmSoftware, &snap.completeness);
    let nonrpm_state_str = match nonrpm_state {
        SectionState::Normal => "normal",
        SectionState::Degraded => "degraded",
        SectionState::Failed => "failed",
    };

    // ── Users & Groups section data (conditional) ─────────────
    // SECURITY: Only whitelisted fields are projected into the template
    // context. password_hash and ssh_keys content are EXCLUDED.
    let has_users = snap.users_groups.is_some();
    let users_list: Vec<Value> = snap
        .users_groups
        .as_ref()
        .map(|ug| {
            ug.users
                .iter()
                .filter_map(|v| serde_json::from_value::<UserGroupDecision>(v.clone()).ok())
                .filter(|u| u.include)
                .map(|u| {
                    Value::from_serialize(serde_json::json!({
                        "name": u.name,
                        "uid": u.uid,
                        "gid": u.gid,
                        "shell": u.shell,
                        "home": u.home,
                        "classification": u.classification,
                        "has_sudo": u.has_sudo.unwrap_or(false),
                        "ssh_key_count": u.ssh_key_count.unwrap_or(0),
                        "groups": u.supplementary_groups.as_deref().unwrap_or(&[]).join(", "),
                        "password_status": format!("{:?}", u.password_choice),
                    }))
                })
                .collect()
        })
        .unwrap_or_default();
    let users_count = users_list.len();
    let users_state = section_state(InspectorId::UsersGroups, &snap.completeness);
    let users_state_str = match users_state {
        SectionState::Normal => "normal",
        SectionState::Degraded => "degraded",
        SectionState::Failed => "failed",
    };

    // ── Warnings section data (always rendered) ───────────────
    let warnings_list: Vec<Value> = snap
        .warnings
        .iter()
        .map(|w| {
            Value::from_serialize(serde_json::json!({
                "inspector": w.inspector,
                "message": w.message,
            }))
        })
        .collect();

    // ── Redactions section data (always rendered) ─────────────
    let redaction_count = snap.redactions.len();

    // System type — use serde name for human-readable display
    let system_type = serde_json::to_string(&snap.system_type)
        .unwrap_or_default()
        .trim_matches('"')
        .to_string();

    // Baseline ref from target_image identity (not BaselineData)
    let baseline_ref = snap
        .target_image
        .as_ref()
        .map(|t| t.image_ref.clone())
        .unwrap_or_default();

    // Baseline details for the baseline info panel
    let baseline_digest = snap
        .baseline
        .as_ref()
        .map(|b| b.image_digest.clone())
        .unwrap_or_default();

    let baseline_strategy = snap
        .target_image
        .as_ref()
        .map(|t| {
            match t.strategy {
                inspectah_core::baseline::ResolutionStrategy::CliOverride => "CLI override",
                inspectah_core::baseline::ResolutionStrategy::UniversalBlue => "Universal Blue",
                inspectah_core::baseline::ResolutionStrategy::BootcStatus => "bootc status",
                inspectah_core::baseline::ResolutionStrategy::FedoraAtomicDesktop => {
                    "Fedora Atomic Desktop"
                }
                inspectah_core::baseline::ResolutionStrategy::OsRelease => "os-release",
            }
            .to_string()
        })
        .unwrap_or_default();

    // Host count for fleet snapshots
    let host_count = snap
        .fleet_meta
        .as_ref()
        .map(|fm| fm.host_count as i64)
        .unwrap_or(0);

    let tmpl = env
        .get_template("report/base.html")
        .expect("base template must exist at inspectah-pipeline/templates/report/base.html");

    let failed_val = Value::from(failed_sections);
    let degraded_val = Value::from(degraded_sections);

    let summary_cards_val = Value::from(summary_cards);
    let toc_entries_val = Value::from(toc_entries);

    let packages_val = Value::from(packages);
    let version_changes_val = Value::from(version_changes);
    let config_files_val = Value::from(config_files);
    let services_val = Value::from(services);
    let storage_items_val = Value::from(storage_items);
    let sysctl_overrides_val = Value::from(sysctl_overrides);
    let modules_load_d_val = Value::from(modules_load_d);
    let modprobe_d_val = Value::from(modprobe_d);
    let selinux_modules_val = Value::from(selinux_modules);
    let selinux_booleans_val = Value::from(selinux_booleans);
    let selinux_fcontexts_val = Value::from(selinux_fcontexts);
    let cron_jobs_val = Value::from(cron_jobs);
    let systemd_timers_val = Value::from(systemd_timers);
    let generated_timers_val = Value::from(generated_timers);
    let at_jobs_val = Value::from(at_jobs);
    let nonrpm_items_val = Value::from(nonrpm_items);
    let users_list_val = Value::from(users_list);
    let warnings_list_val = Value::from(warnings_list);

    tmpl.render(context! {
        os_name,
        hostname,
        warning_count,
        failed_sections => failed_val,
        degraded_sections => degraded_val,
        completeness_reason,
        summary_cards => summary_cards_val,
        toc_entries => toc_entries_val,
        system_type,
        baseline_ref,
        baseline_digest,
        baseline_strategy,
        host_count,
        packages => packages_val,
        pkg_count,
        pkg_state => pkg_state_str,
        version_changes => version_changes_val,
        config_files => config_files_val,
        config_count,
        config_state => config_state_str,
        config_conflict_count,
        has_config_packages,
        services => services_val,
        svc_count,
        svc_state => svc_state_str,
        svc_extra_badge,
        // Storage (conditional)
        has_storage,
        storage_items => storage_items_val,
        storage_count,
        storage_state => storage_state_str,
        // Kernel & Boot (conditional)
        has_kernelboot,
        kernel_cmdline,
        sysctl_overrides => sysctl_overrides_val,
        modules_load_d => modules_load_d_val,
        modprobe_d => modprobe_d_val,
        kernelboot_count,
        kernelboot_state => kernelboot_state_str,
        // Security & Access Control (conditional)
        has_security,
        selinux_mode,
        fips_enabled,
        selinux_modules => selinux_modules_val,
        selinux_booleans => selinux_booleans_val,
        selinux_fcontexts => selinux_fcontexts_val,
        security_count,
        security_state => security_state_str,
        security_extra_badge,
        // Scheduled Tasks (conditional)
        has_scheduled,
        cron_jobs => cron_jobs_val,
        systemd_timers => systemd_timers_val,
        generated_timers => generated_timers_val,
        at_jobs => at_jobs_val,
        sched_count,
        sched_state => sched_state_str,
        // Non-RPM Software (conditional)
        has_nonrpm,
        nonrpm_items => nonrpm_items_val,
        nonrpm_count,
        nonrpm_state => nonrpm_state_str,
        // Users & Groups (conditional)
        has_users,
        users_list => users_list_val,
        users_count,
        users_state => users_state_str,
        // Warnings (always rendered)
        warnings_list => warnings_list_val,
        // Redactions (always rendered)
        redaction_count,
        patternfly_css => PF_CSS,
        report_css => REPORT_CSS,
        report_js => REPORT_JS,
        filter_data_json => safe_json,
    })
    .unwrap_or_else(|e| format!("<!-- Template error: {e} -->"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::types::rpm::{
        PackageEntry, PackageState, RpmSection, VersionChange, VersionChangeDirection,
    };

    fn test_snapshot() -> InspectionSnapshot {
        let mut snap = InspectionSnapshot::new();
        snap.rpm = Some(RpmSection {
            packages_added: vec![PackageEntry {
                name: "httpd".into(),
                version: "2.4.57".into(),
                release: "5.el9".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                include: true,
                source_repo: "appstream".into(),
                ..Default::default()
            }],
            ..Default::default()
        });
        snap
    }

    #[test]
    fn test_report_html_renders_with_doctype() {
        let snap = test_snapshot();
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(html.starts_with("<!DOCTYPE html>"));
    }

    #[test]
    fn test_report_html_contains_csp() {
        let snap = test_snapshot();
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(html.contains("Content-Security-Policy"));
        assert!(html.contains("default-src 'none'"));
    }

    #[test]
    fn test_report_html_no_external_urls() {
        let snap = test_snapshot();
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            !html.contains("http://"),
            "report must not contain http:// URLs"
        );
        assert!(
            !html.contains("https://"),
            "report must not contain https:// URLs"
        );
    }

    #[test]
    fn test_report_html_contains_patternfly() {
        let snap = test_snapshot();
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            html.contains("--pf-t--global"),
            "must contain PF design tokens"
        );
    }

    #[test]
    fn test_report_html_escapes_values() {
        let mut snap = test_snapshot();
        snap.rpm.as_mut().unwrap().packages_added[0].name = "<script>alert(1)</script>".into();
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            !html.contains("<script>alert"),
            "must escape snapshot values"
        );
    }

    #[test]
    fn test_report_failed_section_shows_in_completeness_banner() {
        use inspectah_core::types::completeness::{Completeness, InspectorId};
        let mut snap = test_snapshot();
        snap.completeness = Completeness::Incomplete {
            failed_sections: vec![InspectorId::Config],
            degraded_sections: vec![],
            reason: "permission denied".into(),
        };
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            html.contains(r#"<div class="completeness-banner">"#),
            "must render completeness banner element for incomplete snapshot"
        );
        assert!(
            html.contains("Failed:"),
            "banner must label failed sections"
        );
        assert!(
            html.contains("Configuration Files"),
            "banner must name the failed section"
        );
    }

    #[test]
    fn test_report_degraded_section_shows_in_completeness_banner() {
        use inspectah_core::types::completeness::{Completeness, InspectorId};
        let mut snap = test_snapshot();
        snap.completeness = Completeness::Partial {
            degraded_sections: vec![InspectorId::Services],
            reason: "partial timeout".into(),
        };
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            html.contains(r#"<div class="completeness-banner">"#),
            "must render completeness banner element for partial snapshot"
        );
        assert!(
            html.contains("Degraded:"),
            "banner must label degraded sections"
        );
        assert!(
            html.contains("Service State Changes"),
            "banner must name the degraded section"
        );
    }

    #[test]
    fn test_report_completeness_banner_shows_reason() {
        use inspectah_core::types::completeness::{Completeness, InspectorId};
        let mut snap = test_snapshot();
        snap.completeness = Completeness::Incomplete {
            failed_sections: vec![InspectorId::Config],
            degraded_sections: vec![InspectorId::Services],
            reason: "permission denied reading shadow file".into(),
        };
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            html.contains(r#"<div class="completeness-banner">"#),
            "must render completeness banner element"
        );
        assert!(
            html.contains("permission denied reading shadow file"),
            "must show reason text"
        );
        assert!(
            html.contains("Configuration Files"),
            "must show failed section name"
        );
        assert!(
            html.contains("Service State Changes"),
            "must show degraded section name"
        );
    }

    #[test]
    fn test_report_complete_has_no_banner() {
        let snap = test_snapshot();
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            !html.contains(r#"<div class="completeness-banner">"#),
            "complete report must not render completeness banner element"
        );
    }

    #[test]
    fn test_report_source_info_bar() {
        let snap = test_snapshot();
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            html.contains(r#"<div class="source-info">"#),
            "must render source info bar element"
        );
    }

    #[test]
    fn test_report_warnings_not_in_summary_cards() {
        let mut snap = test_snapshot();
        snap.warnings = vec![inspectah_core::types::warnings::Warning {
            inspector: "test".into(),
            message: "test warning".into(),
            severity: None,
            extra: Default::default(),
        }];
        let html = render_report(&snap, &RenderContext { target: None });
        // Cards section should not contain "Warnings" as a card title
        let cards_section = html
            .split(r#"class="report-cards">"#)
            .nth(1)
            .and_then(|rest| rest.split("</div>").next())
            .unwrap_or("");
        assert!(
            !cards_section.contains(">Warnings<"),
            "summary cards must not include Warnings — shown in header badge"
        );
        // But the TOC should contain "Warnings"
        let toc_section = html
            .split(r#"class="report-toc""#)
            .nth(1)
            .and_then(|rest| rest.split("</nav>").next())
            .unwrap_or("");
        assert!(
            toc_section.contains("Warnings"),
            "TOC must include Warnings entry"
        );
    }

    #[test]
    fn test_report_summary_cards_rendered() {
        let snap = test_snapshot();
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            html.contains(r#"class="report-cards">"#),
            "report must contain summary cards grid"
        );
        // Should contain Packages card with count
        assert!(
            html.contains(">Packages<"),
            "summary cards must include Packages section"
        );
    }

    #[test]
    fn test_report_toc_rendered() {
        let snap = test_snapshot();
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            html.contains(r#"class="report-toc""#),
            "report must contain TOC navigation bar"
        );
        assert!(
            html.contains(r##"href="#packages""##),
            "TOC must contain anchor link to packages"
        );
    }

    #[test]
    fn test_report_failed_section_shows_na_in_cards() {
        let mut snap = test_snapshot();
        snap.completeness = Completeness::Incomplete {
            failed_sections: vec![InspectorId::Config],
            degraded_sections: vec![],
            reason: "permission denied".into(),
        };
        let html = render_report(&snap, &RenderContext { target: None });
        // The card for Config Files should show "n/a" (minijinja escapes
        // the slash to &#x2f; in HTML output — that's correct behavior).
        assert!(
            html.contains("n&#x2f;a") || html.contains("n/a"),
            "failed section card must show n/a for count"
        );
        // The card should have the text-failed class
        assert!(
            html.contains("text-failed"),
            "failed section card must have text-failed class"
        );
    }

    #[test]
    fn test_report_header_contains_warning_count() {
        let mut snap = test_snapshot();
        snap.warnings.push(inspectah_core::types::warnings::Warning {
            inspector: "test".into(),
            message: "test warning".into(),
            severity: None,
            extra: Default::default(),
        });
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            html.contains("1 warning"),
            "header must show warning count"
        );
    }

    // -----------------------------------------------------------------------
    // Packages section tests (T7)
    // -----------------------------------------------------------------------

    #[test]
    fn test_report_contains_packages_section() {
        let snap = test_snapshot();
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            html.contains("Packages"),
            "report must contain Packages section"
        );
        assert!(
            html.contains("httpd"),
            "packages table must contain package name"
        );
        assert!(
            html.contains("2.4.57"),
            "packages table must contain package version"
        );
    }

    #[test]
    fn test_report_empty_packages_shows_zero() {
        let mut snap = InspectionSnapshot::new();
        snap.rpm = Some(RpmSection::default());
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            html.contains("(0)"),
            "empty packages section must show (0) badge"
        );
        assert!(
            html.contains("No packages added"),
            "empty packages section must show empty state message"
        );
    }

    #[test]
    fn test_report_packages_table_columns() {
        let snap = test_snapshot();
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(html.contains("<th>Name</th>"), "table must have Name column");
        assert!(
            html.contains("<th>Version</th>"),
            "table must have Version column"
        );
        assert!(
            html.contains("<th>Release</th>"),
            "table must have Release column"
        );
        assert!(html.contains("<th>Arch</th>"), "table must have Arch column");
        assert!(html.contains("<th>Repo</th>"), "table must have Repo column");
    }

    #[test]
    fn test_report_packages_shows_repo() {
        let snap = test_snapshot();
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            html.contains("appstream"),
            "packages table must show source repo"
        );
    }

    #[test]
    fn test_report_version_changes_rendered() {
        let mut snap = test_snapshot();
        snap.rpm.as_mut().unwrap().version_changes = vec![VersionChange {
            name: "openssl".into(),
            arch: "x86_64".into(),
            host_version: "3.0.8".into(),
            base_version: "3.0.7".into(),
            direction: VersionChangeDirection::Upgrade,
            ..Default::default()
        }];
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            html.contains("Version Changes"),
            "must render version changes sub-section"
        );
        assert!(
            html.contains("openssl"),
            "version changes must contain package name"
        );
        assert!(
            html.contains("3.0.8"),
            "version changes must contain host version"
        );
        assert!(
            html.contains("3.0.7"),
            "version changes must contain base version"
        );
    }

    #[test]
    fn test_report_no_version_changes_when_empty() {
        let snap = test_snapshot();
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            !html.contains("Version Changes"),
            "must not show version changes sub-section when empty"
        );
    }

    // -----------------------------------------------------------------------
    // Baseline info panel tests (T7)
    // -----------------------------------------------------------------------

    #[test]
    fn test_report_baseline_panel_rendered() {
        let mut snap = test_snapshot();
        snap.target_image = Some(inspectah_core::baseline::TargetImageIdentity {
            image_ref: "registry.redhat.io/rhel9/rhel-bootc:9.4".into(),
            strategy: inspectah_core::baseline::ResolutionStrategy::BootcStatus,
        });
        snap.baseline = Some(inspectah_core::baseline::BaselineData {
            image_digest: "sha256:abc123".into(),
            packages: Default::default(),
            extracted_at: "2026-06-01T00:00:00Z".into(),
        });
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            html.contains("Baseline Comparison"),
            "must render baseline info panel"
        );
        // minijinja auto-escapes / to &#x2f; in HTML output
        assert!(
            html.contains("registry.redhat.io&#x2f;rhel9&#x2f;rhel-bootc:9.4")
                || html.contains("registry.redhat.io/rhel9/rhel-bootc:9.4"),
            "must show target image ref"
        );
        assert!(
            html.contains("sha256:abc123"),
            "must show baseline digest"
        );
        assert!(
            html.contains("bootc status"),
            "must show resolution strategy"
        );
    }

    #[test]
    fn test_report_no_baseline_panel_when_absent() {
        let snap = test_snapshot();
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            !html.contains("Baseline Comparison"),
            "must not render baseline panel when no target image"
        );
    }

    #[test]
    fn test_report_packages_failed_state() {
        let mut snap = InspectionSnapshot::new();
        snap.completeness = Completeness::Incomplete {
            failed_sections: vec![InspectorId::Rpm],
            degraded_sections: vec![],
            reason: "rpm db locked".into(),
        };
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            html.contains("data unavailable"),
            "failed packages section must show data unavailable badge"
        );
    }

    // -----------------------------------------------------------------------
    // Configuration Files section tests (T8)
    // -----------------------------------------------------------------------

    #[test]
    fn test_report_contains_config_section() {
        let mut snap = test_snapshot();
        snap.config = Some(inspectah_core::types::config::ConfigSection {
            files: vec![inspectah_core::types::config::ConfigFileEntry {
                path: "/etc/httpd/conf/httpd.conf".into(),
                kind: inspectah_core::types::config::ConfigFileKind::RpmOwnedModified,
                include: true,
                ..Default::default()
            }],
        });
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            html.contains("Configuration Files"),
            "report must contain Configuration Files section"
        );
        assert!(
            html.contains("httpd.conf"),
            "config table must contain file path"
        );
    }

    #[test]
    fn test_report_config_table_columns() {
        let mut snap = test_snapshot();
        snap.config = Some(inspectah_core::types::config::ConfigSection {
            files: vec![inspectah_core::types::config::ConfigFileEntry {
                path: "/etc/sysctl.conf".into(),
                kind: inspectah_core::types::config::ConfigFileKind::Unowned,
                category: inspectah_core::types::config::ConfigCategory::Sysctl,
                include: true,
                ..Default::default()
            }],
        });
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(html.contains("<th>Path</th>"), "table must have Path column");
        assert!(html.contains("<th>Kind</th>"), "table must have Kind column");
        assert!(
            html.contains("<th>Category</th>"),
            "table must have Category column"
        );
        assert!(
            html.contains("sysctl"),
            "config table must show category value"
        );
    }

    #[test]
    fn test_report_config_shows_package_column_when_present() {
        let mut snap = test_snapshot();
        snap.config = Some(inspectah_core::types::config::ConfigSection {
            files: vec![inspectah_core::types::config::ConfigFileEntry {
                path: "/etc/httpd/conf/httpd.conf".into(),
                kind: inspectah_core::types::config::ConfigFileKind::RpmOwnedModified,
                package: Some("httpd".into()),
                include: true,
                ..Default::default()
            }],
        });
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            html.contains("<th>Package</th>"),
            "table must have Package column when packages present"
        );
        assert!(
            html.contains(">httpd<"),
            "table must show package name"
        );
    }

    #[test]
    fn test_report_empty_config_shows_empty_state() {
        let mut snap = test_snapshot();
        snap.config = Some(inspectah_core::types::config::ConfigSection {
            files: vec![],
        });
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            html.contains("(0)"),
            "empty config section must show (0) badge"
        );
        assert!(
            html.contains("No configuration file changes detected"),
            "empty config section must show empty state message"
        );
    }

    #[test]
    fn test_report_config_failed_state() {
        let mut snap = InspectionSnapshot::new();
        snap.completeness = Completeness::Incomplete {
            failed_sections: vec![InspectorId::Config],
            degraded_sections: vec![],
            reason: "permission denied".into(),
        };
        let html = render_report(&snap, &RenderContext { target: None });
        // The config section should show "data unavailable"
        assert!(
            html.contains("Configuration Files"),
            "failed config section must still have title"
        );
    }

    // -----------------------------------------------------------------------
    // Service State Changes section tests (T8)
    // -----------------------------------------------------------------------

    #[test]
    fn test_report_contains_services_section() {
        let mut snap = test_snapshot();
        snap.services = Some(inspectah_core::types::services::ServiceSection {
            state_changes: vec![inspectah_core::types::services::ServiceStateChange {
                unit: "firewalld.service".into(),
                current_state: inspectah_core::types::services::ServiceUnitState::Enabled,
                default_state: Some(inspectah_core::types::services::PresetDefault::Disable),
                include: true,
                locked: false,
                owning_package: Some("firewalld".into()),
                fleet: None,
                attention_reason: None,
            }],
            enabled_units: vec!["firewalld.service".into()],
            disabled_units: vec![],
            drop_ins: vec![],
            preset_matched_units: vec![],
        });
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            html.contains("Service State Changes"),
            "report must contain Service State Changes section"
        );
        assert!(
            html.contains("firewalld.service"),
            "services table must contain unit name"
        );
        assert!(
            html.contains("enabled"),
            "services table must show current state"
        );
    }

    #[test]
    fn test_report_services_table_columns() {
        let mut snap = test_snapshot();
        snap.services = Some(inspectah_core::types::services::ServiceSection {
            state_changes: vec![inspectah_core::types::services::ServiceStateChange {
                unit: "sshd.service".into(),
                current_state: inspectah_core::types::services::ServiceUnitState::Disabled,
                default_state: Some(inspectah_core::types::services::PresetDefault::Enable),
                include: false,
                locked: false,
                owning_package: Some("openssh-server".into()),
                fleet: None,
                attention_reason: None,
            }],
            enabled_units: vec![],
            disabled_units: vec!["sshd.service".into()],
            drop_ins: vec![],
            preset_matched_units: vec![],
        });
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            html.contains("<th>Unit</th>"),
            "table must have Unit column"
        );
        assert!(
            html.contains("<th>Current State</th>"),
            "table must have Current State column"
        );
        assert!(
            html.contains("<th>Default State</th>"),
            "table must have Default State column"
        );
        assert!(
            html.contains("<th>Action</th>"),
            "table must have Action column"
        );
        assert!(
            html.contains("exclude"),
            "services table must show exclude for include=false"
        );
    }

    #[test]
    fn test_report_empty_services_shows_empty_state() {
        let mut snap = test_snapshot();
        snap.services = Some(inspectah_core::types::services::ServiceSection {
            state_changes: vec![],
            enabled_units: vec![],
            disabled_units: vec![],
            drop_ins: vec![],
            preset_matched_units: vec![],
        });
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            html.contains("(0)"),
            "empty services section must show (0) badge"
        );
        assert!(
            html.contains("No service state changes detected"),
            "empty services section must show empty state message"
        );
    }

    #[test]
    fn test_report_services_extra_badge() {
        let mut snap = test_snapshot();
        snap.services = Some(inspectah_core::types::services::ServiceSection {
            state_changes: vec![
                inspectah_core::types::services::ServiceStateChange {
                    unit: "firewalld.service".into(),
                    current_state: inspectah_core::types::services::ServiceUnitState::Enabled,
                    default_state: Some(inspectah_core::types::services::PresetDefault::Disable),
                    include: true,
                    locked: false,
                    owning_package: None,
                    fleet: None,
                    attention_reason: None,
                },
                inspectah_core::types::services::ServiceStateChange {
                    unit: "cups.service".into(),
                    current_state: inspectah_core::types::services::ServiceUnitState::Masked,
                    default_state: None,
                    include: true,
                    locked: false,
                    owning_package: None,
                    fleet: None,
                    attention_reason: None,
                },
            ],
            enabled_units: vec!["firewalld.service".into()],
            disabled_units: vec![],
            drop_ins: vec![],
            preset_matched_units: vec![],
        });
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            html.contains("1 enabled"),
            "extra badge must show enabled count"
        );
        assert!(
            html.contains("1 masked"),
            "extra badge must show masked count"
        );
    }

    // -----------------------------------------------------------------------
    // Storage section tests (T9)
    // -----------------------------------------------------------------------

    #[test]
    fn test_report_contains_storage_section() {
        let mut snap = test_snapshot();
        snap.storage = Some(inspectah_core::types::storage::StorageSection {
            fstab_entries: vec![inspectah_core::types::storage::FstabEntry {
                device: "/dev/sda1".into(),
                mount_point: "/boot".into(),
                fstype: "xfs".into(),
                options: "defaults".into(),
                ..Default::default()
            }],
            ..Default::default()
        });
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            html.contains("Storage"),
            "report must contain Storage section"
        );
        assert!(
            html.contains("/dev/sda1") || html.contains("&#x2f;dev&#x2f;sda1"),
            "storage table must contain device"
        );
        assert!(
            html.contains("/boot") || html.contains("&#x2f;boot"),
            "storage table must contain mount point"
        );
        assert!(
            html.contains("xfs"),
            "storage table must contain fs type"
        );
    }

    #[test]
    fn test_report_storage_table_columns() {
        let mut snap = test_snapshot();
        snap.storage = Some(inspectah_core::types::storage::StorageSection {
            fstab_entries: vec![inspectah_core::types::storage::FstabEntry {
                device: "/dev/sda1".into(),
                mount_point: "/boot".into(),
                fstype: "xfs".into(),
                options: "defaults".into(),
                ..Default::default()
            }],
            ..Default::default()
        });
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(html.contains("<th>Device</th>"), "table must have Device column");
        assert!(html.contains("<th>Mount Point</th>"), "table must have Mount Point column");
        assert!(html.contains("<th>Type</th>"), "table must have Type column");
        assert!(html.contains("<th>Options</th>"), "table must have Options column");
    }

    #[test]
    fn test_report_storage_empty_shows_empty_state() {
        let mut snap = test_snapshot();
        snap.storage = Some(inspectah_core::types::storage::StorageSection::default());
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            html.contains("No fstab entries detected"),
            "empty storage section must show empty state"
        );
    }

    #[test]
    fn test_report_absent_storage_not_rendered() {
        let snap = test_snapshot();
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            !html.contains(r#"id="storage""#),
            "absent storage section must not be rendered"
        );
    }

    // -----------------------------------------------------------------------
    // Kernel & Boot section tests (T9)
    // -----------------------------------------------------------------------

    #[test]
    fn test_report_contains_kernel_section() {
        let mut snap = test_snapshot();
        snap.kernel_boot = Some(inspectah_core::types::kernelboot::KernelBootSection {
            cmdline: "quiet crashkernel=auto".into(),
            sysctl_overrides: vec![inspectah_core::types::kernelboot::SysctlOverride {
                key: "kernel.sysrq".into(),
                runtime: "16".into(),
                default: "0".into(),
                source: "/etc/sysctl.d/99-custom.conf".into(),
                include: true,
                locked: false,
                fleet: None,
            }],
            ..Default::default()
        });
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            html.contains("Kernel &amp; Boot") || html.contains("Kernel & Boot"),
            "report must contain Kernel & Boot section"
        );
        assert!(
            html.contains("quiet crashkernel=auto"),
            "kernel section must show cmdline"
        );
        assert!(
            html.contains("kernel.sysrq"),
            "kernel section must show sysctl key"
        );
    }

    #[test]
    fn test_report_kernel_sysctl_table_columns() {
        let mut snap = test_snapshot();
        snap.kernel_boot = Some(inspectah_core::types::kernelboot::KernelBootSection {
            sysctl_overrides: vec![inspectah_core::types::kernelboot::SysctlOverride {
                key: "net.ipv4.ip_forward".into(),
                runtime: "1".into(),
                default: "0".into(),
                source: "/etc/sysctl.d/10-forwarding.conf".into(),
                include: true,
                locked: false,
                fleet: None,
            }],
            ..Default::default()
        });
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(html.contains("<th>Key</th>"), "sysctl table must have Key column");
        assert!(html.contains("<th>Runtime</th>"), "sysctl table must have Runtime column");
        assert!(html.contains("<th>Default</th>"), "sysctl table must have Default column");
        assert!(html.contains("<th>Source</th>"), "sysctl table must have Source column");
    }

    #[test]
    fn test_report_absent_kernel_not_rendered() {
        let snap = test_snapshot();
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            !html.contains(r#"id="kernel-boot""#),
            "absent kernel section must not be rendered"
        );
    }

    // -----------------------------------------------------------------------
    // Security & Access Control section tests (T9)
    // -----------------------------------------------------------------------

    #[test]
    fn test_report_contains_security_section() {
        let mut snap = test_snapshot();
        snap.selinux = Some(inspectah_core::types::selinux::SelinuxSection {
            mode: "enforcing".into(),
            custom_modules: vec!["myapp".into()],
            boolean_overrides: vec![
                serde_json::json!({"name": "httpd_can_network_connect", "state": true}),
            ],
            fcontext_rules: vec!["/opt/app(/.*)?".into()],
            fips_mode: false,
            ..Default::default()
        });
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            html.contains("Security &amp; Access Control")
                || html.contains("Security & Access Control"),
            "report must contain Security & Access Control section"
        );
        assert!(
            html.contains("enforcing"),
            "security section must show SELinux mode"
        );
        assert!(
            html.contains("myapp"),
            "security section must show custom module"
        );
        assert!(
            html.contains("httpd_can_network_connect"),
            "security section must show boolean override"
        );
    }

    #[test]
    fn test_report_security_fips_enabled() {
        let mut snap = test_snapshot();
        snap.selinux = Some(inspectah_core::types::selinux::SelinuxSection {
            mode: "enforcing".into(),
            fips_mode: true,
            ..Default::default()
        });
        let html = render_report(&snap, &RenderContext { target: None });
        // Should show "enabled" somewhere in the FIPS row
        assert!(
            html.contains("enabled"),
            "security section must show FIPS enabled"
        );
    }

    #[test]
    fn test_report_absent_security_not_rendered() {
        let snap = test_snapshot();
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            !html.contains(r#"id="security""#),
            "absent security section must not be rendered"
        );
    }

    #[test]
    fn test_report_security_extra_badge_shows_mode() {
        let mut snap = test_snapshot();
        snap.selinux = Some(inspectah_core::types::selinux::SelinuxSection {
            mode: "permissive".into(),
            ..Default::default()
        });
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            html.contains("permissive"),
            "security extra badge must show SELinux mode"
        );
    }

    // -----------------------------------------------------------------------
    // Failed-conditional section tests (T9 — proof #16)
    // -----------------------------------------------------------------------

    #[test]
    fn test_report_failed_conditional_section_renders() {
        let mut snap = InspectionSnapshot::new();
        // Storage is absent (None) but in failed_sections
        snap.completeness = Completeness::Incomplete {
            failed_sections: vec![InspectorId::Storage],
            degraded_sections: vec![],
            reason: "storage inspector failed".into(),
        };
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            html.contains("Storage"),
            "failed section must be rendered even when data absent"
        );
        assert!(
            html.contains("data unavailable"),
            "failed section shows data unavailable"
        );
    }

    #[test]
    fn test_report_absent_conditional_section_not_rendered() {
        let snap = InspectionSnapshot::new();
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            !html.contains(r#"id="storage""#),
            "absent section must not be rendered"
        );
    }

    #[test]
    fn test_report_failed_kernel_renders() {
        let mut snap = InspectionSnapshot::new();
        snap.completeness = Completeness::Incomplete {
            failed_sections: vec![InspectorId::KernelBoot],
            degraded_sections: vec![],
            reason: "kernel inspector failed".into(),
        };
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            html.contains("Kernel"),
            "failed kernel section must be rendered"
        );
        assert!(
            html.contains("data unavailable"),
            "failed kernel section shows data unavailable"
        );
    }

    #[test]
    fn test_report_failed_security_renders() {
        let mut snap = InspectionSnapshot::new();
        snap.completeness = Completeness::Incomplete {
            failed_sections: vec![InspectorId::Selinux],
            degraded_sections: vec![],
            reason: "selinux inspector failed".into(),
        };
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            html.contains("Security"),
            "failed security section must be rendered"
        );
        assert!(
            html.contains("data unavailable"),
            "failed security section shows data unavailable"
        );
    }

    // -----------------------------------------------------------------------
    // Scheduled Tasks section tests (T10)
    // -----------------------------------------------------------------------

    #[test]
    fn test_report_scheduled_tasks_renders_with_data() {
        use inspectah_core::types::scheduled::{CronJob, ScheduledTaskSection, SystemdTimer};
        let mut snap = test_snapshot();
        snap.scheduled_tasks = Some(ScheduledTaskSection {
            cron_jobs: vec![CronJob {
                path: "/etc/cron.d/backup".into(),
                source: "file".into(),
                include: true,
                ..Default::default()
            }],
            systemd_timers: vec![SystemdTimer {
                name: "logrotate.timer".into(),
                on_calendar: "daily".into(),
                description: "Rotate logs".into(),
                include: true,
                ..Default::default()
            }],
            ..Default::default()
        });
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            html.contains("Scheduled Tasks"),
            "must render Scheduled Tasks section heading"
        );
        assert!(
            html.contains("/etc/cron.d/backup")
                || html.contains("&#x2f;etc&#x2f;cron.d&#x2f;backup"),
            "must render cron job path"
        );
        assert!(
            html.contains("logrotate.timer"),
            "must render systemd timer name"
        );
        assert!(html.contains("(2)"), "badge must show total count 2");
    }

    #[test]
    fn test_report_scheduled_tasks_hidden_when_absent() {
        let snap = test_snapshot();
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            !html.contains(r#"id="scheduled-tasks""#),
            "scheduled tasks section must not render when data is absent"
        );
    }

    // -----------------------------------------------------------------------
    // Non-RPM Software section tests (T10)
    // -----------------------------------------------------------------------

    #[test]
    fn test_report_nonrpm_renders_with_data() {
        use inspectah_core::types::nonrpm::{NonRpmItem, NonRpmSoftwareSection};
        let mut snap = test_snapshot();
        snap.non_rpm_software = Some(NonRpmSoftwareSection {
            items: vec![NonRpmItem {
                path: "/opt/app/bin".into(),
                name: "custom-app".into(),
                method: "binary".into(),
                confidence: "high".into(),
                lang: "c".into(),
                version: "1.0.0".into(),
                include: true,
                ..Default::default()
            }],
            ..Default::default()
        });
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            html.contains("Non-RPM Software"),
            "must render Non-RPM Software section heading"
        );
        assert!(
            html.contains("custom-app"),
            "must render non-RPM item name"
        );
        assert!(
            html.contains("/opt/app/bin") || html.contains("&#x2f;opt&#x2f;app&#x2f;bin"),
            "must render non-RPM item path"
        );
    }

    #[test]
    fn test_report_nonrpm_hidden_when_absent() {
        let snap = test_snapshot();
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            !html.contains(r#"id="nonrpm""#),
            "non-RPM section must not render when data is absent"
        );
    }

    // -----------------------------------------------------------------------
    // Warnings section tests (T10)
    // -----------------------------------------------------------------------

    #[test]
    fn test_report_warnings_always_renders() {
        let snap = test_snapshot();
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            html.contains(r#"id="warnings""#),
            "warnings section must always render even with 0 warnings"
        );
        assert!(
            html.contains("No warnings"),
            "empty warnings section must show empty state"
        );
    }

    #[test]
    fn test_report_warnings_renders_with_data() {
        let mut snap = test_snapshot();
        snap.warnings = vec![inspectah_core::types::warnings::Warning {
            inspector: "config".into(),
            message: "Found orphaned config file".into(),
            severity: None,
            extra: Default::default(),
        }];
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            html.contains("Found orphaned config file"),
            "must render warning message"
        );
        assert!(
            html.contains("report-section--warning"),
            "warnings section must have warning class"
        );
    }

    // -----------------------------------------------------------------------
    // Redactions section tests (T10)
    // -----------------------------------------------------------------------

    #[test]
    fn test_report_redactions_always_renders() {
        let snap = test_snapshot();
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            html.contains(r#"id="redactions""#),
            "redactions section must always render"
        );
        assert!(
            html.contains("secrets-review.md"),
            "redactions section must point to secrets-review.md"
        );
    }

    #[test]
    fn test_report_redactions_shows_count() {
        use inspectah_core::types::redaction::{
            DetectionMethod, RedactionFinding, RedactionKind,
        };
        let mut snap = test_snapshot();
        snap.redactions = vec![
            RedactionFinding {
                path: "/etc/shadow".into(),
                source: "file".into(),
                kind: RedactionKind::Excluded,
                pattern: String::new(),
                remediation: String::new(),
                line: None,
                replacement: None,
                detection_method: DetectionMethod::Pattern,
                confidence: None,
                finding_kind: None,
            },
            RedactionFinding {
                path: "/etc/pki/key.pem".into(),
                source: "file".into(),
                kind: RedactionKind::Excluded,
                pattern: String::new(),
                remediation: String::new(),
                line: None,
                replacement: None,
                detection_method: DetectionMethod::Pattern,
                confidence: None,
                finding_kind: None,
            },
        ];
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            html.contains("2 item(s) redacted"),
            "redactions must show count of redacted items"
        );
    }

    // -----------------------------------------------------------------------
    // Failed section rendering for T10 sections
    // -----------------------------------------------------------------------

    #[test]
    fn test_report_failed_scheduled_renders() {
        let mut snap = InspectionSnapshot::new();
        snap.completeness = Completeness::Incomplete {
            failed_sections: vec![InspectorId::ScheduledTasks],
            degraded_sections: vec![],
            reason: "scheduled tasks inspector failed".into(),
        };
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            html.contains("Scheduled Tasks"),
            "failed scheduled tasks section must be rendered"
        );
        assert!(
            html.contains("data unavailable"),
            "failed scheduled tasks section shows data unavailable"
        );
    }

    #[test]
    fn test_report_failed_nonrpm_renders() {
        let mut snap = InspectionSnapshot::new();
        snap.completeness = Completeness::Incomplete {
            failed_sections: vec![InspectorId::NonRpmSoftware],
            degraded_sections: vec![],
            reason: "non-rpm inspector failed".into(),
        };
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            html.contains("Non-RPM Software"),
            "failed non-RPM section must be rendered"
        );
        assert!(
            html.contains("data unavailable"),
            "failed non-RPM section shows data unavailable"
        );
    }

    // -----------------------------------------------------------------------
    // Users & Groups section tests (T11)
    // -----------------------------------------------------------------------

    fn users_snapshot() -> InspectionSnapshot {
        let mut snap = test_snapshot();
        snap.users_groups = Some(inspectah_core::types::users::UserGroupSection {
            users: vec![serde_json::json!({
                "name": "alice",
                "uid": 1000,
                "gid": 1000,
                "shell": "/bin/bash",
                "home": "/home/alice",
                "include": true,
                "classification": "interactive",
                "containerfile_strategy": "useradd",
                "password_choice": "preserve",
                "has_sudo": true,
                "ssh_key_count": 2,
                "supplementary_groups": ["wheel", "docker"]
            })],
            ..Default::default()
        });
        snap
    }

    #[test]
    fn test_report_users_excludes_password_hash() {
        let mut snap = InspectionSnapshot::new();
        snap.users_groups = Some(inspectah_core::types::users::UserGroupSection {
            users: vec![serde_json::json!({
                "name": "alice",
                "uid": 1000,
                "gid": 1000,
                "shell": "/bin/bash",
                "home": "/home/alice",
                "include": true,
                "classification": "interactive",
                "containerfile_strategy": "useradd",
                "password_choice": "preserve",
                "password_hash": "$6$secret_hash_value",
                "ssh_keys": ["ssh-ed25519 AAAA_secret_key_content"]
            })],
            ..Default::default()
        });
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(html.contains("alice"), "user name should appear");
        assert!(
            !html.contains("secret_hash_value"),
            "password_hash must not appear in HTML"
        );
        assert!(
            !html.contains("secret_key_content"),
            "ssh_keys content must not appear in HTML"
        );
    }

    #[test]
    fn test_report_contains_users_section() {
        let snap = users_snapshot();
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            html.contains("Users &amp; Groups") || html.contains("Users & Groups"),
            "report must contain Users & Groups section"
        );
        assert!(
            html.contains("alice"),
            "users table must contain user name"
        );
    }

    #[test]
    fn test_report_users_table_columns() {
        let snap = users_snapshot();
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(html.contains("<th>Name</th>"), "table must have Name column");
        assert!(html.contains("<th>UID</th>"), "table must have UID column");
        assert!(html.contains("<th>GID</th>"), "table must have GID column");
        assert!(
            html.contains("<th>Shell</th>"),
            "table must have Shell column"
        );
        assert!(html.contains("<th>Home</th>"), "table must have Home column");
        assert!(
            html.contains("<th>Classification</th>"),
            "table must have Classification column"
        );
        assert!(html.contains("<th>Sudo</th>"), "table must have Sudo column");
        assert!(
            html.contains("<th>SSH Keys</th>"),
            "table must have SSH Keys column"
        );
        assert!(
            html.contains("<th>Groups</th>"),
            "table must have Groups column"
        );
    }

    #[test]
    fn test_report_users_shows_field_values() {
        let snap = users_snapshot();
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            html.contains("interactive"),
            "must show classification value"
        );
        assert!(
            html.contains("wheel, docker"),
            "must show supplementary groups"
        );
        assert!(html.contains(">yes<"), "must show sudo=yes for has_sudo=true");
    }

    #[test]
    fn test_report_users_filters_excluded() {
        let mut snap = test_snapshot();
        snap.users_groups = Some(inspectah_core::types::users::UserGroupSection {
            users: vec![
                serde_json::json!({
                    "name": "included_user",
                    "uid": 1000, "gid": 1000,
                    "shell": "/bin/bash", "home": "/home/included",
                    "include": true,
                    "classification": "interactive",
                    "containerfile_strategy": "useradd",
                    "password_choice": "none"
                }),
                serde_json::json!({
                    "name": "excluded_user",
                    "uid": 1001, "gid": 1001,
                    "shell": "/sbin/nologin", "home": "/home/excluded",
                    "include": false,
                    "classification": "system",
                    "containerfile_strategy": "skip",
                    "password_choice": "none"
                }),
            ],
            ..Default::default()
        });
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            html.contains("included_user"),
            "included user must appear"
        );
        assert!(
            !html.contains("excluded_user"),
            "excluded user must not appear in rendered HTML"
        );
    }

    #[test]
    fn test_report_users_empty_shows_empty_state() {
        let mut snap = test_snapshot();
        snap.users_groups = Some(inspectah_core::types::users::UserGroupSection::default());
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            html.contains("No user accounts detected"),
            "empty users section must show empty state message"
        );
    }

    #[test]
    fn test_report_users_absent_not_rendered() {
        let snap = test_snapshot();
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            !html.contains(r#"id="users-groups""#),
            "absent users section must not be rendered"
        );
    }

    #[test]
    fn test_report_failed_users_renders() {
        let mut snap = InspectionSnapshot::new();
        snap.completeness = Completeness::Incomplete {
            failed_sections: vec![InspectorId::UsersGroups],
            degraded_sections: vec![],
            reason: "users inspector failed".into(),
        };
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            html.contains("Users"),
            "failed users section must be rendered"
        );
        assert!(
            html.contains("data unavailable"),
            "failed users section shows data unavailable"
        );
    }
}
