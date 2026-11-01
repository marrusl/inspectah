//! Every `COPY <src>` the Containerfile renderer emits has a `<src>` in
//! the export tree rendered alongside it.
//!
//! There is no legitimate case for the other outcome: `podman build` dies
//! at that step every time, naming a path rather than a cause. Two
//! build-breakers were found this way on real hosts rather than by the
//! suite, and they are mirror images of each other, because the two
//! export paths stage different trees from the same renderer:
//!
//! - the refine web download stripped `quadlet/` while still emitting
//!   `COPY quadlet/ /etc/containers/systemd/`;
//! - the CLI scan tarball emitted `COPY language-packages/...` for a
//!   directory only the refine export wrote.
//!
//! A guard over one path would have passed throughout, so both run here.
//!
//! **The fixture is the other half of the guard.** A snapshot with no
//! quadlets, no drop-ins and no language packages satisfies the invariant
//! vacuously, which is how both defects survived two green suites. This
//! one is built from collector output, through the three calls `scan.rs`
//! makes, so the optional directories are populated the way a real host
//! populates them.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::atomic::AtomicBool;

use inspectah_collect::executor::mock::MockExecutor;
use inspectah_collect::inspectors::config::ConfigInspector;
use inspectah_collect::inspectors::containers::ContainersInspector;
use inspectah_collect::inspectors::nonrpm::NonRpmInspector;
use inspectah_collect::inspectors::rpm::RpmInspector;
use inspectah_collect::inspectors::services::ServicesInspector;
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::traits::executor::ExecResult;
use inspectah_core::traits::inspector::Inspector;
use inspectah_core::traits::progress::NullProgress;
use inspectah_core::traits::renderer::RenderContext;
use inspectah_core::types::os::OsRelease;
use inspectah_core::types::system::SourceSystem;
use inspectah_pipeline::collect::collect;
use inspectah_pipeline::redaction::{RedactOptions, redact};
use inspectah_pipeline::render;
use inspectah_pipeline::validate::validate;
use inspectah_refine::session::RefineSession;

// ── The invariant ────────────────────────────────────────────────────

/// Every `COPY` source the Containerfile at `root/Containerfile` names,
/// resolved against `root`. Commented-out lines are excluded: they are
/// suggestions for the operator, not instructions the build runs.
fn copy_sources(containerfile: &str) -> Vec<String> {
    let mut sources = Vec::new();
    for line in containerfile.lines() {
        let Some(rest) = line.strip_prefix("COPY ") else {
            continue;
        };
        // `COPY [--flag]... <src>... <dest>`: everything but the
        // destination is a source. The renderer quotes destinations, never
        // sources, but strip quotes from both so a future quoted source
        // does not read as a missing file with a literal quote in its name.
        let args: Vec<&str> = rest
            .split_whitespace()
            .filter(|a| !a.starts_with("--"))
            .collect();
        if args.len() < 2 {
            continue;
        }
        for src in &args[..args.len() - 1] {
            sources.push(src.trim_matches('\'').trim_matches('"').to_string());
        }
    }
    sources
}

/// Panics naming every COPY source `root` does not contain.
fn assert_every_copy_source_exists(path_label: &str, root: &Path) {
    let containerfile = std::fs::read_to_string(root.join("Containerfile"))
        .unwrap_or_else(|e| panic!("{path_label}: no Containerfile in the export: {e}"));
    let sources = copy_sources(&containerfile);
    assert!(
        !sources.is_empty(),
        "{path_label}: the fixture emitted no COPY lines, so this proves nothing"
    );

    let missing: Vec<&String> = sources
        .iter()
        .filter(|src| !root.join(src).exists())
        .collect();
    assert!(
        missing.is_empty(),
        "{path_label}: the Containerfile COPYs sources the export does not \
         contain, so `podman build` dies at the first of them:\n  {missing:?}\n\
         Export tree roots: {:?}",
        top_level(root)
    );
}

fn top_level(root: &Path) -> BTreeSet<String> {
    std::fs::read_dir(root)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect()
        })
        .unwrap_or_default()
}

// ── The fixture ──────────────────────────────────────────────────────

const QUADLET_DIR: &str = "/etc/containers/systemd";
const QUADLET_UNIT: &str = "/etc/containers/systemd/webapp.container";
const QUADLET_CONTENT: &str =
    "[Container]\nImage=registry.example.com/webapp:1.0\nPublishPort=8080:8080\n";

const DROP_IN: &str = "/etc/systemd/system/httpd.service.d/limits.conf";
const DROP_IN_CONTENT: &str = "[Service]\nLimitNOFILE=65535\n";

const PLAIN_CONFIG: &str = "/etc/httpd/conf/httpd.conf";
const PLAIN_CONFIG_CONTENT: &str = "ServerRoot \"/etc/httpd\"\n";

/// An npm project under `/opt`: a lockfile plus its manifest is the
/// high-confidence case, and the only one that renders an active
/// `COPY language-packages/...` rather than a commented-out suggestion.
const NPM_PROJECT: &str = "/opt/webapp";
const NPM_PACKAGE_JSON: &str = r#"{"name": "webapp", "version": "1.0.0"}"#;
const NPM_LOCK_JSON: &str = r#"{
  "name": "webapp",
  "lockfileVersion": 3,
  "packages": {
    "node_modules/express": { "version": "4.18.2" }
  }
}"#;

const SYSTEMCTL: &str = "UNIT FILE                 STATE     PRESET\n\
                         httpd.service             enabled   enabled\n\
                         \n\
                         1 unit files listed.\n";
const PRESET: &str = "enable httpd.service\n";

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

/// A host that populates the optional export directories: a quadlet, a
/// service drop-in, an ordinary config file, and an npm project.
fn populated_host_mock() -> MockExecutor {
    MockExecutor::new()
        // -- RPM prerequisite for the config walker --------------------
        .with_command(
            "rpm -qa --queryformat %{EPOCH}:%{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}\n",
            ok("0:bash-5.2.26-3.el9.x86_64\n0:nodejs-20.11.0-1.el9.x86_64\n"),
        )
        .with_command("rpm -Va", ok(""))
        .with_command(
            "rpm -qa --queryformat [%{NAME}\\t%{FILENAMES}\\n]",
            ok("bash\t/usr/bin/bash\n"),
        )
        // -- /etc walk -------------------------------------------------
        .with_dir("/etc", vec!["systemd", "httpd", "containers"])
        .with_dir("/etc/systemd", vec!["system"])
        .with_dir("/etc/systemd/system", vec!["httpd.service.d"])
        .with_dir("/etc/systemd/system/httpd.service.d", vec!["limits.conf"])
        .with_file(DROP_IN, DROP_IN_CONTENT)
        .with_dir("/etc/httpd", vec!["conf"])
        .with_dir("/etc/httpd/conf", vec!["httpd.conf"])
        .with_file(PLAIN_CONFIG, PLAIN_CONFIG_CONTENT)
        // -- quadlets --------------------------------------------------
        .with_dir("/etc/containers", vec!["systemd"])
        .with_dir(QUADLET_DIR, vec!["webapp.container"])
        .with_file(QUADLET_UNIT, QUADLET_CONTENT)
        // -- services --------------------------------------------------
        .with_command(
            "systemctl list-unit-files --type=service --no-pager",
            ok(SYSTEMCTL),
        )
        .with_dir("/usr/lib/systemd/system-preset", vec!["90-default.preset"])
        .with_file("/usr/lib/systemd/system-preset/90-default.preset", PRESET)
        .with_command("test -d /etc/systemd/system/httpd.service.d", ok(""))
        // -- npm project -----------------------------------------------
        .with_dir("/opt", vec!["webapp"])
        .with_dir("/opt/webapp", vec!["package.json", "package-lock.json"])
        .with_file("/opt/webapp/package.json", NPM_PACKAGE_JSON)
        .with_file("/opt/webapp/package-lock.json", NPM_LOCK_JSON)
}

/// `collect()` then `validate` then `redact` — the three calls the scan
/// command makes, in its order.
fn scan_like(exec: &MockExecutor) -> InspectionSnapshot {
    let inspectors: Vec<Box<dyn Inspector>> = vec![
        Box::new(RpmInspector::new()),
        Box::new(ConfigInspector::new()),
        Box::new(ServicesInspector::new()),
        Box::new(ContainersInspector),
        Box::new(NonRpmInspector::new()),
    ];
    let collected = collect(
        &pkg_source(),
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

/// Unpack a `.tar.gz` and return the single top-level directory inside
/// it — the export root the Containerfile's COPY paths are relative to.
fn unpack(tarball: &Path, into: &Path) -> std::path::PathBuf {
    let file = std::fs::File::open(tarball).expect("the tarball opens");
    let gz = flate2::read::GzDecoder::new(file);
    tar::Archive::new(gz)
        .unpack(into)
        .expect("the tarball unpacks");

    let mut roots: Vec<std::path::PathBuf> = std::fs::read_dir(into)
        .expect("the unpack directory reads")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .collect();
    assert_eq!(roots.len(), 1, "a tarball has one top-level member");
    roots.pop().expect("one root")
}

// ── The fixture is not vacuous ───────────────────────────────────────

/// Without this, both tests below would pass on a snapshot that emits no
/// optional COPY at all — which is exactly the state in which the two
/// real defects went unnoticed.
#[test]
fn the_fixture_populates_the_optional_export_directories() {
    let snapshot = scan_like(&populated_host_mock());

    let quadlets: Vec<&str> = snapshot
        .containers
        .as_ref()
        .expect("the containers section")
        .quadlet_units
        .iter()
        .map(|q| q.path.as_str())
        .collect();
    // The collector stores the path without its leading slash.
    assert_eq!(
        quadlets,
        vec![QUADLET_UNIT.trim_start_matches('/')],
        "the collector must find the quadlet"
    );

    let drop_ins: Vec<&str> = snapshot
        .services
        .as_ref()
        .expect("the services section")
        .drop_ins
        .iter()
        .map(|d| d.path.as_str())
        .collect();
    assert!(
        drop_ins.contains(&DROP_IN),
        "the collector must find the drop-in, got {drop_ins:?}"
    );

    let manifests: Vec<&str> = snapshot
        .non_rpm_software
        .as_ref()
        .expect("the non-RPM section")
        .items
        .iter()
        .filter(|i| !i.manifest_files.is_empty())
        .map(|i| i.path.as_str())
        .collect();
    assert!(
        manifests
            .iter()
            .any(|p| p.trim_start_matches('/') == NPM_PROJECT.trim_start_matches('/')),
        "the collector must capture the npm manifests, got {manifests:?}"
    );

    let dir = tempfile::tempdir().expect("tempdir");
    render::render_all(&snapshot, &RenderContext { target: None }, dir.path()).expect("render");
    let containerfile =
        std::fs::read_to_string(dir.path().join("Containerfile")).expect("Containerfile");
    for expected in [
        "COPY quadlet/",
        "COPY drop-ins/",
        "COPY language-packages/npm/",
    ] {
        assert!(
            containerfile.contains(expected),
            "the fixture must make the renderer emit `{expected}`, or the \
             invariant holds vacuously over it"
        );
    }
}

// ── Both export paths ────────────────────────────────────────────────

/// The CLI scan tarball. This is the path that emitted
/// `COPY language-packages/...` for a directory only the refine export
/// wrote: the manifest writer lived in the refine crate.
#[test]
fn the_cli_scan_export_satisfies_every_copy_it_emits() {
    let snapshot = scan_like(&populated_host_mock());
    let dir = tempfile::tempdir().expect("tempdir");
    render::render_all(&snapshot, &RenderContext { target: None }, dir.path()).expect("render");

    assert_every_copy_source_exists("CLI scan export", dir.path());
}

/// The refine web download. This is the path that stripped `quadlet/`
/// while still emitting `COPY quadlet/ /etc/containers/systemd/`: a
/// top-level allowlist deleted the directory after the config tree wrote
/// it and before the Containerfile referenced it.
#[test]
fn the_refine_web_export_satisfies_every_copy_it_emits() {
    let snapshot = scan_like(&populated_host_mock());
    let session = RefineSession::new(snapshot);

    let dir = tempfile::tempdir().expect("tempdir");
    let tarball = dir.path().join("export.tar.gz");
    session
        .export_tarball(&tarball, session.generation())
        .expect("the export writes");

    let unpacked = dir.path().join("unpacked");
    std::fs::create_dir_all(&unpacked).expect("mkdir");
    let root = unpack(&tarball, &unpacked);

    assert_every_copy_source_exists("refine web export", &root);
}
