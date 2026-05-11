//! Redaction coverage tests for new inspector surfaces (services, storage, kernelboot).
//!
//! These tests plant known secrets in each new section and verify the redaction
//! engine detects and (where high-confidence) redacts them inline.

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::kernelboot::{ConfigSnippet, KernelBootSection};
use inspectah_core::types::redaction::{FindingKind, RedactionState};
use inspectah_core::types::services::{ServiceSection, SystemdDropIn};
use inspectah_core::types::storage::{CredentialRef, FstabEntry, StorageSection};
use inspectah_pipeline::redaction::engine::{redact, RedactOptions};

// ---------------------------------------------------------------------------
// Test 1: Systemd drop-in with Environment= secret
// ---------------------------------------------------------------------------

#[test]
fn test_dropin_env_secret_redacted() {
    let mut snapshot = InspectionSnapshot::new();
    snapshot.services = Some(ServiceSection {
        drop_ins: vec![SystemdDropIn {
            unit: "myapp.service".into(),
            path: "/etc/systemd/system/myapp.service.d/override.conf".into(),
            content: "[Service]\nEnvironment=DB_PASSWORD=secret123\n".into(),
            include: true,
            ..Default::default()
        }],
        ..Default::default()
    });

    redact(&mut snapshot, &RedactOptions::default());

    // Secret value must be redacted inline
    let services = snapshot.services.as_ref().unwrap();
    assert!(
        !services.drop_ins[0].content.contains("secret123"),
        "secret value must be redacted from drop-in content"
    );
    assert!(
        services.drop_ins[0].content.contains("REDACTED_"),
        "drop-in content must contain redaction token"
    );

    // Finding must be recorded
    assert!(
        !snapshot.redactions.is_empty(),
        "redactions must contain findings from drop-in scan"
    );
    assert!(
        snapshot
            .redactions
            .iter()
            .any(|f| f.path.contains("myapp.service")),
        "finding must reference the drop-in path"
    );
}

// ---------------------------------------------------------------------------
// Test 2: Mount options with credentials= path flagged
// ---------------------------------------------------------------------------

#[test]
fn test_mount_credential_option_flagged() {
    let mut snapshot = InspectionSnapshot::new();
    snapshot.storage = Some(StorageSection {
        fstab_entries: vec![FstabEntry {
            device: "//server/share".into(),
            mount_point: "/mnt/cifs".into(),
            fstype: "cifs".into(),
            options: "credentials=/etc/cifs-creds,uid=1000".into(),
            ..Default::default()
        }],
        ..Default::default()
    });

    redact(&mut snapshot, &RedactOptions::default());

    // Credential reference in mount options must produce a finding
    assert!(
        !snapshot.redactions.is_empty(),
        "credential mount option must produce a finding"
    );
    assert!(
        snapshot.redactions.iter().any(|f| f.path == "/etc/fstab"
            && f.finding_kind == Some(FindingKind::GenericCredential)),
        "finding must be GenericCredential for credential mount option"
    );
}

// ---------------------------------------------------------------------------
// Test 3: Mount options with password= flagged
// ---------------------------------------------------------------------------

#[test]
fn test_mount_password_option_flagged() {
    let mut snapshot = InspectionSnapshot::new();
    snapshot.storage = Some(StorageSection {
        fstab_entries: vec![FstabEntry {
            device: "//server/share".into(),
            mount_point: "/mnt/smb".into(),
            fstype: "cifs".into(),
            options: "password=hunter2,uid=1000".into(),
            ..Default::default()
        }],
        ..Default::default()
    });

    redact(&mut snapshot, &RedactOptions::default());

    assert!(
        !snapshot.redactions.is_empty(),
        "password mount option must produce a finding"
    );
}

// ---------------------------------------------------------------------------
// Test 4: Credential refs flagged
// ---------------------------------------------------------------------------

#[test]
fn test_credential_ref_flagged() {
    let mut snapshot = InspectionSnapshot::new();
    snapshot.storage = Some(StorageSection {
        credential_refs: vec![CredentialRef {
            mount_point: "/mnt/backup".into(),
            credential_path: "/etc/backup-creds".into(),
            source: "fstab".into(),
        }],
        ..Default::default()
    });

    redact(&mut snapshot, &RedactOptions::default());

    assert!(
        !snapshot.redactions.is_empty(),
        "credential ref must produce a finding"
    );
    assert!(
        snapshot.redactions.iter().any(|f| f
            .remediation
            .contains("/etc/backup-creds")),
        "finding remediation must reference the credential path"
    );
}

// ---------------------------------------------------------------------------
// Test 5: Kernel cmdline with password= redacted
// ---------------------------------------------------------------------------

#[test]
fn test_cmdline_password_redacted() {
    let mut snapshot = InspectionSnapshot::new();
    snapshot.kernel_boot = Some(KernelBootSection {
        cmdline: "quiet crashkernel=auto password=hunter2 rd.lvm.lv=vg/root".into(),
        ..Default::default()
    });

    redact(&mut snapshot, &RedactOptions::default());

    // Password value must be redacted
    let kb = snapshot.kernel_boot.as_ref().unwrap();
    assert!(
        !kb.cmdline.contains("hunter2"),
        "password value must be redacted from cmdline"
    );
    assert!(
        kb.cmdline.contains("REDACTED_"),
        "cmdline must contain redaction token"
    );

    assert!(
        !snapshot.redactions.is_empty(),
        "cmdline password must produce a finding"
    );
    assert!(
        snapshot
            .redactions
            .iter()
            .any(|f| f.path == "/proc/cmdline"),
        "finding must reference /proc/cmdline"
    );
}

// ---------------------------------------------------------------------------
// Test 6: Dracut config snippet with secret scanned
// ---------------------------------------------------------------------------

#[test]
fn test_dracut_config_secret_scanned() {
    let mut snapshot = InspectionSnapshot::new();
    snapshot.kernel_boot = Some(KernelBootSection {
        dracut_conf: vec![ConfigSnippet {
            path: "/etc/dracut.conf.d/iscsi.conf".into(),
            content: "iscsi_password=mysecret\n".into(),
        }],
        ..Default::default()
    });

    redact(&mut snapshot, &RedactOptions::default());

    // Secret must be redacted inline
    let kb = snapshot.kernel_boot.as_ref().unwrap();
    assert!(
        !kb.dracut_conf[0].content.contains("mysecret"),
        "secret must be redacted from dracut config"
    );

    assert!(
        !snapshot.redactions.is_empty(),
        "dracut config secret must produce a finding"
    );
}

// ---------------------------------------------------------------------------
// Test 7: Modprobe config with secret scanned
// ---------------------------------------------------------------------------

#[test]
fn test_modprobe_config_secret_scanned() {
    let mut snapshot = InspectionSnapshot::new();
    snapshot.kernel_boot = Some(KernelBootSection {
        modprobe_d: vec![ConfigSnippet {
            path: "/etc/modprobe.d/custom.conf".into(),
            content: "options mymod secret=topSecret42\n".into(),
        }],
        ..Default::default()
    });

    redact(&mut snapshot, &RedactOptions::default());

    let kb = snapshot.kernel_boot.as_ref().unwrap();
    assert!(
        !kb.modprobe_d[0].content.contains("topSecret42"),
        "secret must be redacted from modprobe config"
    );
    assert!(
        !snapshot.redactions.is_empty(),
        "modprobe config secret must produce a finding"
    );
}

// ---------------------------------------------------------------------------
// Test 8: Tuned custom profile with secret scanned
// ---------------------------------------------------------------------------

#[test]
fn test_tuned_profile_secret_scanned() {
    let mut snapshot = InspectionSnapshot::new();
    snapshot.kernel_boot = Some(KernelBootSection {
        tuned_custom_profiles: vec![ConfigSnippet {
            path: "/etc/tuned/custom/tuned.conf".into(),
            content: "[main]\napi_key=sk-12345abcde\n".into(),
        }],
        ..Default::default()
    });

    redact(&mut snapshot, &RedactOptions::default());

    let kb = snapshot.kernel_boot.as_ref().unwrap();
    assert!(
        !kb.tuned_custom_profiles[0]
            .content
            .contains("sk-12345abcde"),
        "secret must be redacted from tuned profile"
    );
    assert!(
        !snapshot.redactions.is_empty(),
        "tuned profile secret must produce a finding"
    );
}

// ---------------------------------------------------------------------------
// Test 9: All three surfaces combined — full secrets review
// ---------------------------------------------------------------------------

#[test]
fn test_secrets_review_reports_all_findings() {
    let mut snapshot = InspectionSnapshot::new();

    // Services: drop-in with password
    snapshot.services = Some(ServiceSection {
        drop_ins: vec![SystemdDropIn {
            unit: "db.service".into(),
            path: "/etc/systemd/system/db.service.d/env.conf".into(),
            content: "[Service]\nEnvironment=DB_PASSWORD=pass1\n".into(),
            include: true,
            ..Default::default()
        }],
        ..Default::default()
    });

    // Storage: credential ref + fstab with credentials=
    snapshot.storage = Some(StorageSection {
        fstab_entries: vec![FstabEntry {
            device: "//nas/share".into(),
            mount_point: "/mnt/nas".into(),
            fstype: "cifs".into(),
            options: "credentials=/root/.smbcreds".into(),
            ..Default::default()
        }],
        credential_refs: vec![CredentialRef {
            mount_point: "/mnt/nas".into(),
            credential_path: "/root/.smbcreds".into(),
            source: "fstab".into(),
        }],
        ..Default::default()
    });

    // Kernelboot: cmdline password + dracut secret
    snapshot.kernel_boot = Some(KernelBootSection {
        cmdline: "quiet password=bootpass".into(),
        dracut_conf: vec![ConfigSnippet {
            path: "/etc/dracut.conf.d/net.conf".into(),
            content: "token=abc123\n".into(),
        }],
        ..Default::default()
    });

    redact(&mut snapshot, &RedactOptions::default());

    // At least: 1 drop-in + 1 fstab credential option + 1 credential ref + 1 cmdline + 1 dracut
    assert!(
        snapshot.redactions.len() >= 5,
        "expected at least 5 findings across all surfaces, got {}",
        snapshot.redactions.len()
    );

    // Verify inline redaction happened
    let services = snapshot.services.as_ref().unwrap();
    assert!(
        !services.drop_ins[0].content.contains("pass1"),
        "services secret must be redacted"
    );

    let kb = snapshot.kernel_boot.as_ref().unwrap();
    assert!(
        !kb.cmdline.contains("bootpass"),
        "cmdline secret must be redacted"
    );
}

// ---------------------------------------------------------------------------
// Test 10: Clean surfaces produce no false positives
// ---------------------------------------------------------------------------

#[test]
fn test_clean_surfaces_no_false_positives() {
    let mut snapshot = InspectionSnapshot::new();

    snapshot.services = Some(ServiceSection {
        drop_ins: vec![SystemdDropIn {
            unit: "httpd.service".into(),
            path: "/etc/systemd/system/httpd.service.d/limits.conf".into(),
            content: "[Service]\nLimitNOFILE=65535\n".into(),
            include: true,
            ..Default::default()
        }],
        ..Default::default()
    });

    snapshot.storage = Some(StorageSection {
        fstab_entries: vec![FstabEntry {
            device: "/dev/sda1".into(),
            mount_point: "/boot".into(),
            fstype: "xfs".into(),
            options: "defaults".into(),
            ..Default::default()
        }],
        ..Default::default()
    });

    snapshot.kernel_boot = Some(KernelBootSection {
        cmdline: "quiet crashkernel=auto rd.lvm.lv=vg/root".into(),
        dracut_conf: vec![ConfigSnippet {
            path: "/etc/dracut.conf.d/lvm.conf".into(),
            content: "add_dracutmodules+=\" lvm \"\n".into(),
        }],
        ..Default::default()
    });

    redact(&mut snapshot, &RedactOptions::default());

    assert!(
        snapshot.redactions.is_empty(),
        "clean surfaces must produce zero findings, got: {:?}",
        snapshot.redactions
    );

    // Trust state should be FullyRedacted (no findings = clean)
    match &snapshot.redaction_state {
        Some(RedactionState::FullyRedacted { .. }) => {}
        other => panic!("expected FullyRedacted for clean snapshot, got {other:?}"),
    }
}
