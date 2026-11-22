//! The machine-state class, end to end through the scan path.
//!
//! Every host is a [`SysctlHost`] run through the real inspectors over
//! the read gate. Each case checks the snapshot the collectors produced,
//! the mock's read log (what was opened, and how much of it), and every
//! file both export paths write: the scan's own export (`render_all`) and
//! a refine session's export of the scan tarball.

use crate::sysctl_hosts::{
    SysctlHost, export_session, reload, render_export, scan_exec, scan_with, session,
};
use inspectah_collect::executor::mock::{MockExecutor, ReadKind};
use inspectah_collect::inspectors::config::ConfigInspector;
use inspectah_collect::inspectors::containers::ContainersInspector;
use inspectah_collect::inspectors::kernelboot::KernelbootInspector;
use inspectah_collect::inspectors::network::NetworkInspector;
use inspectah_collect::inspectors::nonrpm::NonRpmInspector;
use inspectah_collect::inspectors::rpm::RpmInspector;
use inspectah_collect::inspectors::scheduled::ScheduledTasksInspector;
use inspectah_collect::inspectors::units::UnitsInspector;
use inspectah_core::aggregate::merge_snapshots;
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::traits::inspector::Inspector;
use inspectah_core::types::FindingKind;
use inspectah_core::types::config::ConfigFileKind;
use inspectah_core::types::machine_state::{
    CarriableMember, MachineStateMember, WithheldConfigEntry,
};
use inspectah_core::types::sysctl::SysctlFileBody;

/// A marker run inside every synthetic key, searched for in every
/// artifact in raw, hex and base64 form.
const MARKER: &str = "MSKEYMARKER0123456789";

/// An encrypted private key, the whole of a file, as `openssl pkcs8
/// -topk8` writes it, carrying [`MARKER`]. Not a real key.
fn encrypted_key() -> String {
    format!(
        "-----BEGIN ENCRYPTED PRIVATE KEY-----\n\
         MIIFHDBOBgkqhkiG9w0BBQ0wQTApBgkqhkiG9w0BBQwwHAQIAgICCAIBAQAEggTI\n\
         MIIFHDBOBgkqhkiG9w0B{MARKER}\n\
         -----END ENCRYPTED PRIVATE KEY-----\n"
    )
}

/// A binary keytab: format version 2, then bytes that are not UTF-8 and
/// [`MARKER`].
fn binary_keytab() -> Vec<u8> {
    let mut bytes = vec![0x05, 0x02, 0x00, 0x00, 0x00, 0x3c, 0xff, 0xfe];
    bytes.extend_from_slice(MARKER.as_bytes());
    bytes.extend_from_slice(&[0x80, 0x81]);
    bytes
}

/// A version-2 keytab whose every byte is below `0x80`: an ASCII
/// principal and a synthetic 16-byte key, valid UTF-8 as a whole.
fn ascii_keytab() -> Vec<u8> {
    let mut bytes = vec![0x05, 0x02, 0x00, 0x00, 0x00, 0x40, 0x00, 0x01];
    bytes.extend_from_slice(b"\x00\x0bEXAMPLE.COM\x00\x04host");
    bytes.extend_from_slice(MARKER.as_bytes());
    bytes
}

fn std_inspectors() -> Vec<Box<dyn Inspector>> {
    vec![
        Box::new(RpmInspector::new()),
        Box::new(ConfigInspector::new()),
        Box::new(KernelbootInspector::new()),
    ]
}

fn withheld<'a>(snap: &'a InspectionSnapshot, path: &str) -> Option<&'a WithheldConfigEntry> {
    snap.config
        .as_ref()?
        .withheld
        .iter()
        .find(|w| w.path == path)
}

fn has_entry(snap: &InspectionSnapshot, path: &str) -> bool {
    snap.config
        .as_ref()
        .is_some_and(|c| c.files.iter().any(|f| f.path == path))
}

/// Standard base64 of `bytes`, for the marker search.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..=chunk.len() {
            out.push(ALPHABET[((n >> (18 - 6 * i)) & 0x3f) as usize] as char);
        }
    }
    out
}

/// Every artifact: the snapshot JSON and each file both exports write.
fn artifacts(snap: &InspectionSnapshot) -> Vec<(String, Vec<u8>)> {
    let mut all = vec![(
        "snapshot".to_string(),
        serde_json::to_vec(snap).expect("the snapshot serializes"),
    )];
    for (name, tree) in [
        ("scan export", render_export(snap)),
        ("refine export", export_session(&session(snap))),
    ] {
        let mut dirs = vec![tree.path().to_path_buf()];
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(&dir).expect("an export directory lists") {
                let file = entry.expect("an export entry reads").path();
                if file.is_dir() {
                    dirs.push(file);
                } else if let Ok(bytes) = std::fs::read(&file) {
                    all.push((format!("{name}: {}", file.display()), bytes));
                }
            }
        }
    }
    all
}

/// The marker is in no artifact, raw, as lowercase hex or as base64 at
/// any of the three alignments.
fn assert_marker_nowhere(snap: &InspectionSnapshot) {
    assert_marker_absent(&artifacts(snap));
}

/// [`assert_marker_nowhere`] over artifacts the caller gathered.
fn assert_marker_absent(artifacts: &[(String, Vec<u8>)]) {
    assert_absent(MARKER, artifacts);
}

/// `secret` is in no artifact, raw, hex or base64.
fn assert_absent(secret: &str, artifacts: &[(String, Vec<u8>)]) {
    let raw = secret.as_bytes().to_vec();
    let hex: String = secret.bytes().map(|b| format!("{b:02x}")).collect();
    let mut needles = vec![raw, hex.into_bytes()];
    for pad in 0..3 {
        let mut padded = vec![b'#'; pad];
        padded.extend_from_slice(secret.as_bytes());
        let encoded = base64(&padded);
        // Characters encoding the padding bytes differ by alignment; the
        // stable middle is what any encoding of a file holding the marker
        // contains.
        let start = pad.div_ceil(3) * 4;
        needles.push(encoded.as_bytes()[start..encoded.len() - 4].to_vec());
    }
    for (where_, bytes) in artifacts {
        for needle in &needles {
            assert!(
                !bytes.windows(needle.len()).any(|w| w == needle.as_slice()),
                "{where_} holds {secret}"
            );
        }
    }
}

fn assert_never_opened(exec: &MockExecutor, path: &str) {
    assert!(
        !exec.was_read(path),
        "{path} was opened: {:?}",
        exec.read_log()
    );
}

/// Collect case 1 and blocker 1: an unowned binary keytab at the class
/// path is withheld by its path, never opened, and its bytes are in no
/// artifact.
#[test]
fn a_binary_keytab_is_withheld_by_path_and_never_opened() {
    let exec = SysctlHost::new()
        .file_bytes("/etc/krb5.keytab", &binary_keytab())
        .build();
    let snap = scan_exec(&exec, std_inspectors());

    let row = withheld(&snap, "/etc/krb5.keytab").expect("the keytab is withheld");
    assert_eq!(row.member, MachineStateMember::KerberosKeytab);
    assert_eq!(row.kind, ConfigFileKind::Unowned);
    assert_eq!(row.mode, Some(0o644));
    assert_eq!(row.size, Some(binary_keytab().len() as u64));
    assert!(!has_entry(&snap, "/etc/krb5.keytab"));
    assert_never_opened(&exec, "/etc/krb5.keytab");
    assert_marker_nowhere(&snap);
}

/// Collect case 2: a keytab at a path no rule names is refused by its
/// header, in either encoding, after a two-byte read and before any full
/// read.
#[test]
fn a_keytab_anywhere_is_refused_by_its_header() {
    let exec = SysctlHost::new()
        .file_bytes("/etc/myapp/service.bin", &binary_keytab())
        .file_bytes("/etc/myapp/service2.bin", &ascii_keytab())
        .build();
    let snap = scan_exec(&exec, std_inspectors());

    for path in ["/etc/myapp/service.bin", "/etc/myapp/service2.bin"] {
        let row = withheld(&snap, path).unwrap_or_else(|| panic!("{path} is withheld"));
        assert_eq!(row.member, MachineStateMember::KerberosKeytab, "{path}");
        let reads: Vec<_> = exec
            .read_log()
            .into_iter()
            .filter(|r| r.path == path)
            .collect();
        assert!(!reads.is_empty(), "{path} was sniffed");
        assert!(
            reads
                .iter()
                .all(|r| r.kind == ReadKind::Head && r.bytes == 2),
            "{path}: only the two-byte header was read: {reads:?}"
        );
    }
    assert_marker_nowhere(&snap);
}

/// Collect case 3: text keytabs, the drift tool's fixture form, are
/// withheld by path and never opened.
#[test]
fn text_keytabs_are_withheld_by_path() {
    let exec = SysctlHost::new()
        .file("/etc/krb5.keytab", &format!("text keytab {MARKER}\n"))
        .file("/etc/krb5.keytab.ad", &format!("text keytab {MARKER}\n"))
        .build();
    let snap = scan_exec(&exec, std_inspectors());

    for path in ["/etc/krb5.keytab", "/etc/krb5.keytab.ad"] {
        assert!(withheld(&snap, path).is_some(), "{path} is withheld");
        assert_never_opened(&exec, path);
    }
    assert_marker_nowhere(&snap);
}

/// Blocker 2: the service key files are withheld class members on every
/// branch, by path, and never opened.
#[test]
fn service_key_files_are_withheld_members() {
    let paths = [
        "/etc/chrony.keys",
        "/etc/rndc.key",
        "/etc/rndc.conf",
        "/etc/unbound/unbound_control.key",
        "/etc/unbound/unbound_server.key",
    ];
    let mut host = SysctlHost::new();
    for path in paths {
        host = host.file(path, &format!("key {MARKER}\n"));
    }
    let exec = host.build();
    let snap = scan_exec(&exec, std_inspectors());

    for path in paths {
        let row = withheld(&snap, path).unwrap_or_else(|| panic!("{path} is withheld"));
        assert_eq!(row.member, MachineStateMember::ServiceKey, "{path}");
        assert_never_opened(&exec, path);
    }
    assert_marker_nowhere(&snap);
}

/// Blocker 3: RHEL 9's `/etc/chrony.keys` is a plain `%config` file of
/// `chrony`. Modified, `rpm -Va` reports it, and the branch that reads
/// modified packaged files withholds it without opening it.
#[test]
fn a_modified_packaged_chrony_keys_is_withheld_on_the_rpm_va_branch() {
    let exec = SysctlHost::new()
        .rpm_owned("/etc/chrony.keys", "chrony")
        .rpm_va("S.5....T.", "/etc/chrony.keys")
        .file("/etc/chrony.keys", &format!("1 SHA1 HEX:{MARKER}\n"))
        .build();
    let snap = scan_exec(&exec, std_inspectors());

    let row = withheld(&snap, "/etc/chrony.keys").expect("withheld");
    assert_eq!(row.kind, ConfigFileKind::RpmOwnedModified);
    assert_eq!(row.member, MachineStateMember::ServiceKey);
    assert!(!has_entry(&snap, "/etc/chrony.keys"));
    assert_never_opened(&exec, "/etc/chrony.keys");
    assert_marker_nowhere(&snap);
}

/// Collect case 6: a modified packaged IQN is a carriable member, held
/// and excluded by default.
#[test]
fn a_modified_iqn_is_a_machine_state_entry() {
    let snap = scan_with(
        SysctlHost::new()
            .rpm_owned("/etc/iscsi/initiatorname.iscsi", "iscsi-initiator-utils")
            .rpm_va("S.5....T.", "/etc/iscsi/initiatorname.iscsi")
            .file(
                "/etc/iscsi/initiatorname.iscsi",
                "InitiatorName=iqn.1994-05.com.redhat:host1\n",
            ),
        std_inspectors(),
    );
    let entry = snap
        .config
        .as_ref()
        .unwrap()
        .files
        .iter()
        .find(|f| f.path == "/etc/iscsi/initiatorname.iscsi")
        .expect("the IQN is an entry");
    assert_eq!(entry.kind, ConfigFileKind::RpmOwnedModified);
    assert_eq!(
        entry.disposition,
        FindingKind::MachineState {
            member: CarriableMember::IscsiInitiatorName,
            include: false
        }
    );
    assert!(entry.content.contains("iqn.1994-05"));
}

/// Collect case 9: whole-file private keys in every form the tools write
/// are withheld at an ordinary path; a certificate alone and a
/// configuration file with an embedded key stay ordinary entries.
#[test]
fn whole_file_private_keys_are_withheld_and_embedded_keys_are_not() {
    let pkcs8 = encrypted_key();
    let plain = format!("-----BEGIN PRIVATE KEY-----\nMIIE{MARKER}\n-----END PRIVATE KEY-----\n");
    let ec = format!(
        "-----BEGIN EC PARAMETERS-----\nBggqhkjOPQMBBw==\n-----END EC PARAMETERS-----\n\
         -----BEGIN EC PRIVATE KEY-----\nMHcC{MARKER}\n-----END EC PRIVATE KEY-----\n"
    );
    let pkcs12 = format!(
        "Bag Attributes\n    localKeyID: 01 02\n    friendlyName: server\n\
         Key Attributes: <No Attributes>\n-----BEGIN PRIVATE KEY-----\nMIIE{MARKER}\n\
         -----END PRIVATE KEY-----\nBag Attributes\n    localKeyID: 01 02\n\
         subject=CN = host\nissuer=CN = ca\n-----BEGIN CERTIFICATE-----\nMIIB\n\
         -----END CERTIFICATE-----\n"
    );
    let pkcs12_encrypted = pkcs12.replace("PRIVATE KEY", "ENCRYPTED PRIVATE KEY");
    let cert_then_key =
        format!("-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----\n{plain}");
    let cert_only = "-----BEGIN CERTIFICATE-----\nMIIBcertonly\n-----END CERTIFICATE-----\n";
    let embedded = "listen = 8443\nkey = <<EOF\n-----BEGIN PRIVATE KEY-----\nMIIEembedded\n\
                    -----END PRIVATE KEY-----\nEOF\n";
    let exec = SysctlHost::new()
        .file("/etc/myapp/pkcs8.pem", &pkcs8)
        .file("/etc/myapp/plain.pem", &plain)
        .file("/etc/myapp/ec.pem", &ec)
        .file("/etc/myapp/pkcs12.pem", &pkcs12)
        .file("/etc/myapp/pkcs12-enc.pem", &pkcs12_encrypted)
        .file("/etc/myapp/cert-key.pem", &cert_then_key)
        .file("/etc/myapp/cert.pem", cert_only)
        .file("/etc/myapp/app.conf", embedded)
        .build();
    let snap = scan_exec(&exec, std_inspectors());

    for name in [
        "pkcs8.pem",
        "plain.pem",
        "ec.pem",
        "pkcs12.pem",
        "pkcs12-enc.pem",
        "cert-key.pem",
    ] {
        let path = format!("/etc/myapp/{name}");
        let row = withheld(&snap, &path).unwrap_or_else(|| panic!("{path} is withheld"));
        assert_eq!(row.member, MachineStateMember::PrivateKeyFile, "{path}");
    }
    assert!(
        has_entry(&snap, "/etc/myapp/cert.pem"),
        "a certificate is configuration"
    );
    assert!(
        has_entry(&snap, "/etc/myapp/app.conf"),
        "an embedded key is redaction's"
    );
    assert_marker_nowhere(&snap);
}

/// Collect case 10: aliases. A symlink to a member, a file under a
/// directory symlink to a member directory, a hard link to a member and a
/// symlink to the machine-id are each withheld and opened under neither
/// name.
#[test]
fn aliases_of_members_are_withheld_and_never_opened() {
    let exec = SysctlHost::new()
        .file("/etc/pki/tls/private/server.key", &encrypted_key())
        .file("/etc/machine-id", "0123456789abcdef0123456789abcdef\n")
        .link("/etc/myapp/tls.key", "/etc/pki/tls/private/server.key")
        .link("/etc/myapp/keys", "/etc/pki/tls/private")
        .hard_link("/etc/myapp/server.key", "/etc/pki/tls/private/server.key")
        .link("/etc/myapp/id", "/etc/machine-id")
        .build();
    let snap = scan_exec(&exec, std_inspectors());

    for (path, member) in [
        (
            "/etc/pki/tls/private/server.key",
            MachineStateMember::PrivateKeyFile,
        ),
        ("/etc/myapp/tls.key", MachineStateMember::PrivateKeyFile),
        (
            "/etc/myapp/keys/server.key",
            MachineStateMember::PrivateKeyFile,
        ),
        ("/etc/myapp/server.key", MachineStateMember::PrivateKeyFile),
        ("/etc/myapp/id", MachineStateMember::MachineId),
        ("/etc/machine-id", MachineStateMember::MachineId),
    ] {
        let row = withheld(&snap, path).unwrap_or_else(|| panic!("{path} is withheld"));
        assert_eq!(row.member, member, "{path}");
        assert_never_opened(&exec, path);
    }
    assert_marker_nowhere(&snap);
}

/// Collect case 23: three more collectors read a key through a symlink.
/// The sysctl file's body is withheld and the section complete; `/etc/hosts`
/// gives no additions and a degraded reason; a Compose file gives no row
/// and a degraded reason. The config walk records each as a withheld row,
/// and nothing opens either name.
#[test]
fn other_collectors_store_nothing_from_a_linked_key() {
    let exec = SysctlHost::new()
        .file("/etc/pki/tls/private/server.key", &encrypted_key())
        .link(
            "/etc/sysctl.d/90-local.conf",
            "/etc/pki/tls/private/server.key",
        )
        .link("/etc/myapp/compose.yaml", "/etc/pki/tls/private/server.key")
        .rpm_owned("/etc/hosts", "setup")
        .rpm_va("S.5....T.", "/etc/hosts")
        .link("/etc/hosts", "/etc/pki/tls/private/server.key")
        .sysctl_a("")
        .build();
    let snap = scan_exec(
        &exec,
        vec![
            Box::new(RpmInspector::new()),
            Box::new(ConfigInspector::new()),
            Box::new(KernelbootInspector::new()),
            Box::new(NetworkInspector::new()),
            Box::new(ContainersInspector::new()),
        ],
    );

    let sysctl = snap
        .kernel_boot
        .as_ref()
        .and_then(|k| k.sysctl.as_ref())
        .expect("the sysctl section is present");
    let file = sysctl
        .files
        .iter()
        .find(|f| f.name == "90-local.conf")
        .expect("the sysctl file is listed");
    assert_eq!(
        file.body,
        SysctlFileBody::Withheld {
            member: MachineStateMember::PrivateKeyFile
        }
    );
    let network = snap.network.as_ref().expect("the network section");
    assert!(network.hosts_additions.is_empty());
    let containers = snap.containers.as_ref().expect("the containers section");
    assert!(
        containers
            .compose_files
            .iter()
            .all(|c| c.path != "/etc/myapp/compose.yaml" && c.path != "etc/myapp/compose.yaml")
    );
    for path in [
        "/etc/sysctl.d/90-local.conf",
        "/etc/myapp/compose.yaml",
        "/etc/hosts",
    ] {
        assert!(withheld(&snap, path).is_some(), "{path} is a withheld row");
        assert_never_opened(&exec, path);
    }
    assert_eq!(
        withheld(&snap, "/etc/hosts").unwrap().kind,
        ConfigFileKind::RpmOwnedModified
    );
    assert_never_opened(&exec, "/etc/pki/tls/private/server.key");
    for reason in ["/etc/hosts", "/etc/myapp/compose.yaml"] {
        assert!(
            snap.warnings.iter().any(|w| w
                .message
                .contains(&format!("machine-state file left out: {reason}"))),
            "{reason} is a degraded reason: {:?}",
            snap.warnings
        );
    }
    assert_marker_nowhere(&snap);
}

/// Collect case 29: a `.env` under an explicit scan root, linked to a key,
/// gives one withheld config row, no `env_files` row and no redaction
/// hint, and a degraded reason; neither name is opened.
#[test]
fn an_env_file_linked_to_a_key_is_left_out() {
    let exec = SysctlHost::new()
        .file("/etc/pki/tls/private/server.key", &encrypted_key())
        .link("/etc/myapp/.env", "/etc/pki/tls/private/server.key")
        .build();
    let snap = scan_exec(
        &exec,
        vec![
            Box::new(RpmInspector::new()),
            Box::new(ConfigInspector::new()),
            Box::new(NonRpmInspector::with_roots(vec!["/etc/myapp".into()])),
        ],
    );

    let row = withheld(&snap, "/etc/myapp/.env").expect("the .env is withheld");
    assert_eq!(row.member, MachineStateMember::PrivateKeyFile);
    let nonrpm = snap.non_rpm_software.as_ref().expect("the non-RPM section");
    assert!(nonrpm.env_files.is_empty(), "{:?}", nonrpm.env_files);
    assert!(
        !snap
            .redaction_hints
            .iter()
            .any(|h| h.path.contains("myapp/.env"))
    );
    assert!(
        snap.warnings.iter().any(|w| w
            .message
            .contains("machine-state file left out: /etc/myapp/.env")),
        "{:?}",
        snap.warnings
    );
    assert_never_opened(&exec, "/etc/myapp/.env");
    assert_never_opened(&exec, "/etc/pki/tls/private/server.key");
    assert_marker_nowhere(&snap);
}

/// Collect case 30: an at job hard-linked to a key gives no row; a timer
/// linked to a key gives no row; a readable timer whose service is linked
/// to a key keeps its row with an empty `exec_start`. Each records a
/// degraded reason, and no key name is opened.
#[test]
fn scheduled_collectors_store_nothing_from_a_key() {
    let exec = SysctlHost::new()
        .file("/etc/pki/tls/private/server.key", &encrypted_key())
        .hard_link("/var/spool/at/a00001", "/etc/pki/tls/private/server.key")
        .link(
            "/etc/systemd/system/backup.timer",
            "/etc/pki/tls/private/server.key",
        )
        .file(
            "/etc/systemd/system/sync.timer",
            "[Unit]\nDescription=Sync\n\n[Timer]\nOnCalendar=daily\n",
        )
        .link(
            "/etc/systemd/system/sync.service",
            "/etc/pki/tls/private/server.key",
        )
        .build();
    let snap = scan_exec(
        &exec,
        vec![
            Box::new(RpmInspector::new()),
            Box::new(ConfigInspector::new()),
            Box::new(ScheduledTasksInspector::new()),
        ],
    );

    let scheduled = snap
        .scheduled_tasks
        .as_ref()
        .expect("the scheduled section");
    assert!(scheduled.at_jobs.is_empty(), "{:?}", scheduled.at_jobs);
    assert!(!scheduled.systemd_timers.iter().any(|t| t.name == "backup"));
    let sync = scheduled
        .systemd_timers
        .iter()
        .find(|t| t.name == "sync")
        .expect("the readable timer keeps its row");
    assert!(sync.exec_start.is_empty());
    let reasons = format!("{:?}", snap.warnings);
    for path in [
        "/var/spool/at/a00001",
        "/etc/systemd/system/backup.timer",
        "/etc/systemd/system/sync.service",
    ] {
        assert!(
            reasons.contains(&format!("machine-state file left out: {path}")),
            "{path}: {reasons}"
        );
        assert_never_opened(&exec, path);
    }
    assert_marker_nowhere(&snap);
}

/// The unmanaged walk's path handling is exercised in the collect crate;
/// this checks the gate's own claim that a file it refuses never reaches
/// a collector's text, through a full scan with no class path at all: a
/// whole-file key at an ordinary path is read once by the gate and
/// dropped.
#[test]
fn a_whole_file_key_at_an_ordinary_path_is_read_once_and_dropped() {
    let exec = SysctlHost::new()
        .file("/etc/myapp/server.pem", &encrypted_key())
        .build();
    let snap = scan_exec(&exec, std_inspectors());
    assert!(withheld(&snap, "/etc/myapp/server.pem").is_some());
    assert!(
        exec.read_log()
            .iter()
            .any(|r| r.path == "/etc/myapp/server.pem" && r.kind == ReadKind::Full),
        "the whole-file check reads the file"
    );
    assert_marker_nowhere(&snap);
}

/// Collect case 24, a regression check on today's command names: a full
/// mock scan runs no command that prints a file's bytes with an argument
/// under `/etc`.
#[test]
fn no_command_prints_an_etc_file() {
    const PRINTERS: &[&str] = &[
        "cat", "head", "tail", "base64", "xxd", "od", "dd", "strings",
    ];
    let exec = SysctlHost::new()
        .file("/etc/pki/tls/private/server.key", &encrypted_key())
        .file("/etc/myapp/app.conf", "a = 1\n")
        .build();
    scan_exec(
        &exec,
        vec![
            Box::new(RpmInspector::new()),
            Box::new(ConfigInspector::new()),
            Box::new(KernelbootInspector::new()),
            Box::new(NetworkInspector::new()),
            Box::new(ContainersInspector::new()),
            Box::new(ScheduledTasksInspector::new()),
            Box::new(NonRpmInspector::new()),
        ],
    );
    for command in exec.command_log() {
        let mut words = command.split_whitespace();
        let Some(name) = words.next() else { continue };
        if PRINTERS.contains(&name) {
            assert!(
                !words.any(|w| w.starts_with("/etc")),
                "{command} prints an /etc file"
            );
        }
    }
}

/// One host of a fleet: its hostname and the files `extra` adds.
fn fleet_host(hostname: &str, extra: impl FnOnce(SysctlHost) -> SysctlHost) -> InspectionSnapshot {
    reload(&scan_with(
        extra(SysctlHost::new().file("/etc/hostname", &format!("{hostname}\n"))),
        std_inspectors(),
    ))
}

/// The aggregate of `hosts`, written and loaded again.
fn aggregate(hosts: Vec<InspectionSnapshot>) -> InspectionSnapshot {
    let (merged, _) = merge_snapshots(hosts, None, None).expect("the hosts merge");
    reload(&merged)
}

const IQN: &str = "/etc/iscsi/initiatorname.iscsi";

/// Collect case 21: every class path is one withheld row with both hosts,
/// whatever each host held: the same IQN, different IQNs, one IQN and one
/// unreadable, a key on one host and an ordinary file on the other. No
/// member content reaches the aggregate, and neither export stages it.
#[test]
fn a_fleet_aggregate_withholds_every_class_path() {
    let iqn =
        |value: &'static str| move |h: SysctlHost| h.file(IQN, &format!("InitiatorName={value}\n"));
    let cases: Vec<(&str, InspectionSnapshot, InspectionSnapshot)> = vec![
        (
            "same IQN",
            fleet_host("host-a", iqn("iqn.2000-01.com.example:same")),
            fleet_host("host-b", iqn("iqn.2000-01.com.example:same")),
        ),
        (
            "different IQNs",
            fleet_host("host-a", iqn("iqn.2000-01.com.example:a")),
            fleet_host("host-b", iqn("iqn.2000-01.com.example:b")),
        ),
        (
            "captured and unreadable",
            fleet_host("host-a", iqn("iqn.2000-01.com.example:a")),
            fleet_host("host-b", |h| {
                h.file_error(IQN, std::io::ErrorKind::PermissionDenied)
            }),
        ),
    ];
    for (name, a, b) in cases {
        let merged = aggregate(vec![a, b]);
        let config = merged.config.as_ref().expect("a config section");
        assert!(!config.files.iter().any(|f| f.path == IQN), "{name}");
        let row = config
            .withheld
            .iter()
            .find(|w| w.path == IQN)
            .unwrap_or_else(|| panic!("{name}: the IQN is withheld"));
        assert_eq!(
            row.member,
            MachineStateMember::Carriable(CarriableMember::IscsiInitiatorName),
            "{name}"
        );
        let hosts = &row.aggregate.as_ref().expect("prevalence").hosts;
        assert_eq!(hosts, &["host-a", "host-b"], "{name}");
        let json = serde_json::to_string(&merged).unwrap();
        assert!(
            !json.contains("iqn.2000-01"),
            "{name}: no IQN in the aggregate"
        );
        for (path, tree) in [
            ("scan export", render_export(&merged)),
            ("refine export", export_session(&session(&merged))),
        ] {
            assert!(
                !tree.path().join("config/etc/iscsi").exists(),
                "{name}: {path}"
            );
        }
    }

    let key_and_ordinary = aggregate(vec![
        fleet_host("host-a", |h| {
            h.file("/etc/myapp/server.pem", &encrypted_key())
        }),
        fleet_host("host-b", |h| {
            h.file("/etc/myapp/server.pem", "listen = 1\nROLE=worker\n")
        }),
    ]);
    let config = key_and_ordinary.config.as_ref().unwrap();
    assert!(
        !config
            .files
            .iter()
            .any(|f| f.path == "/etc/myapp/server.pem")
    );
    let row = config
        .withheld
        .iter()
        .find(|w| w.path == "/etc/myapp/server.pem")
        .expect("withheld");
    assert_eq!(row.member, MachineStateMember::PrivateKeyFile);
    assert_eq!(row.aggregate.as_ref().unwrap().hosts, ["host-a", "host-b"]);
    let json = serde_json::to_string(&key_and_ordinary).unwrap();
    assert!(
        !json.contains("ROLE=worker"),
        "host B's content is dropped too"
    );
    assert_marker_nowhere(&key_and_ordinary);
}

const APP_DROP_IN: &str = "/etc/systemd/system/app.service.d/key.conf";
const SERVER_KEY: &str = "/etc/pki/tls/private/server.key";

/// A host whose `app.service` is enabled at its preset, named `hostname`,
/// for the units inspector.
fn app_unit_host(hostname: &str) -> crate::unit_hosts::UnitHost {
    crate::unit_hosts::UnitHost::new()
        .file("/etc/hostname", &format!("{hostname}\n"))
        .unit("app.service", "enabled")
        .preset("enable app.service")
        .vendor_unit(
            "app.service",
            "[Unit]\nDescription=App\n\n[Service]\nExecStart=/usr/bin/app\n\n\
             [Install]\nWantedBy=multi-user.target\n",
            None,
        )
}

/// The RPM, config and units inspectors.
fn unit_inspectors() -> Vec<Box<dyn Inspector>> {
    vec![
        Box::new(RpmInspector::new()),
        Box::new(ConfigInspector::new()),
        Box::new(UnitsInspector::new()),
    ]
}

/// The drop-in rows of `snap` at `path`, selected or alternative.
fn drop_ins_at<'a>(
    snap: &'a InspectionSnapshot,
    path: &str,
) -> Vec<&'a inspectah_core::types::units::SystemdDropIn> {
    snap.units
        .iter()
        .flat_map(|u| u.drop_ins.iter())
        .filter(|d| d.path == path)
        .collect()
}

/// Collect case 11: a drop-in linked to a key, through the units
/// inspector as well as the config walk: no `units.drop_ins` row, a
/// degraded reason naming it, a withheld config row, neither name opened,
/// and no key bytes in the snapshot or either export.
#[test]
fn a_drop_in_linked_to_a_key_is_left_out_by_the_units_inspector() {
    let exec = app_unit_host("host-a")
        .file(SERVER_KEY, &encrypted_key())
        .link(APP_DROP_IN, SERVER_KEY)
        .build();
    let snap = reload(&scan_exec(&exec, unit_inspectors()));
    assert!(snap.units.is_some(), "the units inspector ran");
    assert!(drop_ins_at(&snap, APP_DROP_IN).is_empty());
    assert!(withheld(&snap, APP_DROP_IN).is_some());
    assert!(
        snap.warnings.iter().any(|w| w
            .message
            .contains(&format!("machine-state file left out: {APP_DROP_IN}"))),
        "{:?}",
        snap.warnings
    );
    assert_never_opened(&exec, APP_DROP_IN);
    assert_never_opened(&exec, SERVER_KEY);
    assert_marker_nowhere(&snap);
}

/// Collect case 28: host A's drop-in path is a symlink to a key, host B's
/// the same path holding a regular drop-in, each scanned with the units
/// inspector. Host B's snapshot has its drop-in row before the merge; the
/// aggregate has one withheld row with both hosts and no config entry or
/// drop-in row at the path, host B's content is nowhere, refine refuses
/// the drop-in's include, and neither export stages or copies it.
#[test]
fn a_two_host_drop_in_at_a_class_path_is_withheld_for_the_fleet() {
    use inspectah_refine::types::{ItemId, RefineError, RefinementOp};
    let host_a = reload(&scan_exec(
        &app_unit_host("host-a")
            .file(SERVER_KEY, &encrypted_key())
            .link(APP_DROP_IN, SERVER_KEY)
            .build(),
        unit_inspectors(),
    ));
    let host_b = reload(&scan_exec(
        &app_unit_host("host-b")
            .file(APP_DROP_IN, "[Service]\nEnvironment=ROLE=worker\n")
            .build(),
        unit_inspectors(),
    ));
    let before = drop_ins_at(&host_b, APP_DROP_IN);
    assert_eq!(
        before.len(),
        1,
        "host B's drop-in is a units row before the merge"
    );
    assert!(before[0].content.contains("ROLE=worker"));

    let merged = aggregate(vec![host_a, host_b]);
    let config = merged.config.as_ref().unwrap();
    assert!(!config.files.iter().any(|f| f.path == APP_DROP_IN));
    let row = config
        .withheld
        .iter()
        .find(|w| w.path == APP_DROP_IN)
        .expect("withheld");
    assert_eq!(row.aggregate.as_ref().unwrap().hosts, ["host-a", "host-b"]);
    assert!(
        drop_ins_at(&merged, APP_DROP_IN).is_empty(),
        "no drop-in row after the merge"
    );
    let json = serde_json::to_string(&merged).unwrap();
    assert!(!json.contains("ROLE=worker"));

    let mut fleet = inspectah_refine::session::RefineSession::new(merged.clone());
    let generation = fleet.generation();
    assert!(matches!(
        fleet.apply(RefinementOp::SetInclude {
            item_id: ItemId::DropIn {
                path: APP_DROP_IN.into()
            },
            include: true,
        }),
        Err(RefineError::UnknownTarget(_))
    ));
    assert_eq!(fleet.generation(), generation, "nothing recorded");

    for (path, tree) in [
        ("scan export", render_export(&merged)),
        ("refine export", export_session(&session(&merged))),
    ] {
        assert!(
            !tree
                .path()
                .join("config/etc/systemd/system/app.service.d")
                .exists(),
            "{path}"
        );
        let containerfile = std::fs::read_to_string(tree.path().join("Containerfile")).unwrap();
        assert!(!containerfile.contains("key.conf"), "{path}");
    }
    assert_marker_nowhere(&merged);
}

/// Collect case 16: snapshots edited by hand, as another tool or an
/// edited file would, are refused at load naming the path, and the same
/// values built in process are refused by the one writer. The unedited
/// collector snapshots and an ordinary multi-variant aggregate load.
#[test]
fn validation_refuses_edited_snapshots_and_accepts_collected_ones() {
    use inspectah_core::snapshot::SnapshotError;
    let keytab = scan_with(
        SysctlHost::new().file_bytes("/etc/krb5.keytab", &binary_keytab()),
        std_inspectors(),
    );
    let iqn = scan_with(
        SysctlHost::new()
            .rpm_owned(IQN, "iscsi-initiator-utils")
            .rpm_va("S.5....T.", IQN)
            .file(IQN, "InitiatorName=iqn.1994-05.com.redhat:h\n"),
        std_inspectors(),
    );
    let ec_key = "-----BEGIN EC PARAMETERS-----\nBggq\n-----END EC PARAMETERS-----\n\
                  -----BEGIN EC PRIVATE KEY-----\nMHcC\n-----END EC PRIVATE KEY-----\n";
    for snap in [&keytab, &iqn] {
        reload(snap);
        snap.to_validated_json()
            .expect("a collected snapshot writes");
    }

    let entry = |path: &str, content: &str, include: bool| {
        serde_json::json!({
            "path": path, "kind": "unowned", "category": "other", "content": content,
            "rpm_va_flags": null, "package": null, "diff_against_rpm": null,
            "disposition": {"kind": "actionable", "include": include},
            "variant_selection": "Only", "aggregate": null
        })
    };
    let mut cases: Vec<(&str, serde_json::Value)> = Vec::new();
    for spelling in [
        "etc/ssh/ssh_host_ed25519_key",
        "/etc/ssh/./ssh_host_ed25519_key",
        "/etc//ssh/ssh_host_ed25519_key",
    ] {
        for include in [true, false] {
            let mut v = serde_json::to_value(&keytab).unwrap();
            v["config"]["files"].as_array_mut().unwrap().push(entry(
                spelling,
                "ssh key text",
                include,
            ));
            cases.push((spelling, v));
        }
    }
    for include in [true, false] {
        let mut v = serde_json::to_value(&keytab).unwrap();
        v["config"]["files"].as_array_mut().unwrap().push(entry(
            "/etc/myapp/server.pem",
            ec_key,
            include,
        ));
        cases.push(("/etc/myapp/server.pem", v));
    }
    let mut v = serde_json::to_value(&keytab).unwrap();
    v["units"] = units_with_drop_in("/etc/systemd/system/app.service.d/k.conf", ec_key);
    cases.push(("/etc/systemd/system/app.service.d/k.conf", v));
    let mut v = serde_json::to_value(&iqn).unwrap();
    v["config"]["files"][0]["path"] = "/etc/myapp/x".into();
    cases.push(("/etc/myapp/x", v));
    let mut v = serde_json::to_value(&keytab).unwrap();
    v["config"]["files"]
        .as_array_mut()
        .unwrap()
        .push(entry("/etc/krb5.keytab", "x", false));
    cases.push(("/etc/krb5.keytab", v));
    let mut v = serde_json::to_value(&keytab).unwrap();
    let row = v["config"]["withheld"][0].clone();
    v["config"]["withheld"].as_array_mut().unwrap().push(row);
    cases.push(("/etc/krb5.keytab", v));
    let mut v = serde_json::to_value(&keytab).unwrap();
    v["units"] = units_with_drop_in("/etc/krb5.keytab", "[Service]\n");
    cases.push(("/etc/krb5.keytab", v));

    for (path, value) in cases {
        match inspectah_core::snapshot::InspectionSnapshot::load(&value.to_string()) {
            Err(SnapshotError::MachineState { path: named, .. }) => assert_eq!(
                inspectah_core::machine_state::normal_path(&named),
                inspectah_core::machine_state::normal_path(path),
                "{path}"
            ),
            other => panic!("{path}: expected a machine-state refusal, got {other:?}"),
        }
        let in_process: InspectionSnapshot = serde_json::from_value(value).unwrap();
        assert!(
            matches!(
                in_process.to_validated_json(),
                Err(SnapshotError::MachineState { .. })
            ),
            "{path}: the writer refuses it too"
        );
    }

    // An ordinary multi-variant aggregate repeats a path within
    // config.files, a selected entry and an alternative: accepted.
    let merged = aggregate(vec![
        fleet_host("host-a", |h| h.file("/etc/myapp/app.conf", "a = 1\n")),
        fleet_host("host-b", |h| h.file("/etc/myapp/app.conf", "a = 2\n")),
    ]);
    let at_path = merged
        .config
        .as_ref()
        .unwrap()
        .files
        .iter()
        .filter(|f| f.path == "/etc/myapp/app.conf")
        .count();
    assert_eq!(at_path, 2);
    merged
        .to_validated_json()
        .expect("an ordinary aggregate writes");
}

/// A units section holding one excluded drop-in at `path`.
fn units_with_drop_in(path: &str, content: &str) -> serde_json::Value {
    use inspectah_core::types::units::{SystemdDropIn, UnitSection};
    let drop_in = SystemdDropIn {
        unit: "app.service".into(),
        path: path.into(),
        content: content.into(),
        disposition: FindingKind::excluded(),
        ..Default::default()
    };
    serde_json::to_value(UnitSection {
        drop_ins: vec![drop_in],
        ..Default::default()
    })
    .unwrap()
}

/// The refine export's `x-export-warnings` entries for `snap`.
fn refine_export_warnings(snap: &InspectionSnapshot) -> Vec<String> {
    let session = session(snap);
    let packed = tempfile::tempdir().expect("tempdir");
    session
        .export_plan(session.generation())
        .expect("the generation matches")
        .render(&packed.path().join("export.tar.gz"))
        .expect("the export writes")
        .into_iter()
        .map(|w| w.to_string())
        .collect()
}

/// Collect case 19: a host whose only finding is its keytab stages no
/// config tree and no config `COPY`, and every provisioning surface names
/// `realm join` on both export paths, the Containerfile without paths.
/// With the Insights state as well, each step appears once on each.
#[test]
fn a_keytab_only_host_gets_every_provisioning_surface() {
    let keytab_only = scan_with(
        SysctlHost::new().file_bytes("/etc/krb5.keytab", &binary_keytab()),
        std_inspectors(),
    );
    for (via, tree) in [
        ("scan export", render_export(&keytab_only)),
        ("refine export", export_session(&session(&keytab_only))),
    ] {
        let staged: Vec<_> = walkdir_files(&tree.path().join("config"));
        assert!(
            !staged.iter().any(|p| p.contains("etc/")),
            "{via}: no config tree for /etc: {staged:?}"
        );
        let cf = std::fs::read_to_string(tree.path().join("Containerfile")).unwrap();
        assert!(!cf.contains("COPY config/etc/"), "{via}:\n{cf}");
        let provisioning: Vec<&str> = cf
            .lines()
            .skip_while(|l| *l != "# === Provisioning ===")
            .take_while(|l| !l.is_empty())
            .collect();
        assert_eq!(
            provisioning,
            vec![
                "# === Provisioning ===",
                "# Not in the image: this host's identity. At provisioning:",
                "#   realm join (or ipa-client-install, adcli join)",
            ],
            "{via}:\n{cf}"
        );
        let audit = std::fs::read_to_string(tree.path().join("audit-report.md")).unwrap();
        assert!(audit.contains("## Provisioning steps"), "{via}");
        assert!(
            audit.contains("`ipa-client-install, realm join or adcli join`: `/etc/krb5.keytab`"),
            "{via}:\n{audit}"
        );
        assert!(audit.contains("#### Machine state (1)"), "{via}");
        assert!(audit.contains("contents not captured"), "{via}");
    }
    let readme =
        std::fs::read_to_string(render_export(&keytab_only).path().join("README.md")).unwrap();
    assert!(readme.contains("## Provisioning steps"), "{readme}");
    let warnings = refine_export_warnings(&keytab_only);
    assert!(
        warnings.contains(
            &"Notice: 1 machine-state file left out. At provisioning: realm join.".to_string()
        ),
        "{warnings:?}"
    );

    let with_insights = scan_with(
        SysctlHost::new()
            .file_bytes("/etc/krb5.keytab", &binary_keytab())
            .file("/etc/insights-client/machine-id", "abc\n")
            .file("/etc/rhsm/facts/insights-client.facts", "{}\n"),
        std_inspectors(),
    );
    let tree = render_export(&with_insights);
    let cf = std::fs::read_to_string(tree.path().join("Containerfile")).unwrap();
    assert_eq!(cf.matches("#   realm join").count(), 1);
    assert_eq!(cf.matches("#   insights-client --register").count(), 1);
    let audit = std::fs::read_to_string(tree.path().join("audit-report.md")).unwrap();
    assert_eq!(
        audit
            .matches("- `insights-client --register or rhc connect`")
            .count(),
        1
    );
    let warnings = refine_export_warnings(&with_insights);
    assert!(
        warnings.contains(
            &"Notice: 3 machine-state files left out. At provisioning: realm join, \
              insights-client --register."
                .to_string()
        ),
        "{warnings:?}"
    );
}

/// Collect case 20: a snapshot built in process, never loaded, with a
/// host key at an odd spelling as an included entry, an included ordinary
/// path holding a key, and an included drop-in at a `config.withheld`
/// path. The stager withholds all three as machine-state files and the
/// Containerfile preview names them; both export paths refuse the snapshot
/// and write nothing.
#[test]
fn the_render_guard_withholds_and_the_writers_refuse() {
    use inspectah_core::types::config::{ConfigFileEntry, ConfigSection};
    use inspectah_core::types::units::{SystemdDropIn, UnitSection};
    use inspectah_pipeline::render::configtree::{WithheldWhy, staged_writes};
    let drop_in_path = "/etc/systemd/system/app.service.d/k.conf";
    let snap = InspectionSnapshot {
        config: Some(ConfigSection {
            files: vec![
                ConfigFileEntry {
                    path: "/etc/ssh/./ssh_host_ed25519_key".into(),
                    content: "host key text".into(),
                    ..Default::default()
                },
                ConfigFileEntry {
                    path: "/etc/myapp/server.pem".into(),
                    content: encrypted_key(),
                    ..Default::default()
                },
            ],
            withheld: vec![WithheldConfigEntry {
                path: drop_in_path.into(),
                kind: ConfigFileKind::Unowned,
                package: None,
                member: MachineStateMember::PrivateKeyFile,
                mode: None,
                size: None,
                aggregate: None,
            }],
        }),
        units: Some(UnitSection {
            drop_ins: vec![SystemdDropIn {
                unit: "app.service".into(),
                path: drop_in_path.into(),
                content: "[Service]\nEnvironment=A=1\n".into(),
                ..Default::default()
            }],
            ..Default::default()
        }),
        ..InspectionSnapshot::new()
    };
    let staged = staged_writes(&snap);
    let withheld: Vec<(&str, &WithheldWhy)> = staged
        .withheld
        .iter()
        .map(|(dest, _, why)| (dest.as_str(), why))
        .collect();
    for (path, member) in [
        (
            "/etc/ssh/ssh_host_ed25519_key",
            MachineStateMember::SshHostKey,
        ),
        ("/etc/myapp/server.pem", MachineStateMember::PrivateKeyFile),
        (drop_in_path, MachineStateMember::PrivateKeyFile),
    ] {
        assert!(
            withheld.contains(&(path, &WithheldWhy::MachineState { member })),
            "{path}: {withheld:?}"
        );
    }
    let preview =
        inspectah_pipeline::render::containerfile::render_containerfile(&snap, None, None);
    for path in [
        "/etc/ssh/ssh_host_ed25519_key",
        "/etc/myapp/server.pem",
        drop_in_path,
    ] {
        assert!(
            preview
                .lines()
                .any(|l| l.starts_with("# FIXME") && l.contains(path) && l.contains("not copied")),
            "{path}:\n{preview}"
        );
    }
    let out = tempfile::tempdir().unwrap();
    assert!(
        inspectah_pipeline::render::render_all(
            &snap,
            &inspectah_core::traits::renderer::RenderContext { target: None },
            out.path()
        )
        .is_err()
    );
    assert!(std::fs::read_dir(out.path()).unwrap().next().is_none());
    let tarball = out.path().join("export.tar.gz");
    assert!(
        inspectah_refine::session::render_refine_export(&snap, &tarball, None, None, None, None)
            .is_err()
    );
    assert!(!tarball.exists());
}

/// Every file under `dir`, as a path string; empty when it is absent.
fn walkdir_files(dir: &std::path::Path) -> Vec<String> {
    let mut files = Vec::new();
    let mut dirs = vec![dir.to_path_buf()];
    while let Some(d) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path);
            } else {
                files.push(path.display().to_string());
            }
        }
    }
    files
}

fn set_include(path: &str, include: bool) -> inspectah_refine::types::RefinementOp {
    inspectah_refine::types::RefinementOp::SetInclude {
        item_id: inspectah_refine::types::ItemId::Config { path: path.into() },
        include,
    }
}

/// Collect case 17: a credential is never overridable. The session refuses
/// the include with `NotCarriable` and records nothing, and the view row
/// is `never` with no `item_id`.
#[test]
fn a_credential_override_is_refused() {
    use inspectah_refine::types::RefineError;
    let snap = scan_with(
        SysctlHost::new().file_bytes("/etc/krb5.keytab", &binary_keytab()),
        std_inspectors(),
    );
    let mut session = session(&snap);
    let before = session.generation();
    match session.apply(set_include("/etc/krb5.keytab", true)) {
        Err(RefineError::NotCarriable(why)) => assert!(why.contains("/etc/krb5.keytab"), "{why}"),
        other => panic!("expected NotCarriable, got {other:?}"),
    }
    assert_eq!(session.generation(), before, "nothing recorded");
    assert!(!session.can_undo());
    let view = inspectah_web::adapter::build_web_view(&session);
    let row = view
        .machine_state
        .iter()
        .find(|r| r.path == "/etc/krb5.keytab")
        .expect("a view row");
    assert_eq!(
        serde_json::to_value(row).unwrap()["override"],
        serde_json::json!("never")
    );
    assert!(row.item_id.is_none());
    assert!(
        !view
            .config_files
            .iter()
            .any(|c| c.row.entry.path == "/etc/krb5.keytab")
    );
    let tree = export_session(&session);
    assert!(!tree.path().join("config/etc/krb5.keytab").exists());
}

/// Collect case 18: a carriable override ships the captured file on both
/// paths, with the `warn` notice on the refine export; undo removes it.
#[test]
fn a_carriable_override_ships_the_file_on_both_paths() {
    let snap = scan_with(
        SysctlHost::new()
            .rpm_owned(IQN, "iscsi-initiator-utils")
            .rpm_va("S.5....T.", IQN)
            .file(IQN, "InitiatorName=iqn.1994-05.com.redhat:override\n"),
        std_inspectors(),
    );
    let mut session = session(&snap);
    let view = inspectah_web::adapter::build_web_view(&session);
    let row = view
        .machine_state
        .iter()
        .find(|r| r.path == IQN)
        .expect("a view row");
    assert_eq!(
        serde_json::to_value(row).unwrap()["override"],
        serde_json::json!("warn")
    );
    assert!(!view.config_files.iter().any(|c| c.row.entry.path == IQN));
    session
        .apply(set_include(IQN, true))
        .expect("the include applies");
    assert!(
        inspectah_web::adapter::build_web_view(&session)
            .machine_state
            .iter()
            .any(|r| r.path == IQN && r.include)
    );

    let staged = |tree: &tempfile::TempDir| {
        std::fs::read_to_string(tree.path().join("config/etc/iscsi/initiatorname.iscsi")).ok()
    };
    let refine_tree = export_session(&session);
    assert_eq!(
        staged(&refine_tree).as_deref(),
        Some("InitiatorName=iqn.1994-05.com.redhat:override\n")
    );
    let warnings = {
        let packed = tempfile::tempdir().unwrap();
        session
            .export_plan(session.generation())
            .unwrap()
            .render(&packed.path().join("e.tar.gz"))
            .unwrap()
            .into_iter()
            .map(|w| w.to_string())
            .collect::<Vec<_>>()
    };
    assert!(
        warnings.contains(&format!(
            "Notice: Host identity included in the image: {IQN}"
        )),
        "{warnings:?}"
    );
    // The CLI path: the saved projection rendered with --from-snapshot.
    let projected = reload(&session.snapshot_projected());
    assert_eq!(
        staged(&render_export(&projected)).as_deref(),
        Some("InitiatorName=iqn.1994-05.com.redhat:override\n")
    );

    session.undo().expect("undo");
    assert!(staged(&export_session(&session)).is_none());
}

/// Collect case 27: an `EditVariant` whose content is a key is refused
/// before it is recorded; a session file edited to carry it is refused on
/// resume, naming the path.
#[test]
fn a_credential_edit_is_refused_and_an_edited_session_file_does_not_resume() {
    use inspectah_refine::types::{ItemId, RefineError, RefinementOp, TimelineEntry};
    let cert = |n: u8| format!("-----BEGIN CERTIFICATE-----\nMIIB{n}\n-----END CERTIFICATE-----\n");
    let merged = aggregate(vec![
        fleet_host("host-a", |h| h.file("/etc/myapp/server.pem", &cert(1))),
        fleet_host("host-b", |h| h.file("/etc/myapp/server.pem", &cert(2))),
    ]);
    let edit = RefinementOp::EditVariant {
        item_id: ItemId::Config {
            path: "/etc/myapp/server.pem".into(),
        },
        content: encrypted_key(),
        based_on: None,
    };

    let tree = render_export(&merged);
    let packed = tempfile::tempdir().unwrap();
    let tarball = packed.path().join("fleet.tar.gz");
    inspectah_pipeline::render::tarball::create_tarball(tree.path(), &tarball, "fleet").unwrap();
    let mut session = inspectah_refine::tarball::from_tarball(&tarball).unwrap();
    session.set_tarball_path(tarball.clone());
    match session.apply(edit.clone()) {
        Err(RefineError::NotCarriable(why)) => assert!(why.contains("deploy time"), "{why}"),
        other => panic!("expected NotCarriable, got {other:?}"),
    }
    assert!(!session.can_undo());
    let session_file = inspectah_refine::autosave::session_file_path(&tarball);
    if session_file.exists() {
        let saved = std::fs::read_to_string(&session_file).unwrap();
        assert!(!saved.contains(MARKER), "the autosave holds no key bytes");
    }

    let state = inspectah_refine::autosave::SessionState {
        schema_version: 3,
        tarball_path: tarball.clone(),
        tarball_hash: inspectah_refine::autosave::compute_tarball_hash(&tarball).unwrap(),
        timeline: vec![TimelineEntry::Op(edit)],
        cursor: 1,
        saved_at: "2026-11-21T00:00:00Z".into(),
    };
    inspectah_refine::autosave::save_session(&state, &tarball).unwrap();
    match inspectah_refine::session::RefineSession::resume_from(&tarball) {
        Err(RefineError::SnapshotLoad(why)) => {
            assert!(why.contains("/etc/myapp/server.pem"), "{why}")
        }
        Ok(_) => panic!("an edited session file must not resume"),
        Err(other) => panic!("expected SnapshotLoad, got {other:?}"),
    }
}

/// Collect case 21, the refine half: on a fleet aggregate every class row
/// is `never`, and an include of a withheld configuration path is
/// refused. Case 31's refine half, on a collected unmanaged row, is in
/// `unmanaged_credentials_are_withheld_on_both_export_paths`.
#[test]
fn withheld_rows_are_never_overridable_in_refine() {
    use inspectah_refine::types::RefineError;
    let merged = aggregate(vec![
        fleet_host("host-a", |h| {
            h.file(IQN, "InitiatorName=iqn.2000-01.com.example:a\n")
        }),
        fleet_host("host-b", |h| {
            h.file(IQN, "InitiatorName=iqn.2000-01.com.example:b\n")
        }),
    ]);
    let rows = inspectah_web::adapter::machine_state_rows(&merged);
    assert!(!rows.is_empty());
    for row in &rows {
        assert_eq!(
            serde_json::to_value(row).unwrap()["override"],
            serde_json::json!("never")
        );
        assert!(row.item_id.is_none());
    }
    let mut fleet = inspectah_refine::session::RefineSession::new(merged);
    assert!(matches!(
        fleet.apply(set_include(IQN, true)),
        Err(RefineError::NotCarriable(_))
    ));
}

/// An OpenSSH private key, the whole of a file, carrying [`MARKER`]. Not a
/// real key.
fn openssh_key() -> String {
    format!(
        "-----BEGIN OPENSSH PRIVATE KEY-----\n\
         b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW\n\
         b3BlbnNzaC1rZXktdjEAAAAA{MARKER}\n\
         -----END OPENSSH PRIVATE KEY-----\n"
    )
}

/// A throwaway test key with no other use: `openssl ecparam -name
/// secp224r1 -genkey -noout | openssl ec -no_public`, a complete SEC1 key
/// of 60 base64 characters, under the 64 an unclosed body needs.
const P224_NO_PUBLIC: &str = concat!(
    "-----BEGIN EC PRIVATE KEY-----\n",
    "MCoCAQEEHBxNjpedw/q3FhtEH8z8waY+IPuvElTSh8QdkM2gBwYFK4EEACE=\n",
    "-----END EC PRIVATE KEY-----\n"
);

/// A throwaway test key with no other use: `openssl ecparam -name
/// secp112r1 -genkey -noout | openssl ec -no_public`, the smallest key
/// `openssl ec` writes, 40 base64 characters.
const SECP112R1_NO_PUBLIC: &str = concat!(
    "-----BEGIN EC PRIVATE KEY-----\n",
    "MBwCAQEEDjQhQBBXwaSzSB00/43AoAcGBSuBBAAG\n",
    "-----END EC PRIVATE KEY-----\n"
);

/// An ordinary binary payload: not UTF-8, no credential.
const ORDINARY_BINARY: &[u8] = b"\x7fELF\x02\x01\x01\x00\xff\xfe\x80 ordinary";

/// A PEM private-key block carrying [`MARKER`], each line led by
/// `indent`. Not a real key.
fn pem_key(label: &str, indent: &str) -> String {
    let body = "MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQC7";
    format!(
        "{indent}-----BEGIN {label}-----\n{indent}{body}\n{indent}{MARKER}\n\
         {indent}{body}\n{indent}-----END {label}-----\n"
    )
}

/// A synthetic certificate, base64 but no key.
fn certificate() -> String {
    let line = "MIIDXTCCAkWgAwIBAgIJAKoK/heBjcOuMA0GCSqGSIb3DQEBBQUAMEUxCzAJBgNV\n";
    format!(
        "-----BEGIN CERTIFICATE-----\n{}-----END CERTIFICATE-----\n",
        line.repeat(16)
    )
}

const MIB: usize = 1 << 20;

/// What an unmanaged host holds under `/opt` and `/srv`: credentials with
/// no link to any class path (an independent binary keytab, a whole-file
/// encrypted PKCS#8 key and an OpenSSH key), a hard link to a key
/// (case 31), and next to each, ordinary files, one of them binary, so
/// every `/opt` directory renders a directory `COPY`. Then keys only a
/// read to the end of the file finds: indented in a YAML block scalar,
/// after more than a MiB of text, and split across the payload check's
/// read buffer; keys whose PEM body formatting OpenSSL accepts (form feed
/// and vertical tab before line ends, one character a line with 64
/// trailing spaces), a key in JSON after a quoted decoy marker and
/// about 64 KiB of padding, and two real short EC keys written with no
/// public key, under 64 base64 characters. And ordinary payloads that must ship
/// unchanged: payloads over a MiB that hold PEM blocks or lead with
/// indentation, and private-key markers with no key after them, in JSON,
/// minified JavaScript, a binary and a script's last comment.
fn unmanaged_payloads() -> Vec<(&'static str, Vec<u8>)> {
    let yaml = format!(
        "server:\n  port: 8443\ntls:\n  private_key: |\n{}  verify: true\n",
        pem_key("PRIVATE KEY", "    ")
    );
    let late = format!(
        "{}{}",
        "worker_threads = 4\n".repeat(MIB / 19 + 1),
        pem_key("EC PRIVATE KEY", "")
    );
    // `-----BEGIN` starts five bytes before the buffer ends.
    let split = format!(
        "{}\n{}",
        "#".repeat(inspectah_core::machine_state::PAYLOAD_READ_CHUNK - 6),
        pem_key("RSA PRIVATE KEY", "")
    );
    let crl = format!(
        "-----BEGIN X509 CRL-----\n{}-----END X509 CRL-----\n",
        "MIIBpzCBkAIBATANBgkqhkiG9w0BAQsFADBFMQswCQYDVQQGEwJBVTETMBEGA1UE\n".repeat(MIB / 64 + 1)
    );
    let bundle = certificate().repeat(MIB / certificate().len() + 1);
    let indented = format!(
        "{{\n{}  \"end\": 0\n}}\n",
        "  \"item\": [1, 2, 3],\n".repeat(MIB / 21 + 1)
    );
    let spaced = format!(" {{\"items\":[{}]}}\n", ["0"; 600_000].join(","));
    // A form feed and a vertical tab before line ends, which OpenSSL strips.
    let ff_vt = pem_key("PRIVATE KEY", "")
        .replacen('\n', "\x0c\n", 2)
        .replacen("\n-----END", "\x0b\n-----END", 1);
    // A real-sized body rewrapped one base64 character a line, each line
    // padded with 64 spaces: far longer than any body limit.
    let rewrapped = format!(
        "-----BEGIN RSA PRIVATE KEY-----\n{}-----END RSA PRIVATE KEY-----\n",
        format!("MIIEowIBAAKCAQEA{MARKER}")
            .repeat(45)
            .chars()
            .map(|c| format!("{c}{}\n", " ".repeat(64)))
            .collect::<String>()
    );
    let delimiters =
        br#"{"begin":"-----BEGIN PRIVATE KEY-----","end":"-----END PRIVATE KEY-----"}"#.to_vec();
    let script =
        b"#!/bin/sh\nopenssl pkey -in \"$1\"\n# \"-----BEGIN PRIVATE KEY-----\" is a delimiter"
            .to_vec();
    let minified = br#"function p(t){return"-----BEGIN PRIVATE KEY-----\n"+t+"\n-----END PRIVATE KEY-----\n"}var a="-----BEGIN PRIVATE KEY-----",b="-----END PRIVATE KEY-----";"#.to_vec();
    let mut resource = ORDINARY_BINARY.to_vec();
    resource
        .extend_from_slice(b"\x00-----BEGIN PRIVATE KEY-----\x00-----END PRIVATE KEY-----\x00\xff");
    let mut payloads = vec![
        ("/opt/app/app.conf", b"a = 1\n".to_vec()),
        ("/opt/app/run", ORDINARY_BINARY.to_vec()),
        ("/opt/myapp/client.kt", binary_keytab()),
        ("/opt/myapp/client.pem", encrypted_key().into_bytes()),
        ("/opt/myapp/app.conf", b"server = example\n".to_vec()),
        ("/opt/myapp/lib.so", ORDINARY_BINARY.to_vec()),
        ("/opt/myapp/config.yaml", yaml.into_bytes()),
        ("/opt/myapp/late.conf", late.into_bytes()),
        ("/opt/myapp/split.txt", split.into_bytes()),
        ("/opt/trust/revocations.pem", crl.into_bytes()),
        ("/opt/trust/bundle.pem", bundle.into_bytes()),
        ("/opt/data/items.json", indented.into_bytes()),
        ("/opt/data/compact.json", spaced.into_bytes()),
        ("/srv/data/id_ed25519", openssh_key().into_bytes()),
        ("/srv/data/readme.txt", b"data\n".to_vec()),
        ("/opt/myapp/ff.pem", ff_vt.into_bytes()),
        ("/opt/myapp/rewrapped.pem", rewrapped.into_bytes()),
        ("/opt/myapp/p224.pem", P224_NO_PUBLIC.as_bytes().to_vec()),
        (
            "/opt/myapp/secp112r1.pem",
            SECP112R1_NO_PUBLIC.as_bytes().to_vec(),
        ),
        ("/opt/app/delimiters.json", delimiters),
        ("/opt/app/check.sh", script),
        ("/opt/app/bundle.min.js", minified),
        ("/opt/app/resource.bin", resource),
    ];
    for padding in DECOY_PADDINGS {
        payloads.push((decoy_path(padding), decoy(padding).into_bytes()));
    }
    payloads
}

/// Padding lengths for [`decoy`], around the 64 KiB body limit an
/// earlier payload check had: below it the decoy's candidate ran out
/// inside the real key's `BEGIN` marker.
const DECOY_PADDINGS: [usize; 5] = [65_400, 65_480, 65_500, 65_514, 65_520];

/// Where the decoy with `padding` lives: the names are leaked so the
/// payload table can hold `&'static str` paths.
fn decoy_path(padding: usize) -> &'static str {
    Box::leak(format!("/opt/myapp/decoy-{padding}.json").into_boxed_str())
}

/// Compact JSON whose first property quotes a private-key `BEGIN` marker,
/// then `padding` bytes of base64 letters, then a real key as a JSON
/// string: the earlier marker must not hide the later key.
fn decoy(padding: usize) -> String {
    format!(
        "{{\"header\":\"-----BEGIN PRIVATE KEY-----\",\"padding\":\"{}\",\"key\":\"{}\"}}",
        "A".repeat(padding),
        pem_key("PRIVATE KEY", "").replace('\n', "\\n")
    )
}

/// The scan command's unmanaged path over `exec`: the collectors, then
/// `scan_unmanaged_files` through the same gate kind, as `run_scan` does
/// with `--include-unmanaged`, then the written snapshot loaded again.
fn scan_unmanaged(exec: &MockExecutor) -> InspectionSnapshot {
    let mut snap = scan_exec(exec, std_inspectors());
    let gate = inspectah_core::machine_state::ScreenedExecutor::new(exec);
    snap.unmanaged_files = Some(inspectah_collect::inspectors::nonrpm::scan_unmanaged_files(
        &gate,
        &[],
        &[],
    ));
    reload(&snap)
}

/// Collect case 31 and the unmanaged credential cases, through real
/// payload bundling, scan packaging, the refine export and both
/// Containerfiles' `COPY` sources. Each credential is a withheld row
/// before anything renders, is opened only by the gate, and is in no
/// artifact; each ordinary file beside it, binary included, is carried
/// byte for byte on both paths.
#[test]
fn unmanaged_credentials_are_withheld_on_both_export_paths() {
    use inspectah_refine::types::{ItemId, RefineError, RefinementOp};
    let host_root = tempfile::tempdir().expect("a host root");
    let mut host = SysctlHost::new()
        .file("/etc/pki/tls/private/server.key", &encrypted_key())
        .hard_link("/opt/app/k", "/etc/pki/tls/private/server.key");
    for (path, bytes) in unmanaged_payloads() {
        host = host.file_bytes(path, &bytes);
        let on_disk = host_root.path().join(path.trim_start_matches('/'));
        std::fs::create_dir_all(on_disk.parent().unwrap()).unwrap();
        std::fs::write(on_disk, &bytes).unwrap();
    }
    // A bundler that copied the hard link would find the key there.
    std::fs::write(host_root.path().join("opt/app/k"), encrypted_key()).unwrap();
    let exec = host.build();
    let snap = scan_unmanaged(&exec);

    let unmanaged = snap.unmanaged_files.as_ref().expect("an unmanaged section");
    let withheld: Vec<(&str, MachineStateMember)> = unmanaged
        .withheld
        .iter()
        .map(|w| (w.path.as_str(), w.member))
        .collect();
    assert_eq!(
        withheld,
        vec![
            ("/opt/app/k", MachineStateMember::PrivateKeyFile),
            ("/opt/myapp/client.kt", MachineStateMember::KerberosKeytab),
            ("/opt/myapp/client.pem", MachineStateMember::PrivateKeyFile),
            ("/opt/myapp/config.yaml", MachineStateMember::PrivateKeyFile),
            (
                "/opt/myapp/decoy-65400.json",
                MachineStateMember::PrivateKeyFile
            ),
            (
                "/opt/myapp/decoy-65480.json",
                MachineStateMember::PrivateKeyFile
            ),
            (
                "/opt/myapp/decoy-65500.json",
                MachineStateMember::PrivateKeyFile
            ),
            (
                "/opt/myapp/decoy-65514.json",
                MachineStateMember::PrivateKeyFile
            ),
            (
                "/opt/myapp/decoy-65520.json",
                MachineStateMember::PrivateKeyFile
            ),
            ("/opt/myapp/ff.pem", MachineStateMember::PrivateKeyFile),
            ("/opt/myapp/late.conf", MachineStateMember::PrivateKeyFile),
            ("/opt/myapp/p224.pem", MachineStateMember::PrivateKeyFile),
            (
                "/opt/myapp/rewrapped.pem",
                MachineStateMember::PrivateKeyFile
            ),
            (
                "/opt/myapp/secp112r1.pem",
                MachineStateMember::PrivateKeyFile
            ),
            ("/opt/myapp/split.txt", MachineStateMember::PrivateKeyFile),
            ("/srv/data/id_ed25519", MachineStateMember::PrivateKeyFile),
        ]
    );
    let mut items: Vec<&str> = unmanaged.items.iter().map(|i| i.path.as_str()).collect();
    items.sort_unstable();
    assert_eq!(
        items,
        vec![
            "/opt/app/app.conf",
            "/opt/app/bundle.min.js",
            "/opt/app/check.sh",
            "/opt/app/delimiters.json",
            "/opt/app/resource.bin",
            "/opt/app/run",
            "/opt/data/compact.json",
            "/opt/data/items.json",
            "/opt/myapp/app.conf",
            "/opt/myapp/lib.so",
            "/opt/trust/bundle.pem",
            "/opt/trust/revocations.pem",
            "/srv/data/readme.txt",
        ]
    );
    assert_never_opened(&exec, "/opt/app/k");

    // The scan's export: render, then bundle from the host root.
    let gate = inspectah_core::machine_state::ScreenedExecutor::new(&exec);
    let scan_tree = render_export(&snap);
    inspectah_pipeline::render::unmanaged::bundle_unmanaged_files(
        &unmanaged.items,
        scan_tree.path(),
        host_root.path(),
        &gate,
    )
    .expect("the ordinary payloads bundle");
    // Refine, opened on the packed scan tarball, exports from it.
    let packed = tempfile::tempdir().unwrap();
    let tarball = packed.path().join("scan.tar.gz");
    inspectah_pipeline::render::tarball::create_tarball(scan_tree.path(), &tarball, "host")
        .expect("the scan tarball packs");
    let mut session = inspectah_refine::tarball::from_tarball(&tarball).expect("refine opens it");
    session.set_tarball_path(tarball.clone());
    let refine_tree = export_session(&session);

    for (via, root) in [
        ("scan export", scan_tree.path()),
        ("refine export", refine_tree.path()),
    ] {
        crate::copy_source_exists_test::assert_every_copy_source_exists(via, root);
        let cf = std::fs::read_to_string(root.join("Containerfile")).unwrap();
        for dir in ["/opt/app/", "/opt/myapp/"] {
            let rel = dir.trim_start_matches('/');
            assert!(
                cf.contains(&format!("COPY unmanaged/{rel} {dir}")),
                "{via}: a directory COPY for {dir}:\n{cf}"
            );
        }
        for (path, _) in &withheld {
            let rel = path.trim_start_matches('/');
            assert!(
                !root.join("unmanaged").join(rel).exists(),
                "{via}: no payload for {path}"
            );
            assert!(
                !cf.lines().any(|l| !l.starts_with('#') && l.contains(path)),
                "{via}: {path} is in no instruction:\n{cf}"
            );
        }
        for (path, bytes) in unmanaged_payloads() {
            if items.contains(&path) {
                let staged = std::fs::read(root.join("unmanaged").join(&path[1..])).ok();
                assert_eq!(staged.as_deref(), Some(bytes.as_slice()), "{via}: {path}");
            }
        }
        let audit = std::fs::read_to_string(root.join("audit-report.md")).unwrap();
        assert!(audit.contains("/opt/myapp/client.pem"), "{via}:\n{audit}");
        assert!(
            audit.contains("deliver it at deploy time"),
            "{via}:\n{audit}"
        );
    }
    let mut all = vec![(
        "snapshot".to_string(),
        serde_json::to_vec(&snap).expect("the snapshot serializes"),
    )];
    for root in [scan_tree.path(), refine_tree.path()] {
        all.extend(
            walkdir_files(root)
                .into_iter()
                .filter_map(|f| std::fs::read(&f).ok().map(|b| (f, b))),
        );
    }
    assert_marker_absent(&all);
    for key in [P224_NO_PUBLIC, SECP112R1_NO_PUBLIC] {
        assert_absent(key.lines().nth(1).expect("a key line"), &all);
    }

    // The refine half: a collected withheld row, never overridable.
    let view = inspectah_web::adapter::build_web_view(&session);
    let row = view
        .machine_state
        .iter()
        .find(|r| r.path == "/opt/app/k")
        .expect("a view row for the hard link");
    assert_eq!(
        serde_json::to_value(row).unwrap()["override"],
        serde_json::json!("never")
    );
    assert!(row.item_id.is_none());
    let before = session.generation();
    for path in ["/opt/app/k", "/opt/myapp/client.kt"] {
        match session.apply(RefinementOp::SetInclude {
            item_id: ItemId::UnmanagedFile { path: path.into() },
            include: true,
        }) {
            Err(RefineError::NotCarriable(_)) => {}
            other => panic!("{path}: expected NotCarriable, got {other:?}"),
        }
    }
    assert_eq!(session.generation(), before, "nothing recorded");
    let warnings: Vec<String> = {
        let out = tempfile::tempdir().unwrap();
        session
            .export_plan(session.generation())
            .unwrap()
            .render(&out.path().join("e.tar.gz"))
            .unwrap()
            .into_iter()
            .map(|w| w.to_string())
            .collect()
    };
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("deliver the private keys at deploy time")),
        "{warnings:?}"
    );
}

/// The inventory reaches members below directory links and below the
/// old depth limit, through a full scan: a certificate-only consumer file
/// in a directory `/etc/pki/consumer/linked` links to, registration JSON
/// in a directory `/etc/insights-client/.state` links to, and a hard link
/// to an entitlement file eleven directories down. None is PEM key
/// content, so only the inventory can withhold them. Each physical path
/// is a withheld row, never opened, and its content is in no artifact.
#[test]
fn members_below_directory_links_and_deep_trees_are_withheld() {
    let deep = "/etc/pki/entitlement/a/b/c/d/e/f/g/h/i/j";
    let cert = format!("-----BEGIN CERTIFICATE-----\nMIIB{MARKER}\n-----END CERTIFICATE-----\n");
    let exec = SysctlHost::new()
        .file("/etc/myapp/consumer/cert.pem", &cert)
        .link("/etc/pki/consumer/linked", "/etc/myapp/consumer")
        .file(
            "/etc/myapp/state/registration.json",
            &format!("{{\"machine_id\": \"{MARKER}\"}}\n"),
        )
        .link("/etc/insights-client/.state", "/etc/myapp/state")
        .file(
            &format!("{deep}/ent.json"),
            &format!("{{\"pool\": \"{MARKER}\"}}\n"),
        )
        .hard_link("/etc/myapp/ent.json", &format!("{deep}/ent.json"))
        .build();
    let snap = reload(&scan_exec(&exec, std_inspectors()));
    for (path, member) in [
        (
            "/etc/myapp/consumer/cert.pem",
            MachineStateMember::RhsmConsumer,
        ),
        (
            "/etc/myapp/state/registration.json",
            MachineStateMember::InsightsState,
        ),
        ("/etc/myapp/ent.json", MachineStateMember::RhsmEntitlement),
    ] {
        let row = withheld(&snap, path).unwrap_or_else(|| panic!("{path} is withheld"));
        assert_eq!(row.member, member, "{path}");
        assert!(!has_entry(&snap, path), "{path}");
        assert_never_opened(&exec, path);
    }
    assert_marker_nowhere(&snap);
}

/// A link back to its own directory changes what the rule matches below
/// it: with `/etc/insights-client/.state -> .`, the registration JSON
/// there is also `.state/registration.json`, an Insights state member,
/// though its own name matches no rule. The inventory lists the
/// directory again under the new matching state, so the physical file,
/// the linked spelling and a hard link in `/etc/myapp` are all withheld,
/// none is opened, and the JSON is in no artifact of either export.
#[test]
fn a_link_that_gives_files_a_member_spelling_is_inventoried() {
    let exec = SysctlHost::new()
        .file(
            "/etc/insights-client/registration.json",
            &format!("{{\"machine_id\": \"{MARKER}\"}}\n"),
        )
        .link("/etc/insights-client/.state", ".")
        .hard_link(
            "/etc/myapp/registration.json",
            "/etc/insights-client/registration.json",
        )
        .build();
    let gate = inspectah_core::machine_state::ScreenedExecutor::new(&exec);
    assert_eq!(gate.gap(), None);
    let spellings = [
        "/etc/insights-client/registration.json",
        "/etc/insights-client/.state/registration.json",
        "/etc/myapp/registration.json",
    ];
    for path in spellings {
        assert_eq!(
            gate.screen(path),
            inspectah_core::machine_state::Screened::Withheld(MachineStateMember::InsightsState),
            "{path}"
        );
    }
    let snap = reload(&scan_exec(&exec, std_inspectors()));
    for path in [
        "/etc/insights-client/registration.json",
        "/etc/myapp/registration.json",
    ] {
        let row = withheld(&snap, path).unwrap_or_else(|| panic!("{path} is withheld"));
        assert_eq!(row.member, MachineStateMember::InsightsState, "{path}");
    }
    for path in spellings {
        assert!(!has_entry(&snap, path), "{path}");
        assert_never_opened(&exec, path);
    }
    assert_marker_nowhere(&snap);
}

/// Two private bind mounts of `/srv/view` at `/etc/insights-client/.a`
/// and `.b`, with `/etc/myapp/registration.json` bind-mounted over the
/// registration file in `.b` alone. Both views share `/srv/view`'s inode,
/// so a walk that took `.a`'s listing for `.b`'s never saw the file
/// mount, and the registration JSON reached the snapshot as an ordinary
/// file. The mounts are modelled in the mock (real ones take root): each
/// view has its own mount id and entries. The file is a withheld row, is
/// never opened, and is in no artifact of either export.
#[test]
fn a_file_mounted_in_one_bind_view_is_inventoried() {
    let registration = "/etc/myapp/registration.json";
    let exec = SysctlHost::new()
        .file("/srv/view/registration.json", "{}\n")
        .file(registration, &format!("{{\"machine_id\": \"{MARKER}\"}}\n"))
        .hard_link(
            "/etc/insights-client/.a/registration.json",
            "/srv/view/registration.json",
        )
        .hard_link("/etc/insights-client/.b/registration.json", registration)
        .build()
        .with_bind_mount("/etc/insights-client/.a", "/srv/view", 2)
        .with_bind_mount("/etc/insights-client/.b", "/srv/view", 3);
    let snap = reload(&scan_exec(&exec, std_inspectors()));
    let row = withheld(&snap, registration).expect("the mounted file is withheld");
    assert_eq!(row.member, MachineStateMember::InsightsState);
    assert!(!has_entry(&snap, registration));
    assert_never_opened(&exec, registration);
    assert_marker_nowhere(&snap);
    for root in [render_export(&snap), export_session(&session(&snap))] {
        let files: Vec<(String, Vec<u8>)> = walkdir_files(root.path())
            .into_iter()
            .filter_map(|f| std::fs::read(&f).ok().map(|b| (f, b)))
            .collect();
        assert_marker_absent(&files);
    }
}

/// With `--preserve subscription --ack-sensitive`, the subscription
/// section's hostname is read through the gate, not its bypass: an
/// `/etc/hostname` linked to an SSH host key leaves the hostname unset,
/// and neither the key nor an error quoting it is in the snapshot, whose
/// subscription section validation does not check.
#[test]
fn the_subscription_hostname_is_read_through_the_gate() {
    use inspectah_collect::inspectors::subscription::SubscriptionInspector;
    let key = "/etc/ssh/ssh_host_ed25519_key";
    let exec = SysctlHost::new()
        .file(key, &openssh_key())
        .link("/etc/hostname", key)
        .build();
    let mut inspectors = std_inspectors();
    inspectors.push(Box::new(SubscriptionInspector::new()));
    let snap = scan_exec(&exec, inspectors);
    let subscription = snap.subscription.as_ref().expect("a subscription section");
    assert_eq!(subscription.source_hostname, None);
    assert_never_opened(&exec, key);
    assert_never_opened(&exec, "/etc/hostname");
    let json = serde_json::to_string(&snap).unwrap();
    assert!(!json.contains("OPENSSH PRIVATE KEY"), "{json}");
    assert_marker_nowhere(&snap);
}

/// The RPM section's two readers of host files report a refusal: a
/// `.repo` file linked to a key gives no repo row, and a `gpgkey=file://`
/// reference to a withheld key file gives no key row. Each is a degraded
/// reason naming the path, the section stays, partial, with the ordinary
/// repo, and neither key name is opened.
#[test]
fn refused_repo_and_key_files_degrade_the_rpm_section() {
    use inspectah_core::types::completeness::{Completeness, InspectorId};
    const KEY: &str = "/etc/pki/tls/private/server.key";
    let exec = SysctlHost::new()
        .file(KEY, &encrypted_key())
        .link("/etc/yum.repos.d/linked.repo", KEY)
        .file(
            "/etc/yum.repos.d/app.repo",
            &format!("[app]\nname=app\nbaseurl=https://example.invalid/app\ngpgkey=file://{KEY}\n"),
        )
        .build();
    let snap = reload(&scan_exec(&exec, std_inspectors()));
    let rpm = snap.rpm.as_ref().expect("the RPM section stays");
    let repos: Vec<&str> = rpm.repo_files.iter().map(|r| r.path.as_str()).collect();
    assert_eq!(repos, vec!["/etc/yum.repos.d/app.repo"]);
    assert!(rpm.gpg_keys.iter().all(|k| k.path != KEY), "no key row");
    assert!(
        matches!(
            &snap.completeness,
            Completeness::Partial { degraded_sections, .. } if degraded_sections.contains(&InspectorId::Rpm)
        ),
        "{:?}",
        snap.completeness
    );
    for path in ["/etc/yum.repos.d/linked.repo", KEY] {
        assert!(
            snap.warnings.iter().any(|w| w
                .message
                .contains(&format!("machine-state file left out: {path}"))),
            "{path} is a degraded reason: {:?}",
            snap.warnings
        );
        assert_never_opened(&exec, path);
    }
    assert_marker_nowhere(&snap);
}

/// Both export paths' provisioning surfaces for `snap`, by name: each
/// Containerfile and audit report, the scan's README and its stderr
/// notices, and the refine export's warnings.
fn provisioning_surfaces(
    snap: &InspectionSnapshot,
    session: &inspectah_refine::session::RefineSession,
) -> Vec<(&'static str, String)> {
    let read = |tree: &tempfile::TempDir, name: &str| {
        std::fs::read_to_string(tree.path().join(name)).unwrap_or_default()
    };
    let scan = render_export(snap);
    let refine = export_session(session);
    let packed = tempfile::tempdir().unwrap();
    let warnings: Vec<String> = session
        .export_plan(session.generation())
        .unwrap()
        .render(&packed.path().join("e.tar.gz"))
        .unwrap()
        .into_iter()
        .map(|w| w.to_string())
        .collect();
    let notices: Vec<String> = inspectah_pipeline::render::machine_state::export_notices(snap)
        .into_iter()
        .map(|w| w.to_string())
        .collect();
    vec![
        ("scan Containerfile", read(&scan, "Containerfile")),
        ("scan audit report", read(&scan, "audit-report.md")),
        ("scan README", read(&scan, "README.md")),
        ("scan notices", notices.join("\n")),
        ("refine Containerfile", read(&refine, "Containerfile")),
        ("refine audit report", read(&refine, "audit-report.md")),
        ("refine warnings", warnings.join("\n")),
    ]
}

/// Text that only a provisioning step or left-out notice writes.
const PROVISIONING_MARKS: &[&str] = &[
    "# === Provisioning ===",
    "## Provisioning steps",
    "At provisioning:",
];

/// A host whose class files are all recreated automatically (machine-id,
/// an SSH host key, the hostname, an IQN) owes no step, so no
/// provisioning section or notice appears on any surface of either
/// export path, while the audit report still lists the rows.
#[test]
fn an_automatic_only_host_has_no_provisioning_notice_anywhere() {
    let snap = scan_with(
        SysctlHost::new()
            .file("/etc/machine-id", "0123456789abcdef\n")
            .file("/etc/ssh/ssh_host_ed25519_key", &openssh_key())
            .file("/etc/hostname", "host-a\n")
            .file(IQN, "InitiatorName=iqn.1994-05.com.redhat:auto\n"),
        std_inspectors(),
    );
    assert!(
        inspectah_pipeline::render::machine_state::provisioning_steps(&snap).is_empty(),
        "no row owes a step"
    );
    assert!(withheld(&snap, "/etc/machine-id").is_some());
    let session = session(&snap);
    for (surface, text) in provisioning_surfaces(&snap, &session) {
        for mark in PROVISIONING_MARKS {
            assert!(!text.contains(mark), "{surface} has {mark:?}:\n{text}");
        }
        if surface.ends_with("audit report") {
            assert!(text.contains("#### Machine state ("), "{surface}:\n{text}");
        }
    }
    assert_marker_nowhere(&snap);
}

/// A `plain` override: a collected `/etc/rhsm/syspurpose/syspurpose.json`
/// owes `subscription-manager register` until it is included. Included
/// in refine, both export paths stage the captured bytes, every `COPY`
/// source exists, the step is gone from every surface (an overridden row
/// owes nothing), and no `warn` notice is added.
#[test]
fn a_plain_override_ships_the_file_and_owes_nothing_on_both_paths() {
    const SYSPURPOSE: &str = "/etc/rhsm/syspurpose/syspurpose.json";
    const CONTENT: &str = "{\"role\": \"Red Hat Enterprise Linux Server\"}\n";
    let snap = scan_with(
        SysctlHost::new().file(SYSPURPOSE, CONTENT),
        std_inspectors(),
    );
    let mut session = session(&snap);
    let row = inspectah_web::adapter::build_web_view(&session)
        .machine_state
        .into_iter()
        .find(|r| r.path == SYSPURPOSE)
        .expect("a view row");
    assert_eq!(
        serde_json::to_value(&row).unwrap()["override"],
        serde_json::json!("plain")
    );
    for (surface, text) in provisioning_surfaces(&snap, &session) {
        if surface != "scan README"
            && surface != "scan audit report"
            && surface != "refine audit report"
        {
            assert!(
                text.contains("subscription-manager register"),
                "{surface} owes the step before the include:\n{text}"
            );
        }
    }

    session
        .apply(set_include(SYSPURPOSE, true))
        .expect("the include applies");
    let projected = reload(&session.snapshot_projected());
    let staged = |tree: &tempfile::TempDir| {
        std::fs::read_to_string(tree.path().join(format!("config{SYSPURPOSE}"))).ok()
    };
    let refine_tree = export_session(&session);
    let scan_tree = render_export(&projected);
    for (via, tree) in [("refine export", &refine_tree), ("scan export", &scan_tree)] {
        assert_eq!(staged(tree).as_deref(), Some(CONTENT), "{via}");
        crate::copy_source_exists_test::assert_every_copy_source_exists(via, tree.path());
    }
    for (surface, text) in provisioning_surfaces(&projected, &session) {
        for mark in PROVISIONING_MARKS {
            assert!(!text.contains(mark), "{surface} has {mark:?}:\n{text}");
        }
        assert!(
            !text.contains("Host identity included"),
            "{surface}: a plain override adds no warn notice"
        );
    }
}
