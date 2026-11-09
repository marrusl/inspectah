//! The export half of the staged-upload lifecycle (§ 3.3, § 8.2).
//!
//! The staging boundary -- which names reach the filesystem -- is pinned in
//! the web crate's `api_test.rs`, where the handler lives. This file is the
//! other end: which staged files reach the export tarball, which names
//! reach a `COPY`, and what happens to the ones that cannot be proved.
//!
//! Every expected value here is transcribed from what the value *should*
//! be, not pasted from a run.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::FindingKind;
use inspectah_core::types::rpm::{PackageEntry, RpmSection};
use inspectah_refine::session::{
    RefineSession, RemovalOutcome, UploadMatchKind, render_refine_export,
};
use inspectah_refine::types::{ItemId, RefineError, RefinementOp};

/// The filename an upload of exactly the host's version (9-11.el9)
/// arrives under.
const EXACT_RPM: &str = "epel-release-9-11.el9.noarch.rpm";
/// A newer upload for the same package.
const NEWER_RPM: &str = "epel-release-9-12.el9.noarch.rpm";
/// A Fedora-style upload filename: not a NEVRA, matched by name and arch
/// after the matcher shortens the extracted name.
const FEDORA_RPM: &str = "epel-release-latest-9.noarch.rpm";
/// A filename the matcher parses and no fixture package claims: its
/// shortened candidates are `epel-next-release-latest`,
/// `epel-next-release`, `epel-next`, `epel`, and none of them is
/// `epel-release`.
const UNMATCHED_RPM: &str = "epel-next-release-latest-9.noarch.rpm";
/// The `name.arch` both of those belong to.
const CANONICAL: &str = "epel-release.noarch";
/// The upload directory's name, a subdirectory so a test can prove nothing
/// was written beside it.
const UPLOAD_SUBDIR: &str = "snap-uploads";

// -- fixtures --------------------------------------------------------------

/// One repo-less `epel-release` at 9-11.el9, included, with no cached RPM.
fn repoless_snapshot() -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "epel-release".into(),
            version: "9".into(),
            release: "11.el9".into(),
            arch: "noarch".into(),
            disposition: FindingKind::included(),
            repoless_annotation: "No repo source".into(),
            ..Default::default()
        }],
        ..Default::default()
    });
    snap
}

/// [`repoless_snapshot`] as the collector reports a package the host's dnf
/// cache already supplies: `repoless_cached` with `cache_path` naming a
/// file outside the upload directory, and no upload. This is the state
/// `prior_cache` exists to restore.
fn dnf_cached_snapshot(cache_path: &Path) -> InspectionSnapshot {
    let mut snap = repoless_snapshot();
    let pkg = &mut snap.rpm.as_mut().unwrap().packages_added[0];
    pkg.repoless_cached = true;
    pkg.cache_path = Some(cache_path.to_string_lossy().into_owned());
    snap
}

/// A session on [`repoless_snapshot`] with its upload directory prepared,
/// and the canonical path `render_refine_export` is given.
struct Harness {
    tmp: tempfile::TempDir,
    session: RefineSession,
    upload_dir: PathBuf,
}

impl Harness {
    fn new() -> Self {
        Self::with_snapshot(repoless_snapshot())
    }

    fn with_snapshot(snap: InspectionSnapshot) -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let mut session = RefineSession::new(snap);
        session.set_upload_dir(tmp.path().join(UPLOAD_SUBDIR));
        session.ensure_upload_dir().unwrap();
        let upload_dir = session.upload_dir_canon().unwrap().to_path_buf();
        Self {
            tmp,
            session,
            upload_dir,
        }
    }

    /// Stage `bytes` under `filename` and record it, the way
    /// `inspectah_web::upload::commit` does: match, place by rename, then
    /// `set_uploaded`. Returns the match the session made.
    ///
    /// The rename is `NamedTempFile::persist`, the same call the handler
    /// makes, so a destination that cannot be renamed over fails here the
    /// way it fails there.
    fn upload(&mut self, filename: &str, bytes: &[u8]) -> Result<UploadMatchKind, std::io::Error> {
        let dest = self.upload_dir.join(filename);
        let matched = self.session.match_uploaded_rpm(filename);
        let part = tempfile::Builder::new()
            .prefix(".upload-")
            .suffix(".part")
            .tempfile_in(&self.upload_dir)
            .unwrap();
        std::fs::write(part.path(), bytes).unwrap();
        part.persist(&dest).map_err(|e| e.error)?;
        let kind = matched.as_ref().map(|m| m.match_kind);
        if let Some(m) = matched {
            self.session
                .set_uploaded(&m.canonical, dest, m.uploaded_version.clone());
        }
        Ok(kind.expect("the fixture's filenames all match epel-release.noarch"))
    }

    /// Record a staged upload the session never placed: the snapshot
    /// fields a scan can arrive with, set directly.
    fn claim_upload(&mut self, cache_path: &Path, uploaded_version: Option<&str>) {
        self.session.set_uploaded(
            CANONICAL,
            cache_path.to_path_buf(),
            uploaded_version.map(str::to_string),
        );
    }

    /// Include every repo-less package.
    ///
    /// Refine pre-excludes a package with no repository provenance, so a
    /// fixture built with `FindingKind::included()` still projects as
    /// excluded; the operator saying yes is a `SetInclude` op, and
    /// without it every assertion about an active `COPY` here would be
    /// testing the pre-excluded comment block instead.
    fn include_all(&mut self) {
        let names: Vec<(String, String)> = self
            .session
            .snapshot()
            .rpm
            .as_ref()
            .unwrap()
            .packages_added
            .iter()
            .map(|p| (p.name.clone(), p.arch.clone()))
            .collect();
        for (name, arch) in names {
            self.set_include(&name, &arch, true);
        }
    }

    fn set_include(&mut self, name: &str, arch: &str, include: bool) {
        self.session
            .apply(RefinementOp::SetInclude {
                item_id: ItemId::Package {
                    name: name.into(),
                    arch: arch.into(),
                },
                include,
            })
            .unwrap();
    }

    /// Export, and return the tarball path with the warnings.
    fn export(&self, source: Option<&Path>) -> (PathBuf, Vec<String>) {
        let out = self.tmp.path().join("export.tar.gz");
        let warnings = render_refine_export(
            &self.session.snapshot_projected(),
            &out,
            None,
            None,
            source,
            Some(&self.upload_dir),
        )
        .unwrap();
        (out, warnings)
    }

    fn export_err(&self, source: Option<&Path>) -> RefineError {
        let out = self.tmp.path().join("export-err.tar.gz");
        render_refine_export(
            &self.session.snapshot_projected(),
            &out,
            None,
            None,
            source,
            Some(&self.upload_dir),
        )
        .expect_err("the export must refuse")
    }

    fn preview(&self) -> &str {
        &self.session.view().containerfile_preview
    }

    /// The fixture package named `name`, as the session holds it.
    fn entry(&self, name: &str) -> &PackageEntry {
        self.session
            .snapshot()
            .rpm
            .as_ref()
            .unwrap()
            .packages_added
            .iter()
            .find(|p| p.name == name)
            .expect("the fixture carries this package")
    }
}

/// The text of a `TarballError`, which is a refusal's whole message.
fn tarball_message(err: RefineError) -> String {
    match err {
        RefineError::TarballError(message) => message,
        other => panic!("expected TarballError, got {other:?}"),
    }
}

/// A fresh session over an upload directory an earlier session left
/// behind, which is what a resume is.
fn resume_over(dir: &Path) -> (RefineSession, Vec<String>) {
    resume_with(repoless_snapshot(), dir)
}

/// The same, over a snapshot that already carries staged-upload state.
fn resume_with(snap: InspectionSnapshot, dir: &Path) -> (RefineSession, Vec<String>) {
    let mut session = RefineSession::new(snap);
    session.set_upload_dir(dir.to_path_buf());
    let warnings = {
        let (_, warnings) = session.ensure_upload_dir().expect("the directory is ours");
        warnings.to_vec()
    };
    (session, warnings)
}

/// [`repoless_snapshot`] plus a `custom-tool.x86_64` that names
/// `cache_path` as its staged file.
///
/// The filename the tests give it is one the matcher parses as
/// `epel-release.noarch`'s, so the two packages disagree about who owns
/// the file and the rescan has to choose. `uploaded` is a parameter
/// because `uploaded` and `cache_path` deserialize separately: a
/// snapshot can name a path without claiming to have staged it.
fn claimed_by_custom_tool(cache_path: &Path, uploaded: bool) -> InspectionSnapshot {
    let mut snap = repoless_snapshot();
    snap.rpm
        .as_mut()
        .unwrap()
        .packages_added
        .push(PackageEntry {
            name: "custom-tool".into(),
            version: "1.0".into(),
            release: "1.el9".into(),
            arch: "x86_64".into(),
            disposition: FindingKind::included(),
            repoless_annotation: "No repo source".into(),
            repoless_cached: true,
            uploaded,
            cache_path: Some(cache_path.to_string_lossy().into_owned()),
            ..Default::default()
        });
    snap
}

/// An upload directory reachable by two spellings, which is the
/// condition every path comparison in the upload path has to survive.
///
/// `<tmp>/link` is a symlink to `<tmp>/real`, and the session is given
/// the directory through `link` while `canonicalize` resolves it through
/// `real`. The link is one level up from the directory itself because
/// `prepare_upload_dir` refuses a symlink at the final component --
/// which is also where a real one would be, in a home directory or a
/// `/var` reached through one.
struct SymlinkedUploadDir {
    _tmp: tempfile::TempDir,
    /// The spelling the session is given, and the one `cache_path` is
    /// recorded under.
    given: PathBuf,
}

fn symlinked_upload_dir() -> SymlinkedUploadDir {
    let tmp = tempfile::tempdir().unwrap();
    let real = tmp.path().join("real");
    std::fs::create_dir(&real).unwrap();
    std::os::unix::fs::symlink(&real, tmp.path().join("link")).unwrap();
    let given = tmp.path().join("link").join(UPLOAD_SUBDIR);
    std::fs::create_dir(&given).unwrap();
    assert_ne!(
        given,
        real.canonicalize().unwrap().join(UPLOAD_SUBDIR),
        "the fixture is only a fixture if the two spellings differ"
    );
    SymlinkedUploadDir { _tmp: tmp, given }
}

// -- tarball and Containerfile readers -------------------------------------

/// One member of an export tarball.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Member {
    entry_type: tar::EntryType,
    bytes: Vec<u8>,
}

/// Every member of `path`, keyed by its path below the top-level directory.
fn members(path: &Path) -> BTreeMap<String, Member> {
    let file = std::fs::File::open(path).unwrap();
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(file));
    let mut out = BTreeMap::new();
    for entry in archive.entries().unwrap() {
        let mut entry = entry.unwrap();
        let entry_type = entry.header().entry_type();
        let full = entry.path().unwrap().to_string_lossy().into_owned();
        let rel = match full.find('/') {
            Some(i) => full[i + 1..].to_string(),
            None => continue,
        };
        if rel.is_empty() {
            continue;
        }
        let mut bytes = Vec::new();
        if entry_type.is_file() {
            std::io::copy(&mut entry, &mut bytes).unwrap();
        }
        out.insert(rel, Member { entry_type, bytes });
    }
    out
}

/// The members under `repoless-packages/`, by basename.
fn repoless_members(path: &Path) -> BTreeMap<String, Member> {
    members(path)
        .into_iter()
        .filter_map(|(k, v)| {
            k.strip_prefix("repoless-packages/")
                .filter(|rest| !rest.is_empty())
                .map(|rest| (rest.to_string(), v))
        })
        .collect()
}

fn containerfile(path: &Path) -> String {
    let member = members(path)
        .remove("Containerfile")
        .expect("every export has a Containerfile");
    String::from_utf8(member.bytes).unwrap()
}

/// The lines a build actually runs: everything that is not a comment.
fn active_lines(containerfile: &str) -> Vec<&str> {
    containerfile
        .lines()
        .filter(|l| !l.trim_start().starts_with('#') && !l.trim().is_empty())
        .collect()
}

/// The three lines an install of `basename` renders that name the file:
/// its `COPY`, its operand in the combined `dnf install`, and its removal.
fn install_lines(basename: &str) -> Vec<String> {
    vec![
        format!("COPY repoless-packages/{basename} /tmp/"),
        format!("    /tmp/{basename} \\"),
        format!("    && rm /tmp/{basename} \\"),
    ]
}

/// The active lines of the repo-less section only: the rest of the
/// Containerfile (`FROM`, the package install) is not this test's subject.
fn repoless_active_lines(containerfile: &str) -> Vec<String> {
    active_lines(containerfile)
        .into_iter()
        .filter(|l| l.contains("repoless-packages/") || l.contains("/tmp/"))
        .map(str::to_string)
        .collect()
}

/// The § 3.3 demotion comment for `epel-release.noarch`.
fn demotion_fixme() -> String {
    "# FIXME: upload the RPM for epel-release.noarch again in refine; \
     the staged file could not be verified for export"
        .into()
}

/// The § 3.3 warning the export response carries for the same package.
fn demotion_warning() -> String {
    "epel-release.noarch: staged RPM not verified, not exported".into()
}

/// Write `path` into a GNU header's 100-byte name field verbatim.
fn set_raw_path(header: &mut tar::Header, path: &str) {
    let bytes = path.as_bytes();
    assert!(bytes.len() < 100, "{path} needs a long-name entry");
    let name = &mut header.as_old_mut().name;
    name.fill(0);
    name[..bytes.len()].copy_from_slice(bytes);
}

/// Build a source scan tarball from entries described as
/// `(path, EntryType, payload-or-link-target)`.
fn source_tarball(dir: &Path, entries: &[(&str, tar::EntryType, &str)]) -> PathBuf {
    let path = dir.join("source.tar.gz");
    let file = std::fs::File::create(&path).unwrap();
    let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::fast());
    let mut builder = tar::Builder::new(encoder);
    for (name, entry_type, payload) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_mode(0o644);
        header.set_entry_type(*entry_type);
        // The path goes into the header's name field directly rather than
        // through `set_path`, which refuses a `..` component. A real
        // hostile tarball has no such scruples, and the whole point of
        // these rows is that `tar::Entry::path()` hands the traversal
        // straight to the reader.
        set_raw_path(&mut header, &format!("source/{name}"));
        if *entry_type == tar::EntryType::Regular {
            header.set_size(payload.len() as u64);
            header.set_cksum();
            builder.append(&header, payload.as_bytes()).unwrap();
        } else {
            header.set_size(0);
            header.set_link_name(payload).unwrap();
            header.set_cksum();
            builder.append(&header, std::io::empty()).unwrap();
        }
    }
    builder.into_inner().unwrap().finish().unwrap();
    path
}

// -- the matched cases (§ 8.2) ---------------------------------------------

#[test]
fn an_exact_nevra_upload_exports_and_names_itself() {
    let mut h = Harness::new();
    assert_eq!(
        h.upload(EXACT_RPM, b"exact").unwrap(),
        UploadMatchKind::Exact
    );
    h.include_all();

    let (out, warnings) = h.export(None);
    assert_eq!(warnings, Vec::<String>::new());
    assert_eq!(
        repoless_active_lines(&containerfile(&out)),
        install_lines(EXACT_RPM)
    );
    assert_eq!(
        repoless_members(&out)
            .get(EXACT_RPM)
            .map(|m| m.bytes.clone()),
        Some(b"exact".to_vec())
    );
    // The uploaded version equals the host's, so there is nothing to
    // report about a difference.
    assert!(!containerfile(&out).contains("the host had"));
    assert!(h.session.snapshot().rpm.as_ref().unwrap().packages_added[0].uploaded);
}

#[test]
fn an_upload_of_a_differing_version_names_only_itself() {
    let mut h = Harness::new();
    assert_eq!(
        h.upload(NEWER_RPM, b"newer").unwrap(),
        UploadMatchKind::VersionDiffers
    );
    h.include_all();

    let (out, _) = h.export(None);
    let cf = containerfile(&out);
    assert_eq!(repoless_active_lines(&cf), install_lines(NEWER_RPM));
    assert!(
        cf.contains("# uploaded epel-release-9-12.el9.noarch; the host had 9-11.el9"),
        "{cf}"
    );
    assert_eq!(
        repoless_members(&out).keys().collect::<Vec<_>>(),
        vec![NEWER_RPM]
    );
    // Nothing anywhere may still name the version the host had: the
    // previous allowlist matched on exactly that filename.
    assert!(!cf.contains(EXACT_RPM), "{cf}");
}

#[test]
fn a_fedora_style_filename_exports_under_the_name_it_arrived_with() {
    let mut h = Harness::new();
    let name = "epel-release-latest-9.noarch.rpm";
    h.upload(name, b"fedora").unwrap();
    h.include_all();

    let (out, _) = h.export(None);
    let cf = containerfile(&out);
    assert_eq!(repoless_active_lines(&cf), install_lines(name));
    // `9` is the version-release the filename carries, re-derived from it.
    assert!(
        cf.contains("# uploaded epel-release-9.noarch; the host had 9-11.el9"),
        "{cf}"
    );
    assert_eq!(
        repoless_members(&out).keys().collect::<Vec<_>>(),
        vec![name]
    );
}

#[test]
fn a_placement_failure_after_staging_leaves_the_package_untouched() {
    let mut h = Harness::new();
    h.upload(EXACT_RPM, b"first").unwrap();
    h.include_all();

    // A non-empty directory at the destination: `rename(2)` refuses it.
    // A read-only directory would fail the staging step instead and test
    // the wrong thing.
    let blocked = h.upload_dir.join(NEWER_RPM);
    std::fs::create_dir(&blocked).unwrap();
    std::fs::write(blocked.join("occupant"), b"x").unwrap();

    let err = h.upload(NEWER_RPM, b"second").unwrap_err();
    assert!(err.raw_os_error().is_some(), "{err}");

    // The previous upload is what the package still points at, and the
    // part file is gone.
    let pkg = &h.session.snapshot().rpm.as_ref().unwrap().packages_added[0];
    assert_eq!(
        pkg.cache_path.as_deref(),
        Some(h.upload_dir.join(EXACT_RPM).to_string_lossy().as_ref())
    );
    let parts: Vec<_> = std::fs::read_dir(&h.upload_dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".part"))
        .collect();
    assert_eq!(parts, Vec::<String>::new());

    let (out, warnings) = h.export(None);
    assert_eq!(warnings, Vec::<String>::new());
    assert_eq!(
        repoless_active_lines(&containerfile(&out)),
        install_lines(EXACT_RPM)
    );
}

#[test]
fn replacing_an_upload_exports_only_the_replacement() {
    let mut h = Harness::new();
    h.upload(EXACT_RPM, b"first").unwrap();
    std::fs::remove_file(h.upload_dir.join(EXACT_RPM)).unwrap();
    h.upload(NEWER_RPM, b"second").unwrap();
    h.include_all();

    let (out, _) = h.export(None);
    let exported = repoless_members(&out);
    assert_eq!(exported.keys().collect::<Vec<_>>(), vec![NEWER_RPM]);
    assert_eq!(exported[NEWER_RPM].bytes, b"second".to_vec());
    assert_eq!(
        repoless_active_lines(&containerfile(&out)),
        install_lines(NEWER_RPM)
    );
}

#[test]
fn uploading_the_same_filename_twice_exports_one_member() {
    let mut h = Harness::new();
    h.upload(EXACT_RPM, b"first").unwrap();
    h.upload(EXACT_RPM, b"second").unwrap();
    h.include_all();

    let (out, _) = h.export(None);
    let exported = repoless_members(&out);
    assert_eq!(exported.keys().collect::<Vec<_>>(), vec![EXACT_RPM]);
    assert_eq!(exported[EXACT_RPM].bytes, b"second".to_vec());
    assert_eq!(
        repoless_active_lines(&containerfile(&out)),
        install_lines(EXACT_RPM)
    );
}

#[test]
fn an_excluded_package_exports_no_staged_upload() {
    let mut h = Harness::new();
    h.upload(EXACT_RPM, b"exact").unwrap();
    h.include_all();
    h.set_include("epel-release", "noarch", false);

    let (out, warnings) = h.export(None);
    // Excluded is a decision, not a failure: nothing to warn about.
    assert_eq!(warnings, Vec::<String>::new());
    assert_eq!(repoless_members(&out), BTreeMap::new());
    assert_eq!(
        repoless_active_lines(&containerfile(&out)),
        Vec::<String>::new()
    );
}

// -- render safety (§ 8.2) -------------------------------------------------

#[test]
fn a_hostile_staged_filename_reaches_no_copy_and_no_member() {
    // `uploaded: true`, an included package, and a *real file* at a
    // hostile name in the upload directory: the file exists, so the only
    // thing standing between it and a `RUN dnf install` naming `/tmp/<name>`
    // is the allowlist. Named here so the guard has a reaching input.
    let hostile = "epel-release-9$(id)-1.noarch.rpm";
    let mut h = Harness::new();
    std::fs::write(h.upload_dir.join(hostile), b"hostile").unwrap();
    let path = h.upload_dir.join(hostile);
    h.claim_upload(&path, Some("9$(id)-1"));
    h.include_all();

    let (out, warnings) = h.export(None);
    let cf = containerfile(&out);
    assert!(!cf.contains("$("), "{cf}");
    assert!(!cf.contains("COPY repoless-packages/"), "{cf}");
    assert!(cf.lines().any(|l| l == demotion_fixme()), "{cf}");
    assert_eq!(repoless_members(&out), BTreeMap::new());
    assert_eq!(warnings, vec![demotion_warning()]);
}

#[test]
fn a_hostile_uploaded_version_in_the_snapshot_reaches_no_comment() {
    // The filename is clean and the file is real, so this upload verifies
    // and renders an active block. The snapshot's `uploaded_version` is
    // the hostile value, and it must not appear: the rendered version is
    // re-derived from the proved basename.
    let mut h = Harness::new();
    std::fs::write(h.upload_dir.join(NEWER_RPM), b"newer").unwrap();
    let path = h.upload_dir.join(NEWER_RPM);
    h.claim_upload(&path, Some("9\nRUN id"));
    h.include_all();

    let (out, warnings) = h.export(None);
    let cf = containerfile(&out);
    assert_eq!(warnings, Vec::<String>::new());
    assert_eq!(repoless_active_lines(&cf), install_lines(NEWER_RPM));
    assert!(!cf.contains("RUN id"), "{cf}");
    assert!(
        cf.contains("# uploaded epel-release-9-12.el9.noarch; the host had 9-11.el9"),
        "{cf}"
    );
}

// -- export confinement (§ 8.2) --------------------------------------------

#[test]
fn an_unprovable_staged_path_is_demoted_and_warned_about() {
    // Each row plants a file whose *bytes* a passing export would carry,
    // then records a `cache_path` the export must refuse. Transcribed
    // from what each one is: the second column is the path the snapshot
    // claims, the third is where the bytes really live.
    #[derive(Clone, Copy)]
    enum Plant {
        /// A regular file outside the upload directory.
        Outside,
        /// A valid-named symlink inside the directory, pointing outside.
        SymlinkOut,
        /// A directory at the staged name. This is the one row that
        /// reaches step 4's regular-file check alone: step 5's
        /// `(dev, ino)` comparison already refuses a symlink, and a fifo
        /// cannot be tested without risking a hang if the check regresses.
        DirectoryAtTheName,
        /// A real file in a subdirectory of the upload directory.
        Subdirectory,
        /// A `/var/cache/dnf` path, as a collector would record.
        DnfCache,
        /// Nothing at all.
        Missing,
    }
    let cases: &[(&str, Plant)] = &[
        ("cache_path outside the upload directory", Plant::Outside),
        (
            "an in-directory symlink pointing outside",
            Plant::SymlinkOut,
        ),
        ("a directory at the staged name", Plant::DirectoryAtTheName),
        ("cache_path in a subdirectory", Plant::Subdirectory),
        ("a /var/cache/dnf path", Plant::DnfCache),
        ("a missing file", Plant::Missing),
    ];

    for (label, plant) in cases {
        let mut h = Harness::new();
        let outside = h.tmp.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        let sentinel = outside.join(EXACT_RPM);

        let claimed = match plant {
            Plant::Outside => {
                std::fs::write(&sentinel, b"sentinel").unwrap();
                sentinel.clone()
            }
            Plant::SymlinkOut => {
                std::fs::write(&sentinel, b"sentinel").unwrap();
                let link = h.upload_dir.join(EXACT_RPM);
                std::os::unix::fs::symlink(&sentinel, &link).unwrap();
                link
            }
            Plant::DirectoryAtTheName => {
                let at_name = h.upload_dir.join(EXACT_RPM);
                std::fs::create_dir(&at_name).unwrap();
                std::fs::write(at_name.join("sentinel-inside"), b"sentinel").unwrap();
                at_name
            }
            Plant::Subdirectory => {
                let sub = h.upload_dir.join("sub");
                std::fs::create_dir_all(&sub).unwrap();
                std::fs::write(sub.join(EXACT_RPM), b"sentinel").unwrap();
                sub.join(EXACT_RPM)
            }
            Plant::DnfCache => PathBuf::from("/var/cache/dnf/epel/packages").join(EXACT_RPM),
            Plant::Missing => h.upload_dir.join(EXACT_RPM),
        };
        h.claim_upload(&claimed, None);
        h.include_all();

        // The export succeeds in every case; the package demotes.
        let (out, warnings) = h.export(None);
        let cf = containerfile(&out);
        assert!(!cf.contains("COPY repoless-packages/"), "{label}: {cf}");
        assert!(cf.lines().any(|l| l == demotion_fixme()), "{label}: {cf}");
        assert_eq!(warnings, vec![demotion_warning()], "{label}");
        assert_eq!(repoless_members(&out), BTreeMap::new(), "{label}");
        // Compared by content, so a member under any name fails this.
        assert!(
            !members(&out).values().any(|m| m.bytes == b"sentinel"),
            "{label}: the sentinel's bytes reached the tarball"
        );
    }
}

#[test]
fn a_fifo_at_the_staged_name_is_refused_before_anything_opens_it() {
    // The one input that reaches step 4's regular-file check *alone*. A
    // symlink and a directory at the staged name are both refused again
    // by step 5's `(dev, ino)` comparison -- checked by deleting step 4
    // and watching those two rows still refuse -- so neither of them can
    // tell whether step 4 is doing anything. A fifo can: opening one for
    // reading blocks until a writer arrives, so a check that happens
    // after the open never happens. The deadline below is the assertion.
    //
    // The whole sequence runs on the worker thread, not just the export.
    // The Containerfile preview runs the same verification, so without
    // step 4 the `SetInclude` that rebuilds the view blocks before the
    // export is reached at all -- which is the larger half of what this
    // guard is worth: a fifo under a staged name would wedge the running
    // server, not only one download.
    //
    // Two channels, so the `mkfifo` subprocess spawn -- the slowest and
    // most variable thing here, and not what is being measured -- is
    // waited for without a deadline and only the verification is timed.
    // On a failing run the worker stays blocked on the fifo forever and
    // its `TempDir` is never dropped, so a failure leaves one directory
    // behind under the system temp root. That is the cost of catching a
    // hang at all.
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut h = Harness::new();
        let fifo = h.upload_dir.join(EXACT_RPM);
        assert!(
            std::process::Command::new("mkfifo")
                .arg(&fifo)
                .status()
                .unwrap()
                .success()
        );
        h.claim_upload(&fifo, None);
        ready_tx.send(()).unwrap();
        h.include_all();
        let _ = done_tx.send(h.export(None).1);
    });
    ready_rx.recv().expect("the fifo setup must finish");
    let warnings = done_rx
        .recv_timeout(std::time::Duration::from_secs(20))
        .expect("the export must refuse the fifo rather than block opening it");
    assert_eq!(warnings, vec![demotion_warning()]);
}

#[test]
fn an_upload_claimed_with_no_prepared_directory_is_demoted() {
    // `upload_dir_canon` is `None` whenever nothing has prepared a
    // directory this session -- a CLI export, or a snapshot that simply
    // arrived with `uploaded: true` set. There is nothing to verify
    // against, so the package demotes, and this is the branch of
    // `prove_staged_uploads` that reaches that conclusion without
    // calling the verifier at all.
    let mut h = Harness::new();
    h.upload(EXACT_RPM, b"exact").unwrap();
    h.include_all();

    let out = h.tmp.path().join("no-dir-export.tar.gz");
    let warnings = render_refine_export(
        &h.session.snapshot_projected(),
        &out,
        None,
        None,
        None,
        None,
    )
    .unwrap();

    assert_eq!(warnings, vec![demotion_warning()]);
    assert_eq!(repoless_members(&out), BTreeMap::new());
    let cf = containerfile(&out);
    assert!(cf.lines().any(|l| l == demotion_fixme()), "{cf}");

    // The control: the same session with the directory passed exports
    // the file, so the demotion above is about the missing directory and
    // not about the fixture.
    let (with_dir, warnings) = h.export(None);
    assert_eq!(warnings, Vec::<String>::new());
    assert_eq!(
        repoless_members(&with_dir).keys().collect::<Vec<_>>(),
        vec![EXACT_RPM]
    );
}

#[test]
fn the_preview_and_the_export_agree_about_every_confinement_case() {
    // The preview runs the same verification without the copy. Without
    // this, the UI could promise a `COPY` the export then refuses -- and
    // the preview is what the operator decides from.
    let mut h = Harness::new();
    let outside = h.tmp.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join(EXACT_RPM), b"sentinel").unwrap();
    h.claim_upload(&outside.join(EXACT_RPM), None);
    h.include_all();

    assert!(!h.preview().contains("COPY repoless-packages/"));
    assert!(h.preview().lines().any(|l| l == demotion_fixme()));

    // And the reverse: a provable upload shows in both.
    let mut h = Harness::new();
    h.upload(EXACT_RPM, b"exact").unwrap();
    h.include_all();
    let (out, _) = h.export(None);
    assert_eq!(
        repoless_active_lines(h.preview()),
        repoless_active_lines(&containerfile(&out))
    );
    assert_eq!(repoless_active_lines(h.preview()), install_lines(EXACT_RPM));
}

// -- output ownership against the source tarball (§ 8.2) -------------------

/// The directory a source tarball's hostile entries try to reach.
fn sentinel_dir(h: &Harness) -> PathBuf {
    let dir = h.tmp.path().join("sentinel");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("target"), b"untouched").unwrap();
    dir
}

#[test]
fn a_link_in_the_source_tarball_cannot_become_the_exported_member() {
    for entry_type in [tar::EntryType::Symlink, tar::EntryType::Link] {
        let mut h = Harness::new();
        let sentinel = sentinel_dir(&h);
        let source = source_tarball(
            h.tmp.path(),
            &[(
                &format!("repoless-packages/{EXACT_RPM}"),
                entry_type,
                sentinel.join("target").to_string_lossy().as_ref(),
            )],
        );
        h.upload(EXACT_RPM, b"staged").unwrap();
        h.include_all();

        // The upload owns this path, so step 2e skips the entry whatever
        // type it is, and step 2f writes the staged bytes.
        let (out, warnings) = h.export(Some(&source));
        assert_eq!(warnings, Vec::<String>::new());
        let exported = repoless_members(&out);
        assert_eq!(exported.keys().collect::<Vec<_>>(), vec![EXACT_RPM]);
        assert_eq!(exported[EXACT_RPM].entry_type, tar::EntryType::Regular);
        assert_eq!(exported[EXACT_RPM].bytes, b"staged".to_vec());
        assert_eq!(
            repoless_active_lines(&containerfile(&out)),
            install_lines(EXACT_RPM)
        );
        assert_eq!(
            std::fs::read(sentinel.join("target")).unwrap(),
            b"untouched".to_vec()
        );
    }
}

#[test]
fn a_cached_rpm_is_not_exported_for_a_package_whose_upload_cannot_be_proved() {
    // The source tarball holds a perfectly good regular RPM at the
    // package's NEVRA. The package claims an upload that cannot be
    // proved. Neither source may produce a member: falling back on the
    // cached file would export something the operator replaced.
    for (label, plant_symlink) in [("an in-directory symlink", true), ("a missing file", false)] {
        let mut h = Harness::new();
        let outside = h.tmp.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join(EXACT_RPM), b"sentinel").unwrap();
        let source = source_tarball(
            h.tmp.path(),
            &[(
                &format!("repoless-packages/{EXACT_RPM}"),
                tar::EntryType::Regular,
                "cached bytes",
            )],
        );
        if plant_symlink {
            std::os::unix::fs::symlink(outside.join(EXACT_RPM), h.upload_dir.join(EXACT_RPM))
                .unwrap();
        }
        h.claim_upload(&h.upload_dir.join(EXACT_RPM), None);
        h.include_all();

        let (out, warnings) = h.export(Some(&source));
        let cf = containerfile(&out);
        assert_eq!(repoless_members(&out), BTreeMap::new(), "{label}");
        assert!(!cf.contains("COPY repoless-packages/"), "{label}: {cf}");
        assert!(cf.lines().any(|l| l == demotion_fixme()), "{label}: {cf}");
        assert_eq!(warnings, vec![demotion_warning()], "{label}");
    }
}

#[test]
fn a_proved_upload_replaces_the_cached_rpm_in_the_tarball() {
    let mut h = Harness::new();
    let source = source_tarball(
        h.tmp.path(),
        &[(
            &format!("repoless-packages/{EXACT_RPM}"),
            tar::EntryType::Regular,
            "cached bytes",
        )],
    );
    h.upload(EXACT_RPM, b"staged bytes").unwrap();
    h.include_all();

    let (out, _) = h.export(Some(&source));
    let exported = repoless_members(&out);
    assert_eq!(exported.keys().collect::<Vec<_>>(), vec![EXACT_RPM]);
    assert_eq!(exported[EXACT_RPM].bytes, b"staged bytes".to_vec());
}

#[test]
fn a_link_at_a_non_uploaded_cached_rpm_fails_the_export() {
    // No upload here: the package is `repoless_cached` straight from the
    // scan, so step 2e's allowlist does hold its NEVRA -- and that is
    // exactly the case where a link entry would make the export's member
    // a pointer at something the scan chose.
    for entry_type in [tar::EntryType::Symlink, tar::EntryType::Link] {
        let mut snap = repoless_snapshot();
        snap.rpm.as_mut().unwrap().packages_added[0].repoless_cached = true;
        let mut h = Harness::with_snapshot(snap);
        h.include_all();
        let sentinel = sentinel_dir(&h);
        let source = source_tarball(
            h.tmp.path(),
            &[(
                &format!("repoless-packages/{EXACT_RPM}"),
                entry_type,
                sentinel.join("target").to_string_lossy().as_ref(),
            )],
        );

        let err = h.export_err(Some(&source));
        assert!(
            matches!(&err, RefineError::TarballError(m) if m.contains(EXACT_RPM)),
            "{entry_type:?}: {err}"
        );
        assert!(!h.tmp.path().join("export-err.tar.gz").exists());
        assert_eq!(
            std::fs::read(sentinel.join("target")).unwrap(),
            b"untouched".to_vec()
        );
    }
}

#[test]
fn a_source_entry_that_escapes_the_payload_directory_fails_the_export() {
    // `tar::Entry::path()` does not clean `..`, so these arrive at the
    // extraction with their traversal intact. Both rows pair a snapshot
    // `name` with the tarball entry that matches the NEVRA it formats to,
    // which is what makes the allowlist a possible route in.
    let cases: &[(&str, &str)] = &[
        (
            "../repoless-packages/epel-release",
            "repoless-packages/../repoless-packages/epel-release-9-11.el9.noarch.rpm",
        ),
        (
            "../../sentinel",
            "repoless-packages/../../sentinel-9-11.el9.noarch.rpm",
        ),
    ];

    for (name, entry) in cases {
        for staged in [false, true] {
            let mut snap = repoless_snapshot();
            let pkg = &mut snap.rpm.as_mut().unwrap().packages_added[0];
            pkg.name = (*name).into();
            pkg.repoless_cached = true;
            let mut h = Harness::with_snapshot(snap);
            let sentinel = sentinel_dir(&h);
            let source = source_tarball(h.tmp.path(), &[(entry, tar::EntryType::Regular, "evil")]);
            if staged {
                let basename = Path::new(entry).file_name().unwrap().to_string_lossy();
                std::fs::write(h.upload_dir.join(basename.as_ref()), b"staged").unwrap();
                let path = h.upload_dir.join(basename.as_ref());
                h.session
                    .set_uploaded(&format!("{name}.noarch"), path, None);
            }

            h.include_all();
            let err = h.export_err(Some(&source));
            assert!(
                matches!(&err, RefineError::TarballError(m) if m.contains("escapes")),
                "{name} staged={staged}: {err}"
            );
            // Nothing was written beside the sentinel, and its own file is
            // as it was.
            let mut beside: Vec<String> = std::fs::read_dir(h.tmp.path())
                .unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            beside.sort();
            assert_eq!(
                beside,
                vec!["sentinel", "snap-uploads", "source.tar.gz"],
                "{name} staged={staged}"
            );
            assert_eq!(
                std::fs::read(sentinel.join("target")).unwrap(),
                b"untouched".to_vec()
            );
        }
    }
}

#[test]
fn a_destination_another_step_already_wrote_fails_the_export() {
    // The reaching input for step 2f's `create_new`: one package is
    // `repoless_cached` and not uploaded, so step 2e extracts its NEVRA
    // into `repoless-packages/`; a *different* package's staged upload
    // carries that same basename. Nothing in the upload path prevents
    // that -- the 409 only refuses a second package staging at a path
    // another package's `cache_path` already names.
    let mut snap = repoless_snapshot();
    let rpm = snap.rpm.as_mut().unwrap();
    rpm.packages_added[0].repoless_cached = true;
    rpm.packages_added.push(PackageEntry {
        name: "other-tool".into(),
        version: "1".into(),
        release: "1.el9".into(),
        arch: "noarch".into(),
        disposition: FindingKind::included(),
        repoless_annotation: "No repo source".into(),
        ..Default::default()
    });
    let mut h = Harness::with_snapshot(snap);
    let source = source_tarball(
        h.tmp.path(),
        &[(
            &format!("repoless-packages/{EXACT_RPM}"),
            tar::EntryType::Regular,
            "cached bytes",
        )],
    );
    std::fs::write(h.upload_dir.join(EXACT_RPM), b"staged").unwrap();
    let path = h.upload_dir.join(EXACT_RPM);
    h.session.set_uploaded("other-tool.noarch", path, None);
    h.include_all();

    let err = h.export_err(Some(&source));
    assert!(
        matches!(&err, RefineError::TarballError(m)
            if m.contains(EXACT_RPM) && m.contains("already has a writer")),
        "{err}"
    );
}

// -- removal and the rescan (§ 3.3, § 8.2) ---------------------------------

#[test]
fn removal_requires_an_upload_and_restores_the_prior_cache() {
    // An upload with nothing behind it: every field the upload set is
    // cleared. `uploaded_version` included -- the row renders it, and a
    // version string on a package with no RPM names an upload that is
    // gone.
    let mut h = Harness::new();
    h.upload(NEWER_RPM, b"newer").unwrap();
    let staged = h.upload_dir.join(NEWER_RPM);
    assert_eq!(
        h.entry("epel-release").uploaded_version.as_deref(),
        Some("9-12.el9"),
        "the upload is what sets the field this removal has to clear"
    );

    assert_eq!(
        h.session
            .unmark_uploaded_rpm("epel-release", "noarch")
            .expect("the removal is not refused"),
        RemovalOutcome::Removed
    );

    assert!(!staged.exists(), "the staged file is deleted");
    let pkg = h.entry("epel-release");
    assert!(!pkg.uploaded);
    assert!(!pkg.repoless_cached);
    assert_eq!(pkg.cache_path, None);
    assert_eq!(pkg.uploaded_version, None);

    // A package the collector found in the host's dnf cache. Nothing was
    // staged for it, so there is nothing here to remove and the file is
    // not this session's to unlink.
    let host_cache = tempfile::tempdir().unwrap();
    let cached = host_cache.path().join(EXACT_RPM);
    std::fs::write(&cached, b"the host's cached rpm").unwrap();
    let mut h = Harness::with_snapshot(dnf_cached_snapshot(&cached));

    assert_eq!(
        h.session
            .unmark_uploaded_rpm("epel-release", "noarch")
            .expect("the removal is not refused"),
        RemovalOutcome::CachedOnly
    );

    assert!(cached.exists(), "the host's cached RPM survives");
    let pkg = h.entry("epel-release");
    assert!(pkg.repoless_cached, "and its cache state is untouched");
    assert_eq!(
        pkg.cache_path.as_deref(),
        Some(cached.to_string_lossy().as_ref())
    );

    // The same package once an upload has displaced the cached RPM:
    // removing the upload hands the package back to the dnf cache rather
    // than leaving it with no RPM at all.
    h.upload(NEWER_RPM, b"newer").unwrap();
    let staged = h.upload_dir.join(NEWER_RPM);
    assert_eq!(
        h.entry("epel-release").cache_path.as_deref(),
        Some(staged.to_string_lossy().as_ref()),
        "the upload is what displaces the cache path this removal restores"
    );

    assert_eq!(
        h.session
            .unmark_uploaded_rpm("epel-release", "noarch")
            .expect("the removal is not refused"),
        RemovalOutcome::Removed
    );

    assert!(!staged.exists(), "the staged file is deleted");
    assert!(cached.exists(), "and the cached RPM it displaced is not");
    let pkg = h.entry("epel-release");
    assert!(!pkg.uploaded);
    assert!(pkg.repoless_cached, "the dnf cache supplies it again");
    assert_eq!(
        pkg.cache_path.as_deref(),
        Some(cached.to_string_lossy().as_ref())
    );
    assert_eq!(pkg.uploaded_version, None);
}

#[test]
fn removal_is_confined_to_the_staging_directory() {
    // `uploaded` and `cache_path` both deserialize from the scan, so the
    // pair proves nothing about where the file is. `claim_upload` sets
    // them the way a snapshot arrives with them: no rename put any of
    // these files where it is.

    // A path outside the upload directory.
    let mut h = Harness::new();
    let outside = h.tmp.path().join("id_ed25519.rpm");
    std::fs::write(&outside, b"not a staged rpm").unwrap();
    h.claim_upload(&outside, None);

    let err = h
        .session
        .unmark_uploaded_rpm("epel-release", "noarch")
        .expect_err("a cache_path outside the upload directory is refused");

    assert_eq!(
        tarball_message(err),
        format!(
            "refusing to remove a staged file: id_ed25519.rpm is not in {}",
            h.upload_dir.display()
        )
    );
    assert!(outside.exists(), "and nothing is deleted");
    assert!(
        h.entry("epel-release").uploaded,
        "a refusal leaves the entry as it was"
    );

    // A symlink inside the directory, wearing a name the rule accepts.
    let mut h = Harness::new();
    let sentinel = h.tmp.path().join("sentinel");
    std::fs::write(&sentinel, b"not a staged rpm").unwrap();
    let link = h.upload_dir.join(EXACT_RPM);
    std::os::unix::fs::symlink(&sentinel, &link).unwrap();
    h.claim_upload(&link, None);

    let err = h
        .session
        .unmark_uploaded_rpm("epel-release", "noarch")
        .expect_err("a symlink in the upload directory is refused");

    assert_eq!(
        tarball_message(err),
        format!("refusing to remove a staged file: {EXACT_RPM} is not a regular file")
    );
    assert!(sentinel.exists(), "the symlink's target is not followed");
    assert!(
        std::fs::symlink_metadata(&link).is_ok(),
        "and the symlink itself stays"
    );
    assert!(h.entry("epel-release").uploaded);

    // A basename the staged-name rule rejects, on a real file in the
    // directory: the name is checked before the location, so this is
    // refused even though it is where staged files live.
    let mut h = Harness::new();
    let bad = h.upload_dir.join("bad name.rpm");
    std::fs::write(&bad, b"not a staged rpm").unwrap();
    h.claim_upload(&bad, None);

    let err = h
        .session
        .unmark_uploaded_rpm("epel-release", "noarch")
        .expect_err("a basename outside the rule is refused");

    assert_eq!(
        tarball_message(err),
        "refusing to remove a staged file: \
         staged filename has a disallowed byte at position 3"
    );
    assert!(bad.exists(), "and nothing is deleted");
    assert!(h.entry("epel-release").uploaded);
}

#[test]
fn rescan_remarks_through_the_matcher_and_warns_on_the_rest() {
    let h = Harness::new();
    let dir = h.upload_dir.clone();
    let sentinel = h.tmp.path().join("sentinel");
    std::fs::write(&sentinel, b"not a staged rpm").unwrap();

    std::fs::write(dir.join(FEDORA_RPM), b"the resumed upload").unwrap();
    std::fs::write(dir.join(UNMATCHED_RPM), b"nothing claims this").unwrap();
    std::fs::write(dir.join("bad name.rpm"), b"outside the rule").unwrap();
    std::fs::write(dir.join(".upload-1234.part"), b"half an rpm").unwrap();
    std::os::unix::fs::symlink(&sentinel, dir.join("x.rpm")).unwrap();

    let (resumed, warnings) = resume_over(&dir);

    // In filename order: `bad name.rpm` (the space sorts first), then
    // `x.rpm`. The two valid staged names in between produce none.
    assert_eq!(
        warnings,
        [
            "skipped bad name.rpm in the upload directory: \
             staged filename has a disallowed byte at position 3",
            "skipped x.rpm in the upload directory: not a regular file",
        ]
    );

    let pkg = &resumed.snapshot().rpm.as_ref().unwrap().packages_added[0];
    assert!(
        pkg.uploaded,
        "a staged file the matcher claims is an upload"
    );
    assert!(pkg.repoless_cached);
    assert_eq!(
        pkg.cache_path.as_deref(),
        Some(dir.join(FEDORA_RPM).to_string_lossy().as_ref())
    );
    assert_eq!(
        pkg.uploaded_version.as_deref(),
        Some("9"),
        "the version re-derived from the filename, as the upload handler derived it"
    );

    assert!(
        dir.join(UNMATCHED_RPM).exists(),
        "an unmatched file stays staged"
    );
    assert!(
        dir.join("bad name.rpm").exists() && std::fs::symlink_metadata(dir.join("x.rpm")).is_ok(),
        "a skipped entry is skipped, not deleted"
    );
    assert!(
        !dir.join(".upload-1234.part").exists(),
        "a regular part file is swept before the rescan lists the directory, \
         so it is removed rather than warned about"
    );
}

#[test]
fn rescan_leaves_a_file_a_package_already_points_at() {
    // The input that reaches the already-decided branch: a snapshot that
    // deserialized with `uploaded` and a `cache_path` in the upload
    // directory, under a filename the matcher parses as a *different*
    // package's. Without the branch the rescan hands the file to
    // `epel-release.noarch`, and the operator's own assignment is gone
    // with no warning that it happened.
    let h = Harness::new();
    let dir = h.upload_dir.clone();
    let staged = dir.join(FEDORA_RPM);
    std::fs::write(&staged, b"custom-tool's file").unwrap();

    let (resumed, warnings) = resume_with(claimed_by_custom_tool(&staged, true), &dir);

    assert_eq!(warnings, Vec::<String>::new());
    let packages = &resumed.snapshot().rpm.as_ref().unwrap().packages_added;
    assert!(
        !packages[0].uploaded,
        "the matcher never gets a say about a file that is already spoken for"
    );
    assert_eq!(packages[0].cache_path, None);
    assert_eq!(
        packages[1].cache_path.as_deref(),
        Some(staged.to_string_lossy().as_ref()),
        "and the package that had it keeps it"
    );
}

#[test]
fn rescan_resolves_the_directory_before_deciding_who_owns_a_file() {
    // The same claim, recorded under the spelling the session was given
    // rather than the canonical one -- which is every claim, because
    // that is what the upload handler records. Comparing the two as
    // text loses the claim, and the test above cannot show it: its
    // harness hands out the canonical path, so both sides already
    // agree.
    let dirs = symlinked_upload_dir();
    let staged = dirs.given.join(FEDORA_RPM);
    std::fs::write(&staged, b"custom-tool's file").unwrap();

    let (resumed, warnings) = resume_with(claimed_by_custom_tool(&staged, true), &dirs.given);

    assert_eq!(warnings, Vec::<String>::new());
    let packages = &resumed.snapshot().rpm.as_ref().unwrap().packages_added;
    assert!(
        !packages[0].uploaded,
        "a claim survives the two spellings only if the parent is resolved"
    );
    assert_eq!(
        packages[1].cache_path.as_deref(),
        Some(staged.to_string_lossy().as_ref()),
        "and is left under the spelling it was recorded with"
    );
}

#[test]
fn rescan_does_not_honour_a_cache_path_with_no_upload() {
    // `uploaded` and `cache_path` deserialize separately, so a crafted
    // snapshot can name a file in the upload directory without claiming
    // to have staged it. Honouring that takes the basename out of the
    // matcher's reach invisibly: the already-decided branch writes no
    // warning, so the package the file really belongs to would just show
    // no upload, with nothing anywhere saying why.
    let h = Harness::new();
    let dir = h.upload_dir.clone();
    let staged = dir.join(FEDORA_RPM);
    std::fs::write(&staged, b"an rpm").unwrap();

    let (resumed, warnings) = resume_with(claimed_by_custom_tool(&staged, false), &dir);

    assert_eq!(warnings, Vec::<String>::new());
    let packages = &resumed.snapshot().rpm.as_ref().unwrap().packages_added;
    assert!(
        packages[0].uploaded,
        "with no upload claimed, the matcher decides and says so"
    );
    assert_eq!(
        packages[0].cache_path.as_deref(),
        Some(staged.to_string_lossy().as_ref())
    );
    assert!(!packages[1].uploaded);
}

#[test]
fn rescan_costs_one_generation_whatever_it_marks() {
    // A resume is one state change from the client's side, and each
    // invalidation is a full `recompute_view` that re-proves every
    // staged upload by opening it -- so a per-file bump is both the
    // wrong count and N passes over N files under the session lock.
    let h = Harness::new();
    let empty = Harness::new();
    let dir = h.upload_dir.clone();
    std::fs::write(dir.join(FEDORA_RPM), b"one").unwrap();
    std::fs::write(dir.join("custom-tool-1.0-1.el9.x86_64.rpm"), b"two").unwrap();

    let mut snap = repoless_snapshot();
    snap.rpm
        .as_mut()
        .unwrap()
        .packages_added
        .push(PackageEntry {
            name: "custom-tool".into(),
            version: "1.0".into(),
            release: "1.el9".into(),
            arch: "x86_64".into(),
            disposition: FindingKind::included(),
            repoless_annotation: "No repo source".into(),
            ..Default::default()
        });
    let (resumed, _) = resume_with(snap, &dir);
    let (untouched, _) = resume_over(&empty.upload_dir);

    let marked = resumed.snapshot().rpm.as_ref().unwrap();
    assert!(
        marked.packages_added.iter().all(|p| p.uploaded),
        "both files matched, so both packages are marked"
    );
    assert_eq!(
        resumed.generation(),
        untouched.generation() + 1,
        "two files, one invalidation"
    );
}

#[test]
fn rescan_names_two_packages_claiming_one_staged_file() {
    // Nothing here can fix this -- moving a staged file is not a
    // rescan's job -- so the alternative to the warning is the export
    // failing at `create_new`, by path and with no mention of either
    // package.
    let h = Harness::new();
    let dir = h.upload_dir.clone();
    let staged = dir.join(FEDORA_RPM);
    std::fs::write(&staged, b"one file, two claims").unwrap();

    let mut snap = claimed_by_custom_tool(&staged, true);
    let epel = &mut snap.rpm.as_mut().unwrap().packages_added[0];
    epel.repoless_cached = true;
    epel.uploaded = true;
    epel.cache_path = Some(staged.to_string_lossy().into_owned());

    let (_, warnings) = resume_with(snap, &dir);

    assert_eq!(
        warnings,
        [format!(
            "{FEDORA_RPM} in the upload directory is claimed by both {CANONICAL} \
             and custom-tool.x86_64; the export cannot write it for both"
        )]
    );
}

#[test]
fn rescan_marks_the_later_of_two_files_for_one_package() {
    // Both filenames match `epel-release.noarch`, so one of them has to
    // lose. `9-11` sorts before `9-12`, so the warning names the first.
    //
    // They are written in the opposite order deliberately, though no
    // test can hold that line: on a filesystem whose listing is hashed,
    // the write order does not reach the read order. What holds it is
    // that the rescan collects into a `BTreeSet`, so there is no sort
    // call to delete.
    let h = Harness::new();
    let dir = h.upload_dir.clone();
    std::fs::write(dir.join(NEWER_RPM), b"the later").unwrap();
    std::fs::write(dir.join(EXACT_RPM), b"the earlier").unwrap();

    let (resumed, warnings) = resume_over(&dir);

    assert_eq!(
        warnings,
        [format!(
            "{EXACT_RPM} in the upload directory also matches {CANONICAL}, \
             which {NEWER_RPM} now holds; it stays staged and unmarked"
        )]
    );
    let pkg = &resumed.snapshot().rpm.as_ref().unwrap().packages_added[0];
    assert_eq!(
        pkg.cache_path.as_deref(),
        Some(dir.join(NEWER_RPM).to_string_lossy().as_ref())
    );
    assert_eq!(pkg.uploaded_version.as_deref(), Some("9-12.el9"));
    assert!(
        dir.join(EXACT_RPM).exists(),
        "the file that lost stays staged for the operator to remove"
    );
}

// -- an upload wins over a disabled repo ------------------------------------

/// The host's version of the disabled-repo package, as its cached RPM.
const OPENH264_CACHED: &str = "openh264-2.4.1-1.el9.x86_64.rpm";
/// The operator's upload, a newer build of the same package.
const OPENH264_UPLOAD: &str = "openh264-2.4.2-1.el9.x86_64.rpm";

/// Top-level export members that carry no `COPY` because they are not
/// image content. The same list, with the same reasons, as
/// `NOT_IMAGE_CONTENT` in the CLI crate's COPY guard.
const NOT_IMAGE_CONTENT: &[&str] = &[
    "Containerfile",
    "inspection-snapshot.json",
    "audit-report.md",
    "schema",
    "inspectah-users.ks",
    "inspectah-users.toml",
    "users",
    "compose",
    "aggregate",
];

/// The reverse COPY guard over an export tarball: every staged file is
/// carried into the image by some active `COPY`, or lives under a member
/// that is not image content.
fn assert_every_staged_file_is_copied(tarball: &Path) {
    let file = containerfile(tarball);
    let sources: Vec<String> = active_lines(&file)
        .into_iter()
        .filter_map(|l| l.strip_prefix("COPY "))
        .filter_map(|rest| {
            let args: Vec<&str> = rest
                .split_whitespace()
                .filter(|a| !a.starts_with("--"))
                .collect();
            (args.len() >= 2).then(|| args[..args.len() - 1].to_vec())
        })
        .flatten()
        .map(|s| s.trim_matches('\'').trim_end_matches('/').to_string())
        .collect();
    let orphaned: Vec<String> = members(tarball)
        .into_iter()
        .filter(|(_, m)| m.entry_type.is_file())
        .map(|(rel, _)| rel)
        .filter(|rel| !NOT_IMAGE_CONTENT.contains(&rel.split('/').next().unwrap_or(rel)))
        .filter(|rel| {
            !sources
                .iter()
                .any(|s| rel == s || rel.starts_with(&format!("{s}/")))
        })
        .collect();
    assert!(
        orphaned.is_empty(),
        "staged with no COPY: {orphaned:?}\n{file}"
    );
}

/// `openh264` from `epel-cisco-openh264`, a repo the host keeps disabled
/// and whose `.repo` file the export carries, with its RPM in the host's
/// dnf cache. Without an upload it installs from that repo.
fn disabled_repo_snapshot(cache_path: &Path) -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "openh264".into(),
            version: "2.4.1".into(),
            release: "1.el9".into(),
            arch: "x86_64".into(),
            source_repo: "epel-cisco-openh264".into(),
            disposition: FindingKind::included(),
            repoless_cached: true,
            cache_path: Some(cache_path.to_string_lossy().into_owned()),
            repoless_annotation: "No repo source \u{2014} repo 'epel-cisco-openh264' not in \
                                  enabled repos \u{2014} cached RPM bundled"
                .into(),
            ..Default::default()
        }],
        repo_files: vec![inspectah_core::types::rpm::RepoFile {
            path: "/etc/yum.repos.d/fedora-cisco-openh264.repo".into(),
            content: "[epel-cisco-openh264]\nname=openh264\n\
                      metalink=https://example.invalid/metalink\nenabled=0\n"
                .into(),
            is_default_repo: false,
            disposition: FindingKind::included(),
            locked: false,
            aggregate: None,
        }],
        ..Default::default()
    });
    snap
}

/// An upload the export verifies wins over the disabled repo: the package
/// installs from the uploaded RPM in the repo-less block, with no
/// `--enablerepo` transaction, and the export stages the upload and
/// nothing else for it, not even the cached RPM a stale scan tarball
/// carries.
#[test]
fn an_upload_for_a_disabled_repo_package_exports_and_installs_from_the_upload() {
    let cache_dir = tempfile::tempdir().unwrap();
    let cache_path = cache_dir.path().join(OPENH264_CACHED);
    std::fs::write(&cache_path, b"cached").unwrap();
    let mut h = Harness::with_snapshot(disabled_repo_snapshot(&cache_path));
    h.set_include("openh264", "x86_64", true);

    // The control: without the upload the package installs from its repo.
    let (before, _) = h.export(None);
    assert!(
        containerfile(&before).contains("--enablerepo=epel-cisco-openh264"),
        "{}",
        containerfile(&before)
    );

    assert_eq!(
        h.upload(OPENH264_UPLOAD, b"uploaded").unwrap(),
        UploadMatchKind::VersionDiffers
    );
    // A scan tarball holding the cached RPM, as one written before the
    // route was shared would.
    let source = source_tarball(
        h.tmp.path(),
        &[(
            &format!("repoless-packages/{OPENH264_CACHED}"),
            tar::EntryType::Regular,
            "cached",
        )],
    );
    let (out, warnings) = h.export(Some(&source));
    let file = containerfile(&out);

    assert_eq!(warnings, Vec::<String>::new());
    assert!(!file.contains("--enablerepo"), "{file}");
    assert!(
        !active_lines(&file).contains(&"    openh264 \\"),
        "no install by name:\n{file}"
    );
    assert_eq!(repoless_active_lines(&file), install_lines(OPENH264_UPLOAD));
    assert_eq!(
        repoless_members(&out),
        BTreeMap::from([(
            OPENH264_UPLOAD.to_string(),
            Member {
                entry_type: tar::EntryType::Regular,
                bytes: b"uploaded".to_vec(),
            },
        )]),
        "the upload and nothing else is staged for the package"
    );
    assert_every_staged_file_is_copied(&out);
}

// -- a demoted repo-enabling package takes an upload -------------------------

/// The upload for the demoted `vendor-release`.
const VENDOR_UPLOAD: &str = "vendor-release-1-1.el9.noarch.rpm";
/// The FIXME a demoted `vendor-release` renders while nothing settles it.
const VENDOR_FIXME: &str = "# FIXME: include /etc/yum.repos.d/bootstrap.repo under Configuration \
                            Files, or re-include repo bootstrap, or upload the RPM; vendor-release \
                            installs from that repo definition, which this export does not carry";

/// `vendor-release`, installed `@commandline` and served by `[bootstrap]`,
/// so it is repo-enabling and installs by name while `bootstrap.repo`
/// ships. `bootstrap-tool` gives the section a member of its own, which
/// the repo control needs before it lets the operator drop the section.
fn repo_enabling_snapshot() -> InspectionSnapshot {
    let package = |name: &str, arch: &str, source_repo: &str| PackageEntry {
        name: name.into(),
        version: "1".into(),
        release: "1.el9".into(),
        arch: arch.into(),
        source_repo: source_repo.into(),
        disposition: FindingKind::included(),
        ..Default::default()
    };
    let mut vendor = package("vendor-release", "noarch", "@commandline");
    vendor.repo_enabling_from = vec![inspectah_core::types::repo::RepoId::new("bootstrap")];
    let mut snap = InspectionSnapshot::new();
    // A baseline, so the classifier keeps `bootstrap-tool` included and
    // the section has a member to drop.
    snap.baseline = Some(inspectah_core::baseline::BaselineData {
        image_digest: "sha256:test".into(),
        packages: std::collections::HashMap::new(),
        extracted_at: "2026-11-08T00:00:00Z".into(),
    });
    snap.rpm = Some(RpmSection {
        packages_added: vec![vendor, package("bootstrap-tool", "x86_64", "bootstrap")],
        repo_files: vec![inspectah_core::types::rpm::RepoFile {
            path: "/etc/yum.repos.d/bootstrap.repo".into(),
            content: "[bootstrap]\nbaseurl=https://example.invalid/bootstrap\nenabled=1\n".into(),
            is_default_repo: false,
            disposition: FindingKind::included(),
            locked: false,
            aggregate: None,
        }],
        ..Default::default()
    });
    snap
}

fn set_include_bootstrap(h: &mut Harness, include: bool) {
    h.session
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::Repo {
                path: "bootstrap".into(),
            },
            include,
        })
        .expect("the section is toggleable");
}

/// The FIXME of a demoted repo-enabling package asks for an upload, and
/// the upload is accepted: matching reads the projection, where the
/// demotion lives. The export then installs from the upload and the FIXME
/// is gone. Re-including the repo serves the package by name again, and
/// the upload is neither used nor staged.
#[test]
fn a_demoted_repo_enabling_package_takes_an_upload_until_its_repo_returns() {
    let mut h = Harness::with_snapshot(repo_enabling_snapshot());
    h.set_include("vendor-release", "noarch", true);
    set_include_bootstrap(&mut h, false);

    let (demoted, _) = h.export(None);
    assert!(
        containerfile(&demoted).contains(VENDOR_FIXME),
        "the control: the demotion asks for an upload:\n{}",
        containerfile(&demoted)
    );

    assert_eq!(
        h.upload(VENDOR_UPLOAD, b"vendor").unwrap(),
        UploadMatchKind::Exact,
        "the upload the FIXME asks for is accepted"
    );
    let (out, warnings) = h.export(None);
    let file = containerfile(&out);
    assert_eq!(warnings, Vec::<String>::new());
    assert!(
        !file.contains(VENDOR_FIXME),
        "the upload settles it:\n{file}"
    );
    assert_eq!(repoless_active_lines(&file), install_lines(VENDOR_UPLOAD));
    assert_eq!(
        repoless_members(&out).keys().collect::<Vec<_>>(),
        vec![VENDOR_UPLOAD]
    );
    assert_every_staged_file_is_copied(&out);

    set_include_bootstrap(&mut h, true);
    let (served, warnings) = h.export(None);
    let file = containerfile(&served);
    assert_eq!(warnings, Vec::<String>::new());
    assert!(
        active_lines(&file).contains(&"    vendor-release \\"),
        "served by its repo again, it installs by name:\n{file}"
    );
    assert_eq!(repoless_active_lines(&file), Vec::<String>::new(), "{file}");
    assert!(
        repoless_members(&served).is_empty(),
        "the upload is not staged without a COPY"
    );
    assert_every_staged_file_is_copied(&served);
}

/// The uploaded package installs before anything that needs it: the
/// `systemctl enable` of a unit it owns, and the plain install of an
/// enabled-repo package that depends on it. Installing it in a late
/// section left both of those to run against a package not yet there.
#[test]
fn an_uploaded_package_installs_before_its_units_and_its_dependents() {
    use inspectah_core::types::services::{
        PresetDefault, ServiceSection, ServiceStateChange, ServiceUnitState,
    };
    const UNIT: &str = "openh264-agent.service";

    let cache_dir = tempfile::tempdir().unwrap();
    let cache_path = cache_dir.path().join(OPENH264_CACHED);
    std::fs::write(&cache_path, b"cached").unwrap();
    let mut snap = disabled_repo_snapshot(&cache_path);
    {
        let rpm = snap.rpm.as_mut().unwrap();
        rpm.packages_added.push(PackageEntry {
            name: "vlc".into(),
            version: "3.0".into(),
            release: "1.el9".into(),
            arch: "x86_64".into(),
            source_repo: "appstream".into(),
            disposition: FindingKind::included(),
            ..Default::default()
        });
        rpm.leaf_packages = Some(vec!["vlc.x86_64".into()]);
        rpm.leaf_dep_tree = serde_json::json!({ "vlc.x86_64": ["openh264.x86_64"] });
    }
    snap.services = Some(ServiceSection {
        state_changes: vec![ServiceStateChange {
            unit: UNIT.into(),
            current_state: ServiceUnitState::Enabled,
            default_state: Some(PresetDefault::Disable),
            disposition: FindingKind::included(),
            locked: false,
            owning_package: Some("openh264".into()),
            aggregate: None,
            attention_reason: None,
            shadow_type: None,
            shadow_rationale: None,
        }],
        enabled_units: vec![],
        disabled_units: vec![],
        drop_ins: vec![],
        preset_matched_units: vec![],
    });
    let mut h = Harness::with_snapshot(snap);
    h.set_include("openh264", "x86_64", true);
    h.set_include("vlc", "x86_64", true);
    h.upload(OPENH264_UPLOAD, b"uploaded").unwrap();

    let (out, warnings) = h.export(None);
    let file = containerfile(&out);
    assert_eq!(warnings, Vec::<String>::new());
    let at = |needle: &str| {
        file.lines()
            .position(|l| l == needle)
            .unwrap_or_else(|| panic!("missing {needle:?}:\n{file}"))
    };
    let install = at(&format!("    /tmp/{OPENH264_UPLOAD} \\"));
    let enable = at(&format!("    {UNIT}"));
    let dependent = at("    vlc \\");
    assert!(
        install < enable && install < dependent,
        "install at {install}, enable at {enable}, dependent at {dependent}:\n{file}"
    );
    assert_eq!(
        file.matches(&format!("COPY repoless-packages/{OPENH264_UPLOAD}"))
            .count(),
        1,
        "installed once:\n{file}"
    );
    assert_every_staged_file_is_copied(&out);
}
