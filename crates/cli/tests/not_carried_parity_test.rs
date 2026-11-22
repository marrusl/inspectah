//! The scan tarball and an unchanged refine export agree on the
//! not-carried class.
//!
//! An auto package that nothing the image installs needs is left out of
//! the image on both export paths, unless it owns a unit enabled on the
//! source host, which both carry. The two paths reach that by different
//! code: the scan applies `apply_scan_package_defaults` to its
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
//!   audit table, compared whole;
//! - the files staged under `config/etc/` and the Containerfile's
//!   `# N config file(s) captured` line, for a host whose only config
//!   finding is an RPM-owned file the operator changed.
//!
//! Two tests reuse the fixture for the refine export's not-carried
//! notice, which names the unit owners an operator excludes and no
//! pulled-in one. Three more check refine's not-carried rows against the
//! export's block and notice, and that excluding a unit owner lists it on
//! every surface and including it again restores the default. One runs a
//! fleet of two such hosts, whose export drops the unit owners and says
//! so as a single host's export would.
//!
//! The last tests reuse the fixture for safety checks rather than parity
//! claims: a config whose content the collector could not capture is
//! staged by neither path, nor by `inspectah aggregate` or a refine export
//! of the aggregate when only some hosts of a fleet could read it, nor
//! after refine locks it and the locked exports are aggregated.
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
//! every other config kind, and everything outside the package, service,
//! `/var` and config surfaces above.
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
use inspectah_collect::inspectors::config::ConfigInspector;
use inspectah_collect::inspectors::rpm::RpmInspector;
use inspectah_collect::inspectors::rpm::repoless::scan_dnf_cache_for_repoless;
use inspectah_collect::inspectors::storage::StorageInspector;
use inspectah_collect::inspectors::units::UnitsInspector;
use inspectah_core::baseline::{
    BaselineData, BaselinePackageEntry, ResolutionStrategy, TargetImageIdentity,
};
use inspectah_core::snapshot::{InspectionSnapshot, SCHEMA_VERSION};
use inspectah_core::traits::executor::ExecResult;
use inspectah_core::traits::inspector::Inspector;
use inspectah_core::traits::progress::NullProgress;
use inspectah_core::traits::renderer::RenderContext;
use inspectah_core::types::aggregate::VariantSelection;
use inspectah_core::types::config::{ConfigFileEntry, ConfigFileKind};
use inspectah_core::types::os::OsRelease;
use inspectah_core::types::system::SourceSystem;
use inspectah_core::types::units::UnitState;
use inspectah_core::types::{AdvisoryType, FindingKind};
use inspectah_pipeline::collect::collect;
use inspectah_pipeline::export_warning::{ExportWarning, ExportWarningKind};
use inspectah_pipeline::redaction::{RedactOptions, redact};
use inspectah_pipeline::render;
use inspectah_pipeline::render::not_carried::{
    NOT_CARRIED_REASON, apply_scan_package_defaults, not_carried_notice, not_carried_packages,
};
use inspectah_pipeline::render::tarball::create_tarball;
use inspectah_pipeline::validate::validate;
use inspectah_refine::normalize::load_for_refine;
use inspectah_refine::session::{RefineSession, render_refine_export};
use inspectah_refine::types::{ItemId, RefinementOp};

// ── The fixture ──────────────────────────────────────────────────────

/// Packages the base image ships. Present on the host too, so they are
/// filtered out of `packages_added` and named in `baseline_package_names`.
const BASE_PACKAGES: &[&str] = &["bash", "systemd"];

/// The one user-installed package. It pulls in `httpd-core` (owns the
/// user-enabled `httpd.service`) and `httpd-filesystem` (owns
/// `/var/lib/httpd`).
const LEAF: &str = "httpd";

/// The auto packages that own an enabled unit, which both paths carry by
/// default: `firewalld` owns the preset-enabled `firewalld.service` and
/// `/var/lib/firewalld`; `rhc` owns `rhcd.service`, which the operator
/// enabled against its preset.
const UNIT_OWNERS: &[(&str, &str)] = &[("firewalld", "noarch"), ("rhc", "x86_64")];

/// The default not-carried list: the one auto package nothing needs.
/// `python3-firewall` is not in it: only `firewalld` needs it, and the
/// carried `firewalld`'s recorded closure says dnf installs it with it.
const EXPECTED_NOT_CARRIED: &[&str] = &["dracut-config-rescue"];

/// The not-carried list once an operator excludes both unit owners, in
/// display order: the owners first, then the rest. With `firewalld` out,
/// nothing installs `python3-firewall` either.
const OWNERS_EXCLUDED_NOT_CARRIED: &[&str] = &[
    "firewalld",
    "rhc",
    "dracut-config-rescue",
    "python3-firewall",
];

/// The notice an export carries when both unit owners are left out.
const OWNERS_NOTICE: &str = "2 packages that own a unit enabled on the source host are not \
     carried: firewalld, rhc. See the Not carried table in audit-report.md and the \
     # Not carried block in the Containerfile.";

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
    ("dracut-config-rescue", "x86_64", "baseos"),
    ("langpacks-en", "noarch", "anaconda"),
    ("vendor-agent", "x86_64", "@commandline"),
];

/// What the batched `rpm -q` dependency query answers for the added
/// packages: each one's requires, by package name, which every package
/// provides. Unlisted packages require nothing.
const RPM_REQUIRES: &str = "@@httpd\nR\thttpd-core\nR\thttpd-filesystem\nR\tbash\nR\tsystemd\n\
    @@firewalld\nR\tpython3-firewall\nR\tsystemd\n";

/// `rpm -qa` file lists, in the sentinel format the RPM inspector reads.
const FILE_OWNERSHIP: &str = "@@bash\n/usr/bin/bash\n\
    @@httpd\n/etc/httpd/conf/httpd.conf\n\
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

/// The leaf package's config file, which the operator edited. It is the
/// only file under `/etc` the config walk sees.
const MODIFIED_CONFIG: &str = "/etc/httpd/conf/httpd.conf";

/// `rpm -Va` reporting a content change to [`MODIFIED_CONFIG`].
const RPM_VA: &str = "S.5....T.  c /etc/httpd/conf/httpd.conf\n";

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

pub(crate) fn ok(stdout: &str) -> ExecResult {
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
/// reads from, and the one modified config file.
fn host_mock() -> MockExecutor {
    let rpm_qa: String = HOST_PACKAGES
        .iter()
        .map(|(name, arch, _)| format!("0:{name}-1.0-1.el9.{arch}\n"))
        .collect();
    let from_repo: String = HOST_PACKAGES
        .iter()
        .map(|(name, arch, repo)| format!("{name} {arch} {repo}\n"))
        .collect();
    let rpm_owned: String = VAR_DIRS
        .iter()
        .map(|(path, _)| format!("{path}\n"))
        .collect();
    let find_var_lib: String = rpm_owned.clone();
    let provides: String = HOST_PACKAGES
        .iter()
        .map(|(name, arch, _)| format!("{name}.{arch}\t0\t\t{name}\n"))
        .collect();

    let mut mock = MockExecutor::new()
        // -- packages --------------------------------------------------
        .with_command(
            "rpm -qa --queryformat %{EPOCH}:%{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}\n",
            ok(&rpm_qa),
        )
        .with_command_prefix(
            "dnf repoquery --installed --queryformat %{name} %{arch} %{from_repo}\n",
            ok(&from_repo),
        )
        .with_command(
            "rpm -qa --queryformat @@%{NAME}\\n[%{FILENAMES}\\n]",
            ok(FILE_OWNERSHIP),
        )
        // -- leaf versus auto ------------------------------------------
        .with_command(
            "dnf repoquery --userinstalled --queryformat %{name}.%{arch}\n",
            ok(&format!("{LEAF}.x86_64\nbash.x86_64\n")),
        )
        .with_command_prefix(
            "rpm -q --queryformat @@%{NAME}\\n[R\\t%{REQUIRENAME}\\n]\
             [W\\t%{RECOMMENDNAME}\\n][S\\t%{SUPPLEMENTNAME}\\n]",
            ok(RPM_REQUIRES),
        )
        .with_command(
            "rpm -qa --queryformat \
             [%{=NAME}.%{=ARCH}\\t%{PROVIDEFLAGS}\\t%{PROVIDEVERSION}\\t%{PROVIDENAME}\\n]",
            ok(&provides),
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
        .with_command("rpm -Va", ok(RPM_VA))
        // -- config ----------------------------------------------------
        .with_dir("/etc", vec!["httpd"])
        .with_dir("/etc/httpd", vec!["conf"])
        .with_dir("/etc/httpd/conf", vec!["httpd.conf"])
        .with_file(MODIFIED_CONFIG, "Listen 8080\n")
        // -- services --------------------------------------------------
        .with_command("systemctl list-unit-files --no-pager", ok(SYSTEMCTL))
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
    // Each listed unit's vendor file, and the link `systemctl enable` made
    // for it, since the listing calls all three `enabled`.
    let wants = "/etc/systemd/system/multi-user.target.wants";
    for unit in ["firewalld.service", "httpd.service", "rhcd.service"] {
        mock = mock
            .with_file(
                &format!("/usr/lib/systemd/system/{unit}"),
                "[Unit]\n\n[Install]\nWantedBy=multi-user.target\n",
            )
            .with_dir_entry("/etc/systemd/system", "multi-user.target.wants")
            .with_dir_entry(wants, unit)
            .with_link(
                &format!("{wants}/{unit}"),
                &format!("/usr/lib/systemd/system/{unit}"),
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
        Box::new(UnitsInspector::new()),
        Box::new(StorageInspector::new()),
        Box::new(ConfigInspector::new()),
    ];
    let base = baseline();
    let collected = collect(
        &pkg_source(),
        &inspectah_core::machine_state::ScreenedExecutor::new(exec),
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
pub(crate) fn scan_export(mut snapshot: InspectionSnapshot, out: &Path) -> InspectionSnapshot {
    apply_scan_package_defaults(&mut snapshot);
    render::render_all(&snapshot, &RenderContext { target: None }, out).expect("render");
    let warnings =
        render::repoless::write_cached_rpms(&snapshot, out).expect("the cached RPMs are staged");
    // A refused NEVRA is dropped with a warning, which would leave this
    // test running on a different tree than the one it means to build.
    assert!(warnings.is_empty(), "cached RPMs not staged: {warnings:?}");
    snapshot
}

/// A refine session opened on `snapshot` and exported with no operations.
/// Returns the unpacked export root.
pub(crate) fn refine_export(snapshot: InspectionSnapshot, dir: &Path) -> PathBuf {
    export_session(RefineSession::new(snapshot), dir)
}

/// `session` exported with no operations. Returns the unpacked export root.
pub(crate) fn export_session(session: RefineSession, dir: &Path) -> PathBuf {
    let tarball = dir.join("export.tar.gz");
    session
        .export_plan(session.generation())
        .expect("the generation matches")
        .render(&tarball)
        .expect("the export writes");
    unpack(&tarball, dir)
}

/// Unpacks `tarball` under `dir` and returns its single top-level member.
pub(crate) fn unpack(tarball: &Path, dir: &Path) -> PathBuf {
    let unpacked = dir.join("unpacked");
    std::fs::create_dir_all(&unpacked).expect("mkdir");
    let file = std::fs::File::open(tarball).expect("the tarball opens");
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
pub(crate) struct Surfaces {
    pub(crate) containerfile: String,
    pub(crate) audit: String,
    pub(crate) var_conf: Option<String>,
    /// Every file staged under `config/etc/`, relative to `config/`, sorted.
    etc_files: Vec<String>,
}

impl Surfaces {
    pub(crate) fn read(root: &Path) -> Self {
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
            etc_files: etc_files(&root.join("config")),
        }
    }

    /// The Containerfile's count of captured config files.
    pub(crate) fn config_count_lines(&self) -> Vec<&str> {
        self.containerfile
            .lines()
            .filter(|l| l.ends_with("config file(s) captured"))
            .collect()
    }

    /// Every name on every `RUN dnf install` line, in order, without the
    /// out-of-class packages.
    pub(crate) fn install_names(&self) -> Vec<String> {
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
    pub(crate) fn systemctl_lines(&self) -> Vec<&str> {
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

    pub(crate) fn omitted_lines(&self) -> Vec<&str> {
        self.containerfile
            .lines()
            .filter(|l| l.starts_with("# Omitted:"))
            .collect()
    }

    /// The `# Not carried:` block, header through its last entry.
    pub(crate) fn not_carried_block(&self) -> Vec<&str> {
        self.containerfile
            .lines()
            .skip_while(|l| !l.starts_with("# Not carried:"))
            .take_while(|l| !l.is_empty())
            .collect()
    }

    /// The `### Not carried` table, heading through its last row.
    pub(crate) fn not_carried_table(&self) -> Vec<&str> {
        self.audit
            .lines()
            .skip_while(|l| !l.starts_with("### Not carried"))
            .enumerate()
            .take_while(|(i, l)| *i < 2 || !l.is_empty())
            .map(|(_, l)| l)
            .collect()
    }
}

/// Every file below `config_root/etc`, as a path relative to `config_root`.
fn etc_files(config_root: &Path) -> Vec<String> {
    let mut files = Vec::new();
    let mut dirs = vec![config_root.join("etc")];
    while let Some(dir) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path);
            } else {
                let rel = path.strip_prefix(config_root).expect("below the root");
                files.push(rel.to_string_lossy().into_owned());
            }
        }
    }
    files.sort();
    files
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

    // Each compared surface has content, or its equality is empty. The
    // omitted lines and `/var` provisioning of a not-carried unit owner
    // are compared under an operator's exclusion, in
    // `excluding_a_unit_owner_lists_it_and_including_it_restores_the_default`.
    let mut installed = scan.install_names();
    installed.sort();
    assert_eq!(
        installed,
        ["firewalld", LEAF, "rhc"],
        "the leaf and both unit owners:\n{}",
        scan.containerfile
    );
    for unit in ["httpd.service", "rhcd.service"] {
        assert!(
            scan.systemctl_lines().iter().any(|l| l.contains(unit)),
            "{unit}, of a pulled-in package and a carried unit owner, is enabled:\n{}",
            scan.containerfile
        );
    }
    assert!(scan.omitted_lines().is_empty(), "{}", scan.containerfile);
    let var_conf = scan.var_conf.as_deref().unwrap_or("");
    assert!(
        !var_conf.contains("/var/lib/firewalld") && !var_conf.contains("/var/lib/httpd"),
        "both directories' packages are installed and create them:\n{var_conf}"
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

    let config = scan_snapshot.config.as_ref().expect("the config section");
    let modified = config
        .files
        .iter()
        .find(|f| f.path == MODIFIED_CONFIG)
        .expect("the collector found the modified config");
    assert_eq!(modified.kind, ConfigFileKind::RpmOwnedModified);
    assert_eq!(modified.package.as_deref(), Some(LEAF));
    let staged = MODIFIED_CONFIG.trim_start_matches('/');
    assert_eq!(
        scan.etc_files,
        vec![staged.to_string()],
        "the scan stages the modified config"
    );
    assert_eq!(scan.config_count_lines().len(), 1, "{}", scan.containerfile);
}

/// Only a package the export names is certain, so the common path guards
/// one command. `rhc` is a carried unit owner, named on the install line,
/// and `rhcd.service` is enabled plainly. `httpd.service` belongs to
/// `httpd-core`, which the leaf `httpd` requires and no line names: the
/// base image could provide what `httpd` needs through a package the host
/// never had, so its enable runs only if the unit file is in the image.
/// The same on the scan and both refine exports.
#[test]
fn the_fixture_guards_only_a_dependencys_command() {
    let run = run();
    let surfaces =
        std::iter::once(("scan", &run.scan)).chain(run.refine.iter().map(|(l, s)| (*l, s)));
    for (label, surfaces) in surfaces {
        let cf = &surfaces.containerfile;
        assert_eq!(
            crate::unit_hosts::guarded_commands(cf),
            [(
                "enable".to_owned(),
                "httpd.service".to_owned(),
                "/usr/lib/systemd/system/httpd.service".to_owned()
            )],
            "{label}:\n{cf}"
        );
        assert!(
            crate::unit_hosts::acts(cf, "enable", "rhcd.service"),
            "{label}: rhcd.service is enabled plainly:\n{cf}"
        );
    }
}

/// The fixture's host enables every repo its packages came from, so no
/// export enables a disabled one, and the scan and both refine exports
/// name the same packages they always did: the leaf and the two carried
/// unit owners, with nothing moved into a disabled-repo transaction.
#[test]
fn the_fixture_enables_no_disabled_repo() {
    let run = run();
    let surfaces =
        std::iter::once(("scan", &run.scan)).chain(run.refine.iter().map(|(l, s)| (*l, s)));
    for (label, surfaces) in surfaces {
        let cf = &surfaces.containerfile;
        assert!(!cf.contains("--enablerepo"), "{label}:\n{cf}");
        assert_eq!(
            surfaces.install_names(),
            ["firewalld", "httpd", "rhc"],
            "{label}:\n{cf}"
        );
    }
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
fn the_config_tree_and_count_match() {
    let run = run();
    for (label, refine) in &run.refine {
        assert_eq!(
            refine.etc_files, run.scan.etc_files,
            "{label}: files staged under config/etc differ"
        );
        assert_eq!(
            refine.config_count_lines(),
            run.scan.config_count_lines(),
            "{label}: the captured config count differs"
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

// ── The export's not-carried notice ──────────────────────────────────

/// The warnings an export of `session` returns.
pub(crate) fn export_warnings(session: &RefineSession) -> Vec<ExportWarning> {
    let dir = tempfile::tempdir().expect("tempdir");
    session
        .export_plan(session.generation())
        .expect("the generation matches")
        .render(&dir.path().join("export.tar.gz"))
        .expect("the export writes")
}

/// The fixture as refine opens it: as collected, and as the scan tarball
/// carries it.
fn refine_snapshots() -> [(&'static str, InspectionSnapshot); 2] {
    let collected = collected(&host_mock());
    let mut scanned = collected.clone();
    apply_scan_package_defaults(&mut scanned);
    [("collected", collected), ("scan tarball", scanned)]
}

/// A session on `snapshot` in which the operator excluded both unit
/// owners.
fn owners_excluded_session(snapshot: InspectionSnapshot) -> RefineSession {
    let mut session = RefineSession::new(snapshot);
    for (name, arch) in UNIT_OWNERS {
        set_package(&mut session, name, arch, false);
    }
    session
}

/// The unit owners are carried by default, so an unchanged export has
/// nothing to notice; refine's view lists them among the packages,
/// included, although neither is a leaf.
#[test]
fn an_unchanged_export_carries_the_unit_owners_without_a_notice() {
    for (label, snapshot) in refine_snapshots() {
        let session = RefineSession::new(snapshot);
        assert_eq!(export_warnings(&session), Vec::new(), "{label}");
        assert_eq!(session.decisions().not_carried_notice, None, "{label}");
        for (name, arch) in UNIT_OWNERS {
            let shown = session
                .view()
                .packages
                .iter()
                .find(|p| p.entry.name == *name && p.entry.arch == *arch)
                .unwrap_or_else(|| panic!("{label}: {name} is in the package list"));
            assert!(shown.entry.disposition.is_included(), "{label}: {name}");
        }
    }
}

/// One notice names the unit owners the operator left out, in the
/// `# Not carried` block's order. `httpd-core` owns the enabled
/// `httpd.service` too, but the leaf pulls it in, so it is carried and
/// not named; `python3-firewall` is not carried but owns no unit.
#[test]
fn the_export_notices_unit_owners_the_operator_excludes() {
    for (label, snapshot) in refine_snapshots() {
        let warnings = export_warnings(&owners_excluded_session(snapshot));
        assert_eq!(
            warnings,
            vec![ExportWarning::notice(OWNERS_NOTICE)],
            "{label}"
        );
        let text = warnings[0].text();
        assert!(!text.contains("httpd"), "{label}: {text}");
        assert!(!text.contains("python3-firewall"), "{label}: {text}");
    }
}

/// The notice leads the warnings when the export also has failures, so
/// the header's cap can only ever drop failures behind it. The repo-less
/// package claims a staged upload, and with no upload directory prepared
/// nothing can prove it, so the export demotes it with a failure.
#[test]
fn the_notice_leads_a_failure_from_the_same_export() {
    for (label, snapshot) in refine_snapshots() {
        let mut projected = owners_excluded_session(snapshot).snapshot_projected();
        let agent = projected
            .rpm
            .as_mut()
            .expect("the rpm section")
            .packages_added
            .iter_mut()
            .find(|p| p.name == REPOLESS_PACKAGE)
            .expect("the repo-less package");
        agent.uploaded = true;
        agent.disposition = FindingKind::included();

        let dir = tempfile::tempdir().expect("tempdir");
        let warnings = render_refine_export(
            &projected,
            &dir.path().join("export.tar.gz"),
            None,
            None,
            None,
            None,
        )
        .expect("the export writes");

        let kinds: Vec<ExportWarningKind> = warnings.iter().map(ExportWarning::kind).collect();
        assert_eq!(
            kinds,
            vec![ExportWarningKind::Notice, ExportWarningKind::Failure],
            "{label}: {warnings:?}"
        );
        assert!(
            warnings[0].text().contains("firewalld, rhc"),
            "{label}: {warnings:?}"
        );
        assert_eq!(
            warnings[1].text(),
            format!("{REPOLESS_PACKAGE}.x86_64: staged RPM not verified, not exported"),
            "{label}"
        );
    }
}

// ── Refine's not-carried rows and the include path ───────────────────

/// What an export of `session` writes, with no operations beyond the
/// session's own.
pub(crate) fn surfaces_of(session: &RefineSession) -> Surfaces {
    let dir = tempfile::tempdir().expect("tempdir");
    let tarball = dir.path().join("export.tar.gz");
    session
        .export_plan(session.generation())
        .expect("the generation matches")
        .render(&tarball)
        .expect("the export writes");
    Surfaces::read(&unpack(&tarball, dir.path()))
}

impl Surfaces {
    /// The package names in the `# Not carried:` block, in its order.
    pub(crate) fn not_carried_block_names(&self) -> Vec<&str> {
        self.not_carried_block()
            .into_iter()
            .skip(1)
            .filter_map(|l| l.strip_prefix("#   "))
            .filter_map(|l| l.split(' ').next())
            .collect()
    }

    /// The package names in the `### Not carried` table, in its order.
    pub(crate) fn not_carried_table_names(&self) -> Vec<&str> {
        self.not_carried_table()
            .into_iter()
            .skip(4)
            .filter_map(|row| row.split('|').nth(1))
            .map(str::trim)
            .collect()
    }
}

/// The display names of the session's not-carried rows, in order.
pub(crate) fn projected_not_carried(session: &RefineSession) -> Vec<String> {
    session
        .decisions()
        .not_carried
        .iter()
        .map(|row| row.display_name.clone())
        .collect()
}

/// Include or exclude one package through refine's ordinary decision op,
/// the one the packages view sends.
pub(crate) fn set_package(session: &mut RefineSession, name: &str, arch: &str, include: bool) {
    session
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::Package {
                name: name.into(),
                arch: arch.into(),
            },
            include,
        })
        .unwrap_or_else(|e| panic!("set {name}.{arch} include={include}: {e}"));
}

/// The rows refine shows are the export's `# Not carried` block, in its
/// order, each with the identity the include op takes, by default and
/// once the operator excludes the unit owners.
#[test]
fn the_projection_lists_the_block_in_its_order() {
    for (label, snapshot) in refine_snapshots() {
        let default = RefineSession::new(snapshot.clone());
        assert_eq!(
            projected_not_carried(&default),
            surfaces_of(&default).not_carried_block_names(),
            "{label}, by default"
        );
        assert_eq!(
            projected_not_carried(&default),
            EXPECTED_NOT_CARRIED,
            "{label}"
        );

        let session = owners_excluded_session(snapshot);
        let export = surfaces_of(&session);
        assert_eq!(
            projected_not_carried(&session),
            export.not_carried_block_names(),
            "{label}"
        );
        assert_eq!(
            projected_not_carried(&session),
            OWNERS_EXCLUDED_NOT_CARRIED,
            "{label}"
        );
        let firewalld = &session.decisions().not_carried[0];
        assert_eq!(
            firewalld.item_id,
            ItemId::Package {
                name: "firewalld".into(),
                arch: "noarch".into()
            },
            "{label}"
        );
        assert_eq!(firewalld.source_repo, "baseos", "{label}");
        assert_eq!(firewalld.enabled_units, ["firewalld.service"], "{label}");
    }
}

/// The rows marked as owning an enabled unit are exactly the packages
/// the not-carried notice names, and the view carries the notice the
/// export sends. `python3-firewall` is listed but owns no unit;
/// `httpd-core` owns one but is pulled in, so it has no row.
#[test]
fn the_projections_unit_owners_are_the_ones_the_notice_names() {
    for (label, snapshot) in refine_snapshots() {
        let session = owners_excluded_session(snapshot);
        let decisions = session.decisions();
        let owners: Vec<&str> = decisions
            .not_carried
            .iter()
            .filter(|row| row.owns_enabled_unit)
            .map(|row| row.display_name.as_str())
            .collect();
        assert_eq!(owners, ["firewalld", "rhc"], "{label}");
        for row in &decisions.not_carried {
            assert_eq!(
                row.owns_enabled_unit,
                !row.enabled_units.is_empty(),
                "{label}: {row:?}"
            );
        }

        let notice = not_carried_notice(&session.snapshot_projected()).expect("a notice");
        assert_eq!(
            decisions.not_carried_notice.as_ref(),
            Some(&notice),
            "{label}"
        );
        assert!(
            notice
                .text()
                .starts_with(&format!("{} packages", owners.len())),
            "{label}: {notice:?}"
        );
        for owner in &owners {
            assert!(notice.text().contains(owner), "{label}: {notice:?}");
        }
        assert_eq!(export_warnings(&session), vec![notice], "{label}");
    }
}

/// Excluding a unit owner lists it on every surface, and including it
/// again puts back exactly the default. `rhc` owns the user-enabled
/// `rhcd.service`, whose `systemctl` line the exclusion omits;
/// `firewalld` owns `/var/lib/firewalld`, which is provisioned once the
/// package that creates it is left out.
///
/// `python3-firewall` follows `firewalld`: carried with it, from its
/// recorded closure, and listed only while it is left out.
#[test]
fn excluding_a_unit_owner_lists_it_and_including_it_restores_the_default() {
    for (label, snapshot) in refine_snapshots() {
        let mut session = RefineSession::new(snapshot);
        let before = surfaces_of(&session);
        let notice_before = session.decisions().not_carried_notice.clone();
        assert_eq!(notice_before, None, "{label}: every unit owner is carried");

        for (name, arch) in UNIT_OWNERS {
            set_package(&mut session, name, arch, false);
            let after = surfaces_of(&session);
            let ctx = format!("{label}, {name} excluded");

            assert!(
                !after.install_names().iter().any(|n| n == name),
                "{ctx}: install list {:?}\n{}",
                after.install_names(),
                after.containerfile
            );
            assert!(
                after.not_carried_block_names().contains(name),
                "{ctx}:\n{}",
                after.containerfile
            );
            assert!(
                after.not_carried_table_names().contains(name),
                "{ctx}:\n{}",
                after.audit
            );
            assert!(
                projected_not_carried(&session).iter().any(|n| n == name),
                "{ctx}: {:?}",
                projected_not_carried(&session)
            );
            let notice = session
                .decisions()
                .not_carried_notice
                .clone()
                .unwrap_or_else(|| panic!("{ctx}: an excluded owner gives a notice"));
            assert!(notice.text().contains(name), "{ctx}: {notice:?}");
            assert_eq!(export_warnings(&session), vec![notice], "{ctx}");

            match *name {
                "rhc" => {
                    assert!(
                        !after
                            .systemctl_lines()
                            .iter()
                            .any(|l| l.contains("rhcd.service")),
                        "{ctx}: the unit is no longer enabled:\n{}",
                        after.containerfile
                    );
                    assert!(
                        after
                            .omitted_lines()
                            .iter()
                            .any(|l| l.contains("rhcd.service")),
                        "{ctx}:\n{}",
                        after.containerfile
                    );
                }
                _ => {
                    let var_conf = after.var_conf.as_deref().unwrap_or("");
                    assert!(
                        var_conf.contains("/var/lib/firewalld"),
                        "{ctx}: the left-out package's directory is provisioned:\n{var_conf}"
                    );
                }
            }
            assert_eq!(
                projected_not_carried(&session)
                    .iter()
                    .any(|n| n == "python3-firewall"),
                *name == "firewalld",
                "{ctx}: python3-firewall is listed only while firewalld is left out"
            );

            set_package(&mut session, name, arch, true);
            let again = surfaces_of(&session);
            let ctx = format!("{label}, {name} included again");
            assert_eq!(again.containerfile, before.containerfile, "{ctx}");
            assert_eq!(
                again.not_carried_table(),
                before.not_carried_table(),
                "{ctx}"
            );
            assert_eq!(again.var_conf, before.var_conf, "{ctx}");
            assert_eq!(
                projected_not_carried(&session),
                EXPECTED_NOT_CARRIED,
                "{ctx}"
            );
            assert_eq!(
                session.decisions().not_carried_notice,
                notice_before,
                "{ctx}"
            );
        }
    }
}

/// The auto package the leaf pulls in that owns the user-enabled
/// `httpd.service`.
const PULLED_IN_OWNER: (&str, &str) = ("httpd-core", "x86_64");

/// A unit owner the leaf pulls in is carried by default in its own right,
/// so excluding the leaf leaves it in the image: installed by name, its
/// unit enabled, and not listed. Excluding the owner alone lists nothing
/// either, since the leaf still pulls it in; only excluding both lists it,
/// with the notice. Each step restores the default exactly.
#[test]
fn excluding_the_leaf_keeps_the_unit_owner_it_pulled_in() {
    let (owner, owner_arch) = PULLED_IN_OWNER;
    for (label, snapshot) in refine_snapshots() {
        let mut session = RefineSession::new(snapshot);
        let shown = session
            .view()
            .packages
            .iter()
            .find(|p| p.entry.name == owner)
            .map(|p| p.entry.disposition.is_included());
        assert_eq!(shown, Some(true), "{label}: {owner} is included by default");
        let before = surfaces_of(&session);
        assert!(
            !before.install_names().iter().any(|n| n == owner),
            "{label}: the installed leaf brings {owner}, so it is not named:\n{}",
            before.containerfile
        );

        set_package(&mut session, LEAF, "x86_64", false);
        let after = surfaces_of(&session);
        let ctx = format!("{label}, {LEAF} excluded");
        assert!(
            after.install_names().iter().any(|n| n == owner),
            "{ctx}: {owner} is installed:\n{}",
            after.containerfile
        );
        assert!(
            after
                .systemctl_lines()
                .iter()
                .any(|l| l.contains("httpd.service")),
            "{ctx}: its unit is enabled:\n{}",
            after.containerfile
        );
        assert!(
            !projected_not_carried(&session).iter().any(|n| n == owner),
            "{ctx}: {:?}",
            projected_not_carried(&session)
        );
        assert!(
            !after.not_carried_block_names().contains(&owner),
            "{ctx}:\n{}",
            after.containerfile
        );
        assert_eq!(session.decisions().not_carried_notice, None, "{ctx}");

        set_package(&mut session, LEAF, "x86_64", true);
        assert_eq!(
            surfaces_of(&session).containerfile,
            before.containerfile,
            "{label}, {LEAF} included again"
        );

        set_package(&mut session, owner, owner_arch, false);
        let ctx = format!("{label}, {owner} excluded");
        let after = surfaces_of(&session);
        assert!(
            after
                .systemctl_lines()
                .iter()
                .any(|l| l.contains("httpd.service")),
            "{ctx}: the leaf still pulls it in:\n{}",
            after.containerfile
        );
        assert!(
            !projected_not_carried(&session).iter().any(|n| n == owner),
            "{ctx}"
        );
        assert_eq!(session.decisions().not_carried_notice, None, "{ctx}");

        set_package(&mut session, LEAF, "x86_64", false);
        let ctx = format!("{label}, {LEAF} and {owner} excluded");
        let after = surfaces_of(&session);
        assert!(
            after.not_carried_block_names().contains(&owner),
            "{ctx}:\n{}",
            after.containerfile
        );
        assert!(
            after
                .omitted_lines()
                .iter()
                .any(|l| l.contains("httpd.service")),
            "{ctx}:\n{}",
            after.containerfile
        );
        let notice = session
            .decisions()
            .not_carried_notice
            .clone()
            .unwrap_or_else(|| panic!("{ctx}: an excluded owner gives a notice"));
        assert!(notice.text().contains(owner), "{ctx}: {notice:?}");

        set_package(&mut session, LEAF, "x86_64", true);
        set_package(&mut session, owner, owner_arch, true);
        assert_eq!(
            surfaces_of(&session).containerfile,
            before.containerfile,
            "{label}, both included again"
        );
    }
}

// ── The scan's not-carried notice ────────────────────────────────────

/// What `inspectah scan` prints to stderr once the tarball is written:
/// each entry on its own line, the label from its kind. The command
/// itself needs root and a real host, so this runs the layer below it:
/// the scan's collect, dispositions and render path (`scan_export`),
/// then the same `not_carried_notice` call and `Display` the command
/// prints.
fn scan_stderr_lines(snapshot: &InspectionSnapshot) -> Vec<String> {
    not_carried_notice(snapshot)
        .iter()
        .map(ToString::to_string)
        .collect()
}

/// The scan carries every unit owner, so on this host it has nothing to
/// notice; the line it would print for an owner left out is the one
/// `Display` gives the shared notice, shown here on the same snapshot with
/// both owners excluded after the fact.
#[test]
fn the_scan_prints_no_notice_when_it_carries_every_unit_owner() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut snapshot = scan_export(collected(&host_mock()), dir.path());
    assert_eq!(scan_stderr_lines(&snapshot), Vec::<String>::new());
    assert_eq!(
        not_carried_packages(&snapshot)
            .iter()
            .map(|p| p.display_name())
            .collect::<Vec<_>>(),
        EXPECTED_NOT_CARRIED,
        "the list is not empty, or the absence proves nothing"
    );

    for pkg in &mut snapshot
        .rpm
        .as_mut()
        .expect("the rpm section")
        .packages_added
    {
        if UNIT_OWNERS.contains(&(pkg.name.as_str(), pkg.arch.as_str())) {
            pkg.disposition = FindingKind::excluded();
        }
    }
    assert_eq!(
        scan_stderr_lines(&snapshot),
        vec![format!("Notice: {OWNERS_NOTICE}")]
    );
}

/// No enabled units, so no not-carried package owns one: the not-carried
/// list is still there, but the scan prints nothing.
#[test]
fn the_scan_prints_nothing_without_not_carried_unit_owners() {
    let mut snapshot = collected(&host_mock());
    snapshot
        .units
        .as_mut()
        .expect("the services section")
        .inventory
        .retain(|e| e.state != UnitState::Enabled);
    let dir = tempfile::tempdir().expect("tempdir");
    let snapshot = scan_export(snapshot, dir.path());
    assert!(
        !not_carried_packages(&snapshot).is_empty(),
        "the list is not empty, or the absence proves nothing"
    );
    assert_eq!(scan_stderr_lines(&snapshot), Vec::<String>::new());
}

// ── The fleet export's not-carried notice ────────────────────────────

/// The image every fleet host's scan resolved.
const FLEET_TARGET_IMAGE: &str = "registry.redhat.io/rhel9/rhel-bootc:9.4";

/// A fleet of two hosts like [`host_mock`]. The merge keeps only packages
/// that are a leaf on every host, so it drops both unit owners, and the
/// fleet export cannot carry them. Both fleet exports list them in the
/// `# Not carried` block and the audit table, `inspectah aggregate` prints
/// the notice the scan would, and the refine export of the aggregate sends
/// it in its warnings: the same notice a single host's export gives.
///
/// Neither export enables `rhcd.service`, which both hosts enabled against
/// its preset: its package is not in the image, so `systemctl enable`
/// would fail the build. Each says why in an `# Omitted:` line instead.
/// `httpd.service` is enabled: the merge dropped `httpd-core` too, but the
/// leaf `httpd` pulls it in.
#[test]
fn the_fleet_export_notices_the_unit_owners_its_merge_drops() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut tarballs = Vec::new();
    for hostname in ["host-a", "host-b"] {
        let out = dir.path().join(hostname);
        let mock = host_mock().with_file("/etc/hostname", &format!("{hostname}\n"));
        // The scan records the image it resolved, and the merge takes the
        // fleet's baseline from the host whose target matches; without one
        // a fleet cannot tell a base-image package from a dropped one.
        let mut snapshot = collected(&mock);
        snapshot.target_image = Some(TargetImageIdentity {
            image_ref: FLEET_TARGET_IMAGE.into(),
            strategy: ResolutionStrategy::OsRelease,
        });
        scan_export(snapshot, &out);
        let tarball = dir.path().join(format!("{hostname}.tar.gz"));
        create_tarball(&out, &tarball, hostname).expect("the scan tarball packs");
        tarballs.push(tarball);
    }

    let aggregate = dir.path().join("aggregate.tar.gz");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_inspectah"))
        .arg("aggregate")
        .args(&tarballs)
        .arg("--output-file")
        .arg(&aggregate)
        .stdin(std::process::Stdio::null())
        .output()
        .expect("inspectah aggregate runs");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert_eq!(
        stderr.lines().last(),
        Some(format!("Notice: {OWNERS_NOTICE}").as_str()),
        "{stderr}"
    );

    let aggregate_root = unpack(&aggregate, &dir.path().join("aggregate"));
    let session =
        inspectah_refine::tarball::from_tarball(&aggregate).expect("refine opens the aggregate");
    assert_eq!(
        export_warnings(&session),
        vec![ExportWarning::notice(OWNERS_NOTICE)],
        "the refine export of the aggregate"
    );
    let refine_dir = dir.path().join("refine");
    std::fs::create_dir_all(&refine_dir).expect("mkdir");
    let refine_root = export_session(session, &refine_dir);

    for (label, root) in [
        ("inspectah aggregate", aggregate_root),
        ("refine export of the aggregate", refine_root),
    ] {
        let fleet = Surfaces::read(&root);
        assert_eq!(
            fleet.not_carried_block_names(),
            ["firewalld", "rhc"],
            "{label}:\n{}",
            fleet.containerfile
        );
        assert_eq!(
            fleet.not_carried_table_names(),
            ["firewalld", "rhc"],
            "{label}:\n{}",
            fleet.audit
        );
        for (name, _) in UNIT_OWNERS {
            assert!(
                !fleet.install_names().iter().any(|n| n == name),
                "{label}: the fleet does not install {name}:\n{}",
                fleet.containerfile
            );
        }
        assert!(
            !fleet
                .systemctl_lines()
                .iter()
                .any(|l| l.contains("rhcd.service")),
            "{label}: the unit of a dropped owner is not enabled:\n{}",
            fleet.containerfile
        );
        assert!(
            fleet
                .omitted_lines()
                .iter()
                .any(|l| l.contains("rhcd.service") && l.contains("rhc")),
            "{label}: the omission is explained:\n{}",
            fleet.containerfile
        );
        assert!(
            fleet
                .systemctl_lines()
                .iter()
                .any(|l| l.contains("httpd.service")),
            "{label}: the unit of a pulled-in package is enabled:\n{}",
            fleet.containerfile
        );
    }
}

// ── A config the collector could not capture ─────────────────────────

/// RPM-owned configs added to [`RPM_VA`] for the uncaptured-content case:
/// one over the content cap, one whose read fails, and one the operator
/// emptied on purpose.
const OVERSIZED_RPM_CONFIG: &str = "/etc/httpd/conf.d/ssl.conf";
const UNREADABLE_RPM_CONFIG: &str = "/etc/httpd/conf.d/welcome.conf";
const EMPTIED_RPM_CONFIG: &str = "/etc/httpd/conf.d/autoindex.conf";
/// Unowned files from the `/etc` walk: one over the cap, one that does not
/// read as text.
const OVERSIZED_UNOWNED: &str = "/etc/vendor/big.conf";
const UNREADABLE_UNOWNED: &str = "/etc/vendor/agent.db";

/// The host from [`host_mock`], plus configs whose content the collector
/// cannot capture and one that is empty on the host.
fn uncaptured_config_mock() -> MockExecutor {
    let rpm_va = format!(
        "{RPM_VA}S.5....T.  c {OVERSIZED_RPM_CONFIG}\n\
         S.5....T.  c {UNREADABLE_RPM_CONFIG}\n\
         S.5....T.  c {EMPTIED_RPM_CONFIG}\n"
    );
    let ownership = FILE_OWNERSHIP.replace(
        "@@httpd\n/etc/httpd/conf/httpd.conf\n",
        &format!(
            "@@httpd\n/etc/httpd/conf/httpd.conf\n{OVERSIZED_RPM_CONFIG}\n\
             {UNREADABLE_RPM_CONFIG}\n{EMPTIED_RPM_CONFIG}\n"
        ),
    );
    // One byte over the config inspector's 256 KiB content cap.
    let oversized = "#".repeat(256 * 1024 + 1);
    host_mock()
        .with_command("rpm -Va", ok(&rpm_va))
        .with_command(
            "rpm -qa --queryformat @@%{NAME}\\n[%{FILENAMES}\\n]",
            ok(&ownership),
        )
        .with_dir("/etc", vec!["httpd", "vendor"])
        .with_dir("/etc/httpd", vec!["conf", "conf.d"])
        .with_dir(
            "/etc/httpd/conf.d",
            vec!["ssl.conf", "welcome.conf", "autoindex.conf"],
        )
        .with_dir("/etc/vendor", vec!["big.conf", "agent.db"])
        .with_file(OVERSIZED_RPM_CONFIG, &oversized)
        .with_file(UNREADABLE_RPM_CONFIG, "")
        .with_file_error(UNREADABLE_RPM_CONFIG, std::io::ErrorKind::PermissionDenied)
        .with_file(EMPTIED_RPM_CONFIG, "")
        .with_file(OVERSIZED_UNOWNED, &oversized)
        .with_file(UNREADABLE_UNOWNED, "")
        .with_file_error(UNREADABLE_UNOWNED, std::io::ErrorKind::InvalidData)
}

/// A config whose content the collector could not capture is never staged:
/// an empty file in its place would replace the package's config, or the
/// operator's, with nothing. A config that is really empty is captured and
/// staged as an empty file. Both hold on the scan and on an untouched
/// refine export, and the audit report names each uncaptured file.
#[test]
fn an_uncaptured_config_is_never_staged_on_either_path() {
    let collected = collected(&uncaptured_config_mock());

    let scan_dir = tempfile::tempdir().expect("tempdir");
    scan_export(collected.clone(), scan_dir.path());
    // The snapshot refine opens is the scan tarball's JSON, read back
    // through refine's loader, so the advisory has to survive
    // serialization and the schema gate to count.
    let json = std::fs::read_to_string(scan_dir.path().join("inspection-snapshot.json"))
        .expect("the scan writes its snapshot");
    let reloaded = load_for_refine(&json).expect("the scan's own snapshot loads");
    assert_eq!(reloaded.schema_version, SCHEMA_VERSION);
    let reloaded_files = &reloaded.config.as_ref().expect("config section").files;
    for path in [
        OVERSIZED_RPM_CONFIG,
        UNREADABLE_RPM_CONFIG,
        OVERSIZED_UNOWNED,
        UNREADABLE_UNOWNED,
    ] {
        let entry = reloaded_files
            .iter()
            .find(|f| f.path == path)
            .unwrap_or_else(|| panic!("{path} is in the reloaded snapshot"));
        assert!(
            matches!(
                entry.disposition,
                FindingKind::Advisory {
                    advisory_type: AdvisoryType::ContentNotCaptured,
                    ..
                }
            ),
            "{path} is still content_not_captured after the JSON round trip: {:?}",
            entry.disposition
        );
    }
    let mut roots = vec![("scan", scan_dir.path().to_path_buf())];
    let mut dirs = vec![scan_dir];
    for (label, snapshot) in [
        ("refine on the collected snapshot", collected),
        ("refine on the scan tarball's reloaded snapshot", reloaded),
    ] {
        let dir = tempfile::tempdir().expect("tempdir");
        roots.push((label, refine_export(snapshot, dir.path())));
        dirs.push(dir);
    }

    let uncaptured = [
        OVERSIZED_RPM_CONFIG,
        UNREADABLE_RPM_CONFIG,
        OVERSIZED_UNOWNED,
        UNREADABLE_UNOWNED,
    ];
    for (label, root) in &roots {
        let surfaces = Surfaces::read(root);
        for path in uncaptured {
            let staged = path.trim_start_matches('/');
            assert!(
                !surfaces.etc_files.iter().any(|f| f == staged),
                "{label}: {path} was not captured but is staged: {:?}",
                surfaces.etc_files
            );
        }
        let emptied = root
            .join("config")
            .join(EMPTIED_RPM_CONFIG.trim_start_matches('/'));
        let len = std::fs::metadata(&emptied)
            .unwrap_or_else(|e| panic!("{label}: {EMPTIED_RPM_CONFIG} is staged: {e}"))
            .len();
        assert_eq!(len, 0, "{label}: the emptied config is staged as it is");
        assert!(
            surfaces
                .etc_files
                .iter()
                .any(|f| f == MODIFIED_CONFIG.trim_start_matches('/')),
            "{label}: the readable modified config is still staged"
        );
    }

    let audit = Surfaces::read(&roots[0].1).audit;
    for path in uncaptured {
        assert!(
            audit.lines().any(|l| l.contains(path)),
            "the scan's audit report names {path}:\n{audit}"
        );
    }
    drop(dirs);
}

// ── A config only some hosts of a fleet could capture ────────────────

/// The RPM-owned config the fleet tests vary between hosts. Every host
/// reports it modified; on some the copy is empty, on others the read
/// fails.
const FLEET_CONFIG: &str = EMPTIED_RPM_CONFIG;

/// What one host's scan found at [`FLEET_CONFIG`].
#[derive(Clone, Copy, Debug)]
enum FleetCopy {
    /// Read, and empty.
    Empty,
    /// Present, but the read was refused.
    Unreadable,
}

/// The host from [`host_mock`], named `hostname`, with [`FLEET_CONFIG`]
/// modified and found as `copy` says.
fn fleet_host_mock(hostname: &str, copy: FleetCopy) -> MockExecutor {
    let rpm_va = format!("{RPM_VA}S.5....T.  c {FLEET_CONFIG}\n");
    let ownership = FILE_OWNERSHIP.replace(
        "@@httpd\n/etc/httpd/conf/httpd.conf\n",
        &format!("@@httpd\n/etc/httpd/conf/httpd.conf\n{FLEET_CONFIG}\n"),
    );
    let mock = host_mock()
        .with_command("rpm -Va", ok(&rpm_va))
        .with_command(
            "rpm -qa --queryformat @@%{NAME}\\n[%{FILENAMES}\\n]",
            ok(&ownership),
        )
        .with_file("/etc/hostname", &format!("{hostname}\n"))
        .with_dir("/etc/httpd", vec!["conf", "conf.d"])
        .with_dir("/etc/httpd/conf.d", vec!["autoindex.conf"])
        .with_file(FLEET_CONFIG, "");
    match copy {
        FleetCopy::Empty => mock,
        FleetCopy::Unreadable => {
            mock.with_file_error(FLEET_CONFIG, std::io::ErrorKind::PermissionDenied)
        }
    }
}

/// The merged snapshot and the two unpacked export roots for one fleet.
struct FleetRun {
    merged: InspectionSnapshot,
    roots: Vec<(&'static str, PathBuf)>,
    _dir: tempfile::TempDir,
}

/// Scans each host, packs its scan tarball, runs the `inspectah aggregate`
/// binary over the tarballs, and exports the aggregate tarball from a
/// refine session with no operations.
fn fleet_run(hosts: &[(&str, FleetCopy)]) -> FleetRun {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut tarballs = Vec::new();
    for (hostname, copy) in hosts {
        let out = dir.path().join(hostname);
        scan_export(collected(&fleet_host_mock(hostname, *copy)), &out);
        let tarball = dir.path().join(format!("{hostname}.tar.gz"));
        create_tarball(&out, &tarball, hostname).expect("the scan tarball packs");
        tarballs.push(tarball);
    }
    aggregate_and_refine(dir, &tarballs)
}

/// Runs the `inspectah aggregate` binary over `tarballs`, then exports the
/// aggregate tarball from a refine session with no operations. Everything
/// is written under `dir`.
fn aggregate_and_refine(dir: tempfile::TempDir, tarballs: &[PathBuf]) -> FleetRun {
    let aggregate = dir.path().join("aggregate.tar.gz");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_inspectah"))
        .arg("aggregate")
        .args(tarballs)
        .arg("--output-file")
        .arg(&aggregate)
        .stdin(std::process::Stdio::null())
        .output()
        .expect("inspectah aggregate runs");
    assert!(
        output.status.success(),
        "inspectah aggregate failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let aggregate_dir = dir.path().join("aggregate");
    let aggregate_root = unpack(&aggregate, &aggregate_dir);
    let json = std::fs::read_to_string(aggregate_root.join("inspection-snapshot.json"))
        .expect("the aggregate carries its snapshot");
    let merged = InspectionSnapshot::load(&json).expect("the merged snapshot loads");

    let refine_dir = dir.path().join("refine");
    std::fs::create_dir_all(&refine_dir).expect("mkdir");
    let session =
        inspectah_refine::tarball::from_tarball(&aggregate).expect("refine opens the aggregate");
    let refine_root = export_session(session, &refine_dir);

    FleetRun {
        merged,
        roots: vec![
            ("inspectah aggregate", aggregate_root),
            ("refine export of the aggregate", refine_root),
        ],
        _dir: dir,
    }
}

impl FleetRun {
    fn entries(&self, path: &str) -> Vec<&ConfigFileEntry> {
        self.merged
            .config
            .as_ref()
            .expect("config section")
            .files
            .iter()
            .filter(|f| f.path == path)
            .collect()
    }

    /// Neither export stages [`FLEET_CONFIG`] or keeps an empty variant of
    /// it, and both still stage the config every host could read.
    fn assert_fleet_config_not_staged(&self, label: &str) {
        self.assert_not_staged(label, &[FLEET_CONFIG]);
    }

    /// Neither export stages any of `paths` or keeps a variant of one, and
    /// both still stage the config every host could read.
    fn assert_not_staged(&self, label: &str, paths: &[&str]) {
        for (export, root) in &self.roots {
            assert_not_staged(&format!("{label}, {export}"), root, paths);
        }
    }
}

/// The export at `root` stages none of `paths` and keeps no variant of
/// one, and still stages the config every host could read.
fn assert_not_staged(label: &str, root: &Path, paths: &[&str]) {
    let surfaces = Surfaces::read(root);
    for path in paths {
        let staged = path.trim_start_matches('/');
        assert!(
            !surfaces.etc_files.iter().any(|f| f == staged),
            "{label}: {path} is staged: {:?}",
            surfaces.etc_files
        );
        let variants = root.join("aggregate/variants").join(staged);
        let kept: Vec<_> = std::fs::read_dir(&variants)
            .map(|d| d.filter_map(|e| e.ok()).map(|e| e.path()).collect())
            .unwrap_or_default();
        assert!(
            kept.is_empty(),
            "{label}: no host's content of {path} is unknown-as-empty: {kept:?}"
        );
    }
    assert!(
        surfaces
            .etc_files
            .iter()
            .any(|f| f == MODIFIED_CONFIG.trim_start_matches('/')),
        "{label}: the config every host read is still staged"
    );
}

/// The disposition of an entry the collector could not capture, if it is
/// one.
fn capture_failure(entry: &ConfigFileEntry) -> Option<&str> {
    match &entry.disposition {
        FindingKind::Advisory {
            advisory_type: AdvisoryType::ContentNotCaptured,
            rationale,
        } => Some(rationale),
        _ => None,
    }
}

/// One host's empty copy of a config and another host's refused read of
/// the same file are not one variant. The merged snapshot keeps the
/// refused read as its own `content_not_captured` advisory, with its
/// reason and only that host, and the empty copy counts only the host it
/// came from, so no entry claims both hosts and nothing is staged. Run
/// with the hosts swapped, since the first host of a variant is the one
/// its merged entry is built from.
#[test]
fn a_fleet_config_one_host_could_not_read_is_not_staged_empty() {
    for (empty_host, unreadable_host) in [("host-a", "host-b"), ("host-b", "host-a")] {
        let label = format!("empty on {empty_host}, unreadable on {unreadable_host}");
        let run = fleet_run(&[
            (empty_host, FleetCopy::Empty),
            (unreadable_host, FleetCopy::Unreadable),
        ]);

        let entries = run.entries(FLEET_CONFIG);
        assert_eq!(entries.len(), 2, "{label}: {entries:?}");
        assert!(
            entries.iter().all(|e| !e.disposition.is_included()),
            "{label}: nothing is included: {entries:?}"
        );

        let uncaptured = entries
            .iter()
            .find(|e| capture_failure(e).is_some())
            .unwrap_or_else(|| panic!("{label}: the refused read survives: {entries:?}"));
        let reason = capture_failure(uncaptured).expect("a capture failure");
        assert!(
            reason.contains("permission denied"),
            "{label}: the reason survives: {reason}"
        );
        assert_eq!(uncaptured.variant_selection, VariantSelection::Alternative);
        let prevalence = uncaptured.aggregate.as_ref().expect("prevalence");
        assert_eq!(
            prevalence.hosts,
            vec![unreadable_host.to_string()],
            "{label}"
        );

        let captured = entries
            .iter()
            .find(|e| capture_failure(e).is_none())
            .unwrap_or_else(|| panic!("{label}: the empty copy: {entries:?}"));
        assert_eq!(captured.content, "", "{label}");
        assert_eq!(captured.variant_selection, VariantSelection::Selected);
        let prevalence = captured.aggregate.as_ref().expect("prevalence");
        assert_eq!(
            (prevalence.count, prevalence.total),
            (1, 2),
            "{label}: the refused read is no vote for the empty copy"
        );
        assert_eq!(prevalence.hosts, vec![empty_host.to_string()], "{label}");

        run.assert_fleet_config_not_staged(&label);
    }
}

/// When no host could read the config, the merge is one advisory naming
/// every host, and nothing is staged.
#[test]
fn a_fleet_config_no_host_could_read_is_one_advisory() {
    let run = fleet_run(&[
        ("host-a", FleetCopy::Unreadable),
        ("host-b", FleetCopy::Unreadable),
    ]);

    let entries = run.entries(FLEET_CONFIG);
    assert_eq!(entries.len(), 1, "{entries:?}");
    let entry = entries[0];
    let reason = capture_failure(entry)
        .unwrap_or_else(|| panic!("still content_not_captured: {:?}", entry.disposition));
    assert!(reason.contains("permission denied"), "{reason}");
    assert_eq!(entry.variant_selection, VariantSelection::Only);
    let prevalence = entry.aggregate.as_ref().expect("prevalence");
    assert_eq!((prevalence.count, prevalence.total), (2, 2));

    run.assert_fleet_config_not_staged("unreadable on both hosts");
}

// ── Configs refine locks, which the collector could not capture ──────

/// A merge-hostile config: refine locks it out of the image on load.
const CRYPTTAB: &str = "/etc/crypttab";
/// inspectah's own COPR repo file: refine locks it out of the image when
/// it sets its config defaults.
const INSPECTAH_COPR_REPO: &str =
    "/etc/yum.repos.d/_copr:copr.fedorainfracloud.org:example:inspectah.repo";
/// Both of the above. Each is locked by a different refine pass, and both
/// meet the final locked-item clamp every projection runs.
const LOCKED_CONFIGS: &[&str] = &[CRYPTTAB, INSPECTAH_COPR_REPO];

/// The host from [`host_mock`], named `hostname`, with unowned copies of
/// [`LOCKED_CONFIGS`] the collector reads only when `captured` is set.
fn locked_config_host_mock(hostname: &str, captured: bool) -> MockExecutor {
    let (repo_dir, repo_file) = INSPECTAH_COPR_REPO
        .rsplit_once('/')
        .expect("an absolute path");
    let mut mock = host_mock()
        .with_file("/etc/hostname", &format!("{hostname}\n"))
        .with_dir("/etc", vec!["httpd", "crypttab", "yum.repos.d"])
        .with_dir(repo_dir, vec![repo_file])
        .with_file(CRYPTTAB, "luks-0000 UUID=0000 none discard\n")
        .with_file(
            INSPECTAH_COPR_REPO,
            "[copr:copr.fedorainfracloud.org:example:inspectah]\nbaseurl=https://example.invalid/\n",
        );
    if !captured {
        for path in LOCKED_CONFIGS {
            mock = mock.with_file_error(path, std::io::ErrorKind::PermissionDenied);
        }
    }
    mock
}

/// Scans the host [`locked_config_host_mock`] builds and packs its scan
/// tarball under `dir`. Returns the tarball.
fn locked_config_scan(dir: &Path, hostname: &str, captured: bool) -> PathBuf {
    let out = dir.join(hostname);
    scan_export(
        collected(&locked_config_host_mock(hostname, captured)),
        &out,
    );
    let tarball = dir.join(format!("{hostname}.tar.gz"));
    create_tarball(&out, &tarball, hostname).expect("the scan tarball packs");
    tarball
}

/// The snapshot the export at `root` carries.
fn exported_snapshot(root: &Path) -> InspectionSnapshot {
    let json = std::fs::read_to_string(root.join("inspection-snapshot.json"))
        .expect("the export carries its snapshot");
    InspectionSnapshot::load(&json).expect("the exported snapshot loads")
}

/// Every entry for `path` in `snapshot`'s config section.
fn config_entries<'a>(snapshot: &'a InspectionSnapshot, path: &str) -> Vec<&'a ConfigFileEntry> {
    snapshot
        .config
        .as_ref()
        .expect("config section")
        .files
        .iter()
        .filter(|f| f.path == path)
        .collect()
}

/// Refine locks [`LOCKED_CONFIGS`], and the lock keeps a capture failure:
/// it holds the file out of the image without claiming the file's
/// content is known. A refine export that rewrote the failure as an
/// ordinary exclusion would carry an empty entry indistinguishable from
/// an empty file, and aggregating two such exports would include it and
/// stage an empty file over a config no host could read.
#[test]
fn a_locked_uncaptured_config_survives_refine_export_and_aggregation() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut exports = Vec::new();
    for hostname in ["host-a", "host-b"] {
        let scan = locked_config_scan(dir.path(), hostname, false);
        let refine_dir = dir.path().join(format!("{hostname}-refine"));
        std::fs::create_dir_all(&refine_dir).expect("mkdir");
        let session =
            inspectah_refine::tarball::from_tarball(&scan).expect("refine opens the scan");
        let root = export_session(session, &refine_dir);

        let exported = exported_snapshot(&root);
        for path in LOCKED_CONFIGS {
            let entries = config_entries(&exported, path);
            assert_eq!(entries.len(), 1, "{hostname}: {entries:?}");
            let entry = entries[0];
            assert!(entry.locked, "{hostname}: refine locks {path}");
            let reason = capture_failure(entry).unwrap_or_else(|| {
                panic!(
                    "{hostname}: {path} is still content_not_captured in the refine export: {:?}",
                    entry.disposition
                )
            });
            assert!(reason.contains("permission denied"), "{hostname}: {reason}");
        }
        assert_not_staged(&format!("{hostname}, refine export"), &root, LOCKED_CONFIGS);
        exports.push(refine_dir.join("export.tar.gz"));
    }

    let run = aggregate_and_refine(dir, &exports);
    for path in LOCKED_CONFIGS {
        let entries = run.entries(path);
        assert_eq!(entries.len(), 1, "{path}: {entries:?}");
        let entry = entries[0];
        assert!(
            capture_failure(entry).is_some(),
            "{path} is still content_not_captured after the merge: {:?}",
            entry.disposition
        );
        assert!(!entry.disposition.is_included(), "{path}: {entry:?}");
        let prevalence = entry.aggregate.as_ref().expect("prevalence");
        assert_eq!((prevalence.count, prevalence.total), (2, 2), "{path}");
    }
    run.assert_not_staged("refined exports of two hosts", LOCKED_CONFIGS);
}

/// When one host read [`LOCKED_CONFIGS`] and another could not, refine of
/// the aggregate locks both entries on load. The lock keeps the refused
/// read a capture failure, so the export writes no empty `.content` file
/// for it. Run with the hosts swapped, since the first host of a variant
/// is the one its merged entry is built from.
#[test]
fn a_locked_config_one_host_could_not_read_leaves_no_empty_variant() {
    for (captured_host, unreadable_host) in [("host-a", "host-b"), ("host-b", "host-a")] {
        let label = format!("read on {captured_host}, unreadable on {unreadable_host}");
        let dir = tempfile::tempdir().expect("tempdir");
        let tarballs = vec![
            locked_config_scan(dir.path(), captured_host, true),
            locked_config_scan(dir.path(), unreadable_host, false),
        ];
        let run = aggregate_and_refine(dir, &tarballs);

        let refined = exported_snapshot(&run.roots[1].1);
        for path in LOCKED_CONFIGS {
            let entries = config_entries(&refined, path);
            assert_eq!(entries.len(), 2, "{label}: {entries:?}");
            assert!(
                entries
                    .iter()
                    .all(|e| e.locked && !e.disposition.is_included()),
                "{label}: refine locks both entries for {path}: {entries:?}"
            );
            let uncaptured = entries
                .iter()
                .find(|e| capture_failure(e).is_some())
                .unwrap_or_else(|| {
                    panic!("{label}: the refused read of {path} survives the lock: {entries:?}")
                });
            assert_eq!(uncaptured.variant_selection, VariantSelection::Alternative);
            let prevalence = uncaptured.aggregate.as_ref().expect("prevalence");
            assert_eq!(
                prevalence.hosts,
                vec![unreadable_host.to_string()],
                "{label}"
            );
        }
        run.assert_not_staged(&label, LOCKED_CONFIGS);
    }
}
