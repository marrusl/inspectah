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
use inspectah_collect::inspectors::rpm::repoless::scan_dnf_cache_for_repoless;
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

// ── The reverse invariant ────────────────────────────────────────────
//
// The guard above holds one direction: every `COPY` names something the
// archive carries. It cannot catch the mirror defect, because deleting a
// `COPY` leaves the archive a strict superset of what the Containerfile
// references, which satisfies it perfectly. A mutation sample on
// 2026-11-04 deleted `COPY flatpak/ /usr/share/inspectah/flatpak/` and the
// whole workspace suite stayed green: the payload stays in the tarball,
// never reaches the image, and the artifact still builds. So this half
// holds the other direction.
//
// **Granularity is per file, with directory-prefix matching, and that
// ruling is the point of this comment.** Both coarser and finer forms
// were tried against real mutations and both are wrong:
//
//   - *Per top-level directory* is too coarse. Deleting
//     `COPY flatpak/ /usr/share/inspectah/flatpak/` leaves
//     `COPY flatpak/flatpak-provision.service ...` behind, so `flatpak/`
//     still looks reachable while the manifest the service reads at boot
//     no longer ships. The build passes and the image fails on first boot.
//
//   - *Per literal COPY line* is too fine. The config tree is copied by
//     root: `write_config_tree` returns the roots it materialized and the
//     renderer emits one `COPY config/<root>/ /<root>/` for each, so
//     anything deeper is already carried. Deleting the dedicated
//     `COPY config/etc/systemd/system/ ...` or
//     `COPY config/usr/lib/bootc/kargs.d/...` line changes nothing a build
//     can observe, because `COPY config/etc/ /etc/` and
//     `COPY config/usr/ /usr/` carry those paths anyway. Measured, not
//     assumed, on 2026-11-04. Those two lines are redundant rather than
//     load-bearing, and a per-line check would fail on both and teach the
//     next person to weaken the guard.
//
// So the question asked of each staged file is "does some COPY carry
// *this file* into the image", where a `COPY` of a directory carries
// everything beneath it. That is the strongest form of the invariant that
// is actually true of this renderer.

/// Top-level export members that correctly carry no `COPY`, each with the
/// reason. An entry here is a claim that the member is not image content:
/// it is a record for the operator, or an input to tooling that runs
/// outside the build. **Do not add a name here to make a failure go away.**
/// A staged directory with no reason to be exempt is the defect this guard
/// is for.
const NOT_IMAGE_CONTENT: &[(&str, &str)] = &[
    (
        "Containerfile",
        "the build instruction itself, not an input to it",
    ),
    (
        "inspection-snapshot.json",
        "the machine-readable record of the scan: re-import and audit, never installed",
    ),
    (
        "audit-report.md",
        "the human-readable report that ships beside the artifact",
    ),
    (
        "schema",
        "the snapshot JSON schema, a record alongside inspection-snapshot.json",
    ),
    (
        "inspectah-users.ks",
        "kickstart input, consumed by Anaconda at install time rather than by the build",
    ),
    (
        "inspectah-users.toml",
        "image-builder blueprint input, consumed outside the build for the same reason",
    ),
    (
        "users",
        "SSH key material staged for the kickstart and blueprint above to reference",
    ),
    (
        "compose",
        "reference-only by design: the containers renderer documents that compose emits no COPY or RUN",
    ),
    (
        "aggregate",
        "per-variant material for multi-host review; a single image is built from one variant, not from these",
    ),
    // The four below are staged by the CLI scan path only.
    (
        "README.md",
        "operator instructions for the tarball, including how to run the build",
    ),
    (
        "audit-report.html",
        "the browsable form of audit-report.md, same reason",
    ),
    (
        "kickstart-suggestion.ks",
        "a suggestion for the operator to adapt, consumed by Anaconda if used at all",
    ),
    (
        "secrets-review.md",
        "the redaction review the operator reads before sharing the tarball; must never be installed",
    ),
];

/// Every file under `root`, as a path relative to it.
fn staged_files(root: &Path) -> Vec<String> {
    fn walk(dir: &Path, base: &Path, out: &mut Vec<String>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in rd.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, base, out);
            } else if let Ok(rel) = path.strip_prefix(base) {
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

/// Panics naming every staged file that no `COPY` carries into the image
/// and that does not live under a declared non-content member.
fn assert_every_staged_file_is_copied(path_label: &str, root: &Path) {
    let containerfile = std::fs::read_to_string(root.join("Containerfile"))
        .unwrap_or_else(|e| panic!("{path_label}: no Containerfile in the export: {e}"));
    let sources = copy_sources(&containerfile);

    let files = staged_files(root);
    assert!(
        !files.is_empty(),
        "{path_label}: the export staged no files, so this proves nothing"
    );

    let exempt: BTreeSet<&str> = NOT_IMAGE_CONTENT.iter().map(|(name, _)| *name).collect();
    // The top-level member a relative path belongs to.
    let top_of = |rel: &str| rel.split('/').next().unwrap_or(rel).to_string();

    // A `COPY <src>` carries `rel` when `src` is the file itself or a
    // directory above it. Sources arrive with or without a trailing slash.
    let carried = |rel: &str| {
        sources.iter().any(|src| {
            let s = src.trim_end_matches('/');
            rel == s || rel.starts_with(&format!("{s}/"))
        })
    };

    let orphaned: Vec<&String> = files
        .iter()
        .filter(|rel| !exempt.contains(top_of(rel).as_str()) && !carried(rel))
        .collect();

    assert!(
        orphaned.is_empty(),
        "{path_label}: the export stages files that no COPY carries into the \
         image, so `podman build` succeeds while the content is silently \
         dropped:\n  {orphaned:?}\n\
         If one of these is genuinely not image content, add its top-level \
         directory to NOT_IMAGE_CONTENT with the reason. If it is content, \
         the renderer has stopped emitting the COPY that carried it.\n\
         COPY sources: {sources:?}"
    );
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
/// A second npm project with a lockfile and no `package.json`. `npm ci`
/// needs both files, so the collector keeps this below high confidence
/// and it renders commented: no active `COPY` may name a manifest the
/// project does not have.
const NPM_LOCK_ONLY_PROJECT: &str = "/opt/lockonly";
const NPM_LOCK_JSON: &str = r#"{
  "name": "webapp",
  "lockfileVersion": 3,
  "packages": {
    "node_modules/express": { "version": "4.18.2" }
  }
}"#;

/// Two repo-less packages with an RPM in the host's dnf cache. The
/// `@commandline` one installs from its cached RPM, so `repoless-packages/`
/// is staged and copied, which is what keeps the reverse guard live over
/// that directory. The other comes from a repo the host keeps disabled
/// and whose `.repo` file the export carries: it installs from that repo
/// with `--enablerepo`, so no export may stage its cached RPM.
const CACHED_TOOL_RPM: &str = "custom-tool-1.0-1.el9.x86_64.rpm";
const DISABLED_REPO_RPM: &str = "openh264-2.4.1-1.el9.x86_64.rpm";
const DISABLED_REPO_FILE: &str = "/etc/yum.repos.d/fedora-cisco-openh264.repo";
const DISABLED_REPO_CONTENT: &str = "[epel-cisco-openh264]\n\
     name=openh264\n\
     metalink=https://example.invalid/metalink\n\
     enabled=0\n";

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
/// service drop-in, an ordinary config file, an npm project, and two
/// cached repo-less RPMs. The RPMs are real files under `cache_dir`,
/// because the scan tarball's writer copies them from the host.
fn populated_host_mock(cache_dir: &Path) -> MockExecutor {
    let cached: Vec<String> = [CACHED_TOOL_RPM, DISABLED_REPO_RPM]
        .iter()
        .map(|name| {
            let path = cache_dir.join(name);
            std::fs::write(&path, b"rpm payload").expect("write cached RPM");
            path.to_string_lossy().into_owned()
        })
        .collect();
    MockExecutor::new()
        // -- RPM prerequisite for the config walker --------------------
        .with_command(
            "rpm -qa --queryformat %{EPOCH}:%{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}\n",
            ok(
                "0:bash-5.2.26-3.el9.x86_64\n0:nodejs-20.11.0-1.el9.x86_64\n\
                0:custom-tool-1.0-1.el9.x86_64\n0:openh264-2.4.1-1.el9.x86_64\n",
            ),
        )
        // -- source repos and the repo-less scan -----------------------
        .with_command(
            "dnf repoquery --installed --queryformat %{name} %{from_repo}\n bash",
            ok("bash baseos\n"),
        )
        .with_command(
            "dnf repoquery --installed --queryformat %{name} %{from_repo}\n \
             custom-tool nodejs openh264",
            ok("custom-tool @commandline\nnodejs appstream\nopenh264 epel-cisco-openh264\n"),
        )
        .with_command(
            "dnf repolist --enabled -q",
            ok("repo id    repo name\nbaseos     BaseOS\nappstream  AppStream\n"),
        )
        .with_command(
            "find /var/cache/dnf -name *.rpm -type f",
            ok(&format!("{}\n", cached.join("\n"))),
        )
        .with_command("rpm -Va", ok(""))
        .with_command(
            "rpm -qa --queryformat [%{NAME}\\t%{FILENAMES}\\n]",
            ok("bash\t/usr/bin/bash\n"),
        )
        // -- /etc walk -------------------------------------------------
        .with_dir(
            "/etc",
            vec!["systemd", "httpd", "containers", "yum.repos.d"],
        )
        .with_dir("/etc/yum.repos.d", vec!["fedora-cisco-openh264.repo"])
        .with_file(DISABLED_REPO_FILE, DISABLED_REPO_CONTENT)
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
        // -- flatpaks ---------------------------------------------------
        // `flatpak/` is the only top-level export directory that is image
        // content and has no parent `COPY` above it, so it is the one case
        // that proves the reverse invariant is live. Without an app here
        // the directory is never staged and that half holds vacuously:
        // measured on 2026-11-04 by deleting
        // `COPY flatpak/ /usr/share/inspectah/flatpak/` and watching this
        // file stay green.
        .with_command("which flatpak", ok("/usr/bin/flatpak"))
        .with_command(
            "flatpak list --app --system --columns=application,origin,branch",
            ok("org.mozilla.firefox\tflathub\tstable\n"),
        )
        .with_command(
            "flatpak remote-list --system --columns=name,url",
            ok("flathub\thttps://dl.flathub.org/repo/\n"),
        )
        // -- services --------------------------------------------------
        .with_command(
            "systemctl list-unit-files --type=service --no-pager",
            ok(SYSTEMCTL),
        )
        .with_dir("/usr/lib/systemd/system-preset", vec!["90-default.preset"])
        .with_file("/usr/lib/systemd/system-preset/90-default.preset", PRESET)
        .with_command("test -d /etc/systemd/system/httpd.service.d", ok(""))
        // -- npm project -----------------------------------------------
        .with_dir("/opt", vec!["webapp", "lockonly"])
        .with_dir("/opt/webapp", vec!["package.json", "package-lock.json"])
        .with_file("/opt/webapp/package.json", NPM_PACKAGE_JSON)
        .with_file("/opt/webapp/package-lock.json", NPM_LOCK_JSON)
        .with_dir("/opt/lockonly", vec!["package-lock.json"])
        .with_file("/opt/lockonly/package-lock.json", NPM_LOCK_JSON)
}

/// `collect()` then `validate`, `redact` and the dnf cache scan: the calls
/// the scan command makes, in its order.
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
    if let Some(rpm) = snapshot.rpm.as_mut() {
        scan_dnf_cache_for_repoless(exec, rpm);
    }
    snapshot
}

/// What the scan command stages for its tarball: the render, then the
/// cached repo-less RPMs.
fn scan_export(snapshot: &InspectionSnapshot, out: &Path) {
    render::render_all(snapshot, &RenderContext { target: None }, out).expect("render");
    render::repoless::write_cached_rpms(snapshot.rpm.as_ref().expect("rpm section"), out)
        .expect("the cached RPMs are staged");
}

/// Pack a staged scan export the way the scan command does, under one
/// top-level directory, so refine can extract payloads from it.
fn pack(root: &Path, tarball: &Path) {
    let file = std::fs::File::create(tarball).expect("create tarball");
    let gz = flate2::write::GzEncoder::new(file, flate2::Compression::fast());
    let mut builder = tar::Builder::new(gz);
    builder
        .append_dir_all("host-inspectah", root)
        .expect("pack the export");
    builder
        .into_inner()
        .expect("finish the tarball")
        .finish()
        .expect("flush the tarball");
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
    let cache = tempfile::tempdir().expect("tempdir");
    let snapshot = scan_like(&populated_host_mock(cache.path()));

    let rpm = snapshot.rpm.as_ref().expect("the rpm section");
    for name in ["custom-tool", "openh264"] {
        let pkg = rpm
            .packages_added
            .iter()
            .find(|p| p.name == name)
            .unwrap_or_else(|| panic!("the collector must record {name}"));
        assert!(
            pkg.repoless_cached && pkg.cache_path.is_some(),
            "{name} must be repo-less with a cached RPM, got {pkg:?}"
        );
    }
    assert_eq!(
        rpm.packages_added
            .iter()
            .find(|p| p.name == "openh264")
            .map(|p| p.source_repo.as_str()),
        Some("epel-cisco-openh264"),
        "openh264 is recorded from its disabled repo"
    );

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

    let lock_only = snapshot
        .non_rpm_software
        .as_ref()
        .expect("the non-RPM section")
        .items
        .iter()
        .find(|i| i.path == NPM_LOCK_ONLY_PROJECT.trim_start_matches('/'))
        .expect("the collector must find the lockfile-only npm project");
    assert_eq!(
        lock_only.manifest_files.keys().collect::<Vec<_>>(),
        vec!["package-lock.json"],
        "the lockfile-only project holds one manifest"
    );
    assert_ne!(
        lock_only.confidence, "high",
        "one manifest is never high confidence"
    );

    let dir = tempfile::tempdir().expect("tempdir");
    scan_export(&snapshot, dir.path());
    let containerfile =
        std::fs::read_to_string(dir.path().join("Containerfile")).expect("Containerfile");
    assert!(
        containerfile.contains("--enablerepo=epel-cisco-openh264"),
        "openh264 installs from its disabled repo:\n{containerfile}"
    );
    assert!(
        containerfile.contains(&format!("# RUN cd '{NPM_LOCK_ONLY_PROJECT}' && npm ci")),
        "the lockfile-only project renders commented"
    );
    assert!(
        containerfile
            .lines()
            .filter(|l| l.contains(NPM_LOCK_ONLY_PROJECT))
            .all(|l| l.starts_with('#')),
        "nothing about the lockfile-only project is active"
    );
    for expected in [
        "COPY quadlet/",
        "COPY drop-ins/",
        "COPY language-packages/npm/",
        "COPY flatpak/",
        "COPY repoless-packages/custom-tool-",
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
    let cache = tempfile::tempdir().expect("tempdir");
    let snapshot = scan_like(&populated_host_mock(cache.path()));
    let dir = tempfile::tempdir().expect("tempdir");
    scan_export(&snapshot, dir.path());

    assert_every_copy_source_exists("CLI scan export", dir.path());
    assert_every_staged_file_is_copied("CLI scan export", dir.path());
}

/// The refine web download. This is the path that stripped `quadlet/`
/// while still emitting `COPY quadlet/ /etc/containers/systemd/`: a
/// top-level allowlist deleted the directory after the config tree wrote
/// it and before the Containerfile referenced it.
#[test]
fn the_refine_web_export_satisfies_every_copy_it_emits() {
    let cache = tempfile::tempdir().expect("tempdir");
    let snapshot = scan_like(&populated_host_mock(cache.path()));

    // Refine extracts the cached RPMs from the scan tarball it was opened
    // on, so the export is made from one.
    let dir = tempfile::tempdir().expect("tempdir");
    let staged = dir.path().join("scan");
    scan_export(&snapshot, &staged);
    // A scan tarball written before the route was shared also carries the
    // disabled-repo package's cached RPM. Refine must not extract it.
    std::fs::copy(
        cache.path().join(DISABLED_REPO_RPM),
        staged.join("repoless-packages").join(DISABLED_REPO_RPM),
    )
    .expect("stage the stale cached RPM");
    let source = dir.path().join("scan.tar.gz");
    pack(&staged, &source);

    let mut session = RefineSession::new(snapshot);
    // With no baseline the classifier starts both packages excluded, and
    // the repo-less trust gate would pre-exclude the `@commandline` one in
    // any case. Include both, so the export has an active COPY to satisfy
    // and a disabled-repo install whose stale RPM it must leave behind.
    for name in ["custom-tool", "openh264"] {
        session
            .apply(inspectah_refine::types::RefinementOp::SetInclude {
                item_id: inspectah_refine::types::ItemId::Package {
                    name: name.into(),
                    arch: "x86_64".into(),
                },
                include: true,
            })
            .unwrap_or_else(|e| panic!("{name} can be included: {e:?}"));
    }
    session.set_tarball_path(source);

    let tarball = dir.path().join("export.tar.gz");
    session
        .export_plan(session.generation())
        .expect("the generation matches")
        .render(&tarball)
        .expect("the export writes");

    let unpacked = dir.path().join("unpacked");
    std::fs::create_dir_all(&unpacked).expect("mkdir");
    let root = unpack(&tarball, &unpacked);

    assert_every_copy_source_exists("refine web export", &root);
    assert_every_staged_file_is_copied("refine web export", &root);
    let containerfile = std::fs::read_to_string(root.join("Containerfile")).expect("read");
    assert!(
        containerfile.contains("--enablerepo=epel-cisco-openh264"),
        "openh264 installs from its disabled repo, so its stale RPM is the case under test:\n{containerfile}"
    );
    assert!(
        root.join("repoless-packages")
            .join(CACHED_TOOL_RPM)
            .exists(),
        "the included cached RPM is extracted, or the reverse guard is vacuous over it"
    );
}
