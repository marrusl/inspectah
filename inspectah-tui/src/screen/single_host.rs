//! SingleHost screen -- two-panel layout with sidebar + triage list + status bar.
//!
//! Composes section nav, triage list, and status bar widgets into the
//! main single-host inspection view.

use std::collections::HashMap;

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};

use inspectah_refine::session::RefineSession;
use inspectah_refine::types::{ItemId, TriageBucket};

use crate::sections::{SECTION_ORDER, build_section_entries};
use crate::theme::ColorTier;
use crate::types::{DetailMode, FocusTarget, SectionId, TuiState};
use crate::widget::containerfile::ContainerfileWidget;
use crate::widget::detail_view::{DetailContentType, DetailData, DetailViewWidget};
use crate::widget::info_bar::{InfoBarData, InfoBarWidget};
use crate::widget::section_nav::SectionNavWidget;
use crate::widget::status_bar::StatusBarWidget;
use crate::widget::triage_list::{ListItem, TriageGroup, TriageListWidget};
use crate::widget::user_strategy::{UserEntry, UserStrategyWidget};

const SIDEBAR_WIDTH: u16 = 18;

/// A raw item tuple: (name, detail, triage_group, include_state, item_id, has_content).
type RawItem = (
    String,
    String,
    TriageGroup,
    Option<bool>,
    Option<ItemId>,
    bool,
);

pub struct SingleHostScreen;

impl Default for SingleHostScreen {
    fn default() -> Self {
        Self
    }
}

impl SingleHostScreen {
    pub fn new() -> Self {
        Self
    }

    pub fn render(
        &self,
        frame: &mut Frame,
        session: &RefineSession,
        state: &TuiState,
        tier: ColorTier,
    ) {
        let area = frame.area();

        // Top-level vertical split: main area + status bar (1 row).
        let vertical = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(1)])
            .split(area);

        let main_area = vertical[0];
        let status_area = vertical[1];

        // --- Build section data (needed for both layouts and status bar) ---
        let entries = build_section_entries(session);
        let active_section_id = SECTION_ORDER
            .get(state.active_section)
            .copied()
            .unwrap_or(SectionId::Packages);
        let items = build_list_items(session, active_section_id, state);

        // --- Containerfile toggle: hide sidebar, split 50/50 ---
        if state.show_containerfile {
            let halves = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(main_area);

            let list_area = halves[0];
            let cf_area = halves[1];

            // Left: triage list.
            let list_focused = state.focus == FocusTarget::ItemList;
            let list_widget = TriageListWidget::new(
                &items,
                state.cursor,
                active_section_id,
                list_focused,
                tier,
                0,
            );
            frame.render_widget(list_widget, list_area);

            // Right: containerfile preview.
            let view = session.view();
            let cf_widget = ContainerfileWidget::new(&view.containerfile_preview, tier);
            frame.render_widget(cf_widget, cf_area);
        } else {
            // --- Default layout: sidebar + item list ---
            let horizontal = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Length(SIDEBAR_WIDTH), Constraint::Min(20)])
                .split(main_area);

            let sidebar_area = horizontal[0];
            let list_area = horizontal[1];

            // --- Sidebar ---
            let sidebar_focused = state.focus == FocusTarget::Sidebar;
            let sidebar = SectionNavWidget::new(
                &entries,
                state.active_section,
                sidebar_focused,
                tier,
                state.sidebar_scroll,
            );
            frame.render_widget(sidebar, sidebar_area);

            // --- Users section: specialized strategy view ---
            if active_section_id == SectionId::Users {
                let user_entries = build_user_entries(session);
                let list_focused = state.focus == FocusTarget::ItemList;
                let user_widget =
                    UserStrategyWidget::new(&user_entries, state.cursor, list_focused, tier);
                frame.render_widget(user_widget, list_area);
            // --- Triage list / Detail view ---
            // Fullscreen detail replaces the item list entirely.
            } else if state.detail_mode == DetailMode::Fullscreen {
                if let Some(data) = build_detail_data(session, state, &items) {
                    frame.render_widget(
                        DetailViewWidget::new(&data, state.detail_scroll, tier),
                        list_area,
                    );
                }
            } else {
                // Split list area for info bar when active.
                let (list_render_area, info_area) = if state.detail_mode == DetailMode::InfoBar {
                    let split = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([Constraint::Min(5), Constraint::Length(4)])
                        .split(list_area);
                    (split[0], Some(split[1]))
                } else {
                    (list_area, None)
                };

                let list_focused = state.focus == FocusTarget::ItemList;
                let list_widget = TriageListWidget::new(
                    &items,
                    state.cursor,
                    active_section_id,
                    list_focused,
                    tier,
                    0, // scroll_offset -- wired in Task 11
                );
                frame.render_widget(list_widget, list_render_area);

                // --- Info bar ---
                if let Some(info_area) = info_area
                    && let Some(data) = build_info_bar_data(session, state, &items)
                {
                    frame.render_widget(InfoBarWidget::new(&data, tier), info_area);
                }
            }
        }

        // --- Status bar ---
        let view = session.view();
        let stats = &view.stats;

        // Find current section's entry for included/excluded counts.
        let section_entry = entries.iter().find(|e| e.id == active_section_id);

        let (included, excluded) = section_entry
            .map(|e| (e.included, e.excluded))
            .unwrap_or((0, 0));

        // Count reviewed items for this section (items whose viewed key
        // starts with the section's prefix).
        let (reviewed, total_reviewable) = if let Some(prefix) = active_section_id.viewed_prefix() {
            let prefix_colon = format!("{prefix}:");
            let reviewed = session
                .viewed_ids()
                .iter()
                .filter(|id| id.starts_with(&prefix_colon))
                .count();
            let total = items.iter().filter(|i| !i.is_group_header).count();
            (reviewed, total)
        } else {
            (0, 0)
        };

        let status = StatusBarWidget::new(tier)
            .stats(included, excluded, stats.needs_review_count)
            .reviewed_progress(reviewed, total_reviewable)
            .decision_section(active_section_id.is_decision())
            .flash(state.flash.as_ref());
        frame.render_widget(status, status_area);
    }
}

/// Map a `TriageBucket` from session data to the TUI's `TriageGroup`.
fn bucket_to_group(bucket: TriageBucket) -> TriageGroup {
    match bucket {
        TriageBucket::Investigate => TriageGroup::Investigate,
        TriageBucket::Site => TriageGroup::Site,
        TriageBucket::Baseline => TriageGroup::Baseline,
    }
}

/// Build `ListItem`s for the active section from session data.
///
/// Decision sections (Packages, Configs, Services, Containers, Sysctls,
/// Tuned) produce grouped items with triage buckets and toggle state.
/// Reference sections (VerChanges, KernelBoot, Network, Storage,
/// ScheduledTasks, NonRpmSoftware, SELinux) produce flat lists.
/// Users returns empty — rendered by `UserStrategyWidget` instead.
pub fn build_list_items(
    session: &RefineSession,
    section: SectionId,
    state: &TuiState,
) -> Vec<ListItem> {
    let raw_items: Vec<RawItem> = match section {
        SectionId::Packages => {
            let view = session.view();
            view.packages
                .iter()
                .map(|pkg| {
                    let name = if pkg.entry.arch.is_empty() || pkg.entry.arch == "noarch" {
                        pkg.entry.name.clone()
                    } else {
                        format!("{}.{}", pkg.entry.name, pkg.entry.arch)
                    };
                    let detail = if pkg.entry.source_repo.is_empty() {
                        pkg.entry.version.clone()
                    } else {
                        format!("{} ({})", pkg.entry.version, pkg.entry.source_repo)
                    };
                    let group = bucket_to_group(pkg.triage.bucket());
                    let id = ItemId::Package {
                        name: pkg.entry.name.clone(),
                        arch: pkg.entry.arch.clone(),
                    };
                    (
                        name,
                        detail,
                        group,
                        Some(pkg.entry.include),
                        Some(id),
                        false,
                    )
                })
                .collect()
        }
        SectionId::Configs => {
            let view = session.view();
            view.config_files
                .iter()
                .map(|cfg| {
                    let name = cfg.entry.path.clone();
                    let detail = format!("{:?}", cfg.entry.category);
                    let group = bucket_to_group(cfg.triage.bucket());
                    let id = ItemId::Config {
                        path: cfg.entry.path.clone(),
                    };
                    // Configs have diff content available for fullscreen detail.
                    let has_content =
                        cfg.entry.diff_against_rpm.is_some() || !cfg.entry.content.is_empty();
                    (
                        name,
                        detail,
                        group,
                        Some(cfg.entry.include),
                        Some(id),
                        has_content,
                    )
                })
                .collect()
        }
        // ── Decision sections ─────────────────────────────────────
        SectionId::Services => {
            let decisions = session.decisions();
            let reference = session.reference();
            let mut items: Vec<RawItem> = Vec::new();

            // Decision: service state changes (togglable).
            for svc in &decisions.service_states {
                let name = svc.entry.unit.clone();
                let detail = format!("{:?}", svc.entry.current_state);
                let group = bucket_to_group(svc.triage.bucket());
                let id = ItemId::Service {
                    unit: svc.entry.unit.clone(),
                };
                items.push((
                    name,
                    detail,
                    group,
                    Some(svc.entry.include),
                    Some(id),
                    false,
                ));
            }

            // Decision: service drop-ins (togglable).
            for di in &decisions.service_dropins {
                let name = format!("{} (drop-in)", di.entry.unit);
                let detail = di.entry.path.clone();
                let group = bucket_to_group(di.triage.bucket());
                let id = ItemId::DropIn {
                    path: di.entry.path.clone(),
                };
                let has_content = !di.entry.content.is_empty();
                items.push((
                    name,
                    detail,
                    group,
                    Some(di.entry.include),
                    Some(id),
                    has_content,
                ));
            }

            // Reference: services sub-collections (read-only, no toggle).
            let rs = &reference.services;
            for s in &rs.divergent {
                items.push((
                    s.unit.clone(),
                    "divergent".into(),
                    TriageGroup::Site,
                    None,
                    None,
                    false,
                ));
            }
            for s in &rs.preset_matched_with_dropins {
                items.push((
                    s.unit.clone(),
                    "preset+dropins".into(),
                    TriageGroup::Site,
                    None,
                    None,
                    false,
                ));
            }
            for s in &rs.preset_unknown_enabled {
                items.push((
                    s.unit.clone(),
                    "unknown (enabled)".into(),
                    TriageGroup::Site,
                    None,
                    None,
                    false,
                ));
            }
            for s in &rs.preset_unknown_disabled {
                items.push((
                    s.unit.clone(),
                    "unknown (disabled)".into(),
                    TriageGroup::Site,
                    None,
                    None,
                    false,
                ));
            }
            for s in &rs.standalone_dropins {
                items.push((
                    s.unit.clone(),
                    format!("drop-in: {}", s.path),
                    TriageGroup::Site,
                    None,
                    None,
                    !s.content.is_empty(),
                ));
            }
            for s in &rs.omitted {
                items.push((
                    s.unit.clone(),
                    format!("omitted: {}", s.reason),
                    TriageGroup::Baseline,
                    None,
                    None,
                    false,
                ));
            }
            for a in &rs.advisories {
                items.push((
                    a.unit.clone(),
                    format!("advisory ({})", a.owning_package),
                    TriageGroup::Investigate,
                    None,
                    None,
                    false,
                ));
            }
            for w in &rs.warnings {
                items.push((
                    w.unit.clone(),
                    format!("warning: {}", w.message),
                    TriageGroup::Investigate,
                    None,
                    None,
                    false,
                ));
            }

            items
        }
        SectionId::Containers => {
            let decisions = session.decisions();
            let reference = session.reference();
            let mut items: Vec<RawItem> = Vec::new();

            // Decision: quadlets (togglable).
            for q in &decisions.quadlets {
                let name = q.entry.name.clone();
                let detail = q.entry.image.clone();
                let group = bucket_to_group(q.triage.bucket());
                let id = ItemId::Quadlet {
                    path: q.entry.path.clone(),
                };
                let has_content = !q.entry.content.is_empty();
                items.push((
                    name,
                    detail,
                    group,
                    Some(q.entry.include),
                    Some(id),
                    has_content,
                ));
            }

            // Decision: flatpaks (togglable).
            for f in &decisions.flatpaks {
                let name = f.entry.app_id.clone();
                let detail = format!("{}/{}", f.entry.origin, f.entry.branch);
                let group = bucket_to_group(f.triage.bucket());
                let id = ItemId::Flatpak {
                    app_id: f.entry.app_id.clone(),
                    remote: f.entry.remote.clone(),
                    branch: f.entry.branch.clone(),
                };
                items.push((name, detail, group, Some(f.entry.include), Some(id), false));
            }

            // Reference: running containers (read-only).
            let rc = &reference.containers;
            for c in &rc.running_containers {
                items.push((
                    c.name.clone(),
                    format!("{} ({})", c.image, c.status),
                    TriageGroup::Site,
                    None,
                    None,
                    false,
                ));
            }

            // Reference: compose files (read-only).
            for cf in &rc.compose_files {
                let svc_names: Vec<&str> = cf.services.iter().map(|s| s.service.as_str()).collect();
                items.push((
                    cf.path.clone(),
                    svc_names.join(", "),
                    TriageGroup::Site,
                    None,
                    None,
                    false,
                ));
            }

            items
        }
        SectionId::Sysctls => {
            let decisions = session.decisions();
            decisions
                .sysctls
                .iter()
                .map(|s| {
                    let name = s.entry.key.clone();
                    let detail = format!("{} (source: {})", s.entry.runtime, s.entry.source);
                    let group = bucket_to_group(s.triage.bucket());
                    let id = ItemId::Sysctl {
                        key: s.entry.key.clone(),
                    };
                    (name, detail, group, Some(s.entry.include), Some(id), false)
                })
                .collect()
        }
        SectionId::Tuned => {
            let decisions = session.decisions();
            decisions
                .tuned
                .iter()
                .map(|t| {
                    let name = t.active_profile.clone();
                    let detail = if t.custom_profiles.is_empty() {
                        "standard profile".into()
                    } else {
                        format!("custom: {}", t.custom_profiles.join(", "))
                    };
                    let group = bucket_to_group(t.triage.bucket());
                    let id = ItemId::TunedSelection {
                        profile: t.active_profile.clone(),
                    };
                    (name, detail, group, Some(t.include), Some(id), false)
                })
                .collect()
        }
        // Users section is rendered by UserStrategyWidget, not the triage list.
        SectionId::Users => Vec::new(),

        // ── Reference sections (flat list, no toggle) ────────────
        SectionId::VerChanges => {
            let reference = session.reference();
            let vc = &reference.version_changes;
            let mut items: Vec<RawItem> = Vec::new();
            for v in &vc.downgrades {
                let name = format!("{}.{}", v.name, v.arch);
                let detail = format!("{} -> {} (downgrade)", v.host_version, v.base_version);
                items.push((name, detail, TriageGroup::Investigate, None, None, false));
            }
            for v in &vc.upgrades {
                let name = format!("{}.{}", v.name, v.arch);
                let detail = format!("{} -> {} (upgrade)", v.host_version, v.base_version);
                items.push((name, detail, TriageGroup::Site, None, None, false));
            }
            items
        }
        SectionId::KernelBoot => {
            let reference = session.reference();
            let kb = &reference.kernel_boot;
            let mut items: Vec<RawItem> = Vec::new();

            if let Some(ref cmdline) = kb.cmdline {
                items.push((
                    "cmdline".into(),
                    cmdline.clone(),
                    TriageGroup::Site,
                    None,
                    None,
                    true,
                ));
            }
            if let Some(ref grub) = kb.grub_defaults {
                items.push((
                    "grub defaults".into(),
                    grub.clone(),
                    TriageGroup::Site,
                    None,
                    None,
                    true,
                ));
            }
            if let Some(ref tuned) = kb.tuned_active {
                items.push((
                    "tuned active".into(),
                    tuned.clone(),
                    TriageGroup::Site,
                    None,
                    None,
                    false,
                ));
            }
            if let Some(ref locale) = kb.locale {
                items.push((
                    "locale".into(),
                    locale.clone(),
                    TriageGroup::Site,
                    None,
                    None,
                    false,
                ));
            }
            if let Some(ref tz) = kb.timezone {
                items.push((
                    "timezone".into(),
                    tz.clone(),
                    TriageGroup::Site,
                    None,
                    None,
                    false,
                ));
            }
            for s in &kb.sysctl_overrides {
                items.push((
                    s.key.clone(),
                    format!("{} (default: {})", s.runtime, s.default),
                    TriageGroup::Site,
                    None,
                    Some(ItemId::Sysctl { key: s.key.clone() }),
                    false,
                ));
            }
            for m in &kb.non_default_modules {
                items.push((
                    m.name.clone(),
                    format!("size: {}, used by: {}", m.size, m.used_by),
                    TriageGroup::Site,
                    None,
                    Some(ItemId::KernelModule {
                        name: m.name.clone(),
                    }),
                    false,
                ));
            }
            for c in &kb.modules_load_d {
                items.push((
                    c.path.clone(),
                    "modules-load.d".into(),
                    TriageGroup::Site,
                    None,
                    None,
                    !c.content.is_empty(),
                ));
            }
            for c in &kb.modprobe_d {
                items.push((
                    c.path.clone(),
                    "modprobe.d".into(),
                    TriageGroup::Site,
                    None,
                    None,
                    !c.content.is_empty(),
                ));
            }
            for c in &kb.dracut_conf {
                items.push((
                    c.path.clone(),
                    "dracut.conf".into(),
                    TriageGroup::Site,
                    None,
                    None,
                    !c.content.is_empty(),
                ));
            }
            for c in &kb.custom_tuned_profiles {
                items.push((
                    c.path.clone(),
                    "tuned profile".into(),
                    TriageGroup::Site,
                    None,
                    None,
                    !c.content.is_empty(),
                ));
            }
            for a in &kb.alternatives {
                items.push((
                    a.name.clone(),
                    format!("{} ({})", a.path, a.status),
                    TriageGroup::Site,
                    None,
                    None,
                    false,
                ));
            }

            items
        }
        SectionId::Network => {
            let reference = session.reference();
            let net = &reference.network;
            let mut items: Vec<RawItem> = Vec::new();

            for c in &net.connections {
                items.push((
                    c.name.clone(),
                    format!("{} ({})", c.conn_type, c.method),
                    TriageGroup::Site,
                    None,
                    Some(ItemId::NMConnection {
                        path: c.path.clone(),
                    }),
                    false,
                ));
            }
            for z in &net.firewall_zones {
                items.push((
                    z.name.clone(),
                    format!("zone ({})", z.path),
                    TriageGroup::Site,
                    None,
                    Some(ItemId::FirewallZone {
                        path: z.path.clone(),
                    }),
                    !z.content.is_empty(),
                ));
            }
            for r in &net.firewall_direct_rules {
                items.push((
                    format!("{}/{}", r.table, r.chain),
                    format!("ipv{} prio={}", r.ipv, r.priority),
                    TriageGroup::Site,
                    None,
                    None,
                    false,
                ));
            }
            for r in &net.static_routes {
                items.push((
                    r.name.clone(),
                    r.path.clone(),
                    TriageGroup::Site,
                    None,
                    None,
                    false,
                ));
            }
            for route in &net.ip_routes {
                items.push((
                    "ip route".into(),
                    route.clone(),
                    TriageGroup::Site,
                    None,
                    None,
                    false,
                ));
            }
            for rule in &net.ip_rules {
                items.push((
                    "ip rule".into(),
                    rule.clone(),
                    TriageGroup::Site,
                    None,
                    None,
                    false,
                ));
            }
            if !net.resolv_provenance.is_empty() {
                items.push((
                    "resolv.conf".into(),
                    net.resolv_provenance.clone(),
                    TriageGroup::Site,
                    None,
                    None,
                    false,
                ));
            }
            for h in &net.hosts_additions {
                items.push((
                    "/etc/hosts".into(),
                    h.clone(),
                    TriageGroup::Site,
                    None,
                    None,
                    false,
                ));
            }
            for p in &net.proxy_env {
                items.push((
                    "proxy".into(),
                    format!("{}: {}", p.source, p.line),
                    TriageGroup::Site,
                    None,
                    None,
                    false,
                ));
            }

            items
        }
        SectionId::Storage => {
            let reference = session.reference();
            let st = &reference.storage;
            let mut items: Vec<RawItem> = Vec::new();

            for f in &st.fstab_entries {
                items.push((
                    f.mount_point.clone(),
                    format!("{} ({} {})", f.device, f.fstype, f.options),
                    TriageGroup::Site,
                    None,
                    Some(ItemId::Fstab {
                        mount_point: f.mount_point.clone(),
                    }),
                    false,
                ));
            }
            for m in &st.mount_points {
                items.push((
                    m.target.clone(),
                    format!("{} ({} {})", m.source, m.fstype, m.options),
                    TriageGroup::Site,
                    None,
                    None,
                    false,
                ));
            }
            for lv in &st.lvm_volumes {
                items.push((
                    format!("{}/{}", lv.vg_name, lv.lv_name),
                    lv.lv_size.clone(),
                    TriageGroup::Site,
                    None,
                    None,
                    false,
                ));
            }
            for d in &st.var_directories {
                items.push((
                    d.path.clone(),
                    format!("{} — {}", d.size_estimate, d.recommendation),
                    TriageGroup::Site,
                    None,
                    None,
                    false,
                ));
            }
            for cr in &st.credential_refs {
                items.push((
                    cr.credential_path.clone(),
                    format!("{} ({})", cr.mount_point, cr.source),
                    TriageGroup::Site,
                    None,
                    None,
                    false,
                ));
            }

            items
        }
        SectionId::ScheduledTasks => {
            let reference = session.reference();
            reference
                .scheduled_tasks
                .iter()
                .map(|t| {
                    let detail = t.summary.as_deref().unwrap_or("").to_string();
                    let has_content = t.content.is_some();
                    (
                        t.key.clone(),
                        detail,
                        TriageGroup::Site,
                        None,
                        None,
                        has_content,
                    )
                })
                .collect()
        }
        SectionId::NonRpmSoftware => {
            let reference = session.reference();
            reference
                .non_rpm_software
                .iter()
                .map(|s| {
                    let detail = s.summary.as_deref().unwrap_or("").to_string();
                    let has_content = s.content.is_some();
                    let id = ItemId::NonRpm {
                        name: s.key.clone(),
                    };
                    (
                        s.key.clone(),
                        detail,
                        TriageGroup::Site,
                        None,
                        Some(id),
                        has_content,
                    )
                })
                .collect()
        }
        SectionId::SELinux => {
            let reference = session.reference();
            reference
                .selinux
                .iter()
                .map(|s| {
                    let detail = s.summary.as_deref().unwrap_or("").to_string();
                    let has_content = s.content.is_some();
                    (
                        s.key.clone(),
                        detail,
                        TriageGroup::Site,
                        None,
                        None,
                        has_content,
                    )
                })
                .collect()
        }
    };

    build_grouped_items(&raw_items, state, section)
}

/// Build info bar data for the currently selected item.
///
/// Extracts key-value fields based on section type. Packages show version,
/// repo, and triage group. Configs show kind, category. Other sections
/// fall back to name + detail.
fn build_info_bar_data(
    session: &RefineSession,
    state: &TuiState,
    items: &[ListItem],
) -> Option<InfoBarData> {
    let item = items.get(state.cursor)?;
    if item.is_group_header {
        return None;
    }

    let active_section_id = SECTION_ORDER
        .get(state.active_section)
        .copied()
        .unwrap_or(SectionId::Packages);

    let fields = match (active_section_id, &item.item_id) {
        (SectionId::Packages, Some(ItemId::Package { name, arch })) => {
            let view = session.view();
            if let Some(pkg) = view
                .packages
                .iter()
                .find(|p| p.entry.name == *name && p.entry.arch == *arch)
            {
                vec![
                    ("Version".into(), pkg.entry.version.clone()),
                    (
                        "Repo".into(),
                        if pkg.entry.source_repo.is_empty() {
                            "unknown".into()
                        } else {
                            pkg.entry.source_repo.clone()
                        },
                    ),
                    ("Triage".into(), format!("{:?}", pkg.triage.bucket())),
                ]
            } else {
                vec![("Detail".into(), item.detail.clone())]
            }
        }
        (SectionId::Configs, Some(ItemId::Config { path })) => {
            let view = session.view();
            if let Some(cfg) = view.config_files.iter().find(|c| c.entry.path == *path) {
                vec![
                    ("Kind".into(), format!("{:?}", cfg.entry.kind)),
                    ("Category".into(), format!("{:?}", cfg.entry.category)),
                ]
            } else {
                vec![("Detail".into(), item.detail.clone())]
            }
        }
        _ => {
            // Fallback for unwired sections.
            if item.detail.is_empty() {
                vec![]
            } else {
                vec![("Detail".into(), item.detail.clone())]
            }
        }
    };

    Some(InfoBarData {
        name: item.name.clone(),
        fields,
    })
}

/// Build fullscreen detail data for the currently selected item.
///
/// For configs: shows diff content (or raw content if no diff available).
/// For packages: shows a key-value plain text summary.
/// Other sections fall back to a plain text summary.
fn build_detail_data(
    session: &RefineSession,
    state: &TuiState,
    items: &[ListItem],
) -> Option<DetailData> {
    let item = items.get(state.cursor)?;
    if item.is_group_header {
        return None;
    }

    let active_section_id = SECTION_ORDER
        .get(state.active_section)
        .copied()
        .unwrap_or(SectionId::Packages);

    // Position string (1-indexed, excluding headers).
    let item_index = items
        .iter()
        .take(state.cursor + 1)
        .filter(|i| !i.is_group_header)
        .count();
    let total_items = items.iter().filter(|i| !i.is_group_header).count();
    let position = format!("{} of {}", item_index, total_items);

    match (active_section_id, &item.item_id) {
        (SectionId::Configs, Some(ItemId::Config { path })) => {
            let view = session.view();
            if let Some(cfg) = view.config_files.iter().find(|c| c.entry.path == *path) {
                let (content, content_type) = if let Some(ref diff) = cfg.entry.diff_against_rpm {
                    (diff.clone(), DetailContentType::Diff)
                } else if !cfg.entry.content.is_empty() {
                    (cfg.entry.content.clone(), DetailContentType::PlainText)
                } else {
                    (
                        "(no content available)".into(),
                        DetailContentType::PlainText,
                    )
                };
                Some(DetailData {
                    title: cfg.entry.path.clone(),
                    content,
                    content_type,
                    include: Some(cfg.entry.include),
                    position,
                })
            } else {
                None
            }
        }
        (SectionId::Packages, Some(ItemId::Package { name, arch })) => {
            let view = session.view();
            if let Some(pkg) = view
                .packages
                .iter()
                .find(|p| p.entry.name == *name && p.entry.arch == *arch)
            {
                let content = format!(
                    "Name: {}\nVersion: {}\nArch: {}\nRepo: {}\nTriage: {:?}\nInclude: {}",
                    pkg.entry.name,
                    pkg.entry.version,
                    pkg.entry.arch,
                    if pkg.entry.source_repo.is_empty() {
                        "unknown"
                    } else {
                        &pkg.entry.source_repo
                    },
                    pkg.triage.bucket(),
                    pkg.entry.include,
                );
                Some(DetailData {
                    title: item.name.clone(),
                    content,
                    content_type: DetailContentType::PlainText,
                    include: Some(pkg.entry.include),
                    position,
                })
            } else {
                None
            }
        }
        _ => {
            // Fallback plain text for other sections.
            if item.detail.is_empty() {
                Some(DetailData {
                    title: item.name.clone(),
                    content: "(no detail available)".into(),
                    content_type: DetailContentType::PlainText,
                    include: item.included,
                    position,
                })
            } else {
                Some(DetailData {
                    title: item.name.clone(),
                    content: format!("Detail: {}", item.detail),
                    content_type: DetailContentType::PlainText,
                    include: item.included,
                    position,
                })
            }
        }
    }
}

/// Group raw items by triage bucket with collapsible headers.
///
/// The canonical bucket order is Investigate, Site, Baseline.
fn build_grouped_items(items: &[RawItem], state: &TuiState, _section: SectionId) -> Vec<ListItem> {
    const BUCKET_ORDER: &[TriageGroup] = &[
        TriageGroup::Investigate,
        TriageGroup::Site,
        TriageGroup::Baseline,
    ];

    // Group items by triage group.
    let mut grouped: HashMap<TriageGroup, Vec<&RawItem>> = HashMap::new();
    for item in items {
        grouped.entry(item.2).or_default().push(item);
    }

    let mut result = Vec::new();

    for (group_idx, &bucket) in BUCKET_ORDER.iter().enumerate() {
        let group_items = match grouped.get(&bucket) {
            Some(items) if !items.is_empty() => items,
            _ => continue,
        };

        let is_collapsed = state
            .collapsed_groups
            .contains(&(state.active_section, group_idx));

        // Group header.
        result.push(ListItem::header(bucket, group_items.len(), is_collapsed));

        if !is_collapsed {
            for (idx, item) in group_items.iter().enumerate() {
                if item.5 {
                    result.push(ListItem::item_with_content(
                        item.0.clone(),
                        item.1.clone(),
                        bucket,
                        item.3,
                        idx,
                        item.4.clone(),
                    ));
                } else {
                    result.push(ListItem::item(
                        item.0.clone(),
                        item.1.clone(),
                        bucket,
                        item.3,
                        idx,
                        item.4.clone(),
                    ));
                }
            }
        }
    }

    result
}

/// Build user entries from session decisions for the UserStrategyWidget.
pub fn build_user_entries(session: &RefineSession) -> Vec<UserEntry> {
    session
        .decisions()
        .users_groups
        .iter()
        .map(|u| {
            let has_password = u
                .password_status
                .as_deref()
                .map(|s| s == "password_set")
                .unwrap_or(false);
            UserEntry {
                username: u.name.clone(),
                uid: u.uid,
                strategy: u.containerfile_strategy.clone(),
                has_password,
                password_choice: u.password_choice.clone(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_to_group_maps_all_variants() {
        assert_eq!(
            bucket_to_group(TriageBucket::Investigate),
            TriageGroup::Investigate
        );
        assert_eq!(bucket_to_group(TriageBucket::Site), TriageGroup::Site);
        assert_eq!(
            bucket_to_group(TriageBucket::Baseline),
            TriageGroup::Baseline
        );
    }

    #[test]
    fn empty_items_produce_empty_grouped() {
        let items: Vec<RawItem> = Vec::new();
        let state = TuiState::new(14);
        let result = build_grouped_items(&items, &state, SectionId::Packages);
        assert!(result.is_empty());
    }

    #[test]
    fn grouped_items_have_headers_then_items() {
        let items: Vec<RawItem> = vec![
            (
                "httpd".to_string(),
                "2.4".to_string(),
                TriageGroup::Investigate,
                Some(true),
                None,
                false,
            ),
            (
                "nginx".to_string(),
                "1.24".to_string(),
                TriageGroup::Investigate,
                Some(false),
                None,
                false,
            ),
            (
                "bash".to_string(),
                "5.2".to_string(),
                TriageGroup::Baseline,
                Some(true),
                None,
                false,
            ),
        ];
        // Baseline (group_idx 2) is collapsed by default in TuiState::new.
        let state = TuiState::new(14);
        let result = build_grouped_items(&items, &state, SectionId::Packages);

        // Investigate header + 2 items + Baseline header (collapsed, no items).
        assert_eq!(result.len(), 4);
        assert!(result[0].is_group_header);
        assert_eq!(result[0].group, TriageGroup::Investigate);
        assert_eq!(result[0].group_count, 2);
        assert!(!result[1].is_group_header);
        assert_eq!(result[1].name, "httpd");
        assert!(!result[2].is_group_header);
        assert_eq!(result[2].name, "nginx");
        assert!(result[3].is_group_header);
        assert_eq!(result[3].group, TriageGroup::Baseline);
        assert!(result[3].is_collapsed);
    }
}
