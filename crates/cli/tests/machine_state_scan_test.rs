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
        "-----BEGIN ENCRYPTED PRIVATE KEY-----\nMIIFHDBOBgkqhkiG9w0B{MARKER}\n\
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
    let raw = MARKER.as_bytes().to_vec();
    let hex: String = MARKER.bytes().map(|b| format!("{b:02x}")).collect();
    let mut needles = vec![raw, hex.into_bytes()];
    for pad in 0..3 {
        let mut padded = vec![b'#'; pad];
        padded.extend_from_slice(MARKER.as_bytes());
        let encoded = base64(&padded);
        // Characters encoding the padding bytes differ by alignment; the
        // stable middle is what any encoding of a file holding the marker
        // contains.
        let start = pad.div_ceil(3) * 4;
        needles.push(encoded.as_bytes()[start..encoded.len() - 4].to_vec());
    }
    for (where_, bytes) in artifacts(snap) {
        for needle in &needles {
            assert!(
                !bytes.windows(needle.len()).any(|w| w == needle.as_slice()),
                "{where_} holds the key marker"
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

/// Collect case 28, the configuration half: host A's drop-in path is a
/// symlink to a key, host B's the same path holding a regular drop-in.
/// The aggregate has one withheld row with both hosts and no entry at
/// the path, and host B's content is nowhere.
#[test]
fn a_two_host_drop_in_at_a_class_path_is_withheld_for_the_fleet() {
    const DROP_IN: &str = "/etc/systemd/system/app.service.d/key.conf";
    let merged = aggregate(vec![
        fleet_host("host-a", |h| {
            h.file("/etc/pki/tls/private/server.key", &encrypted_key())
                .link(DROP_IN, "/etc/pki/tls/private/server.key")
        }),
        fleet_host("host-b", |h| {
            h.file(DROP_IN, "[Service]\nEnvironment=ROLE=worker\n")
        }),
    ]);
    let config = merged.config.as_ref().unwrap();
    assert!(!config.files.iter().any(|f| f.path == DROP_IN));
    let row = config
        .withheld
        .iter()
        .find(|w| w.path == DROP_IN)
        .expect("withheld");
    assert_eq!(row.aggregate.as_ref().unwrap().hosts, ["host-a", "host-b"]);
    let json = serde_json::to_string(&merged).unwrap();
    assert!(!json.contains("ROLE=worker"));
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
