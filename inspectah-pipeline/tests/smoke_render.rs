//! Renderer smoke tests — verify each inspector's data reaches the correct
//! artifact consumers.
//!
//! These are SMOKE tests: they prove data REACHES the renderer, not that every
//! field is perfectly formatted. Each test builds a snapshot manually (no
//! inspector execution), calls the relevant renderer, and checks for key
//! markers in the output.

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::kernelboot::{ConfigSnippet, KernelBootSection, SysctlOverride};
// redaction types used indirectly via the redaction engine in test 8
use inspectah_core::types::services::{ServiceSection, ServiceStateChange, SystemdDropIn};
use inspectah_core::types::storage::{CredentialRef, FstabEntry, StorageSection};
use inspectah_pipeline::render::{audit, configtree, containerfile, kickstart, report, secrets};
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Helpers: snapshot builders
// ---------------------------------------------------------------------------

fn snapshot_with_services() -> InspectionSnapshot {
    use inspectah_core::types::services::{PresetDefault, ServiceUnitState};
    let mut snap = InspectionSnapshot::new();
    snap.services = Some(ServiceSection {
        state_changes: vec![
            ServiceStateChange {
                unit: "httpd.service".into(),
                current_state: ServiceUnitState::Enabled,
                default_state: Some(PresetDefault::Disable),
                include: true,
                owning_package: None,
                fleet: None,
                attention_reason: None,
            },
            ServiceStateChange {
                unit: "cups.service".into(),
                current_state: ServiceUnitState::Disabled,
                default_state: Some(PresetDefault::Enable),
                include: true,
                owning_package: None,
                fleet: None,
                attention_reason: None,
            },
        ],
        enabled_units: vec!["httpd.service".into()],
        disabled_units: vec!["cups.service".into()],
        drop_ins: vec![SystemdDropIn {
            unit: "httpd.service".into(),
            path: "etc/systemd/system/httpd.service.d/override.conf".into(),
            content: "[Service]\nLimitNOFILE=65535\n".into(),
            include: true,
            ..Default::default()
        }],
        preset_matched_units: Vec::new(),
    });
    snap
}

fn snapshot_with_storage() -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.storage = Some(StorageSection {
        fstab_entries: vec![
            FstabEntry {
                device: "/dev/sda1".into(),
                mount_point: "/boot".into(),
                fstype: "xfs".into(),
                options: "defaults".into(),
                ..Default::default()
            },
            FstabEntry {
                device: "//server/share".into(),
                mount_point: "/mnt/cifs".into(),
                fstype: "cifs".into(),
                options: "credentials=/etc/cifs-creds,uid=1000".into(),
                ..Default::default()
            },
        ],
        credential_refs: vec![CredentialRef {
            mount_point: "/mnt/backup".into(),
            credential_path: "/etc/backup-creds".into(),
            source: "fstab".into(),
        }],
        ..Default::default()
    });
    snap
}

fn snapshot_with_kernelboot() -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.kernel_boot = Some(KernelBootSection {
        cmdline: "quiet crashkernel=auto nosmt=force".into(),
        sysctl_overrides: vec![SysctlOverride {
            key: "net.ipv4.ip_forward".into(),
            runtime: "1".into(),
            default: "0".into(),
            source: "/etc/sysctl.d/99-custom.conf".into(),
            include: true,
        }],
        modules_load_d: vec![ConfigSnippet {
            path: "etc/modules-load.d/br_netfilter.conf".into(),
            content: "br_netfilter\n".into(),
        }],
        modprobe_d: vec![ConfigSnippet {
            path: "etc/modprobe.d/blacklist-nouveau.conf".into(),
            content: "blacklist nouveau\n".into(),
        }],
        dracut_conf: vec![ConfigSnippet {
            path: "etc/dracut.conf.d/lvm.conf".into(),
            content: "add_dracutmodules+=\" lvm \"\n".into(),
        }],
        ..Default::default()
    });
    snap
}

fn snapshot_all_sections() -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.services = snapshot_with_services().services;
    snap.storage = snapshot_with_storage().storage;
    snap.kernel_boot = snapshot_with_kernelboot().kernel_boot;
    snap
}

// ---------------------------------------------------------------------------
// Test 1: services_in_containerfile
// Services section produces systemctl enable/disable commands in Containerfile.
// ---------------------------------------------------------------------------

#[test]
fn services_in_containerfile() {
    let snap = snapshot_with_services();
    let output = containerfile::render_containerfile(&snap, None);

    assert!(
        output.contains("systemctl enable httpd.service"),
        "Containerfile must contain systemctl enable for httpd.service"
    );
    assert!(
        output.contains("systemctl disable cups.service"),
        "Containerfile must contain systemctl disable for cups.service"
    );
    assert!(
        output.contains("Service Enablement"),
        "Containerfile must contain the Service Enablement section heading"
    );
}

// ---------------------------------------------------------------------------
// Test 2: services_dropins_in_config_tree
// Drop-in files from services section materialize in the config tree output.
// ---------------------------------------------------------------------------

#[test]
fn services_dropins_in_config_tree() {
    let snap = snapshot_with_services();
    let dir = TempDir::new().unwrap();

    configtree::write_config_tree(&snap, dir.path()).unwrap();

    // Drop-in must appear in config/ tree
    let dropin_path = dir
        .path()
        .join("config/etc/systemd/system/httpd.service.d/override.conf");
    assert!(
        dropin_path.exists(),
        "drop-in must be materialized in config/ tree at {}",
        dropin_path.display()
    );

    let content = std::fs::read_to_string(&dropin_path).unwrap();
    assert!(
        content.contains("LimitNOFILE=65535"),
        "drop-in content must match the snapshot data"
    );

    // Drop-in must also appear in drop-ins/ mirror
    let mirror_path = dir
        .path()
        .join("drop-ins/etc/systemd/system/httpd.service.d/override.conf");
    assert!(
        mirror_path.exists(),
        "drop-in must be mirrored in drop-ins/ directory"
    );
}

// ---------------------------------------------------------------------------
// Test 3: storage_in_kickstart
// Storage fstab entries with NFS/CIFS produce directives in kickstart.
// ---------------------------------------------------------------------------

#[test]
fn storage_in_kickstart() {
    let snap = snapshot_with_storage();
    let ks = kickstart::render_kickstart(&snap);

    assert!(
        ks.contains("CIFS") || ks.contains("cifs"),
        "kickstart must mention CIFS remote mount from fstab"
    );
    assert!(
        ks.contains("//server/share"),
        "kickstart must reference the CIFS device"
    );
    assert!(
        ks.contains("/mnt/cifs"),
        "kickstart must reference the CIFS mount point"
    );
}

// ---------------------------------------------------------------------------
// Test 4: storage_not_in_containerfile
// Storage section does NOT inject content into Containerfile — storage is
// handled via kickstart, not baked into the image.
// ---------------------------------------------------------------------------

#[test]
fn storage_not_in_containerfile() {
    let snap = snapshot_with_storage();
    let output = containerfile::render_containerfile(&snap, None);

    // Storage devices and mount points must NOT appear in the Containerfile
    assert!(
        !output.contains("/dev/sda1"),
        "Containerfile must not contain storage device /dev/sda1"
    );
    assert!(
        !output.contains("//server/share"),
        "Containerfile must not contain storage device //server/share"
    );
    assert!(
        !output.contains("/mnt/cifs"),
        "Containerfile must not contain storage mount point"
    );
    // No storage-related section heading
    assert!(
        !output.contains("Storage"),
        "Containerfile must not contain a Storage section heading"
    );
}

// ---------------------------------------------------------------------------
// Test 5: kernelboot_sysctl_in_containerfile
// Kernelboot sysctl overrides produce comments referencing sysctl in
// the Containerfile.
// ---------------------------------------------------------------------------

#[test]
fn kernelboot_sysctl_in_containerfile() {
    let snap = snapshot_with_kernelboot();
    let output = containerfile::render_containerfile(&snap, None);

    assert!(
        output.contains("Kernel and Boot Configuration") || output.contains("Kernel Arguments"),
        "Containerfile must contain a kernel/boot section heading"
    );
    // sysctl overrides produce a comment reference
    assert!(
        output.contains("sysctl"),
        "Containerfile must reference sysctl overrides"
    );
}

// ---------------------------------------------------------------------------
// Test 6: kernelboot_configs_in_config_tree
// Kernelboot snippets (modules-load.d, modprobe.d, dracut.conf.d) materialize
// in the config tree.
// ---------------------------------------------------------------------------

#[test]
fn kernelboot_configs_in_config_tree() {
    let snap = snapshot_with_kernelboot();
    let dir = TempDir::new().unwrap();

    configtree::write_config_tree(&snap, dir.path()).unwrap();

    // modules-load.d
    let modules_path = dir
        .path()
        .join("config/etc/modules-load.d/br_netfilter.conf");
    assert!(
        modules_path.exists(),
        "modules-load.d snippet must be materialized in config tree"
    );
    let content = std::fs::read_to_string(&modules_path).unwrap();
    assert!(
        content.contains("br_netfilter"),
        "modules-load.d content must match snapshot data"
    );

    // modprobe.d
    let modprobe_path = dir
        .path()
        .join("config/etc/modprobe.d/blacklist-nouveau.conf");
    assert!(
        modprobe_path.exists(),
        "modprobe.d snippet must be materialized in config tree"
    );
    let content = std::fs::read_to_string(&modprobe_path).unwrap();
    assert!(
        content.contains("blacklist nouveau"),
        "modprobe.d content must match snapshot data"
    );

    // dracut.conf.d
    let dracut_path = dir.path().join("config/etc/dracut.conf.d/lvm.conf");
    assert!(
        dracut_path.exists(),
        "dracut.conf.d snippet must be materialized in config tree"
    );

    // kargs.d (from cmdline with nosmt=force, an operator karg)
    let kargs_path = dir
        .path()
        .join("config/usr/lib/bootc/kargs.d/inspectah-migrated.toml");
    assert!(
        kargs_path.exists(),
        "kargs.d toml must be materialized for operator kernel arguments"
    );
    let kargs_content = std::fs::read_to_string(&kargs_path).unwrap();
    assert!(
        kargs_content.contains("nosmt=force"),
        "kargs.d toml must contain the operator kernel argument"
    );
}

// ---------------------------------------------------------------------------
// Test 7: all_sections_in_audit_report
// Audit report includes headings/content for all three new inspector surfaces.
// ---------------------------------------------------------------------------

#[test]
fn all_sections_in_audit_report() {
    let snap = snapshot_all_sections();
    let md = audit::render_audit(&snap);

    // Services state changes appear
    assert!(
        md.contains("Service State Changes"),
        "audit report must contain Service State Changes heading"
    );
    assert!(
        md.contains("httpd.service"),
        "audit report must list the httpd.service state change"
    );

    // Storage section appears with fstab and credential data
    assert!(
        md.contains("## Storage"),
        "audit report must contain Storage heading"
    );
    assert!(
        md.contains("Fstab Entries"),
        "audit report must contain Fstab Entries sub-heading"
    );
    assert!(
        md.contains("/dev/sda1"),
        "audit report must list fstab device /dev/sda1"
    );
    assert!(
        md.contains("//server/share"),
        "audit report must list CIFS device"
    );
    assert!(
        md.contains("Credential References"),
        "audit report must contain Credential References sub-heading"
    );
    assert!(
        md.contains("/etc/backup-creds"),
        "audit report must list credential path"
    );

    // Kernel & Boot section appears with sysctl, modules, dracut
    assert!(
        md.contains("## Kernel & Boot"),
        "audit report must contain Kernel & Boot heading"
    );
    assert!(
        md.contains("Sysctl Overrides"),
        "audit report must contain Sysctl Overrides sub-heading"
    );
    assert!(
        md.contains("net.ipv4.ip_forward"),
        "audit report must list the sysctl override key"
    );
    assert!(
        md.contains("Loaded Module Configs"),
        "audit report must contain Loaded Module Configs sub-heading"
    );
    assert!(
        md.contains("br_netfilter"),
        "audit report must list modules-load.d snippet"
    );
    assert!(
        md.contains("Dracut Configs"),
        "audit report must contain Dracut Configs sub-heading"
    );
    assert!(
        md.contains("Kernel Command Line"),
        "audit report must contain Kernel Command Line sub-heading"
    );

    // Verify the report renders successfully with all sections populated
    assert!(
        md.contains("# Audit Report"),
        "audit report must contain the top-level heading"
    );
}

// ---------------------------------------------------------------------------
// Test 7b: all_sections_in_html_report
// HTML report includes storage and kernelboot sections.
// ---------------------------------------------------------------------------

#[test]
fn all_sections_in_html_report() {
    let snap = snapshot_all_sections();
    let context = inspectah_core::traits::renderer::RenderContext { target: None };
    let html = report::render_report(&snap, &context);

    // Storage section appears
    assert!(
        html.contains("<h2>Storage</h2>"),
        "HTML report must contain Storage heading"
    );
    assert!(
        html.contains("/dev/sda1"),
        "HTML report must list fstab device /dev/sda1"
    );
    assert!(
        html.contains("//server/share"),
        "HTML report must list CIFS device"
    );

    // Kernel & Boot section appears
    assert!(
        html.contains("Kernel &amp; Boot"),
        "HTML report must contain Kernel & Boot heading"
    );
    assert!(
        html.contains("Sysctl Overrides"),
        "HTML report must contain Sysctl Overrides sub-heading"
    );
    assert!(
        html.contains("net.ipv4.ip_forward"),
        "HTML report must list sysctl override key"
    );
    assert!(
        html.contains("br_netfilter"),
        "HTML report must list modules-load.d config"
    );

    // Summary cards include storage and kernelboot counts
    assert!(
        html.contains("Storage Entries"),
        "HTML report must have Storage Entries summary card"
    );
    assert!(
        html.contains("Kernel/Boot Items"),
        "HTML report must have Kernel/Boot Items summary card"
    );
}

// ---------------------------------------------------------------------------
// Test 8: credential_refs_in_secrets_review
// Storage credential refs and services drop-in secrets produce findings
// in the secrets review after redaction.
// ---------------------------------------------------------------------------

#[test]
fn credential_refs_in_secrets_review() {
    let mut snap = snapshot_all_sections();

    // Plant secrets that the redaction engine will detect:
    // 1. Drop-in with a password environment variable
    snap.services = Some(ServiceSection {
        drop_ins: vec![SystemdDropIn {
            unit: "myapp.service".into(),
            path: "etc/systemd/system/myapp.service.d/override.conf".into(),
            content: "[Service]\nEnvironment=DB_PASSWORD=supersecret\n".into(),
            include: true,
            ..Default::default()
        }],
        ..Default::default()
    });

    // 2. Storage with credential mount option
    snap.storage = Some(StorageSection {
        fstab_entries: vec![FstabEntry {
            device: "//server/share".into(),
            mount_point: "/mnt/cifs".into(),
            fstype: "cifs".into(),
            options: "credentials=/etc/cifs-creds,uid=1000".into(),
            ..Default::default()
        }],
        credential_refs: vec![CredentialRef {
            mount_point: "/mnt/backup".into(),
            credential_path: "/etc/backup-creds".into(),
            source: "fstab".into(),
        }],
        ..Default::default()
    });

    // Run redaction to populate snap.redactions
    inspectah_pipeline::redaction::engine::redact(
        &mut snap,
        &inspectah_pipeline::redaction::engine::RedactOptions::default(),
    );

    // Now render the secrets review
    let md = secrets::render_secrets_review(&snap);

    assert!(
        md.contains("# Secrets Review"),
        "secrets review must contain top-level heading"
    );
    // Must NOT say "No redactions recorded" — we planted secrets
    assert!(
        !md.contains("No redactions recorded"),
        "secrets review must report findings, not 'No redactions recorded'"
    );
    // Check that findings reference our planted sources
    assert!(
        md.contains("myapp.service") || md.contains("DB_PASSWORD"),
        "secrets review must reference the drop-in secret finding"
    );
}

// ---------------------------------------------------------------------------
// Test 9: render_all produces consistent artifacts with all sections
// Integration check: render_all with all three sections populates all 8
// artifacts without errors.
// ---------------------------------------------------------------------------

#[test]
fn render_all_with_all_sections() {
    let snap = snapshot_all_sections();
    let context = inspectah_core::traits::renderer::RenderContext { target: None };
    let dir = TempDir::new().unwrap();

    inspectah_pipeline::render::render_all(&snap, &context, dir.path()).unwrap();

    // All 8 artifacts must exist
    let expected_files = [
        "Containerfile",
        "report.html",
        "audit-report.md",
        "secrets-review.md",
        "README.md",
        "kickstart-suggestion.ks",
        "inspection-snapshot.json",
    ];

    for name in &expected_files {
        let path = dir.path().join(name);
        assert!(path.exists(), "{} must exist after render_all", name);
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(!content.is_empty(), "{} must be non-empty", name);
    }

    assert!(
        dir.path().join("config").exists(),
        "config/ directory must exist after render_all"
    );

    // Cross-check: Containerfile references services
    let cf = std::fs::read_to_string(dir.path().join("Containerfile")).unwrap();
    assert!(
        cf.contains("systemctl enable httpd.service"),
        "render_all Containerfile must contain services data"
    );

    // Cross-check: audit report references services, storage, and kernelboot
    let audit = std::fs::read_to_string(dir.path().join("audit-report.md")).unwrap();
    assert!(
        audit.contains("httpd.service"),
        "render_all audit report must contain services data"
    );
    assert!(
        audit.contains("## Storage"),
        "render_all audit report must contain Storage section"
    );
    assert!(
        audit.contains("## Kernel & Boot"),
        "render_all audit report must contain Kernel & Boot section"
    );

    // Cross-check: HTML report references storage and kernelboot
    let html_report = std::fs::read_to_string(dir.path().join("report.html")).unwrap();
    assert!(
        html_report.contains("<h2>Storage</h2>"),
        "render_all HTML report must contain Storage section"
    );
    assert!(
        html_report.contains("Kernel &amp; Boot"),
        "render_all HTML report must contain Kernel & Boot section"
    );
}
