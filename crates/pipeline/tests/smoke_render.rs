//! Renderer smoke tests — verify each inspector's data reaches the correct
//! artifact consumers.
//!
//! These are SMOKE tests: they prove data REACHES the renderer, not that every
//! field is perfectly formatted. Each test builds a snapshot manually (no
//! inspector execution), calls the relevant renderer, and checks for key
//! markers in the output.
use inspectah_core::types::FindingKind;

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
                disposition: FindingKind::included(),
                locked: false,
                owning_package: None,
                aggregate: None,
                attention_reason: None,
                shadow_type: None,
                shadow_rationale: None,
            },
            ServiceStateChange {
                unit: "cups.service".into(),
                current_state: ServiceUnitState::Disabled,
                default_state: Some(PresetDefault::Enable),
                disposition: FindingKind::included(),
                locked: false,
                owning_package: None,
                aggregate: None,
                attention_reason: None,
                shadow_type: None,
                shadow_rationale: None,
            },
        ],
        enabled_units: vec!["httpd.service".into()],
        disabled_units: vec!["cups.service".into()],
        drop_ins: vec![SystemdDropIn {
            unit: "httpd.service".into(),
            path: "etc/systemd/system/httpd.service.d/override.conf".into(),
            content: "[Service]\nLimitNOFILE=65535\n".into(),
            disposition: FindingKind::included(),
            locked: false,
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
            disposition: FindingKind::included(),
            locked: false,
            aggregate: None,
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
    let output = containerfile::render_containerfile(&snap, None, None);

    assert!(
        output.contains("systemctl enable") && output.contains("httpd.service"),
        "Containerfile must contain systemctl enable for httpd.service"
    );
    assert!(
        output.contains("systemctl disable") && output.contains("cups.service"),
        "Containerfile must contain systemctl disable for cups.service"
    );
    assert!(
        output.contains("Service Enablement"),
        "Containerfile must contain the Service Enablement section heading"
    );
}

// ---------------------------------------------------------------------------
// Test 2: services_dropins_in_config_tree
// Service-owned drop-in files go to drop-ins/ only — NOT config/.
// The services renderer owns these paths.
// ---------------------------------------------------------------------------

#[test]
fn services_dropins_in_config_tree() {
    let snap = snapshot_with_services();
    let dir = TempDir::new().unwrap();

    configtree::write_config_tree(&snap, dir.path()).unwrap();

    // Service-owned drop-in must NOT appear in config/ tree
    let config_path = dir
        .path()
        .join("config/etc/systemd/system/httpd.service.d/override.conf");
    assert!(
        !config_path.exists(),
        "service-owned drop-in must NOT be in config/ — services renderer owns it"
    );

    // Drop-in must appear in drop-ins/ directory
    let dropin_path = dir
        .path()
        .join("drop-ins/etc/systemd/system/httpd.service.d/override.conf");
    assert!(
        dropin_path.exists(),
        "drop-in must be written to drop-ins/ directory"
    );

    let content = std::fs::read_to_string(&dropin_path).unwrap();
    assert!(
        content.contains("LimitNOFILE=65535"),
        "drop-in content must match the snapshot data"
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
    let output = containerfile::render_containerfile(&snap, None, None);

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
    let output = containerfile::render_containerfile(&snap, None, None);

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
#[ignore = "section templates deferred to T5-T12; un-ignore when Storage/KernelBoot templates land"]
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
            disposition: FindingKind::included(),
            locked: false,
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
#[ignore = "section templates deferred to T5-T12; un-ignore when Storage/KernelBoot templates land"]
fn render_all_with_all_sections() {
    let snap = snapshot_all_sections();
    let context = inspectah_core::traits::renderer::RenderContext { target: None };
    let dir = TempDir::new().unwrap();

    inspectah_pipeline::render::render_all(&snap, &context, dir.path()).unwrap();

    // All 8 artifacts must exist
    let expected_files = [
        "Containerfile",
        "audit-report.html",
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
        cf.contains("systemctl enable") && cf.contains("httpd.service"),
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
    let html_report = std::fs::read_to_string(dir.path().join("audit-report.html")).unwrap();
    assert!(
        html_report.contains("<h2>Storage</h2>"),
        "render_all HTML report must contain Storage section"
    );
    assert!(
        html_report.contains("Kernel &amp; Boot"),
        "render_all HTML report must contain Kernel & Boot section"
    );
}

// ---------------------------------------------------------------------------
// SELinux file context rules — spec § 3.1, § 5.1, § 8.9 render half
//
// These are not smoke tests. The emitted commands are the whole of the
// decision the renderer makes about the target system, so every one of them is
// compared whole against the § 3.1 template with its placeholders filled
// (§ 8.9), never by substring.
// ---------------------------------------------------------------------------

mod selinux_support;

use inspectah_core::types::selinux::{FcontextFileType, FcontextSource};
use selinux_support as sel;

/// § 5.1's "After" text for the host's own rule, quoted whole. The fixture is
/// the one the spec rendered it from: one `Label` on `/srv/www(/.*)?` and the
/// pre-existing `ssh_port_t` port label.
const AFTER_TEXT: &str = r#"# SELinux file contexts (1 custom rule). Labels for image content are computed
# when bootc pulls the image, from the image's own policy under /etc/selinux, so
# a rule only needs to be in the policy store: no restorecon at build time. Each
# rule below is either a command or a FIXME saying why it could not be one. Data
# you restore into /var (/var/srv included) keeps the labels it arrived with;
# run restorecon -R on it before starting the services that use it.
# The file context and port rules below need semanage, which this base does not
# carry; policycoreutils-python-utils provides it.
RUN dnf install -y policycoreutils-python-utils \
    && dnf clean all \
    && rm -rf \
        /var/cache/dnf \
        /var/lib/dnf/history* \
        /var/log/dnf* \
        /var/log/hawkey.log \
        /var/log/rhsm
# Each rule removes its own key from the local store if present, then adds it, or
# modifies it when the target policy defines the key; the rules end the local
# list in the order the source host held them. A rule whose path the target
# already maps as an equivalence is skipped with a FIXME in the build output,
# because deleting its key would delete that equivalence.
# /srv is a symlink to /var/srv on this base and the policy has no /var/srv alias;
# this makes every /srv rule, stock and custom, apply to the real path. The
# equivalence is added when absent and left alone when identical; a different
# target mapping stops the build with an instruction.
RUN eqs="$(semanage fcontext -l -C)" || { printf 'FIXME: could not list the target file contexts before adding the equivalence for %s; fix semanage in the base, or exclude this rule, then rebuild\n' '/var/srv' >&2; exit 1; }; \
    eq="$(printf '%s\n' "$eqs" | awk -v s='/var/srv' '$2 == "=" && $1 == s { print $3 }')"; \
    if [ -z "$eq" ]; then semanage fcontext -a -e '/srv' '/var/srv'; \
    elif [ "$eq" != '/srv' ]; then printf 'FIXME: %s is already equivalent to %s on the target, not %s; delete that equivalence with semanage fcontext -d -e %s %s, or exclude this rule, then rebuild\n' '/var/srv' "$eq" '/srv' "$eq" '/var/srv' >&2; exit 1; fi
RUN spec='/srv/www(/.*)?'; \
    eqs="$(semanage fcontext -l -C)" || { printf 'FIXME: could not list the target file contexts before replaying the rule for %s; fix semanage in the base, or exclude this rule, then rebuild\n' "$spec" >&2; exit 1; }; \
    if printf '%s\n' "$eqs" | awk '$2 == "=" { print $1 }' | grep -qxF -- "$spec"; then \
      printf 'FIXME: skipped the file context rule for %s; the target maps that path as an equivalence, which semanage fcontext -d would delete instead of a label; delete it with semanage fcontext -d -e <its target> %s, or exclude this rule, then rebuild\n' "$spec" "$spec" >&2; \
    else semanage fcontext -d "$spec" 2>/dev/null || true; \
      semanage fcontext -a -t httpd_sys_content_t "$spec" \
      || semanage fcontext -m -t httpd_sys_content_t "$spec"; fi
# 1 custom SELinux port label(s) detected
RUN semanage port -a -t ssh_port_t -p tcp 2222"#;

const HTTPD_TYPE: &str = "httpd_sys_content_t";
const HTTPD_ARGS: &str = "-t httpd_sys_content_t";

/// The host rule's snapshot: § 5.1's "Before" and "After" are both this one.
fn host_snapshot() -> inspectah_core::snapshot::InspectionSnapshot {
    sel::snapshot(
        vec![sel::label(
            "/srv/www(/.*)?",
            FcontextFileType::All,
            HTTPD_TYPE,
        )],
        FcontextSource::PolicyStore,
        vec![sel::port_label("ssh_port_t", "tcp", "2222")],
    )
}

fn render_block_text(snap: &inspectah_core::snapshot::InspectionSnapshot) -> String {
    let output = containerfile::render_containerfile(snap, None, None);
    sel::selinux_block(&output).join("\n")
}

fn rendered_runs(snap: &inspectah_core::snapshot::InspectionSnapshot) -> Vec<String> {
    sel::block_runs(snap)
        .into_iter()
        .map(|r| r.rendered)
        .collect()
}

/// The `RUN` of the install precondition, which leads every block that renders
/// a rule or a port label. Its two comment lines are not part of the `RUN`;
/// everything from index 2 on is, continuations included.
fn install_run() -> String {
    sel::INSTALL_PRECONDITION[2..].join("\n")
}

#[test]
fn selinux_block_renders_the_spec_after_text() {
    assert_eq!(render_block_text(&host_snapshot()), AFTER_TEXT);
}

#[test]
fn selinux_two_rules_render_two_guarded_forms_in_collected_order() {
    let snap = sel::snapshot_rules(vec![
        sel::label("/opt/alpha(/.*)?", FcontextFileType::All, HTTPD_TYPE),
        sel::label("/opt/beta(/.*)?", FcontextFileType::All, HTTPD_TYPE),
    ]);
    assert_eq!(
        rendered_runs(&snap),
        vec![
            install_run(),
            sel::rule1_rendered("/opt/alpha(/.*)?", "", HTTPD_ARGS),
            sel::rule1_rendered("/opt/beta(/.*)?", "", HTTPD_ARGS),
        ]
    );
}

#[test]
fn selinux_directory_rule_carries_f_d_on_all_three_commands() {
    let snap = sel::snapshot_rules(vec![sel::label(
        "/opt/dir(/.*)?",
        FcontextFileType::Directory,
        HTTPD_TYPE,
    )]);
    let expected = sel::rule1_rendered("/opt/dir(/.*)?", " -f d", "-f d -t httpd_sys_content_t");
    // The three commands are the `-d`, the `-a`, and the `-m`; the whole-body
    // comparison below pins each, and this count says so in one line.
    assert_eq!(expected.matches("-f d").count(), 3);
    assert_eq!(rendered_runs(&snap), vec![install_run(), expected]);
}

#[test]
fn selinux_unlabel_renders_none_inside_the_same_guard() {
    let snap = sel::snapshot_rules(vec![sel::unlabel("/opt/none(/.*)?", FcontextFileType::All)]);
    assert_eq!(
        rendered_runs(&snap),
        vec![
            install_run(),
            sel::rule1_rendered("/opt/none(/.*)?", "", "-t '<<none>>'"),
        ]
    );
}

/// The only reachable pattern where `-f` meets a `-t` that did not come from
/// a context: `Unlabel` carries no context field at all, so `-t '<<none>>'` is
/// a literal. § 5.1 states both halves and never crosses them.
#[test]
fn selinux_unlabel_with_a_file_type_carries_both_flags() {
    let spec = "/opt/none(/.*)?";
    let snap = sel::snapshot_rules(vec![sel::unlabel(spec, FcontextFileType::Directory)]);
    assert_eq!(
        rendered_runs(&snap),
        vec![
            install_run(),
            sel::rule1_rendered(spec, " -f d", "-f d -t '<<none>>'"),
        ]
    );
}

/// `-r` is suppressed for `None` and for `Some("s0")` alike. Every other
/// fixture in this file uses `Some(..)`, so `None` is a branch nothing else
/// reaches — the shape an `unwrap_or` gets wrong.
#[test]
fn selinux_level_none_suppresses_the_range_flag() {
    let spec = "/opt/level(/.*)?";
    let cases: &[(&str, Option<&str>, &str)] = &[
        ("level None", None, "-t httpd_sys_content_t"),
        ("level s0", Some("s0"), "-t httpd_sys_content_t"),
        // Paired positive, in this test rather than another, so the two
        // suppressions above are read against a case that does emit `-r`.
        (
            "level s0-s0:c0",
            Some("s0-s0:c0"),
            "-r s0-s0:c0 -t httpd_sys_content_t",
        ),
    ];
    for (case, level, am_args) in cases {
        let snap = sel::snapshot_rules(vec![sel::label_with(
            spec,
            FcontextFileType::All,
            inspectah_core::types::selinux::SelinuxContext {
                user: "system_u".into(),
                role: "object_r".into(),
                type_: HTTPD_TYPE.into(),
                level: level.map(|l| l.to_string()),
            },
        )]);
        assert_eq!(
            rendered_runs(&snap),
            vec![install_run(), sel::rule1_rendered(spec, "", am_args)],
            "{case}"
        );
    }
}

/// `-f`, `-r`, `-s`, `-t`, in that order on the `-a` and the `-m`, with the
/// `-d` carrying only `-f` and the spec. No spec section fixes the order; it
/// is a ruling, recorded at `sel::FLAG_ORDER`.
#[test]
fn selinux_optional_context_flags_render_in_canonical_order() {
    let cases: &[(&str, &str, &str, FcontextFileType, &str, &str)] = &[
        // case, user, level, file type, expected -d extra, expected -a/-m args
        (
            "non-default user",
            "user_u",
            "s0",
            FcontextFileType::All,
            "",
            "-s user_u -t httpd_sys_content_t",
        ),
        (
            "non-default level",
            "system_u",
            "s0-s0:c0",
            FcontextFileType::All,
            "",
            "-r s0-s0:c0 -t httpd_sys_content_t",
        ),
        (
            "all four flags",
            "user_u",
            "s0-s0:c0",
            FcontextFileType::Directory,
            " -f d",
            "-f d -r s0-s0:c0 -s user_u -t httpd_sys_content_t",
        ),
    ];

    for (case, user, level, file_type, d_args, am_args) in cases {
        let spec = "/opt/flags(/.*)?";
        let snap = sel::snapshot_rules(vec![sel::label_with(
            spec,
            *file_type,
            inspectah_core::types::selinux::SelinuxContext {
                user: (*user).into(),
                role: "object_r".into(),
                type_: HTTPD_TYPE.into(),
                level: Some((*level).into()),
            },
        )]);
        assert_eq!(
            rendered_runs(&snap),
            vec![install_run(), sel::rule1_rendered(spec, d_args, am_args)],
            "{case}"
        );
    }

    // The order itself, stated once rather than inferred from the strings
    // above: every flag the all-four case carries appears in FLAG_ORDER order.
    let all_four = "-f d -r s0-s0:c0 -s user_u -t httpd_sys_content_t";
    let positions: Vec<usize> = sel::FLAG_ORDER
        .iter()
        .map(|flag| {
            all_four
                .find(&format!("{flag} "))
                .unwrap_or_else(|| panic!("{flag} missing from the all-four argument list"))
        })
        .collect();
    assert!(
        positions.windows(2).all(|w| w[0] < w[1]),
        "the all-four argument list does not follow FLAG_ORDER: {positions:?}"
    );
}

#[test]
fn selinux_no_run_line_contains_echo() {
    // Every form the block can emit, in one fixture, so the ban is checked
    // against a block that actually holds each of them.
    let snap = sel::snapshot(
        vec![
            sel::label("/srv/www(/.*)?", FcontextFileType::All, HTTPD_TYPE),
            sel::unlabel("/opt/none(/.*)?", FcontextFileType::Directory),
            sel::equivalence("/home1", "/home"),
        ],
        FcontextSource::SemanageList,
        vec![sel::port_label("ssh_port_t", "tcp", "2222")],
    );
    let runs = sel::block_runs(&snap);
    // The ban is on the diagnostic form, so prove the fixture rendered a
    // diagnostic. Without this the loop below passes over a block whose rules
    // all came out as comments, which is not the thing being banned.
    assert!(
        runs.iter().any(|r| r.rendered.contains("printf 'FIXME:")),
        "the fixture rendered no build-time diagnostic, so the echo ban is vacuous"
    );
    for run in &runs {
        assert!(
            !run.rendered.contains("echo"),
            "a RUN of the block uses echo:\n{}",
            run.rendered
        );
    }
}

#[test]
fn selinux_no_restorecon_outside_the_comment_sentence() {
    let block = render_block_text(&host_snapshot());
    assert!(
        block.contains("run restorecon -R on"),
        "the block must keep the § 5.1 sentence about data restored into /var"
    );
    for line in block.lines() {
        if line.trim_start().starts_with('#') {
            continue;
        }
        assert!(
            !line.contains("restorecon"),
            "restorecon outside a comment line: {line:?}"
        );
    }
}

#[test]
fn selinux_no_line_contains_dash_m_dash_e() {
    // `-m -e` is the form semanage has no handler for; the equivalence path is
    // add-or-refuse, never modify.
    let snap = sel::snapshot_rules(vec![
        sel::equivalence("/var/srv", "/srv"),
        sel::equivalence("/home1", "/home"),
        sel::label("/srv/www(/.*)?", FcontextFileType::All, HTTPD_TYPE),
    ]);
    let block = render_block_text(&snap);
    assert!(
        block.contains("-a -e"),
        "the fixture must render at least one equivalence, or the ban below is vacuous"
    );
    assert!(
        !block.contains("-m -e"),
        "the block emits `-m -e`:\n{block}"
    );
}

#[test]
fn selinux_equivalence_run_appears_once_for_two_srv_rules_and_never_for_opt() {
    let srv = sel::snapshot_rules(vec![
        sel::label("/srv/alpha(/.*)?", FcontextFileType::All, HTTPD_TYPE),
        sel::label("/srv/beta(/.*)?", FcontextFileType::All, HTTPD_TYPE),
    ]);
    assert_eq!(
        rendered_runs(&srv),
        vec![
            install_run(),
            sel::rule2_rendered("/var/srv", "/srv"),
            sel::rule1_rendered("/srv/alpha(/.*)?", "", HTTPD_ARGS),
            sel::rule1_rendered("/srv/beta(/.*)?", "", HTTPD_ARGS),
        ]
    );

    let opt = sel::snapshot_rules(vec![sel::label(
        "/opt/alpha(/.*)?",
        FcontextFileType::All,
        HTTPD_TYPE,
    )]);
    assert_eq!(
        rendered_runs(&opt),
        vec![
            install_run(),
            sel::rule1_rendered("/opt/alpha(/.*)?", "", HTTPD_ARGS),
        ]
    );
}

#[test]
fn selinux_collected_srv_equivalence_and_the_synthesis_emit_one_run() {
    let snap = sel::snapshot_rules(vec![
        sel::equivalence("/var/srv", "/srv"),
        sel::label("/srv/www(/.*)?", FcontextFileType::All, HTTPD_TYPE),
    ]);
    let runs = sel::block_runs(&snap);
    let equivalences: Vec<&String> = runs
        .iter()
        .map(|r| &r.rendered)
        .filter(|r| r.contains("-a -e"))
        .collect();
    assert_eq!(
        equivalences,
        vec![&sel::rule2_rendered("/var/srv", "/srv")],
        "the collected row and the synthesized one are one equivalence, emitted once"
    );
}

#[test]
fn selinux_conflicting_collected_equivalence_emits_no_run_and_a_fixme_naming_both() {
    // The collected target shares no suffix with `/var/srv` or `/srv`, so
    // naming it in the FIXME is a fact a test can check on its own. An earlier
    // `/opt/srv` could not be told apart from the synthesized `/srv`.
    const COLLECTED_TARGET: &str = "/mnt/data";
    let snap = sel::snapshot_rules(vec![
        sel::equivalence("/var/srv", COLLECTED_TARGET),
        sel::label("/srv/www(/.*)?", FcontextFileType::All, HTTPD_TYPE),
    ]);
    let block = render_block_text(&snap);
    for run in sel::block_runs(&snap) {
        assert!(
            !run.rendered.contains("-a -e"),
            "a conflicting equivalence pair must emit no equivalence RUN:\n{}",
            run.rendered
        );
    }
    assert!(
        block.contains(sel::UNRENDERABLE_FIXME_INSTRUCTION),
        "the conflict must render the two-line FIXME, imperative first:\n{block}"
    );
    let cause = block
        .lines()
        .find(|l| l.starts_with(sel::UNRENDERABLE_FIXME_CAUSE_PREFIX))
        .unwrap_or_else(|| panic!("no cause line under the FIXME instruction:\n{block}"));
    // The two rows of the conflict. The synthesized row's `/srv` is not
    // asserted here: `/var/srv` ends in it, so no fixture makes it
    // independently checkable, and the sentence is not byte-specified yet.
    assert!(
        cause.contains("/var/srv"),
        "cause names no subject: {cause}"
    );
    assert!(
        cause.contains(COLLECTED_TARGET),
        "cause names no collected target: {cause}"
    );
}

/// § 4: the Containerfile's header count is decision-bearing rules only.
/// `Unparsed` carries no disposition and is not a decision, so it does not
/// count. Today `containerfile.rs:1446-1449` counts `fcontext_rules.len()`.
#[test]
fn selinux_header_count_excludes_unparsed_rules() {
    const UNPARSED_RAW: &str = "this is not a file_contexts line";
    const HEADER: &str = "# SELinux file contexts (1 custom rule).";

    let with_unparsed = render_block_text(&sel::snapshot_rules(vec![
        sel::label("/opt/a(/.*)?", FcontextFileType::All, HTTPD_TYPE),
        sel::unparsed(UNPARSED_RAW),
    ]));
    // The Unparsed rule must actually have reached the block, or the count
    // below is 1 because the fixture was dropped rather than because the
    // count excludes it.
    assert!(
        with_unparsed.contains(UNPARSED_RAW),
        "the Unparsed rule never reached the block:\n{with_unparsed}"
    );
    assert!(
        with_unparsed.contains(HEADER),
        "one decision-bearing rule and one Unparsed must count 1:\n{with_unparsed}"
    );

    // Paired: the same count from a fixture with the decision-bearing rule
    // alone, so the assertion above is about the Unparsed rule and not about
    // the header's wording.
    let without = render_block_text(&sel::snapshot_rules(vec![sel::label(
        "/opt/a(/.*)?",
        FcontextFileType::All,
        HTTPD_TYPE,
    )]));
    assert!(
        without.contains(HEADER),
        "the one-rule header is not the text this test thinks:\n{without}"
    );
}

#[test]
fn selinux_semanage_list_renders_the_order_caveat_and_policy_store_does_not() {
    let rules = || {
        vec![sel::label(
            "/opt/a(/.*)?",
            FcontextFileType::All,
            HTTPD_TYPE,
        )]
    };

    let listed = sel::snapshot(rules(), FcontextSource::SemanageList, Vec::new());
    let block = render_block_text(&listed);
    let note_at = block
        .lines()
        .position(|l| l == sel::SEMANAGE_LIST_NOTE)
        .unwrap_or_else(|| panic!("a SemanageList snapshot must carry the caveat:\n{block}"));
    let first_rule_at = block
        .lines()
        .position(|l| l.starts_with("RUN spec="))
        .unwrap_or_else(|| panic!("no rule rendered in:\n{block}"));
    assert!(
        note_at < first_rule_at,
        "the caveat must precede the first rule:\n{block}"
    );

    let stored = sel::snapshot(rules(), FcontextSource::PolicyStore, Vec::new());
    let stored_block = render_block_text(&stored);
    assert!(
        !stored_block.contains("# NOTE:"),
        "a PolicyStore snapshot must carry no order caveat:\n{stored_block}"
    );
}

// --- Operand validation: one case per forbidden byte per operand kind -------

/// The three bytes single quotes do not make inert (Global Constraints,
/// § 3.1 rule 4). Named so the three cases below read as one rule.
const UNQUOTABLE_BYTES: &[(&str, &str)] = &[("apostrophe", "'"), ("newline", "\n"), ("nul", "\0")];

#[test]
fn selinux_unquotable_bytes_reject_every_operand_kind() {
    for (name, byte) in UNQUOTABLE_BYTES {
        let spec = format!("/tmp/a{byte}b");

        // A label `spec`.
        assert_rejected(
            &sel::snapshot_rules(vec![sel::label(&spec, FcontextFileType::All, HTTPD_TYPE)]),
            &spec,
            &format!("label spec with {name}"),
        );
        // An equivalence `subject`.
        assert_rejected(
            &sel::snapshot_rules(vec![sel::equivalence(&spec, "/srv")]),
            &spec,
            &format!("equivalence subject with {name}"),
        );
        // An equivalence `labeled_like`.
        assert_rejected(
            &sel::snapshot_rules(vec![sel::equivalence("/var/other", &spec)]),
            &spec,
            &format!("equivalence labeled_like with {name}"),
        );
    }
}

#[test]
fn selinux_spec_without_a_leading_slash_is_rejected() {
    // `semanage fcontext -a --help` answers with usage text and exits 0, so a
    // spec that is not a path would skip its rule silently (§ 3.1 rule 4).
    assert_rejected(
        &sel::snapshot_rules(vec![sel::label(
            "--help",
            FcontextFileType::All,
            HTTPD_TYPE,
        )]),
        "--help",
        "a spec with no leading slash",
    );
}

/// A rejected rule renders the two-line FIXME, imperative first, and no
/// command of any kind.
fn assert_rejected(
    snap: &inspectah_core::snapshot::InspectionSnapshot,
    offending: &str,
    case: &str,
) {
    let block = render_block_text(snap);
    let expected = sel::unrenderable_fixme(offending);
    let joined = expected.join("\n");
    assert!(
        block.contains(&joined),
        "{case}: the two-line FIXME is missing or out of order.\nexpected:\n{joined}\ngot:\n{block}"
    );
    for run in sel::block_runs(snap) {
        assert!(
            !run.command.contains("semanage fcontext"),
            "{case}: a rejected rule rendered a command:\n{}",
            run.rendered
        );
    }
}

#[test]
fn selinux_shell_looking_operands_reach_only_comment_lines() {
    let snap = sel::snapshot_rules(vec![
        sel::equivalence("/tmp/$(id)", "/tmp"),
        sel::equivalence("/srv;rm -rf /", "/srv"),
    ]);
    let block = render_block_text(&snap);
    // Both operands fail § 3.1 rule 4 for an equivalence — `(`/`)` on the
    // first, whitespace on the second — so each must reach the Containerfile
    // through the two-line FIXME and nothing else.
    for operand in ["/tmp/$(id)", "/srv;rm -rf /"] {
        assert_rejected(&snap, operand, operand);
    }
    for operand in ["/tmp/$(id)", "/srv;rm -rf /"] {
        let carriers: Vec<&str> = block.lines().filter(|l| l.contains(operand)).collect();
        assert!(
            !carriers.is_empty(),
            "the block must report {operand:?} somewhere:\n{block}"
        );
        for line in carriers {
            assert!(
                line.trim_start().starts_with('#'),
                "{operand:?} reached a non-comment line: {line:?}"
            );
        }
    }
    for run in sel::block_runs(&snap) {
        assert!(
            !run.command.contains(";rm"),
            "a RUN carries a command separator from an operand:\n{}",
            run.rendered
        );
    }
}

// --- § 8.9's self-check on the whole-body comparison ------------------------

#[test]
fn selinux_whole_body_comparison_rejects_the_round_2_echo_diagnostic() {
    let snap = sel::snapshot_rules(vec![sel::equivalence("/var/srv", "/srv")]);
    let rendered = sel::block_runs(&snap)
        .into_iter()
        .map(|r| r.rendered)
        .find(|r| r.contains("-a -e"))
        .unwrap_or_else(|| panic!("the fixture rendered no equivalence RUN"));

    let expected = sel::rule2_rendered("/var/srv", "/srv");
    let round2 = sel::rule2_rendered_round2_echo("/var/srv", "/srv");

    // The positive half: the renderer emits the printf form.
    assert_eq!(rendered, expected);
    // The self-check: the comparison above is one a double-quoted `echo`
    // diagnostic fails, so passing it says something.
    assert_ne!(
        expected, round2,
        "the substitution left the body unchanged, so the comparison proves nothing"
    );
    assert_ne!(rendered, round2);
}

// --- The install precondition, four cases -----------------------------------

#[test]
fn selinux_install_precondition_renders_for_an_fcontext_rule_alone() {
    let snap = sel::snapshot_rules(vec![sel::label(
        "/opt/a(/.*)?",
        FcontextFileType::All,
        HTTPD_TYPE,
    )]);
    assert_precondition_present(&snap, "RUN spec=");
}

#[test]
fn selinux_install_precondition_renders_for_a_port_label_alone() {
    // `RUN semanage port -a` shipped at 6e8a52b1 with the identical defect,
    // so a snapshot with no fcontext rule at all still needs the line.
    let snap = sel::snapshot(
        Vec::new(),
        FcontextSource::PolicyStore,
        vec![sel::port_label("ssh_port_t", "tcp", "2222")],
    );
    assert_precondition_present(&snap, "RUN semanage port -a");
}

#[test]
fn selinux_install_precondition_absent_without_rules_or_port_labels() {
    // The fixture `containerfile.rs`'s section-ordering test uses: a block
    // that renders for FIPS alone, with no rule and no port label.
    let mut snap = inspectah_core::snapshot::InspectionSnapshot::new();
    snap.selinux = Some(inspectah_core::types::selinux::SelinuxSection {
        fips_mode: true,
        ..Default::default()
    });
    let block = render_block_text(&snap);

    // Paired positive: the same three lines, from the same renderer, on a
    // fixture that does carry a rule. Without it this test would hold against
    // a renderer that never emits the precondition at all.
    let with_rule = render_block_text(&sel::snapshot_rules(vec![sel::label(
        "/opt/a(/.*)?",
        FcontextFileType::All,
        HTTPD_TYPE,
    )]));
    for line in sel::INSTALL_PRECONDITION {
        assert!(
            with_rule.contains(line),
            "the precondition line {line:?} never renders at all:\n{with_rule}"
        );
        assert!(
            !block.contains(line),
            "a block with neither a rule nor a port label emitted {line:?}:\n{block}"
        );
    }
}

/// The precondition installs the tool the emitted commands need, and sits
/// immediately before the first of them. A block whose every rule failed
/// validation emits comments and no command, so there is nothing to install
/// for and nothing to sit before.
#[test]
fn selinux_install_precondition_absent_when_every_rule_is_unrenderable() {
    let unrenderable = sel::snapshot_rules(vec![
        sel::label("/tmp/it's", FcontextFileType::All, HTTPD_TYPE),
        sel::equivalence("/srv;rm -rf /", "/srv"),
    ]);
    let block = render_block_text(&unrenderable);
    assert!(
        block.contains(sel::UNRENDERABLE_FIXME_INSTRUCTION),
        "the fixture must actually reject its rules, or this proves nothing:\n{block}"
    );
    for run in sel::block_runs(&unrenderable) {
        assert!(
            !run.command.contains("semanage fcontext"),
            "a rejected rule rendered a command:\n{}",
            run.rendered
        );
    }
    for line in sel::INSTALL_PRECONDITION {
        assert!(
            !block.contains(line),
            "a block with no emitted semanage command carries {line:?}:\n{block}"
        );
    }

    // Paired positive: one of the same rules made renderable, same renderer.
    let renderable = render_block_text(&sel::snapshot_rules(vec![sel::label(
        "/tmp/ok",
        FcontextFileType::All,
        HTTPD_TYPE,
    )]));
    for line in sel::INSTALL_PRECONDITION {
        assert!(
            renderable.contains(line),
            "the precondition line {line:?} never renders at all:\n{renderable}"
        );
    }
}

#[test]
fn selinux_install_precondition_is_never_folded_into_the_package_list() {
    use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection};

    let mut snap = sel::snapshot_rules(vec![sel::label(
        "/opt/a(/.*)?",
        FcontextFileType::All,
        HTTPD_TYPE,
    )]);
    snap.rpm = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "httpd".into(),
            arch: "x86_64".into(),
            state: PackageState::Added,
            source_repo: "appstream".into(),
            disposition: FindingKind::included(),
            ..Default::default()
        }],
        leaf_packages: Some(vec!["httpd.x86_64".into()]),
        ..Default::default()
    });

    let output = containerfile::render_containerfile(&snap, None, None);
    let active: Vec<&str> = output
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .filter(|l| l.contains(sel::SEMANAGE_PACKAGE))
        .collect();
    assert_eq!(
        active,
        vec![sel::INSTALL_PRECONDITION[2]],
        "semanage's package belongs to exactly one active line, its own RUN, and \
         never to the package set the refine UI presented for decision"
    );
    assert!(
        output.contains("RUN dnf install -y \\"),
        "the fixture must render a package install block, or the check above is vacuous"
    );
}

/// The three precondition lines render once, in order, ahead of the block's
/// first `semanage` command, and carry no `--enablerepo`.
fn assert_precondition_present(
    snap: &inspectah_core::snapshot::InspectionSnapshot,
    first_semanage_prefix: &str,
) {
    let block = render_block_text(snap);
    let joined = sel::INSTALL_PRECONDITION.join("\n");
    assert!(
        block.contains(&joined),
        "the precondition's three lines are missing or out of order.\nexpected:\n{joined}\ngot:\n{block}"
    );
    assert_eq!(
        block.matches(sel::INSTALL_PRECONDITION[2]).count(),
        1,
        "the install RUN renders exactly once:\n{block}"
    );

    let lines: Vec<&str> = block.lines().collect();
    let install_at = lines
        .iter()
        .position(|l| *l == sel::INSTALL_PRECONDITION[2])
        .expect("the install RUN is present");
    let first_semanage_at = lines
        .iter()
        .position(|l| l.starts_with(first_semanage_prefix))
        .unwrap_or_else(|| panic!("no line starting {first_semanage_prefix:?} in:\n{block}"));
    assert!(
        install_at < first_semanage_at,
        "the precondition must precede the first semanage command:\n{block}"
    );

    // `policycoreutils-python-utils` is in AppStream; Task 17's CRB rule puts
    // `--enablerepo` on every other `dnf ... install -y` and must not reach it.
    assert!(
        !lines[install_at].contains("--enablerepo"),
        "the precondition carries --enablerepo: {}",
        lines[install_at]
    );
}

// --- Fix round 1: the operand positions and paths the first suite missed ----

use inspectah_core::types::selinux::{FcontextRule, SelinuxContext};

fn context_of(type_: &str, user: &str, level: Option<&str>) -> SelinuxContext {
    SelinuxContext {
        user: user.into(),
        role: "object_r".into(),
        type_: type_.into(),
        level: level.map(|l| l.to_string()),
    }
}

/// `-t`, `-s` and `-r` are the only operands the emitted command carries
/// **unquoted**, so the bare-token check, and no quoting, is what keeps them
/// inert. Nothing in the first suite reached it: every fixture's context was
/// `httpd_sys_content_t` / `system_u` / `s0`, all of which pass.
#[test]
fn selinux_hostile_context_tokens_reject_the_rule() {
    const SPEC: &str = "/opt/tokens(/.*)?";
    // One case per unquoted field, each failing for a different reason: a
    // command substitution, whitespace, a backslash.
    let cases: &[(&str, &str, &str, Option<&str>)] = &[
        ("type", "t`c6_exec`", "system_u", Some("s0")),
        ("user", HTTPD_TYPE, "u u", Some("s0")),
        ("level", HTTPD_TYPE, "system_u", Some(r"s0\x")),
    ];
    for (field, type_, user, level) in cases {
        let offending = match *field {
            "type" => *type_,
            "user" => *user,
            _ => level.expect("the level case carries one"),
        };
        let snap = sel::snapshot_rules(vec![sel::label_with(
            SPEC,
            FcontextFileType::All,
            context_of(type_, user, *level),
        )]);
        assert_rejected(&snap, offending, &format!("hostile {field}"));
    }

    // Paired positive: the same three fields, carrying values that pass,
    // render the command. Without it the rejections above would hold against
    // a renderer that refuses every context.
    let ok = sel::snapshot_rules(vec![sel::label_with(
        SPEC,
        FcontextFileType::All,
        context_of(HTTPD_TYPE, "user_u", Some("s0-s0:c0")),
    )]);
    assert_eq!(
        rendered_runs(&ok),
        vec![
            install_run(),
            sel::rule1_rendered(SPEC, "", "-r s0-s0:c0 -s user_u -t httpd_sys_content_t"),
        ]
    );
}

/// An excluded rule is a rule the user removed in the refine UI. Nothing in
/// the first suite used one, so a renderer that ignored the disposition
/// passed all 36 tests. § 4's aggregate row makes exclusion the normal
/// outcome of hosts disagreeing, so this is the common path.
#[test]
fn selinux_excluded_rules_render_nothing_anywhere() {
    const EXCLUDED: &str = "/opt/the-user-removed-this(/.*)?";
    const INCLUDED: &str = "/opt/a(/.*)?";

    let excluded_rule = |spec: &str| FcontextRule::Label {
        spec: spec.into(),
        file_type: FcontextFileType::All,
        context: sel::ctx(HTTPD_TYPE),
        disposition: FindingKind::excluded(),
        aggregate: None,
    };

    let mixed = sel::snapshot_rules(vec![
        sel::label(INCLUDED, FcontextFileType::All, HTTPD_TYPE),
        excluded_rule(EXCLUDED),
    ]);
    assert_eq!(
        rendered_runs(&mixed),
        vec![install_run(), sel::rule1_rendered(INCLUDED, "", HTTPD_ARGS),],
        "an excluded rule renders no command"
    );
    let block = render_block_text(&mixed);
    assert!(
        !block.contains(EXCLUDED),
        "an excluded rule reached the block, in a comment or otherwise:\n{block}"
    );
    assert!(
        block.contains("# SELinux file contexts (1 custom rule)."),
        "the count must exclude it too:\n{block}"
    );

    // Excluded-only: the section carries no fcontext content at all, so the
    // install precondition has nothing to install for.
    let only_excluded = sel::snapshot_rules(vec![excluded_rule(EXCLUDED)]);
    let output = containerfile::render_containerfile(&only_excluded, None, None);
    assert!(!output.contains(EXCLUDED), "excluded-only:\n{output}");
    for line in sel::INSTALL_PRECONDITION {
        assert!(
            !output.contains(line),
            "excluded-only emitted {line:?}:\n{output}"
        );
    }
}

/// The synthesis fires on a top-level boundary, not a string prefix: `/srv`
/// itself and `/srv(/.*)?` need the equivalence, `/srvbackup` does not. Every
/// fixture in the first suite used `/srv/...`, so `starts_with` would have
/// passed all of them while giving a `/srvbackup` host a spurious
/// equivalence and the install precondition that comes with it.
#[test]
fn selinux_synthesis_fires_on_a_top_level_boundary_only() {
    let cases: &[(&str, bool)] = &[
        ("/srv", true),
        ("/srv(/.*)?", true),
        ("/srv/www(/.*)?", true),
        ("/srvbackup/data(/.*)?", false),
        ("/opt/srv/data(/.*)?", false),
    ];
    for (spec, expected) in cases {
        let snap = sel::snapshot_rules(vec![sel::label(spec, FcontextFileType::All, HTTPD_TYPE)]);
        let emitted = rendered_runs(&snap).iter().any(|r| r.contains("-a -e"));
        assert_eq!(emitted, *expected, "{spec}");
    }
}

/// `-e` takes a literal path, so an equivalence operand refuses the regex
/// characters; a label `spec` is a regex and keeps them. The backslash is the
/// one § 3.1 rule 4 gives a reason for: the subject reaches `awk -v s=`,
/// which escape-processes its assignment value, so a surviving backslash
/// would compare a different string at build time with no diagnostic.
#[test]
fn selinux_regex_bytes_reject_an_equivalence_operand_and_render_in_a_spec() {
    for (name, byte) in [
        ("backslash", r"\"),
        ("star", "*"),
        ("bracket", "["),
        ("pipe", "|"),
    ] {
        let operand = format!("/tmp/a{byte}b");
        assert_rejected(
            &sel::snapshot_rules(vec![sel::equivalence(&operand, "/srv")]),
            &operand,
            &format!("equivalence subject with a {name}"),
        );
        assert_rejected(
            &sel::snapshot_rules(vec![sel::equivalence("/var/other", &operand)]),
            &operand,
            &format!("equivalence labeled_like with a {name}"),
        );
        let as_spec = sel::snapshot_rules(vec![sel::label(
            &operand,
            FcontextFileType::All,
            HTTPD_TYPE,
        )]);
        assert_eq!(
            rendered_runs(&as_spec),
            vec![install_run(), sel::rule1_rendered(&operand, "", HTTPD_ARGS)],
            "a label spec keeps its {name}"
        );
    }
}

/// A space in a label spec defeats the build-time guard: `awk` splits the
/// target's listing on one space where the collector splits on two or more,
/// so the subject column is read short, `grep -qxF` does not match, and the
/// rule proceeds to a `semanage fcontext -d` that deletes the equivalence.
#[test]
fn selinux_spec_with_whitespace_is_rejected() {
    assert_rejected(
        &sel::snapshot_rules(vec![sel::label(
            "/var/my srv",
            FcontextFileType::All,
            HTTPD_TYPE,
        )]),
        "/var/my srv",
        "a label spec carrying a space",
    );
}

/// Control characters are inert to the shell inside single quotes, but the
/// rendered Containerfile is read by someone who was not the scanner, and a
/// spec carrying CR or an ANSI escape displays as something other than its
/// bytes.
#[test]
fn selinux_control_characters_are_rejected() {
    for (name, byte) in [("carriage return", "\r"), ("escape", "\x1b")] {
        let spec = format!("/tmp/a{byte}b");
        assert_rejected(
            &sel::snapshot_rules(vec![sel::label(&spec, FcontextFileType::All, HTTPD_TYPE)]),
            &spec,
            &format!("a label spec carrying a {name}"),
        );
    }
}

/// Two collected rows claiming one subject. The first suite reached the
/// collected-versus-synthesized fold only, so the collected-versus-collected
/// arm was never taken.
#[test]
fn selinux_two_collected_rows_with_one_subject_dedup_or_conflict() {
    let identical = sel::snapshot_rules(vec![
        sel::equivalence("/a", "/b"),
        sel::equivalence("/a", "/b"),
    ]);
    assert_eq!(
        rendered_runs(&identical),
        vec![install_run(), sel::rule2_rendered("/a", "/b")],
        "two identical rows are one equivalence"
    );

    let differing = sel::snapshot_rules(vec![
        sel::equivalence("/a", "/b"),
        sel::equivalence("/a", "/c"),
    ]);
    for run in sel::block_runs(&differing) {
        assert!(
            !run.rendered.contains("-a -e"),
            "two rows claiming /a must emit no equivalence RUN:\n{}",
            run.rendered
        );
    }
    let block = render_block_text(&differing);
    let cause = block
        .lines()
        .find(|l| l.starts_with(sel::UNRENDERABLE_FIXME_CAUSE_PREFIX))
        .unwrap_or_else(|| panic!("no cause line:\n{block}"));
    assert!(
        cause.contains("\"/b\""),
        "cause names no first target: {cause}"
    );
    assert!(
        cause.contains("\"/c\""),
        "cause names no second target: {cause}"
    );
}

/// A collected row mapping the other direction is a conflict whether or not
/// anything triggers the synthesis (Mark, 2026-10-28): `/srv` is a symlink to
/// `/var/srv` on the image, so the row inverts the base's own layout and is
/// wrong on its own terms.
#[test]
fn selinux_reversed_equivalence_is_a_conflict_with_or_without_a_srv_rule() {
    let cases: Vec<(&str, Vec<FcontextRule>)> = vec![
        ("alone", vec![sel::equivalence("/srv", "/var/srv")]),
        (
            "with a /srv rule",
            vec![
                sel::equivalence("/srv", "/var/srv"),
                sel::label("/srv/www(/.*)?", FcontextFileType::All, HTTPD_TYPE),
            ],
        ),
    ];
    for (case, rules) in cases {
        let snap = sel::snapshot_rules(rules);
        for run in sel::block_runs(&snap) {
            assert!(
                !run.rendered.contains("-a -e"),
                "{case}: a reversed row must emit no equivalence RUN:\n{}",
                run.rendered
            );
        }
        let block = render_block_text(&snap);
        assert!(
            block.contains(sel::UNRENDERABLE_FIXME_INSTRUCTION),
            "{case}: no FIXME:\n{block}"
        );
    }
}

/// Measured on rhel9/rhel-bootc:9.8: once `/var/srv = /srv` is in the store,
/// `semanage fcontext -a` **and** `-m` refuse a spec under `/var/srv`, and
/// rule 1 has no `|| true` after the `-m`, so the build dies. It dies because
/// of an equivalence this block added two lines earlier, so the renderer
/// refuses the rule and hands the operator the rewritten spec instead.
#[test]
fn selinux_rule_under_an_emitted_equivalence_subject_is_refused() {
    const UNDER: &str = "/var/srv/data(/.*)?";
    const REWRITTEN: &str = "/srv/data(/.*)?";
    let snap = sel::snapshot_rules(vec![
        sel::label("/srv/www(/.*)?", FcontextFileType::All, HTTPD_TYPE),
        sel::label(UNDER, FcontextFileType::All, HTTPD_TYPE),
    ]);
    let block = render_block_text(&snap);
    assert!(
        block.contains("-a -e"),
        "the equivalence must be emitted, or there is no collision to refuse:\n{block}"
    );
    for run in sel::block_runs(&snap) {
        assert!(
            !run.rendered.contains(UNDER),
            "the colliding rule rendered a command:\n{}",
            run.rendered
        );
    }
    let cause = block
        .lines()
        .find(|l| l.starts_with(sel::UNRENDERABLE_FIXME_CAUSE_PREFIX))
        .unwrap_or_else(|| panic!("no cause line:\n{block}"));
    assert!(cause.contains(UNDER), "cause names no spec: {cause}");
    assert!(
        cause.contains(REWRITTEN),
        "cause does not hand over the rewritten spec: {cause}"
    );

    // Paired positive: without an emitted equivalence the same rule renders.
    let alone = sel::snapshot_rules(vec![sel::label(UNDER, FcontextFileType::All, HTTPD_TYPE)]);
    assert_eq!(
        rendered_runs(&alone),
        vec![install_run(), sel::rule1_rendered(UNDER, "", HTTPD_ARGS)],
    );
}

/// `selinux_no_restorecon_outside_the_comment_sentence` skips comment lines,
/// and the line this task removed (`# RUN restorecon -Rv /`) was a comment,
/// so restoring it verbatim would pass. This collects every carrier, comment
/// or not, over the block layouts that test's one fixture never produces.
#[test]
fn selinux_restorecon_appears_only_in_its_three_known_sentences() {
    const KNOWN: &[&str] = &[
        "# a rule only needs to be in the policy store: no restorecon at build time. Each",
        "# run restorecon -R on it before starting the services that use it.",
        sel::UNRENDERABLE_FIXME_INSTRUCTION,
    ];
    let fixtures: Vec<(&str, inspectah_core::snapshot::InspectionSnapshot)> = vec![
        ("host rule", host_snapshot()),
        (
            "equivalence only",
            sel::snapshot_rules(vec![sel::equivalence("/var/other", "/other")]),
        ),
        (
            "all unrenderable",
            sel::snapshot_rules(vec![sel::label(
                "/tmp/it's",
                FcontextFileType::All,
                HTTPD_TYPE,
            )]),
        ),
        (
            "sorted source",
            sel::snapshot(
                vec![sel::label(
                    "/opt/a(/.*)?",
                    FcontextFileType::All,
                    HTTPD_TYPE,
                )],
                FcontextSource::SemanageList,
                Vec::new(),
            ),
        ),
        (
            "port label only",
            sel::snapshot(
                Vec::new(),
                FcontextSource::PolicyStore,
                vec![sel::port_label("ssh_port_t", "tcp", "2222")],
            ),
        ),
    ];
    let mut seen = 0usize;
    for (case, snap) in fixtures {
        for line in render_block_text(&snap).lines() {
            if !line.contains("restorecon") {
                continue;
            }
            seen += 1;
            assert!(
                KNOWN.contains(&line),
                "{case}: unexpected restorecon carrier {line:?}"
            );
        }
    }
    assert!(
        seen >= KNOWN.len(),
        "the fixtures reached {seen} carriers, so the check is thin"
    );
}
