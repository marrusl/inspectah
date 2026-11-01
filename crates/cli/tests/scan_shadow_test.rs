//! A `/etc/sysctl.d/` file that produced sysctl overrides reaches the
//! image through those overrides and through nothing else.
//!
//! These run the `scan` command's own path — `collect()` then `validate`
//! then `redact`, the three calls `scan.rs` makes — and never
//! `run_pipeline()`, because the production scan does not call it and a
//! cross-inspector pass placed outside `collect()` would be skipped by
//! the one path that matters.

use std::sync::atomic::AtomicBool;

use inspectah_collect::executor::mock::MockExecutor;
use inspectah_collect::inspectors::config::ConfigInspector;
use inspectah_collect::inspectors::kernelboot::KernelbootInspector;
use inspectah_collect::inspectors::rpm::RpmInspector;
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::traits::executor::ExecResult;
use inspectah_core::traits::inspector::Inspector;
use inspectah_core::traits::progress::NullProgress;
use inspectah_core::traits::renderer::RenderContext;
use inspectah_core::types::completeness::{Completeness, InspectorId};
use inspectah_core::types::os::OsRelease;
use inspectah_core::types::system::SourceSystem;
use inspectah_pipeline::collect::collect;
use inspectah_pipeline::redaction::{RedactOptions, redact};
use inspectah_pipeline::render;
use inspectah_pipeline::validate::validate;
use inspectah_refine::session::RefineSession;
use inspectah_refine::types::{ItemId, RefineError, RefinementOp};

/// The file whose single key `sysctl -a` reports a different value for.
/// Its keys are the decision; the file itself is inventory.
const SHADOWED: &str = "/etc/sysctl.d/99-driftify.conf";
/// The file whose keys match the running kernel, so it produced no
/// override and stays an ordinary actionable config entry.
const UNSHADOWED: &str = "/etc/sysctl.d/hugepages.conf";
const SHADOWED_KEY: &str = "vm.swappiness";

const SYSCTL_A: &str = "vm.swappiness = 60\nvm.nr_hugepages = 0\n";
const CMDLINE: &str = "BOOT_IMAGE=/vmlinuz root=/dev/mapper/rhel-root ro quiet\n";
const LSMOD: &str = "Module                  Size  Used by\nbridge                307200  0\n";

fn pkg_source() -> SourceSystem {
    SourceSystem::PackageBased {
        os_release: OsRelease {
            name: "Red Hat Enterprise Linux".into(),
            version_id: "9.4".into(),
            id: "rhel".into(),
            ..Default::default()
        },
    }
}

fn ok(stdout: &str) -> ExecResult {
    ExecResult {
        stdout: stdout.into(),
        exit_code: 0,
        ..Default::default()
    }
}

/// A host with two unowned files under `/etc/sysctl.d/`: one whose key
/// the running kernel disagrees with, one whose key it matches.
fn two_sysctl_files_mock() -> MockExecutor {
    MockExecutor::new()
        // -- RPM prerequisite for the config walker --------------------
        .with_command(
            "rpm -qa --queryformat %{EPOCH}:%{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}\n",
            ok("0:bash-5.2.26-3.el9.x86_64\n"),
        )
        .with_command("rpm -Va", ok(""))
        .with_command(
            "rpm -qa --queryformat [%{NAME}\\t%{FILENAMES}\\n]",
            ok("bash\t/usr/bin/bash\n"),
        )
        // -- /etc walk -------------------------------------------------
        .with_dir("/etc", vec!["sysctl.d"])
        .with_dir("/etc/sysctl.d", vec!["99-driftify.conf", "hugepages.conf"])
        .with_file(SHADOWED, "vm.swappiness = 10\n")
        .with_file(UNSHADOWED, "vm.nr_hugepages = 0\n")
        .with_dir("/usr/lib/sysctl.d", vec![])
        // -- kernel/boot -----------------------------------------------
        .with_file("/proc/cmdline", CMDLINE)
        .with_command("lsmod", ok(LSMOD))
        .with_command("sysctl -a", ok(SYSCTL_A))
}

/// `collect()` then `validate` then `redact` — the three calls the scan
/// command makes, in its order.
fn scan_like(exec: &MockExecutor, inspectors: Vec<Box<dyn Inspector>>) -> InspectionSnapshot {
    let source = pkg_source();
    let collected = collect(
        &source,
        exec,
        &inspectors,
        None,
        &NullProgress,
        &AtomicBool::new(false),
    );
    let validated = validate(collected).expect("the snapshot validates");
    let mut snapshot = validated.state.snapshot;
    redact(&mut snapshot, &RedactOptions::default());
    snapshot
}

fn all_three() -> Vec<Box<dyn Inspector>> {
    vec![
        Box::new(RpmInspector::new()),
        Box::new(ConfigInspector::new()),
        Box::new(KernelbootInspector::new()),
    ]
}

/// The snapshot as it is written: serialized, so the assertions read the
/// wire form the refine session and every downstream consumer load.
fn written(snapshot: &InspectionSnapshot) -> serde_json::Value {
    serde_json::to_value(snapshot).expect("the snapshot serializes")
}

fn config_entry<'a>(written: &'a serde_json::Value, path: &str) -> &'a serde_json::Value {
    written["config"]["files"]
        .as_array()
        .expect("the config section carries files")
        .iter()
        .find(|f| f["path"] == path)
        .unwrap_or_else(|| panic!("no config entry for {path}"))
}

fn render_to_dir(snapshot: &InspectionSnapshot) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    render::render_all(snapshot, &RenderContext { target: None }, dir.path()).expect("render");
    dir
}

fn set_include(item_id: ItemId, include: bool) -> RefinementOp {
    RefinementOp::SetInclude { item_id, include }
}

/// Both directions of a toggle on the shadowed file are refused, and the
/// refusal is the 422 the client needs.
fn assert_shadowed_file_is_422(session: &mut RefineSession) {
    for include in [true, false] {
        let before = session.timeline_len();
        let result = session.apply(set_include(
            ItemId::Config {
                path: SHADOWED.into(),
            },
            include,
        ));
        match result {
            Err(RefineError::InventoryNotToggleable(_)) => {}
            other => panic!("SetInclude(Config, {include}) must be refused, got {other:?}"),
        }
        assert_eq!(
            session.timeline_len(),
            before,
            "a refused op must not extend the timeline"
        );
    }
}

fn synthesized(dir: &tempfile::TempDir) -> std::path::PathBuf {
    dir.path()
        .join("sysctl/etc/sysctl.d/99-inspectah-migrated.conf")
}

fn source_file_in_export(dir: &tempfile::TempDir) -> std::path::PathBuf {
    dir.path().join("config/etc/sysctl.d/99-driftify.conf")
}

// ---------------------------------------------------------------------

#[test]
fn a_sysctl_source_file_is_inventory_and_its_keys_are_the_decision() {
    let snapshot = scan_like(&two_sysctl_files_mock(), all_three());
    let written = written(&snapshot);

    let shadowed = config_entry(&written, SHADOWED);
    assert_eq!(
        shadowed["shadowed_by"], "sysctl",
        "the file that produced an override is shadowed by it, got {shadowed}"
    );
    assert_eq!(
        shadowed["disposition"]["kind"], "inventory",
        "a shadowed entry is inventory, got {shadowed}"
    );

    let unshadowed = config_entry(&written, UNSHADOWED);
    assert!(
        unshadowed.get("shadowed_by").is_none(),
        "a file that produced no override is not shadowed, got {unshadowed}"
    );
    assert_eq!(
        unshadowed["disposition"]["kind"], "actionable",
        "an unshadowed entry stays actionable, got {unshadowed}"
    );
}

#[test]
fn degraded_for_an_unrelated_reason_still_shadows() {
    // /proc/cmdline is unreadable. The sysctl files and `sysctl -a`
    // answer normally, so the override list is the criterion and it is
    // non-empty regardless of the section's degraded flag.
    let exec = two_sysctl_files_mock()
        .with_file_error("/proc/cmdline", std::io::ErrorKind::PermissionDenied);
    let snapshot = scan_like(&exec, all_three());

    match &snapshot.completeness {
        Completeness::Partial {
            degraded_sections, ..
        } => assert!(
            degraded_sections.contains(&InspectorId::KernelBoot),
            "an unreadable /proc/cmdline degrades the kernel/boot section"
        ),
        other => panic!("expected Partial completeness, got {other:?}"),
    }
    let overrides = &snapshot
        .kernel_boot
        .as_ref()
        .expect("the degraded section is still present")
        .sysctl_overrides;
    assert_eq!(
        overrides.len(),
        1,
        "the sysctl collection succeeded, so the override list is non-empty"
    );
    assert_eq!(overrides[0].key, SHADOWED_KEY);

    let written = written(&snapshot);
    assert_eq!(
        config_entry(&written, SHADOWED)["shadowed_by"],
        "sysctl",
        "a section degraded for an unrelated reason still shadows"
    );

    // The export carries the synthesized drop-in and not the source file.
    let dir = render_to_dir(&snapshot);
    assert!(
        synthesized(&dir).exists(),
        "the synthesized file is written"
    );
    assert!(
        !source_file_in_export(&dir).exists(),
        "the shadowed source file is not copied"
    );
    let containerfile =
        std::fs::read_to_string(dir.path().join("Containerfile")).expect("Containerfile");
    assert!(
        containerfile.contains("COPY sysctl/etc/sysctl.d/99-inspectah-migrated.conf"),
        "the Containerfile copies the synthesized drop-in, got:\n{containerfile}"
    );
    assert!(
        !containerfile.contains("config/etc/sysctl.d/99-driftify.conf"),
        "nothing names the shadowed source file, got:\n{containerfile}"
    );

    // The key is the decision, and the file is refused — throughout.
    let mut session = RefineSession::new(snapshot);
    assert_shadowed_file_is_422(&mut session);
    let view = inspectah_web::adapter::build_web_view(&session);
    assert!(
        view.sysctls.iter().any(|s| s.key == SHADOWED_KEY),
        "/api/view offers the key as a decision"
    );

    let rendered_keys = |session: &RefineSession| -> String {
        let dir = render_to_dir(&session.snapshot_projected());
        assert!(
            !source_file_in_export(&dir).exists(),
            "the source file stays absent whatever the key decision is"
        );
        std::fs::read_to_string(synthesized(&dir)).unwrap_or_default()
    };

    assert!(rendered_keys(&session).contains(SHADOWED_KEY));
    session
        .apply(set_include(
            ItemId::Sysctl {
                key: SHADOWED_KEY.into(),
            },
            false,
        ))
        .expect("the key is toggleable");
    assert!(!rendered_keys(&session).contains(SHADOWED_KEY));
    assert_shadowed_file_is_422(&mut session);

    session.undo().expect("undo");
    assert!(rendered_keys(&session).contains(SHADOWED_KEY));
    session.redo().expect("redo");
    assert!(!rendered_keys(&session).contains(SHADOWED_KEY));
    assert_shadowed_file_is_422(&mut session);
}

#[test]
fn a_failed_sysctl_collection_leaves_both_files_actionable() {
    // `sysctl -a` fails while /proc/cmdline reads. The section is
    // degraded with an empty override list, so no file is shadowed.
    let exec = two_sysctl_files_mock().with_command(
        "sysctl -a",
        ExecResult {
            exit_code: 1,
            stderr: "sysctl: unavailable".into(),
            ..Default::default()
        },
    );

    for inspectors in [
        all_three(),
        // ... and with the kernel inspector absent, the same.
        vec![
            Box::new(RpmInspector::new()) as Box<dyn Inspector>,
            Box::new(ConfigInspector::new()),
        ],
    ] {
        let kernel_ran = inspectors.len() == 3;
        let snapshot = scan_like(&exec, inspectors);

        if kernel_ran {
            let overrides = &snapshot
                .kernel_boot
                .as_ref()
                .expect("the degraded section is present")
                .sysctl_overrides;
            assert!(
                overrides.is_empty(),
                "a failed sysctl collection produces no overrides"
            );
        }

        let written = written(&snapshot);
        for path in [SHADOWED, UNSHADOWED] {
            let entry = config_entry(&written, path);
            assert!(
                entry.get("shadowed_by").is_none(),
                "{path} must not be shadowed when no override names it, got {entry}"
            );
            assert_eq!(
                entry["disposition"]["kind"], "actionable",
                "{path} stays actionable, got {entry}"
            );
        }

        let dir = render_to_dir(&snapshot);
        assert!(
            source_file_in_export(&dir).exists(),
            "an unshadowed file is copied as plain config"
        );
        assert!(
            !synthesized(&dir).exists(),
            "no overrides means no synthesized drop-in"
        );
        let containerfile =
            std::fs::read_to_string(dir.path().join("Containerfile")).expect("Containerfile");
        assert!(
            !containerfile.contains("99-inspectah-migrated.conf"),
            "no synthesized drop-in is named, got:\n{containerfile}"
        );

        let mut session = RefineSession::new(snapshot);
        let view = inspectah_web::adapter::build_web_view(&session);
        assert!(
            view.sysctls.is_empty(),
            "/api/view offers no sysctl decision"
        );

        session
            .apply(set_include(
                ItemId::Config {
                    path: SHADOWED.into(),
                },
                false,
            ))
            .expect("an unshadowed config file is toggleable");
        let dir = render_to_dir(&session.snapshot_projected());
        assert!(
            !source_file_in_export(&dir).exists(),
            "excluding the file removes it from the export"
        );
    }
}
