//! The machine-state read gate over [`MockExecutor`], and the collectors
//! that consume what it refuses.

use inspectah_collect::executor::mock::{MockExecutor, ReadKind};
use inspectah_collect::inspectors::config::ConfigInspector;
use inspectah_collect::inspectors::nonrpm::scan_unmanaged_files;
use inspectah_core::machine_state::{
    KEYTAB_HEADER_LEN, ScreenBypass, Screened, ScreenedExecutor, refusal,
};
use inspectah_core::traits::executor::{ExecResult, Executor};
use inspectah_core::traits::inspector::{InspectionContext, Inspector, InspectorError, RpmState};
use inspectah_core::traits::progress::NullProgress;
use inspectah_core::types::completeness::SectionData;
use inspectah_core::types::config::ConfigFileKind;
use inspectah_core::types::machine_state::{CarriableMember, MachineStateMember};
use inspectah_core::types::os::OsRelease;
use inspectah_core::types::system::SourceSystem;
use std::path::Path;

const KEY: &str =
    "-----BEGIN ENCRYPTED PRIVATE KEY-----\nMIIFgatemarker\n-----END ENCRYPTED PRIVATE KEY-----\n";

fn host_with_key() -> MockExecutor {
    MockExecutor::new()
        .with_dir("/etc", vec!["pki", "machine-id"])
        .with_dir("/etc/pki", vec!["tls"])
        .with_dir("/etc/pki/tls", vec!["private"])
        .with_dir("/etc/pki/tls/private", vec!["server.key"])
        .with_file("/etc/pki/tls/private/server.key", KEY)
        .with_file("/etc/machine-id", "0123\n")
}

fn ok(stdout: &str) -> ExecResult {
    ExecResult {
        stdout: stdout.into(),
        exit_code: 0,
        ..Default::default()
    }
}

fn pkg_source() -> SourceSystem {
    SourceSystem::PackageBased {
        os_release: OsRelease {
            id: "rhel".into(),
            version_id: "9.4".into(),
            ..Default::default()
        },
    }
}

/// Collect case 25: each refusal kind, what each one reads, and what the
/// gate leaves alone.
#[test]
fn the_gate_refuses_by_path_resolved_path_inode_header_and_content() {
    let exec = host_with_key()
        .with_link("/etc/myapp/id", "/etc/machine-id")
        .with_hard_link("/opt/app/k", "/etc/pki/tls/private/server.key")
        .with_file("/opt/app/creds.pem", KEY)
        .with_file_bytes("/opt/app/kt", &[0x05, 0x02, 0xff, 0x00, 0x41])
        .with_file("/etc/hostname", "host1\n")
        .with_file("/etc/app.conf", "listen = 1\n");
    let gate = ScreenedExecutor::new(&exec);

    let cases = [
        (
            "/etc/pki/tls/private/server.key",
            MachineStateMember::PrivateKeyFile,
        ),
        ("/etc/myapp/id", MachineStateMember::MachineId),
        ("/opt/app/k", MachineStateMember::PrivateKeyFile),
        ("/opt/app/kt", MachineStateMember::KerberosKeytab),
        ("/opt/app/creds.pem", MachineStateMember::PrivateKeyFile),
    ];
    for (path, member) in cases {
        let err = gate.read_file(Path::new(path)).expect_err(path);
        assert_eq!(refusal(&err), Some(member), "{path}");
        assert!(!err.to_string().contains("gatemarker"), "{path}");
    }

    // By path, alias or inode: nothing opened.
    for path in [
        "/etc/pki/tls/private/server.key",
        "/etc/myapp/id",
        "/opt/app/k",
    ] {
        assert!(!exec.was_read(path), "{path} was opened");
    }
    // By header: one head read of exactly the header length.
    let kt: Vec<_> = exec
        .read_log()
        .into_iter()
        .filter(|r| r.path == "/opt/app/kt")
        .collect();
    assert_eq!(kt.len(), 1, "{kt:?}");
    assert_eq!(kt[0].kind, ReadKind::Head);
    assert_eq!(kt[0].bytes, KEYTAB_HEADER_LEN);
    // By content: read in full by the gate, then dropped.
    assert!(
        exec.read_log()
            .iter()
            .any(|r| r.path == "/opt/app/creds.pem" && r.kind == ReadKind::Full)
    );

    assert_eq!(
        gate.screen("/etc/hostname"),
        Screened::Carriable(CarriableMember::Hostname)
    );
    assert_eq!(
        gate.read_file(Path::new("/etc/hostname")).unwrap(),
        "host1\n"
    );
    assert_eq!(gate.screen("/etc/app.conf"), Screened::Ordinary);
    assert_eq!(
        gate.read_file(Path::new("/etc/app.conf")).unwrap(),
        "listen = 1\n"
    );
}

/// The subscription section reads entitlement certificates on purpose,
/// through the one named bypass.
#[test]
fn the_subscription_bypass_reads_what_the_gate_refuses() {
    let cert = "-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----\n";
    let exec = MockExecutor::new()
        .with_dir("/etc/pki", vec!["entitlement"])
        .with_dir("/etc/pki/entitlement", vec!["1.pem"])
        .with_file("/etc/pki/entitlement/1.pem", cert);
    let gate = ScreenedExecutor::new(&exec);
    assert!(
        gate.read_file(Path::new("/etc/pki/entitlement/1.pem"))
            .is_err()
    );
    let raw = gate.bypass(ScreenBypass::SubscriptionSection);
    assert_eq!(
        raw.read_file(Path::new("/etc/pki/entitlement/1.pem"))
            .unwrap(),
        cert
    );
}

/// Collect case 8: a carriable member the scan cannot read is withheld,
/// with its stat, and the section is degraded.
#[test]
fn an_unreadable_carriable_member_is_withheld() {
    let exec = MockExecutor::new()
        .with_dir("/etc", vec!["iscsi"])
        .with_dir("/etc/iscsi", vec!["initiatorname.iscsi"])
        .with_file_error(
            "/etc/iscsi/initiatorname.iscsi",
            std::io::ErrorKind::PermissionDenied,
        )
        .with_file_mode("/etc/iscsi/initiatorname.iscsi", 0o100_600)
        .with_command("dnf history list --reverse", ExecResult::default());
    let rpm_state = RpmState::default();
    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &ScreenedExecutor::new(&exec),
        rpm_state: Some(&rpm_state),
        baseline_data: None,
    };
    let section = match ConfigInspector::new().inspect(&ctx, &NullProgress) {
        Err(InspectorError::Degraded { partial, reason }) => {
            assert!(reason.contains("initiatorname.iscsi"), "{reason}");
            let SectionData::Config(section) = partial.section else {
                panic!("expected a config section");
            };
            section
        }
        other => panic!("expected a degraded section, got {other:?}"),
    };
    assert!(section.files.is_empty());
    let row = &section.withheld[0];
    assert_eq!(row.path, "/etc/iscsi/initiatorname.iscsi");
    assert_eq!(
        row.member,
        MachineStateMember::Carriable(CarriableMember::IscsiInitiatorName)
    );
    assert_eq!(row.mode, Some(0o600));
}

/// Collect cases 4 and 5: the unowned walk reaches a removed package's
/// Insights facts first, and the orphan pass does not admit it again; a
/// keytab the silent `/etc/alternatives/*` glob keeps from the walk is
/// withheld by the orphan branch, unopened.
#[test]
fn the_walk_and_orphan_branches_withhold_members_once() {
    let exec = MockExecutor::new()
        .with_dir("/etc", vec!["rhsm", "alternatives"])
        .with_dir("/etc/rhsm", vec!["facts"])
        .with_dir("/etc/rhsm/facts", vec!["insights-client.facts"])
        .with_file("/etc/rhsm/facts/insights-client.facts", "{\"id\": 1}\n")
        .with_dir("/etc/alternatives", vec!["example.keytab"])
        .with_file(
            "/etc/alternatives/example.keytab",
            "text keytab gatemarker\n",
        )
        .with_command(
            "dnf history list --reverse",
            ok("ID | Command line | Date | Action | Altered\n\
                insights-client 2 | remove | 2024-02-01 | Erase | 1\n\
                example 3 | remove | 2024-02-02 | Erase | 1\n"),
        );
    let rpm_state = RpmState::default();
    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &ScreenedExecutor::new(&exec),
        rpm_state: Some(&rpm_state),
        baseline_data: None,
    };
    let output = ConfigInspector::new()
        .inspect(&ctx, &NullProgress)
        .expect("the inspector succeeds");
    let SectionData::Config(section) = output.section else {
        panic!("expected a config section");
    };
    let rows: Vec<(&str, &ConfigFileKind, Option<&str>)> = section
        .withheld
        .iter()
        .map(|w| (w.path.as_str(), &w.kind, w.package.as_deref()))
        .collect();
    assert_eq!(
        rows,
        vec![
            (
                "/etc/alternatives/example.keytab",
                &ConfigFileKind::Orphaned,
                Some("example")
            ),
            (
                "/etc/rhsm/facts/insights-client.facts",
                &ConfigFileKind::Unowned,
                None
            ),
        ]
    );
    assert!(!exec.was_read("/etc/alternatives/example.keytab"));
    assert!(!exec.was_read("/etc/rhsm/facts/insights-client.facts"));
}

/// Collect case 31, the collect half: a hard link in `/opt` to a private
/// key is a withheld unmanaged row, in no `items` entry; its neighbour is
/// an item.
#[test]
fn the_unmanaged_walk_withholds_a_hard_link_to_a_key() {
    let stat = ExecResult {
        stdout: "12 1700000000 0 0 644\n".into(),
        exit_code: 0,
        ..Default::default()
    };
    let not_owned = ExecResult {
        stdout: "not owned\n".into(),
        exit_code: 1,
        ..Default::default()
    };
    let exec = host_with_key()
        .with_dir("/opt", vec!["app"])
        .with_dir("/opt/app", vec!["k", "app.conf"])
        .with_hard_link("/opt/app/k", "/etc/pki/tls/private/server.key")
        .with_file("/opt/app/app.conf", "a = 1\n")
        .with_dir("/srv", vec![])
        .with_dir("/usr/local", vec![])
        .with_command("stat -c %s %Y %u %g %a /opt/app/app.conf", stat.clone())
        .with_command("stat -c %s %Y %u %g %a /opt/app/k", stat)
        .with_command("rpm -qf /opt/app/app.conf", not_owned.clone())
        .with_command("rpm -qf /opt/app/k", not_owned)
        .with_command("file -b /opt/app/app.conf", ok("ASCII text\n"));
    let gate = ScreenedExecutor::new(&exec);
    let section = scan_unmanaged_files(&gate, &[], &[]);

    let items: Vec<&str> = section.items.iter().map(|i| i.path.as_str()).collect();
    assert_eq!(items, vec!["/opt/app/app.conf"]);
    assert_eq!(section.total_count, 1);
    assert_eq!(section.withheld.len(), 1);
    assert_eq!(section.withheld[0].path, "/opt/app/k");
    assert_eq!(
        section.withheld[0].member,
        MachineStateMember::PrivateKeyFile
    );
    assert!(!exec.was_read("/opt/app/k"));
    assert!(
        !exec
            .command_log()
            .iter()
            .any(|c| c.contains("/opt/app/k")
                && (c.starts_with("sha256sum") || c.starts_with("stat"))),
        "{:?}",
        exec.command_log()
    );
}
