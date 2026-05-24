//! Integration tests for ContainersInspector.
//!
//! Runs the actual Rust inspector on fixture data via MockExecutor and
//! verifies output is structurally correct. Follows the same pattern as
//! parity_test.rs (Slice 2a inspectors).

use inspectah_collect::executor::mock::MockExecutor;
use inspectah_collect::inspectors::containers::ContainersInspector;
use inspectah_core::traits::executor::ExecResult;
use inspectah_core::traits::inspector::{InspectionContext, Inspector, InspectorError};
use inspectah_core::traits::progress::NullProgress;
use inspectah_core::types::completeness::SectionData;
use inspectah_core::types::containers::ContainerSection;
use inspectah_core::types::os::OsRelease;
use inspectah_core::types::system::SourceSystem;

// ── Shared helpers ──────────────────────────────────────────────────

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

// ── Fixtures ────────────────────────────────────────────────────────

const QUADLET_CONTAINER_FIXTURE: &str =
    include_str!("../../testdata/fixtures/containers/webapp.container");
const QUADLET_VOLUME_FIXTURE: &str =
    include_str!("../../testdata/fixtures/containers/webapp-data.volume");
const COMPOSE_FIXTURE: &str = include_str!("../../testdata/fixtures/containers/compose.yaml");
const PODMAN_PS_FIXTURE: &str = include_str!("../../testdata/fixtures/containers/podman-ps.json");
const PODMAN_INSPECT_FIXTURE: &str =
    include_str!("../../testdata/fixtures/containers/podman-inspect.json");
// ── Mock builder ────────────────────────────────────────────────────

fn containers_happy_mock() -> MockExecutor {
    MockExecutor::new()
        // Quadlet system dirs
        .with_dir(
            "/etc/containers/systemd",
            vec!["webapp.container", "webapp-data.volume"],
        )
        .with_file(
            "/etc/containers/systemd/webapp.container",
            QUADLET_CONTAINER_FIXTURE,
        )
        .with_file(
            "/etc/containers/systemd/webapp-data.volume",
            QUADLET_VOLUME_FIXTURE,
        )
        // Other quadlet dirs: empty or missing (NotFound is silent skip)
        .with_dir("/usr/share/containers/systemd", vec![])
        .with_dir("/etc/systemd/system", vec![])
        // User quadlet dirs: needs /etc/passwd for user discovery
        .with_file(
            "/etc/passwd",
            "alice:x:1000:1000:Alice:/home/alice:/bin/bash\n",
        )
        // alice's user quadlet dir does not exist (NotFound -> silent skip)
        // Compose files: place one under /opt
        .with_dir("/opt", vec!["myapp"])
        .with_dir("/opt/myapp", vec!["compose.yaml"])
        .with_file("/opt/myapp/compose.yaml", COMPOSE_FIXTURE)
        // /srv and /etc compose search: empty
        .with_dir("/srv", vec![])
        // Podman running containers
        .with_command(
            "which podman",
            ExecResult {
                stdout: "/usr/bin/podman\n".into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_command(
            "podman ps --format json",
            ExecResult {
                stdout: PODMAN_PS_FIXTURE.into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_command(
            "podman inspect abc123def456",
            ExecResult {
                stdout: PODMAN_INSPECT_FIXTURE.into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        // Flatpak
        .with_command(
            "which flatpak",
            ExecResult {
                stdout: "/usr/bin/flatpak\n".into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_command(
            "flatpak list --app --system --columns=application,origin,branch",
            ExecResult {
                stdout: "org.mozilla.firefox\tflathub\tstable\norg.libreoffice.LibreOffice\tflathub\tstable\norg.gimp.GIMP\tflathub\tstable\n".into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_command(
            "flatpak remote-list --system --columns=name,url",
            ExecResult {
                stdout: "flathub\thttps://dl.flathub.org/repo/\n".into(),
                exit_code: 0,
                ..Default::default()
            },
        )
}

// ── Tests ───────────────────────────────────────────────────────────

/// Happy path: quadlets + compose + podman + flatpak all produce data.
#[test]
fn test_containers_inspector_happy_path() {
    let exec = containers_happy_mock();
    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
        baseline_data: None,
    };

    let result = ContainersInspector::new().inspect(&ctx, &NullProgress);

    // Compose fixture has POSTGRES_PASSWORD, which triggers a degraded hint
    // from the anchor/alias check. Extract section regardless.
    let output = match result {
        Ok(o) => o,
        Err(InspectorError::Degraded { partial, .. }) => *partial,
        Err(e) => panic!("unexpected error: {e}"),
    };

    let section = match &output.section {
        SectionData::Containers(s) => s,
        other => panic!("expected SectionData::Containers, got {:?}", other),
    };

    // Quadlet units
    assert!(
        !section.quadlet_units.is_empty(),
        "inspector must find quadlet units from fixture"
    );
    let has_container = section
        .quadlet_units
        .iter()
        .any(|q| q.name.ends_with(".container"));
    assert!(has_container, "must find .container quadlet unit");
    let has_volume = section
        .quadlet_units
        .iter()
        .any(|q| q.name.ends_with(".volume"));
    assert!(has_volume, "must find .volume quadlet unit");

    // Compose files
    assert!(
        !section.compose_files.is_empty(),
        "inspector must find compose files from fixture"
    );
    assert!(
        !section.compose_files[0].images.is_empty(),
        "compose file must have extracted images"
    );

    // Running containers
    assert!(
        !section.running_containers.is_empty(),
        "inspector must find running containers from podman"
    );
    assert_eq!(
        section.running_containers[0].name, "webapp",
        "container name must match fixture"
    );

    // Flatpak apps
    assert!(
        !section.flatpak_apps.is_empty(),
        "inspector must find flatpak apps from fixture"
    );
    assert_eq!(section.flatpak_apps[0].app_id, "org.mozilla.firefox");
}

/// Empty system: all directories missing -> empty section.
#[test]
fn test_containers_inspector_empty_system() {
    // No quadlet dirs, no compose dirs, no podman, no flatpak.
    let exec = MockExecutor::new()
        // podman not installed
        .with_command(
            "which podman",
            ExecResult {
                stderr: "podman not found".into(),
                exit_code: 1,
                ..Default::default()
            },
        )
        // flatpak not installed
        .with_command(
            "which flatpak",
            ExecResult {
                stderr: "flatpak not found".into(),
                exit_code: 1,
                ..Default::default()
            },
        );

    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
        baseline_data: None,
    };

    let result = ContainersInspector::new().inspect(&ctx, &NullProgress);

    let output = match result {
        Ok(o) => o,
        Err(InspectorError::Degraded { partial, .. }) => *partial,
        Err(e) => panic!("unexpected error: {e}"),
    };

    let section = match &output.section {
        SectionData::Containers(s) => s,
        other => panic!("expected SectionData::Containers, got {:?}", other),
    };

    assert!(
        section.quadlet_units.is_empty(),
        "no quadlet dirs means no units"
    );
    assert!(
        section.compose_files.is_empty(),
        "no compose dirs means no files"
    );
    assert!(
        section.running_containers.is_empty(),
        "podman failure means no containers"
    );
    assert!(section.flatpak_apps.is_empty(), "no flatpak means no apps");
}

/// Podman failure -> Degraded output with warning.
#[test]
fn test_containers_inspector_degraded_podman() {
    let exec = MockExecutor::new()
        // Quadlet dir: PermissionDenied on the primary system dir
        .with_dir_error(
            "/etc/containers/systemd",
            std::io::ErrorKind::PermissionDenied,
        )
        .with_dir("/usr/share/containers/systemd", vec![])
        .with_dir("/etc/systemd/system", vec![])
        // Podman is installed but ps returns bad JSON
        .with_command(
            "which podman",
            ExecResult {
                stdout: "/usr/bin/podman\n".into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_command(
            "podman ps --format json",
            ExecResult {
                stdout: "not valid json".into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        // No flatpak
        .with_command(
            "which flatpak",
            ExecResult {
                exit_code: 1,
                ..Default::default()
            },
        );

    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
        baseline_data: None,
    };

    let result = ContainersInspector::new().inspect(&ctx, &NullProgress);

    match result {
        Err(InspectorError::Degraded { partial, reason }) => {
            assert!(
                reason.contains("Permission denied") || reason.contains("JSON parse error"),
                "degraded reason should mention permission or JSON parse error, got: {reason}"
            );
            assert!(
                matches!(partial.section, SectionData::Containers(_)),
                "partial output should still contain a ContainerSection"
            );
        }
        Ok(_) => panic!(
            "expected Degraded error when quadlet dir is PermissionDenied and podman JSON is bad"
        ),
        Err(e) => panic!("expected Degraded, got: {e}"),
    }
}

/// Output round-trips through ContainerSection type.
#[test]
fn test_containers_inspector_json_roundtrip() {
    let exec = containers_happy_mock();
    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
        baseline_data: None,
    };

    let result = ContainersInspector::new().inspect(&ctx, &NullProgress);

    let output = match result {
        Ok(o) => o,
        Err(InspectorError::Degraded { partial, .. }) => *partial,
        Err(e) => panic!("unexpected error: {e}"),
    };

    let section = match &output.section {
        SectionData::Containers(s) => s,
        other => panic!("expected SectionData::Containers, got {:?}", other),
    };

    let rust_json = serde_json::to_string_pretty(section).unwrap();
    let roundtrip: ContainerSection =
        serde_json::from_str(&rust_json).expect("inspector output must be valid JSON");
    let roundtrip_json = serde_json::to_string_pretty(&roundtrip).unwrap();
    assert_eq!(
        rust_json, roundtrip_json,
        "inspector output must round-trip faithfully through ContainerSection"
    );
}
