//! The scan tarball and an unchanged refine export agree on the
//! not-carried class.
//!
//! An auto package that nothing the image installs needs is left out of
//! the image on both export paths, and the two paths reach that by
//! different code: the scan applies `apply_scan_package_defaults` to its
//! snapshot, while refine starts from its own tier defaults and the
//! not-carried list is read from whatever dispositions result. Nothing
//! but this test holds the two together, so it compares what each path
//! writes for one collected host:
//!
//! - the `dnf install` package names, with the two out-of-class packages
//!   (one `anaconda`-sourced, one repo-less) removed from both sides;
//! - the `systemctl` lines and the `# Omitted:` lines;
//! - `inspectah-var.conf`;
//! - the `# Not carried:` Containerfile block and the `### Not carried`
//!   audit table, compared whole.
//!
//! **Scope.** Parity is claimed only for a classified, baseline-backed
//! snapshot: one with leaf data from the collector and a base image, so
//! refine sorts ordinary added packages into the Site tier. Every other
//! known difference between the two paths is outside this test and must
//! not be read into it: the handling of anaconda-sourced and repo-less
//! packages (refine's anaconda classifier and its repo-less trust gate
//! decide those, by design), snapshots with no baseline (refine
//! classifies their added packages Investigate), aggregate snapshots,
//! uploads, the refine export's allowlist and payload re-extraction, and
//! everything outside the package, service and `/var` surfaces above.
//! Widening the claim to cover those is a separate piece of work.
//!
//! The fixture is collector output, built through the calls `scan.rs`
//! makes in its order, with no disposition set by hand. A companion test
//! fails if the fixture stops producing a not-carried list or a
//! baseline, so the comparisons cannot pass over nothing.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use inspectah_collect::executor::mock::MockExecutor;
use inspectah_collect::inspectors::rpm::RpmInspector;
use inspectah_collect::inspectors::rpm::repoless::scan_dnf_cache_for_repoless;
use inspectah_collect::inspectors::services::ServicesInspector;
use inspectah_collect::inspectors::storage::StorageInspector;
use inspectah_core::baseline::{BaselineData, BaselinePackageEntry};
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
use inspectah_pipeline::render::not_carried::{
    NOT_CARRIED_REASON, apply_scan_package_defaults, not_carried_packages,
};
use inspectah_pipeline::validate::validate;
use inspectah_refine::session::RefineSession;

// ── The fixture ──────────────────────────────────────────────────────

/// Packages the base image ships. Present on the host too, so they are
/// filtered out of `packages_added` and named in `baseline_package_names`.
const BASE_PACKAGES: &[&str] = &["bash", "systemd"];

/// The one user-installed package. It pulls in `httpd-core` (owns the
/// user-enabled `httpd.service`) and `httpd-filesystem` (owns
/// `/var/lib/httpd`).
const LEAF: &str = "httpd";

/// The not-carried packages, in display order: the two owning an enabled
/// unit first, then the rest, each part by name. `firewalld` owns the
/// preset-enabled `firewalld.service` and `/var/lib/firewalld`; `rhc` owns
/// `rhcd.service`, which the operator enabled against its preset; only
/// `firewalld` needs `python3-firewall`.
const EXPECTED_NOT_CARRIED: &[&str] = &["firewalld", "rhc", "python3-firewall"];

/// Auto packages outside the class: one installed at system install time,
/// one installed from a local file. Each path handles these its own way by
/// design, so they are removed from the `dnf install` comparison and only
/// checked to be absent from both not-carried lists.
const ANACONDA_PACKAGE: &str = "langpacks-en";
const REPOLESS_PACKAGE: &str = "vendor-agent";
const OUT_OF_CLASS: &[&str] = &[ANACONDA_PACKAGE, REPOLESS_PACKAGE];

/// Every package on the host: (name, arch, source repo).
const HOST_PACKAGES: &[(&str, &str, &str)] = &[
    ("bash", "x86_64", "baseos"),
    ("systemd", "x86_64", "baseos"),
    ("httpd", "x86_64", "appstream"),
    ("httpd-core", "x86_64", "appstream"),
    ("httpd-filesystem", "noarch", "appstream"),
    ("firewalld", "noarch", "baseos"),
    ("python3-firewall", "noarch", "baseos"),
    ("rhc", "x86_64", "baseos"),
    ("langpacks-en", "noarch", "anaconda"),
    ("vendor-agent", "x86_64", "@commandline"),
];

/// What `dnf repoquery --requires --resolve --recursive --installed`
/// answers per added package. Unlisted packages need nothing added.
const RECURSIVE_REQUIRES: &[(&str, &str)] = &[
    (
        "httpd.x86_64",
        "httpd-core.x86_64\nhttpd-filesystem.noarch\nbash.x86_64\nsystemd.x86_64\n",
    ),
    (
        "firewalld.noarch",
        "python3-firewall.noarch\nsystemd.x86_64\n",
    ),
];

/// `rpm -qa` file lists, in the sentinel format the RPM inspector reads.
const FILE_OWNERSHIP: &str = "@@bash\n/usr/bin/bash\n\
    @@httpd-core\n/usr/lib/systemd/system/httpd.service\n\
    @@httpd-filesystem\n/var/lib/httpd\n\
    @@firewalld\n/usr/lib/systemd/system/firewalld.service\n/var/lib/firewalld\n\
    @@rhc\n/usr/lib/systemd/system/rhcd.service\n";

/// `/var` directories, with the package that owns each.
const VAR_DIRS: &[(&str, &str)] = &[
    ("/var/lib/httpd", "httpd-filesystem"),
    ("/var/lib/firewalld", "firewalld"),
];

const SYSTEMCTL: &str = "UNIT FILE                 STATE     PRESET\n\
                         firewalld.service         enabled   enabled\n\
                         httpd.service             enabled   disabled\n\
                         rhcd.service              enabled   disabled\n\
                         \n\
                         3 unit files listed.\n";
const PRESET: &str = "enable firewalld.service\ndisable *\n";

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

/// The base image's package list, as baseline extraction records it.
fn baseline() -> BaselineData {
    let packages = HOST_PACKAGES
        .iter()
        .filter(|(name, _, _)| BASE_PACKAGES.contains(name))
        .map(|(name, arch, _)| {
            (
                format!("{name}.{arch}"),
                BaselinePackageEntry {
                    name: (*name).into(),
                    epoch: None,
                    version: "1.0".into(),
                    release: "1.el9".into(),
                    arch: (*arch).into(),
                },
            )
        })
        .collect();
    BaselineData {
        image_digest: "sha256:0000".into(),
        packages,
        extracted_at: "2026-11-09T00:00:00Z".into(),
    }
}

/// The host: RPM, services and storage, the three inspectors the class
/// reads from.
fn host_mock() -> MockExecutor {
    let rpm_qa: String = HOST_PACKAGES
        .iter()
        .map(|(name, arch, _)| format!("0:{name}-1.0-1.el9.{arch}\n"))
        .collect();
    let from_repo: String = HOST_PACKAGES
        .iter()
        .map(|(name, _, repo)| format!("{name} {repo}\n"))
        .collect();
    let rpm_owned: String = VAR_DIRS
        .iter()
        .map(|(path, _)| format!("{path}\n"))
        .collect();
    let find_var_lib: String = rpm_owned.clone();

    let mut mock = MockExecutor::new()
        // -- packages --------------------------------------------------
        .with_command(
            "rpm -qa --queryformat %{EPOCH}:%{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}\n",
            ok(&rpm_qa),
        )
        .with_command_prefix(
            "dnf repoquery --installed --queryformat %{name} %{from_repo}\n",
            ok(&from_repo),
        )
        .with_command(
            "rpm -qa --queryformat @@%{NAME}\\n[%{FILENAMES}\\n]",
            ok(FILE_OWNERSHIP),
        )
        // -- leaf versus auto ------------------------------------------
        // The batched `rpm -q` dependency query is left unanswered, so
        // the inspector takes the dnf route, which is the simpler one to
        // describe here.
        .with_command(
            "dnf repoquery --userinstalled --queryformat %{name}.%{arch}\n",
            ok(&format!("{LEAF}.x86_64\nbash.x86_64\n")),
        )
        .with_command_prefix(
            "dnf repoquery --requires --resolve --recursive --installed",
            ok(""),
        )
        // -- repo-less scan --------------------------------------------
        .with_command(
            "dnf repolist --enabled -q",
            ok("repo id    repo name\nbaseos     BaseOS\nappstream  AppStream\n"),
        )
        .with_command_prefix(
            "dnf repoquery --available",
            ok(&format!("{ANACONDA_PACKAGE} appstream\n")),
        )
        .with_command("find /var/cache/dnf -name *.rpm -type f", ok(""))
        .with_command("rpm -Va", ok(""))
        // -- services --------------------------------------------------
        .with_command(
            "systemctl list-unit-files --type=service --no-pager",
            ok(SYSTEMCTL),
        )
        .with_dir("/usr/lib/systemd/system-preset", vec!["90-default.preset"])
        .with_file("/usr/lib/systemd/system-preset/90-default.preset", PRESET)
        .with_command(
            "rpm -qf --queryformat %{NAME}\n \
             /usr/lib/systemd/system/httpd.service /usr/lib/systemd/system/rhcd.service",
            ok("httpd-core\nrhc\n"),
        )
        // -- storage ---------------------------------------------------
        .with_file("/etc/fstab", "UUID=0000 / xfs defaults 0 0\n")
        .with_command("findmnt --json", ok(r#"{"filesystems": []}"#))
        .with_command(
            "find /var/lib -mindepth 1 -maxdepth 2 -type d ! -empty",
            ok(&find_var_lib),
        )
        .with_command("rpm --query --all --qf [%{FILENAMES}\\n]", ok(&rpm_owned));
    for (id, requires) in RECURSIVE_REQUIRES {
        mock = mock.with_command(
            &format!(
                "dnf repoquery --requires --resolve --recursive --installed \
                 --queryformat %{{name}}.%{{arch}}\n {id}"
            ),
            ok(requires),
        );
    }
    for (path, owner) in VAR_DIRS {
        mock = mock
            .with_command(
                &format!("stat -c %04a %u %g %U %G {path}"),
                ok("0750 0 0 root root\n"),
            )
            .with_command(
                &format!("rpm --query --file {path} --qf %{{NAME}}\\n"),
                ok(&format!("{owner}\n")),
            );
    }
    mock
}

/// What the scan command does before its dispositions step: `collect`,
/// `validate`, the baseline recorded on the snapshot, `redact`, then the
/// dnf cache pass for repo-less packages.
fn collected(exec: &MockExecutor) -> InspectionSnapshot {
    let inspectors: Vec<Box<dyn Inspector>> = vec![
        Box::new(RpmInspector::new()),
        Box::new(ServicesInspector::new()),
        Box::new(StorageInspector::new()),
    ];
    let base = baseline();
    let collected = collect(
        &pkg_source(),
        exec,
        &inspectors,
        Some(&base),
        &NullProgress,
        &AtomicBool::new(false),
    );
    let validated = validate(collected).expect("the snapshot validates");
    let mut snapshot = validated.state.snapshot;
    snapshot.baseline = Some(base);
    redact(&mut snapshot, &RedactOptions::default());
    if let Some(rpm) = snapshot.rpm.as_mut() {
        scan_dnf_cache_for_repoless(exec, rpm);
    }
    snapshot
}

/// The rest of the scan command: its dispositions step, then what it
/// stages for the tarball.
fn scan_export(mut snapshot: InspectionSnapshot, out: &Path) -> InspectionSnapshot {
    apply_scan_package_defaults(&mut snapshot);
    render::render_all(&snapshot, &RenderContext { target: None }, out).expect("render");
    let warnings =
        render::repoless::write_cached_rpms(snapshot.rpm.as_ref().expect("rpm section"), out)
            .expect("the cached RPMs are staged");
    // A refused NEVRA is dropped with a warning, which would leave this
    // test running on a different tree than the one it means to build.
    assert!(warnings.is_empty(), "cached RPMs not staged: {warnings:?}");
    snapshot
}

/// A refine session opened on `snapshot` and exported with no operations.
/// Returns the unpacked export root.
fn refine_export(snapshot: InspectionSnapshot, dir: &Path) -> PathBuf {
    let session = RefineSession::new(snapshot);
    let tarball = dir.join("export.tar.gz");
    session
        .export_plan(session.generation())
        .expect("the generation matches")
        .render(&tarball)
        .expect("the export writes");

    let unpacked = dir.join("unpacked");
    std::fs::create_dir_all(&unpacked).expect("mkdir");
    let file = std::fs::File::open(&tarball).expect("the tarball opens");
    tar::Archive::new(flate2::read::GzDecoder::new(file))
        .unpack(&unpacked)
        .expect("the tarball unpacks");
    let mut roots: Vec<PathBuf> = std::fs::read_dir(&unpacked)
        .expect("the unpack directory reads")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .collect();
    assert_eq!(roots.len(), 1, "a tarball has one top-level member");
    roots.pop().expect("one root")
}

// ── What is compared ─────────────────────────────────────────────────

/// What one export path wrote, cut down to the compared surfaces.
#[derive(Debug)]
struct Surfaces {
    containerfile: String,
    audit: String,
    var_conf: Option<String>,
}

impl Surfaces {
    fn read(root: &Path) -> Self {
        let read = |rel: &str| {
            std::fs::read_to_string(root.join(rel))
                .unwrap_or_else(|e| panic!("{rel} in {}: {e}", root.display()))
        };
        Self {
            containerfile: read("Containerfile"),
            audit: read("audit-report.md"),
            var_conf: std::fs::read_to_string(
                root.join("config/usr/lib/tmpfiles.d/inspectah-var.conf"),
            )
            .ok(),
        }
    }

    /// Every name on every `RUN dnf install` line, in order, without the
    /// out-of-class packages.
    fn install_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        let mut in_install = false;
        for line in self.containerfile.lines() {
            if line.starts_with("RUN dnf install") {
                in_install = true;
                continue;
            }
            if !in_install {
                continue;
            }
            let item = line.trim().trim_end_matches('\\').trim();
            if item.starts_with("&&") || !line.ends_with('\\') {
                in_install = false;
                continue;
            }
            if !item.starts_with('/') && !OUT_OF_CLASS.contains(&item) {
                names.push(item.to_string());
            }
        }
        names
    }

    /// Every line of every `systemctl` instruction, continuation lines
    /// included: the renderer puts the unit names on those.
    fn systemctl_lines(&self) -> Vec<&str> {
        let mut lines = Vec::new();
        let mut continued = false;
        for line in self.containerfile.lines() {
            if continued || line.contains("systemctl ") {
                lines.push(line);
                continued = line.ends_with('\\');
            }
        }
        lines
    }

    fn omitted_lines(&self) -> Vec<&str> {
        self.containerfile
            .lines()
            .filter(|l| l.starts_with("# Omitted:"))
            .collect()
    }

    /// The `# Not carried:` block, header through its last entry.
    fn not_carried_block(&self) -> Vec<&str> {
        self.containerfile
            .lines()
            .skip_while(|l| !l.starts_with("# Not carried:"))
            .take_while(|l| !l.is_empty())
            .collect()
    }

    /// The `### Not carried` table, heading through its last row.
    fn not_carried_table(&self) -> Vec<&str> {
        self.audit
            .lines()
            .skip_while(|l| !l.starts_with("### Not carried"))
            .enumerate()
            .take_while(|(i, l)| *i < 2 || !l.is_empty())
            .map(|(_, l)| l)
            .collect()
    }
}

/// Both halves for one host. The refine half runs twice: on the snapshot
/// as collected, before the scan's dispositions step, so refine reaches
/// the class on its own defaults; and on the snapshot the scan tarball
/// carries, which is what refine opens in practice.
struct Run {
    scan_snapshot: InspectionSnapshot,
    scan: Surfaces,
    refine: Vec<(&'static str, Surfaces)>,
    _dirs: Vec<tempfile::TempDir>,
}

fn run() -> Run {
    let collected = collected(&host_mock());

    let scan_dir = tempfile::tempdir().expect("tempdir");
    let scan_snapshot = scan_export(collected.clone(), scan_dir.path());
    let scan = Surfaces::read(scan_dir.path());

    let mut dirs = vec![scan_dir];
    let mut refine = Vec::new();
    for (label, snapshot) in [
        ("refine on the collected snapshot", collected),
        (
            "refine on the scan tarball's snapshot",
            scan_snapshot.clone(),
        ),
    ] {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = refine_export(snapshot, dir.path());
        refine.push((label, Surfaces::read(&root)));
        dirs.push(dir);
    }
    Run {
        scan_snapshot,
        scan,
        refine,
        _dirs: dirs,
    }
}

// ── The fixture is not vacuous ───────────────────────────────────────

/// Without this, every comparison below would pass on a snapshot with no
/// baseline or no not-carried package, where both paths emit nothing.
#[test]
fn the_fixture_is_baseline_backed_and_has_a_not_carried_list() {
    let Run {
        scan_snapshot,
        scan,
        ..
    } = run();
    let rpm = scan_snapshot.rpm.as_ref().expect("the rpm section");

    let base = rpm
        .baseline_package_names
        .as_ref()
        .expect("the fixture has a baseline");
    assert_eq!(
        base,
        &BASE_PACKAGES
            .iter()
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
    );
    assert!(
        rpm.leaf_packages.is_some(),
        "the collector classified leaves"
    );

    let listed: Vec<String> = not_carried_packages(&scan_snapshot)
        .iter()
        .map(|p| p.display_name())
        .collect();
    assert_eq!(listed, EXPECTED_NOT_CARRIED, "the scan's not-carried list");

    let auto = rpm.auto_packages.as_ref().expect("auto packages");
    let source_repo: HashMap<&str, &str> = rpm
        .packages_added
        .iter()
        .map(|p| (p.name.as_str(), p.source_repo.as_str()))
        .collect();
    for name in OUT_OF_CLASS {
        assert!(
            auto.iter().any(|id| id.starts_with(&format!("{name}."))),
            "{name} is an auto package, or its absence from the list proves nothing"
        );
    }
    assert_eq!(source_repo.get(ANACONDA_PACKAGE), Some(&"anaconda"));
    let repoless = rpm
        .packages_added
        .iter()
        .find(|p| p.name == REPOLESS_PACKAGE)
        .expect("the repo-less package");
    assert!(
        !repoless.repoless_annotation.is_empty(),
        "{REPOLESS_PACKAGE} is annotated repo-less: {repoless:?}"
    );

    // Each compared surface has content, or its equality is empty.
    assert_eq!(scan.install_names(), vec![LEAF.to_string()]);
    assert!(
        scan.systemctl_lines()
            .iter()
            .any(|l| l.contains("httpd.service")),
        "the pulled-in package's unit is enabled:\n{}",
        scan.containerfile
    );
    assert!(
        scan.omitted_lines()
            .iter()
            .any(|l| l.contains("rhcd.service")),
        "the not-carried package's user-enabled unit is omitted:\n{}",
        scan.containerfile
    );
    let var_conf = scan
        .var_conf
        .as_deref()
        .expect("the scan stages inspectah-var.conf");
    assert!(
        var_conf.contains("/var/lib/firewalld") && !var_conf.contains("/var/lib/httpd"),
        "the not-carried package's directory is provisioned and the pulled-in \
         one's is carried:\n{var_conf}"
    );
    assert_eq!(
        scan.not_carried_block().len(),
        EXPECTED_NOT_CARRIED.len() + 1
    );
    assert_eq!(
        scan.not_carried_table().len(),
        EXPECTED_NOT_CARRIED.len() + 4
    );
    assert!(scan.not_carried_block()[0].contains(NOT_CARRIED_REASON));
}

// ── Parity ───────────────────────────────────────────────────────────

#[test]
fn the_install_names_match() {
    let run = run();
    for (label, refine) in &run.refine {
        assert_eq!(
            refine.install_names(),
            run.scan.install_names(),
            "{label}: `dnf install` names differ from the scan's"
        );
    }
}

#[test]
fn the_systemctl_and_omitted_lines_match() {
    let run = run();
    for (label, refine) in &run.refine {
        assert_eq!(
            refine.systemctl_lines(),
            run.scan.systemctl_lines(),
            "{label}: systemctl lines differ"
        );
        assert_eq!(
            refine.omitted_lines(),
            run.scan.omitted_lines(),
            "{label}: `# Omitted:` lines differ"
        );
    }
}

#[test]
fn inspectah_var_conf_is_byte_identical() {
    let run = run();
    for (label, refine) in &run.refine {
        assert_eq!(
            refine.var_conf, run.scan.var_conf,
            "{label}: inspectah-var.conf differs"
        );
    }
}

#[test]
fn the_not_carried_block_and_table_are_byte_identical() {
    let run = run();
    for (label, refine) in &run.refine {
        assert_eq!(
            refine.not_carried_block(),
            run.scan.not_carried_block(),
            "{label}: the Containerfile's Not carried block differs"
        );
        assert_eq!(
            refine.not_carried_table(),
            run.scan.not_carried_table(),
            "{label}: the audit report's Not carried table differs"
        );
    }
}

#[test]
fn out_of_class_packages_are_in_neither_not_carried_list() {
    let run = run();
    let paths =
        std::iter::once(("scan", &run.scan)).chain(run.refine.iter().map(|(label, s)| (*label, s)));
    for (label, surfaces) in paths {
        let lines: Vec<&str> = surfaces
            .not_carried_block()
            .into_iter()
            .chain(surfaces.not_carried_table())
            .collect();
        for name in OUT_OF_CLASS {
            assert!(
                !lines.iter().any(|l| l.contains(name)),
                "{label}: {name} is outside the class but listed:\n{lines:#?}"
            );
        }
    }
}
