//! Every decision surface reflects the export (spec § 3.9, § 8.15).
//!
//! The invariant: a surface that shows a decision reads the state
//! `render_refine_export` renders from. Each case below asks one
//! question of one surface and the same question of the export, over
//! `GET -> op -> GET -> undo -> GET -> redo -> GET`, rendering the
//! export at every step.
//!
//! The assertions are deliberately stronger than "the two agree". Two
//! sources agree when both are silent, and they would also agree if the
//! feature were absent, so every case states the answer it expects on
//! each side (`before`, then its negation after the op) and pins at
//! least one concrete value per state.
//!
//! Two gaps were found while writing this, both older than it and both
//! in the export's file set rather than in any surface.
//!
//! 1. **Fixed.** The Containerfile's Storage block emitted
//!    `COPY config/usr/lib/tmpfiles.d/inspectah-var.conf` and the refine
//!    export never wrote that member, so a download whose snapshot has a
//!    provisioned `/var` directory failed at that line under
//!    `podman build`. Both render paths now stage it through
//!    `render::write_tmpfiles_conf`, and
//!    `export_contract_test::every_copy_source_is_in_the_archive` holds
//!    the general property: every active `COPY` names something the
//!    archive carries.
//! 2. **Open**, backlogged as
//!    `inspectah-downloaded-export-drops-every-dnf-group-install`. The
//!    `POST /api/tarball` handler passes `None` for the `RenderContext`,
//!    so the *downloaded* Containerfile carries no `dnf group install`
//!    and installs a renderable group's members individually. The
//!    session's own `export_tarball` passes
//!    `self.cached_render_context`, so the CLI and TUI are unaffected
//!    and only the web download diverges from the preview.
//!
//! The group cases below therefore compare `render_state` against which
//! packages reach the install list, and say so at the probe. They are
//! written against what the download does today; the `pin_after` of the
//! include case marks what changes when gap 2 closes.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use inspectah_core::baseline::BaselineData;
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::FindingKind;
use inspectah_core::types::config::{
    ConfigCategory, ConfigFileEntry, ConfigFileKind, ConfigSection,
};
use inspectah_core::types::kernelboot::{KernelBootSection, SysctlOverride};
use inspectah_core::types::redaction::RedactionState;
use inspectah_core::types::rpm::{
    InstalledGroup, PackageEntry, PackageState, RepoFile, RpmSection,
};
use inspectah_core::types::storage::{StorageSection, VarDirBacking, VarDirectory};
use inspectah_refine::autosave::{SessionState, compute_tarball_hash, save_session};
use inspectah_refine::session::{RefineSession, render_refine_export};
use inspectah_refine::types::{ItemId, RefinementOp, TimelineEntry};
use inspectah_web::handlers::AppState;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

// -- Fixture inputs, named so each case says what reaches its surface --

/// A kernel-module config file: `ConfigCategory::KernelModule` puts it
/// in the Kernel & Boot section's pointer rows, and `config.files` is
/// where it is decided.
const DRIFTIFY_CONF: &str = "/etc/modprobe.d/driftify.conf";
/// A tmpfiles.d fragment under `/etc`: the creator `/var/lib/appone`'s
/// verdict names, and the only thing standing between `Carried` and
/// `Provision` for that directory.
const APPONE_CONF: &str = "/etc/tmpfiles.d/appone.conf";
const APPONE_VAR_DIR: &str = "/var/lib/appone";
const APPONE_VAR_MODE: &str = "0750";
/// One of two included sysctl keys. The second keeps the synthesized
/// file in the export after this one is excluded, so the case tests the
/// key's line and not the file's existence.
const SYSCTL_KEY: &str = "net.ipv4.ip_forward";
const SYSCTL_LINE: &str = "net.ipv4.ip_forward = 1";
const OTHER_SYSCTL_KEY: &str = "vm.swappiness";
/// No `/etc/tuned/<profile>/tuned.conf` entry exists for this profile,
/// so the Containerfile takes the `active_profile` branch rather than
/// the excluded-profile-file FIXME.
const TUNED_PROFILE: &str = "throughput-performance";
const VENDOR_REPO: &str = "vendor";
const CONTAINER_GROUP: &str = "Container Management";

// -- Export paths and rendered text, hand-transcribed from the renderers --

const SYNTHESIZED_SYSCTL_PATH: &str = "sysctl/etc/sysctl.d/99-inspectah-migrated.conf";
const DRIFTIFY_EXPORT_PATH: &str = "config/etc/modprobe.d/driftify.conf";
const APPONE_EXPORT_PATH: &str = "config/etc/tmpfiles.d/appone.conf";
/// The `d` line `render_tmpfiles_conf` writes for a `Provision`
/// directory, and the fragment's own line on the host.
const APPONE_D_LINE: &str = "d /var/lib/appone 0750 root root -";
/// `var_dir_section_lines`' header and staging line. The Storage case
/// reads the Containerfile rather than the staged fragment's own text:
/// the Containerfile is where the export states what it will do with the
/// directory, and it is the half that was answering while the member was
/// missing (module note, gap 1). The member's presence is held by
/// `export_contract_test::every_copy_source_is_in_the_archive`.
const VAR_PROVISIONING_HEADER: &str = "# === /var directory provisioning ===";
const VAR_PROVISIONING_COPY: &str =
    "COPY config/usr/lib/tmpfiles.d/inspectah-var.conf /usr/lib/tmpfiles.d/inspectah-var.conf";
/// `var_directory_rows`' advisory rationale for the same directory. The
/// view and the export say the same thing because they are the same
/// sentence.
const APPONE_RATIONALE: &str =
    "created on the host by /etc/tmpfiles.d/appone.conf, which this export does not carry";
/// `var_directory_rows`' summary subtitle, both states.
const ALL_BACKED_STATE: &str = "all backed by tmpfiles.d, unit directives, or package ownership";
const ONE_UNBACKED_STATE: &str = "1 unbacked";

// -- Harness ---------------------------------------------------------------

fn state(snap: InspectionSnapshot) -> Arc<AppState> {
    Arc::new(AppState {
        session: Arc::new(Mutex::new(RefineSession::new(snap))),
    })
}

fn app(state: Arc<AppState>) -> axum::Router {
    inspectah_web::router(state, "http://localhost:8642")
}

async fn get_json(app: &axum::Router, path: &str) -> Value {
    let response = app
        .clone()
        .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK, "GET {path}");
    let body = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&body).unwrap()
}

async fn post_ok(app: &axum::Router, path: &str, body: Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(path)
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(
        status,
        StatusCode::OK,
        "POST {path} must be accepted, got {}",
        String::from_utf8_lossy(&bytes)
    );
}

/// Every regular member of a rendered export, keyed by its logical path
/// with the archive's prefix directory stripped.
struct Export {
    files: BTreeMap<String, String>,
}

impl Export {
    fn containerfile(&self) -> &str {
        self.text("Containerfile")
    }

    fn has(&self, path: &str) -> bool {
        self.files.contains_key(path)
    }

    /// The member's text, or `""` when the export did not write it.
    /// Absence and emptiness answer the same for a `contains` check,
    /// which is what every caller here asks.
    fn text(&self, path: &str) -> &str {
        self.files.get(path).map(String::as_str).unwrap_or("")
    }

    /// Package names from `RUN dnf install -y` blocks only, never from
    /// `dnf group install`.
    fn active_install_list(&self) -> Vec<String> {
        let mut names = Vec::new();
        let mut in_block = false;
        for line in self.containerfile().lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("RUN dnf install -y") {
                in_block = true;
                continue;
            }
            if !in_block {
                continue;
            }
            if trimmed.starts_with("&&") || trimmed.is_empty() {
                in_block = false;
                continue;
            }
            names.push(trimmed.trim_end_matches('\\').trim().to_string());
        }
        names
    }

    fn installs(&self, name: &str) -> bool {
        self.active_install_list().iter().any(|n| n == name)
    }
}

/// Render the session's current projection exactly as `GET /api/export`
/// does, and read the archive back.
fn export_of(state: &AppState) -> Export {
    let (projected, original_includes) = {
        let session = state.session.lock().unwrap();
        let orig: std::collections::HashMap<String, bool> = session
            .snapshot()
            .rpm
            .as_ref()
            .map(|r| {
                r.packages_added
                    .iter()
                    .map(|p| {
                        (
                            format!("{}.{}", p.name, p.arch),
                            p.disposition.is_included(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        (session.snapshot_projected(), orig)
    };

    let dir = tempfile::tempdir().unwrap();
    let tarball = dir.path().join("export.tar.gz");
    render_refine_export(
        &projected,
        &tarball,
        Some(&original_includes),
        None,
        None,
        None,
    )
    .expect("the export renders");
    read_export(&tarball)
}

fn read_export(tarball: &std::path::Path) -> Export {
    let file = std::fs::File::open(tarball).unwrap();
    let gz = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(gz);

    let stem = tarball.file_name().unwrap_or_default().to_string_lossy();
    let prefix = stem.strip_suffix(".tar.gz").unwrap_or(&stem);
    let prefix_slash = format!("{prefix}/");

    let mut files = BTreeMap::new();
    for entry in archive.entries().unwrap() {
        let mut entry = entry.unwrap();
        if entry.header().entry_type() != tar::EntryType::Regular {
            continue;
        }
        let raw = entry.path().unwrap().to_string_lossy().to_string();
        let logical = raw.strip_prefix(&prefix_slash).unwrap_or(&raw).to_string();
        let mut buf = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut buf).unwrap();
        files.insert(logical, String::from_utf8_lossy(&buf).into_owned());
    }
    Export { files }
}

// -- Surface readers -------------------------------------------------------

fn section<'a>(sections: &'a Value, id: &str) -> &'a Value {
    sections
        .as_array()
        .expect("sections is an array")
        .iter()
        .find(|s| s["id"] == id)
        .unwrap_or_else(|| panic!("section {id} is present"))
}

/// A row of the Kernel & Boot section's `customizations` subsection.
fn kernel_boot_row<'a>(sections: &'a Value, item_id: &str) -> &'a Value {
    section(sections, "kernel_boot")["subsections"]
        .as_array()
        .expect("kernel_boot has subsections")
        .iter()
        .find(|s| s["id"] == "customizations")
        .expect("customizations subsection")["items"]
        .as_array()
        .expect("items array")
        .iter()
        .find(|i| i["id"] == item_id)
        .unwrap_or_else(|| panic!("kernel_boot customizations row {item_id} is present"))
}

fn kernel_boot_include(sections: &Value, item_id: &str) -> bool {
    kernel_boot_row(sections, item_id)["decision"]["include"]
        .as_bool()
        .unwrap_or_else(|| panic!("row {item_id} carries a decision"))
}

fn storage_row<'a>(sections: &'a Value, item_id: &str) -> &'a Value {
    section(sections, "storage")["items"]
        .as_array()
        .expect("storage items")
        .iter()
        .find(|i| i["id"] == item_id)
        .unwrap_or_else(|| panic!("storage row {item_id} is present"))
}

/// A `/var` row's advisory, present exactly when the export cannot
/// report the directory as backed.
fn var_dir_has_advisory(sections: &Value, path: &str) -> bool {
    storage_row(sections, path).get("advisory").is_some()
}

fn package_included(view: &Value, name: &str, arch: &str) -> bool {
    let entry = view["packages"]
        .as_array()
        .expect("packages array")
        .iter()
        .find(|p| p["entry"]["name"] == name && p["entry"]["arch"] == arch)
        .unwrap_or_else(|| panic!("package {name}.{arch} is in the view"));
    entry["entry"]["disposition"]["include"]
        .as_bool()
        .expect("an actionable package carries include")
}

fn repo_group<'a>(view: &'a Value, section_id: &str) -> &'a Value {
    view["repo_groups"]
        .as_array()
        .expect("repo_groups array")
        .iter()
        .find(|r| r["section_id"] == section_id)
        .unwrap_or_else(|| panic!("repo group {section_id} is in the view"))
}

fn package_group<'a>(view: &'a Value, name: &str) -> &'a Value {
    view["package_groups"]
        .as_array()
        .expect("package_groups array")
        .iter()
        .find(|g| g["name"] == name)
        .unwrap_or_else(|| panic!("package group {name} is in the view"))
}

fn group_is_renderable(view: &Value, name: &str) -> bool {
    package_group(view, name)["render_state"] == "renderable"
}

/// What a group decision means for the image: both present members of
/// `Container Management` reach the install list.
///
/// Not `dnf group install`. The `POST /api/tarball` handler renders with
/// no `RenderContext`, and `render_containerfile` emits no group install
/// without one, so the mechanism half of § 8.8 is not available from the
/// download this file tests (see the module note, gap 2). Which packages
/// reach the image is, and that is what the decision is about.
fn group_members_installed(export: &Export) -> bool {
    export.installs("buildah") && export.installs("containernetworking-plugins")
}

// -- Fixtures --------------------------------------------------------------

/// Config, sysctl, tuned, and one `/var` directory whose only creator is
/// a `config.files` entry. Four of the table's cases read this one host.
fn kernel_and_storage_snapshot() -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.config = Some(ConfigSection {
        files: vec![
            ConfigFileEntry {
                path: DRIFTIFY_CONF.into(),
                kind: ConfigFileKind::Unowned,
                category: ConfigCategory::KernelModule,
                content: "options kvm_intel nested=1\n".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
            ConfigFileEntry {
                path: APPONE_CONF.into(),
                kind: ConfigFileKind::Unowned,
                category: ConfigCategory::Tmpfiles,
                content: format!("{APPONE_D_LINE}\n"),
                disposition: FindingKind::included(),
                ..Default::default()
            },
        ],
    });
    snap.kernel_boot = Some(KernelBootSection {
        sysctl_overrides: vec![
            SysctlOverride {
                key: SYSCTL_KEY.into(),
                runtime: "1".into(),
                default: "0".into(),
                source: "/etc/sysctl.d/99-custom.conf".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
            SysctlOverride {
                key: OTHER_SYSCTL_KEY.into(),
                runtime: "10".into(),
                default: "60".into(),
                source: "/etc/sysctl.d/99-custom.conf".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
        ],
        tuned_active: TUNED_PROFILE.into(),
        tuned_disposition: FindingKind::included(),
        ..Default::default()
    });
    snap.storage = Some(StorageSection {
        var_directories: vec![VarDirectory {
            path: APPONE_VAR_DIR.into(),
            backing: Some(VarDirBacking::Tmpfiles {
                conf: APPONE_CONF.into(),
                line: APPONE_D_LINE.into(),
            }),
            mode: Some(APPONE_VAR_MODE.into()),
            owner_name: Some("root".into()),
            group_name: Some("root".into()),
            ..Default::default()
        }],
        ..Default::default()
    });
    snap
}

fn empty_baseline() -> BaselineData {
    BaselineData {
        image_digest: "sha256:test".into(),
        packages: std::collections::HashMap::new(),
        extracted_at: "2026-05-17T00:00:00Z".into(),
    }
}

fn vendor_package(name: &str) -> PackageEntry {
    PackageEntry {
        name: name.into(),
        arch: "x86_64".into(),
        state: PackageState::Added,
        source_repo: VENDOR_REPO.into(),
        disposition: FindingKind::included(),
        ..Default::default()
    }
}

/// A `Verified` repo with two actionable unlocked members and no repo op
/// applied. The baseline puts both in the Site bucket so they normalize
/// to included; without it every op below would already be a no-op.
fn vendor_repo_snapshot() -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.baseline = Some(empty_baseline());
    snap.rpm = Some(RpmSection {
        packages_added: vec![vendor_package("alpha"), vendor_package("beta")],
        repo_files: vec![RepoFile {
            path: "/etc/yum.repos.d/vendor.repo".into(),
            content: "[vendor]\nname=Vendor\nbaseurl=http://example.invalid/vendor\n".into(),
            disposition: FindingKind::included(),
            ..Default::default()
        }],
        ..Default::default()
    });
    snap
}

/// The § 3.7 host example: "Container Management" has two present
/// non-leaf members, both excluded at normalization, and one declared
/// member absent from `packages_added`. `vim` is a leaf non-member, so
/// the individual install block is never empty and the member-absence
/// checks cannot hold against an unparsed list.
fn container_group_snapshot() -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.baseline = Some(empty_baseline());
    snap.rpm = Some(RpmSection {
        packages_added: vec![
            PackageEntry {
                name: "buildah".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "appstream".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
            PackageEntry {
                name: "containernetworking-plugins".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "appstream".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
            PackageEntry {
                name: "vim".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "appstream".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
        ],
        leaf_packages: Some(vec!["vim.x86_64".into()]),
        auto_packages: Some(vec![
            "buildah.x86_64".into(),
            "containernetworking-plugins.x86_64".into(),
        ]),
        baseline_suppressed: Some(vec!["podman.x86_64".into()]),
        installed_groups: Some(vec![InstalledGroup {
            name: CONTAINER_GROUP.into(),
            members: vec![
                "buildah".into(),
                "containernetworking-plugins".into(),
                "podman".into(),
            ],
            ..Default::default()
        }]),
        ..Default::default()
    });
    snap
}

// -- Op bodies -------------------------------------------------------------

fn op_body(op: RefinementOp) -> Value {
    serde_json::to_value(TimelineEntry::Op(op)).expect("a timeline entry serializes")
}

fn set_include(item_id: ItemId, include: bool) -> Value {
    op_body(RefinementOp::SetInclude { item_id, include })
}

fn exclude_config(path: &str) -> Value {
    set_include(
        ItemId::Config {
            path: path.to_string(),
        },
        false,
    )
}

// -- The table -------------------------------------------------------------

type Pin = fn(&Value, &Export);

struct Case {
    /// What this case asks, and of which surface.
    name: &'static str,
    endpoint: &'static str,
    snapshot: fn() -> InspectionSnapshot,
    /// Ops applied before the first GET, to reach the state the case
    /// starts from. Never the op under test.
    setup: fn() -> Vec<Value>,
    /// The one op the undo and redo legs move.
    op: fn() -> Value,
    /// What the surface answers about the entity under test.
    surface: fn(&Value) -> bool,
    /// What the export renders for the same entity.
    export: fn(&Export) -> bool,
    /// The answer before the op. The op must move it: every post-op
    /// assertion expects `!before` on both sides, so a case whose op
    /// does not apply fails rather than comparing two unchanged values.
    before: bool,
    /// Concrete values the before-state must show. Equality between two
    /// sources is satisfied by two silences; these are not.
    pin_before: Pin,
    /// The same for the after-state, and different values.
    pin_after: Pin,
}

fn cases() -> Vec<Case> {
    vec![
        Case {
            name: "Kernel & Boot mirror of a Config exclusion \
                   (SetInclude(Config(/etc/modprobe.d/driftify.conf), false))",
            endpoint: "/api/snapshot/sections",
            snapshot: kernel_and_storage_snapshot,
            setup: Vec::new,
            op: || exclude_config(DRIFTIFY_CONF),
            surface: |s| kernel_boot_include(s, DRIFTIFY_CONF),
            export: |e| e.has(DRIFTIFY_EXPORT_PATH),
            before: true,
            pin_before: |s, e| {
                assert_eq!(
                    kernel_boot_row(s, DRIFTIFY_CONF)["decision"]["item_id"],
                    json!({"kind": "Config", "key": {"path": DRIFTIFY_CONF}}),
                    "the row names the entity an /api/op payload would name"
                );
                assert_eq!(
                    e.text(DRIFTIFY_EXPORT_PATH),
                    "options kvm_intel nested=1\n",
                    "the included file reaches the image with its content"
                );
            },
            pin_after: |s, e| {
                assert_eq!(
                    kernel_boot_row(s, DRIFTIFY_CONF)["decision"]["include"],
                    json!(false),
                    "the mirror row reports the exclusion"
                );
                assert_eq!(
                    e.text(DRIFTIFY_EXPORT_PATH),
                    "",
                    "the excluded file is absent from the tarball"
                );
            },
        },
        Case {
            name: "sysctl row and the synthesized drop-in \
                   (SetInclude(Sysctl(net.ipv4.ip_forward), false))",
            endpoint: "/api/snapshot/sections",
            snapshot: kernel_and_storage_snapshot,
            setup: Vec::new,
            op: || {
                set_include(
                    ItemId::Sysctl {
                        key: SYSCTL_KEY.to_string(),
                    },
                    false,
                )
            },
            surface: |s| kernel_boot_include(s, SYSCTL_KEY),
            export: |e| e.text(SYNTHESIZED_SYSCTL_PATH).contains(SYSCTL_LINE),
            before: true,
            pin_before: |s, e| {
                assert_eq!(
                    kernel_boot_row(s, SYSCTL_KEY)["subtitle"],
                    json!("\"1\" (default: \"0\")"),
                    "the row shows the runtime value the drop-in writes"
                );
                assert!(
                    e.text(SYNTHESIZED_SYSCTL_PATH).contains(SYSCTL_LINE),
                    "the included key is written as its own line"
                );
            },
            pin_after: |s, e| {
                assert_eq!(
                    kernel_boot_row(s, SYSCTL_KEY)["decision"]["include"],
                    json!(false)
                );
                let conf = e.text(SYNTHESIZED_SYSCTL_PATH);
                assert!(
                    conf.contains("vm.swappiness = 10"),
                    "the other included key keeps the file alive, so the \
                     assertion below is about the line and not the file: {conf}"
                );
                assert!(
                    !conf.contains(SYSCTL_KEY),
                    "the excluded key leaves the drop-in: {conf}"
                );
            },
        },
        Case {
            name: "tuned row and the active_profile line \
                   (SetInclude(TunedSelection(throughput-performance), false))",
            endpoint: "/api/snapshot/sections",
            snapshot: kernel_and_storage_snapshot,
            setup: Vec::new,
            op: || {
                set_include(
                    ItemId::TunedSelection {
                        profile: TUNED_PROFILE.to_string(),
                    },
                    false,
                )
            },
            surface: |s| kernel_boot_include(s, "tuned_active"),
            export: |e| {
                e.containerfile()
                    .contains("RUN echo \"throughput-performance\" > /etc/tuned/active_profile")
            },
            before: true,
            pin_before: |s, e| {
                assert_eq!(
                    kernel_boot_row(s, "tuned_active")["subtitle"],
                    json!(TUNED_PROFILE),
                    "the row names the profile the Containerfile activates"
                );
                assert!(
                    e.containerfile()
                        .contains("RUN systemctl enable tuned.service"),
                    "the whole tuned block renders, not only the echo line"
                );
            },
            pin_after: |s, e| {
                assert_eq!(
                    kernel_boot_row(s, "tuned_active")["decision"]["include"],
                    json!(false)
                );
                assert!(
                    !e.containerfile().contains("active_profile"),
                    "an excluded profile leaves no activation behind"
                );
            },
        },
        Case {
            name: "Storage /var advisory and the export's provisioning block \
                   (SetInclude(Config(/etc/tmpfiles.d/appone.conf), false))",
            endpoint: "/api/snapshot/sections",
            snapshot: kernel_and_storage_snapshot,
            setup: Vec::new,
            op: || exclude_config(APPONE_CONF),
            surface: |s| var_dir_has_advisory(s, APPONE_VAR_DIR),
            export: |e| e.containerfile().contains(VAR_PROVISIONING_COPY),
            before: false,
            pin_before: |s, e| {
                assert_eq!(
                    storage_row(s, "var_directories")["subtitle"],
                    json!(ALL_BACKED_STATE),
                    "the carried directory is summarized as backed"
                );
                assert_eq!(
                    storage_row(s, APPONE_VAR_DIR)["detail"],
                    json!("0750 root:root"),
                    "a carried directory departing from the default mode \
                     still earns an inventory row, so the advisory check \
                     above is about the advisory and not about the row"
                );
                assert!(
                    e.has(APPONE_EXPORT_PATH),
                    "the creator the view calls carried is in the tarball"
                );
                assert!(
                    !e.containerfile().contains(VAR_PROVISIONING_HEADER),
                    "and the export provisions nothing itself"
                );
            },
            pin_after: |s, e| {
                let row = storage_row(s, APPONE_VAR_DIR);
                assert_eq!(row["advisory"]["advisory_type"], json!("unbacked_var_dir"));
                assert_eq!(
                    row["advisory"]["rationale"],
                    json!(APPONE_RATIONALE),
                    "the rationale names the fragment the export dropped"
                );
                assert_eq!(
                    storage_row(s, "var_directories")["subtitle"],
                    json!(ONE_UNBACKED_STATE),
                    "the summary counts it"
                );
                assert!(
                    !e.has(APPONE_EXPORT_PATH),
                    "the fragment the rationale names is gone from the tarball"
                );
                assert!(
                    e.containerfile().contains(VAR_PROVISIONING_HEADER),
                    "so the export takes the directory on itself: {}",
                    e.containerfile()
                );
            },
        },
        Case {
            name: "repo exclusion in /api/view (SetInclude(Repo(vendor), false))",
            endpoint: "/api/view",
            snapshot: vendor_repo_snapshot,
            setup: Vec::new,
            op: || {
                set_include(
                    ItemId::Repo {
                        path: VENDOR_REPO.to_string(),
                    },
                    false,
                )
            },
            surface: |v| package_included(v, "alpha", "x86_64"),
            export: |e| e.installs("alpha"),
            before: true,
            pin_before: |v, e| {
                let repo = repo_group(v, VENDOR_REPO);
                assert_eq!(repo["effective_member_count"], json!(2));
                assert_eq!(repo["effective_included"], json!(2));
                assert_eq!(repo["enabled"], json!(true));
                assert_eq!(
                    e.active_install_list(),
                    vec!["alpha".to_string(), "beta".to_string()],
                    "both members are installed individually"
                );
            },
            pin_after: |v, e| {
                let repo = repo_group(v, VENDOR_REPO);
                assert_eq!(repo["effective_member_count"], json!(2));
                assert_eq!(
                    repo["effective_included"],
                    json!(0),
                    "the exclusion cascades to both members"
                );
                assert_eq!(repo["enabled"], json!(false));
                assert!(
                    !package_included(v, "beta", "x86_64"),
                    "the view reports the cascade on the other member too"
                );
                assert!(
                    e.active_install_list().is_empty(),
                    "and the export installs neither: {:?}",
                    e.active_install_list()
                );
            },
        },
        Case {
            name: "repo re-include after an exclusion \
                   (SetInclude(Repo(vendor), true) over an excluded repo)",
            endpoint: "/api/view",
            snapshot: vendor_repo_snapshot,
            setup: || {
                vec![set_include(
                    ItemId::Repo {
                        path: VENDOR_REPO.to_string(),
                    },
                    false,
                )]
            },
            op: || {
                set_include(
                    ItemId::Repo {
                        path: VENDOR_REPO.to_string(),
                    },
                    true,
                )
            },
            surface: |v| package_included(v, "alpha", "x86_64"),
            export: |e| e.installs("alpha"),
            before: false,
            pin_before: |v, e| {
                assert_eq!(repo_group(v, VENDOR_REPO)["enabled"], json!(false));
                assert!(e.active_install_list().is_empty());
            },
            pin_after: |v, e| {
                let repo = repo_group(v, VENDOR_REPO);
                assert_eq!(repo["effective_included"], json!(2));
                assert_eq!(repo["enabled"], json!(true));
                assert_eq!(
                    e.active_install_list(),
                    vec!["alpha".to_string(), "beta".to_string()]
                );
            },
        },
        Case {
            name: "group include in /api/view \
                   (SetInclude(Group(Container Management), true))",
            endpoint: "/api/view",
            snapshot: container_group_snapshot,
            setup: Vec::new,
            op: || {
                set_include(
                    ItemId::Group {
                        name: CONTAINER_GROUP.to_string(),
                    },
                    true,
                )
            },
            surface: |v| group_is_renderable(v, CONTAINER_GROUP),
            export: group_members_installed,
            before: false,
            pin_before: |v, e| {
                let group = package_group(v, CONTAINER_GROUP);
                // Both present members normalize to excluded and no
                // group op has run, so this is § 3.7's "member excluded
                // without a group op", not an excluded group.
                assert_eq!(group["render_state"], json!("degraded"));
                assert_eq!(group["degradation_reason"], json!("member_excluded"));
                assert_eq!(
                    group["effective_member_count"],
                    json!(2),
                    "podman is declared but absent, so it is not effective"
                );
                assert_eq!(group["effective_included"], json!(0));
                assert_eq!(
                    e.active_install_list(),
                    vec!["vim".to_string()],
                    "the leaf non-member keeps the install block non-empty, \
                     so the member checks are not read off an unparsed list"
                );
            },
            pin_after: |v, e| {
                let group = package_group(v, CONTAINER_GROUP);
                assert_eq!(group["render_state"], json!("renderable"));
                assert_eq!(group["effective_included"], json!(2));
                // EXPECTED TO GO RED when the export handler is given a
                // render context (backlog:
                // inspectah-downloaded-export-drops-every-dnf-group-install).
                // Today the download has none, so a renderable group's
                // members are installed individually. Once it does, they
                // move into a `dnf group install` and this list becomes
                // `["vim"]`. That is the fix landing, not a regression:
                // change the expected list and add the group install line.
                assert_eq!(
                    e.active_install_list(),
                    vec![
                        "buildah".to_string(),
                        "containernetworking-plugins".to_string(),
                        "vim".to_string(),
                    ],
                    "the group's two present members join the install list, \
                     and podman — declared but baseline-suppressed — does not"
                );
            },
        },
        Case {
            name: "group exclusion in /api/view \
                   (SetInclude(Group(Container Management), false) over an included group)",
            endpoint: "/api/view",
            snapshot: container_group_snapshot,
            setup: || {
                vec![set_include(
                    ItemId::Group {
                        name: CONTAINER_GROUP.to_string(),
                    },
                    true,
                )]
            },
            op: || {
                set_include(
                    ItemId::Group {
                        name: CONTAINER_GROUP.to_string(),
                    },
                    false,
                )
            },
            surface: |v| group_is_renderable(v, CONTAINER_GROUP),
            export: group_members_installed,
            before: true,
            pin_before: |v, e| {
                assert_eq!(
                    package_group(v, CONTAINER_GROUP)["render_state"],
                    json!("renderable")
                );
                assert_eq!(
                    e.active_install_list(),
                    vec![
                        "buildah".to_string(),
                        "containernetworking-plugins".to_string(),
                        "vim".to_string(),
                    ]
                );
            },
            pin_after: |v, e| {
                assert_eq!(
                    package_group(v, CONTAINER_GROUP)["render_state"],
                    json!("excluded")
                );
                assert_eq!(
                    e.active_install_list(),
                    vec!["vim".to_string()],
                    "an excluded group installs neither as a group nor \
                     member by member"
                );
            },
        },
    ]
}

#[tokio::test]
async fn every_decision_surface_reflects_the_export() {
    for case in cases() {
        let state = state((case.snapshot)());
        let app = app(state.clone());

        for op in (case.setup)() {
            post_ok(&app, "/api/op", op).await;
        }

        let steps: [(&str, bool, Pin); 4] = [
            ("before the op", case.before, case.pin_before),
            ("after the op", !case.before, case.pin_after),
            ("after undo", case.before, case.pin_before),
            ("after redo", !case.before, case.pin_after),
        ];

        for (i, (step, expected, pin)) in steps.into_iter().enumerate() {
            match i {
                0 => {}
                1 => post_ok(&app, "/api/op", (case.op)()).await,
                2 => post_ok(&app, "/api/undo", json!({})).await,
                _ => post_ok(&app, "/api/redo", json!({})).await,
            }

            let surface = get_json(&app, case.endpoint).await;
            let export = export_of(&state);

            assert_eq!(
                (case.surface)(&surface),
                expected,
                "{}: the surface {step} must report {expected}",
                case.name
            );
            assert_eq!(
                (case.export)(&export),
                expected,
                "{}: the export {step} must render {expected}",
                case.name
            );
            pin(&surface, &export);
        }
    }
}

// -- Reset and resume ------------------------------------------------------
//
// These two do not fit the table: neither moves a single op, and both
// compare a whole surface against a separately built one rather than
// asking one question of it.

/// Everything a surface comparison needs: the sections document, the
/// view's decision-bearing arrays, and the rendered export.
async fn surfaces(
    app: &axum::Router,
    state: &AppState,
) -> (Value, Value, BTreeMap<String, String>) {
    let sections = get_json(app, "/api/snapshot/sections").await;
    let view = get_json(app, "/api/view").await;
    (sections, view, export_of(state).files)
}

#[tokio::test]
async fn reset_returns_the_surfaces_of_a_fresh_session() {
    let live = state(kernel_and_storage_snapshot());
    let live_app = app(live.clone());

    // The baseline is read from a second server on the same snapshot, so
    // "equal after reset" is not satisfied by a surface that never moved.
    let fresh = state(kernel_and_storage_snapshot());
    let fresh_app = app(fresh.clone());
    let (base_sections, base_view, base_files) = surfaces(&fresh_app, &fresh).await;

    post_ok(&live_app, "/api/op", exclude_config(DRIFTIFY_CONF)).await;
    post_ok(&live_app, "/api/op", exclude_config(APPONE_CONF)).await;

    // Read the surfaces while the ops stand: this is what fills any
    // projection cache, and what reset has to displace.
    let (dirty_sections, _, dirty_files) = surfaces(&live_app, &live).await;
    assert_ne!(
        dirty_sections, base_sections,
        "the ops must move the sections, or the comparison after reset \
         is between two states that were never different"
    );
    assert_ne!(dirty_files, base_files, "and must move the export");

    post_ok(&live_app, "/api/reset", json!({})).await;

    let (sections, view, files) = surfaces(&live_app, &live).await;
    assert_eq!(
        sections, base_sections,
        "reset returns the sections of a fresh session"
    );
    assert_eq!(view["config_files"], base_view["config_files"]);
    assert_eq!(files, base_files, "and a fresh export");
}

const AUTOSAVE_SCHEMA_VERSION: u32 = 3;

fn write_scan_tarball(path: &std::path::Path, snap: &InspectionSnapshot) {
    use flate2::Compression;
    use flate2::write::GzEncoder;

    let json = serde_json::to_string_pretty(snap).unwrap();
    let gz = GzEncoder::new(std::fs::File::create(path).unwrap(), Compression::default());
    let mut tar = tar::Builder::new(gz);
    let bytes = json.as_bytes();
    let mut header = tar::Header::new_gnu();
    header.set_path("inspection-snapshot.json").unwrap();
    header.set_size(bytes.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    tar.append(&header, bytes).unwrap();
    tar.finish().unwrap();
}

#[tokio::test]
async fn a_resumed_session_serves_the_surfaces_of_its_replayed_timeline() {
    let dir = tempfile::tempdir().unwrap();
    let tarball = dir.path().join("scan.tar.gz");
    let mut snap = kernel_and_storage_snapshot();
    // `from_tarball` rejects a snapshot with no provenance.
    snap.redaction_state = Some(RedactionState::FullyRedacted {
        redacted_by: "inspectah-test".into(),
        config_hash: "abc".into(),
    });
    write_scan_tarball(&tarball, &snap);

    let autosaved = TimelineEntry::Op(RefinementOp::SetInclude {
        item_id: ItemId::Config {
            path: APPONE_CONF.to_string(),
        },
        include: false,
    });
    save_session(
        &SessionState {
            schema_version: AUTOSAVE_SCHEMA_VERSION,
            tarball_path: tarball.clone(),
            tarball_hash: compute_tarball_hash(&tarball).unwrap(),
            timeline: vec![autosaved],
            cursor: 1,
            saved_at: "2026-10-30T00:00:00Z".into(),
        },
        &tarball,
    )
    .unwrap();

    let resumed = Arc::new(AppState {
        session: Arc::new(Mutex::new(
            RefineSession::resume_from(&tarball)
                .expect("resume succeeds")
                .expect("a session file exists"),
        )),
    });
    let resumed_app = app(resumed.clone());

    // Read the resumed surfaces first, then move the session: the second
    // comparison is the one a stale projection fails.
    let (resumed_sections, _, _) = surfaces(&resumed_app, &resumed).await;
    post_ok(&resumed_app, "/api/op", exclude_config(DRIFTIFY_CONF)).await;
    let (sections, view, files) = surfaces(&resumed_app, &resumed).await;
    assert_ne!(
        sections, resumed_sections,
        "the second op must move the sections"
    );

    // A fresh session driven through the same two ops over the API.
    let fresh = state(snap);
    let fresh_app = app(fresh.clone());
    post_ok(&fresh_app, "/api/op", exclude_config(APPONE_CONF)).await;
    post_ok(&fresh_app, "/api/op", exclude_config(DRIFTIFY_CONF)).await;
    let (fresh_sections, fresh_view, fresh_files) = surfaces(&fresh_app, &fresh).await;

    assert_eq!(
        sections, fresh_sections,
        "a resumed timeline serves the sections its ops produce"
    );
    assert_eq!(view["config_files"], fresh_view["config_files"]);
    assert_eq!(files, fresh_files, "and renders the same export");
    assert!(
        var_dir_has_advisory(&sections, APPONE_VAR_DIR),
        "the replayed exclusion reaches the /var advisory, so the equality \
         above is between two surfaces that both moved"
    );
}
