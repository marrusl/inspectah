//! The repo section owns its `.repo` files and the GPG keys they name, on
//! collector-built hosts (`process-docs/specs/proposed/2026-11-21-repo-
//! section-owns-repo-files-and-keys.md`, § 9).
//!
//! Every host is a [`UnitHost`] run through the real RPM, config, units
//! and scheduled-task inspectors and the scan's own calls. A hand-built
//! `RepoFile` or `ConfigFileEntry` would be free to encode a layout the
//! collectors cannot produce.

use std::collections::BTreeSet;
use std::path::Path;

use crate::unit_hosts::{
    UnitHost, export_session, reload, render_export, run_pipeline, scan, scan_finished,
    scan_finished_raw, scan_unredacted, session,
};
use inspectah_core::aggregate::merge_snapshots;
use inspectah_core::repo_ownership::{
    OwnedKind, check_repo_ownership, repo_ownership, settle_repo_ownership,
};
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::FindingKind;
use inspectah_core::types::config::ConfigShadow;
use inspectah_core::types::destination::Destination;
use inspectah_core::types::rpm::KeyRejection;
use inspectah_pipeline::export_warning::{ExportWarning, ExportWarningKind};
use inspectah_refine::repo_index::RepoIndex;
use inspectah_refine::session::RefineSession;
use inspectah_refine::types::{
    ContentHash, ItemId, NoOwnerCause, RefineError, RefinementOp, RepoControl, RepoOwnedExport,
    RepoProvenance,
};

const CUSTOM_REPO: &str = "/etc/yum.repos.d/custom.repo";
const CUSTOM_KEY: &str = "/etc/pki/rpm-gpg/RPM-GPG-KEY-custom";
const EPEL_REPO: &str = "/etc/yum.repos.d/epel.repo";
const EPEL_KEY: &str = "/etc/pki/rpm-gpg/RPM-GPG-KEY-EPEL-9";
const ORPHAN_KEY: &str = "/etc/pki/rpm-gpg/RPM-GPG-KEY-orphan";
const RPMNEW: &str = "/etc/yum.repos.d/custom.repo.rpmnew";
const REPO_COPY: &str = "COPY config/etc/yum.repos.d/ /etc/yum.repos.d/";
const KEY_COPY: &str = "COPY config/etc/pki/rpm-gpg/ /etc/pki/rpm-gpg/";

/// A public key block, as the scan accepts it.
const PGP: &str =
    "-----BEGIN PGP PUBLIC KEY BLOCK-----\nmQENBFtest\n-----END PGP PUBLIC KEY BLOCK-----\n";

/// What only private key material holds, so a test can look for it.
const PRIVATE_MATERIAL: &str = "lQOYBPRIVATEKEYMATERIAL0123456789";

/// A public key block, then a private one, after a line of prose: not a
/// whole-file key, so the read gate lets the RPM reader and the config walk
/// read it, and the RPM reader rejects it for its private armor.
fn private_beside_public() -> String {
    format!(
        "Exported for the build team.\n{PGP}-----BEGIN PGP PRIVATE KEY BLOCK-----\n\
         {PRIVATE_MATERIAL}\n-----END PGP PRIVATE KEY BLOCK-----\n"
    )
}

fn custom_repo() -> String {
    format!(
        "[custom]\nname=Custom\nbaseurl=https://custom.example/repo\nenabled=1\n\
         gpgkey=file://{CUSTOM_KEY}\n"
    )
}

fn epel_repo() -> String {
    format!(
        "[epel]\nname=EPEL\nmetalink=https://mirrors.example/epel\nenabled=1\n\
         gpgkey=file://{EPEL_KEY}\n\n[epel-debuginfo]\nname=EPEL debug\n\
         metalink=https://mirrors.example/epel-debug\nenabled=0\ngpgkey=file://{EPEL_KEY}\n"
    )
}

/// The host § 9 uses throughout: an unowned `custom.repo` and its key, a
/// modified `epel.repo` owned by `epel-release` with its key, a key no
/// repo names, a `.rpmnew` beside `custom.repo`, and members `tool-a` and
/// `tool-b` from `custom`, `bat` from `epel`. EPEL's key is named
/// literally: the scan's variable expansion has its own tests.
fn base_host() -> UnitHost {
    UnitHost::new()
        .base_package("bash")
        .file(CUSTOM_REPO, &custom_repo())
        .file(CUSTOM_KEY, PGP)
        .owned_file(EPEL_REPO, &epel_repo(), "epel-release")
        .owned_file(EPEL_KEY, PGP, "epel-release")
        .rpm_va(&format!("S.5....T.  c {EPEL_REPO}\n"), 1)
        .file(ORPHAN_KEY, PGP)
        .file(RPMNEW, &custom_repo())
        .package("epel-release")
        .package_from("tool-a", "custom")
        .package_from("tool-b", "custom")
        .package_from("bat", "epel")
}

/// The parser inputs of T1 to T3, added to `host`.
fn with_parser_inputs(host: UnitHost) -> UnitHost {
    let key = |name: &str| format!("/etc/pki/rpm-gpg/{name}");
    host.file(
        "/etc/yum.repos.d/cont.repo",
        "[cont]\nbaseurl=https://cont.example/\ngpgkey=file:///etc/pki/rpm-gpg/A\n  \
         file:///etc/pki/rpm-gpg/B\n",
    )
    .file(
        "/etc/yum.repos.d/super.repo",
        "[sup]\nbaseurl=https://sup.example/\ngpgkey=file:///etc/pki/rpm-gpg/OLD\n\
         gpgkey=file:///etc/pki/rpm-gpg/NEW\n",
    )
    .file(
        "/etc/yum.repos.d/bare.repo",
        "[bare]\nbaseurl=https://bare.example/\ngpgkey=/etc/pki/rpm-gpg/BARE\n",
    )
    .file(
        "/etc/yum.repos.d/pre.repo",
        "gpgkey=file:///etc/pki/rpm-gpg/PRE\n[pre]\nbaseurl=https://pre.example/\n",
    )
    .file("/etc/yum.repos.d/empty.repo", "# comments only\n")
    .file(
        "/etc/yum.repos.d/reject.repo",
        "[rej]\nbaseurl=https://rej.example/\n\
         gpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-notpgp file:///etc/pki/rpm-gpg/RPM-GPG-KEY-priv\n",
    )
    .file(&key("A"), PGP)
    .file(&key("B"), PGP)
    .file(&key("OLD"), PGP)
    .file(&key("NEW"), PGP)
    .file(&key("BARE"), PGP)
    .file(&key("PRE"), PGP)
    .file(&key("RPM-GPG-KEY-notpgp"), "plain text, not a key\n")
    .file(&key("RPM-GPG-KEY-priv"), &private_beside_public())
}

fn rpm(snap: &InspectionSnapshot) -> &inspectah_core::types::rpm::RpmSection {
    snap.rpm.as_ref().expect("an rpm section")
}

fn key_paths(snap: &InspectionSnapshot) -> BTreeSet<&str> {
    rpm(snap).gpg_keys.iter().map(|k| k.path.as_str()).collect()
}

fn config_row<'a>(
    snap: &'a InspectionSnapshot,
    path: &str,
) -> Option<&'a inspectah_core::types::config::ConfigFileEntry> {
    snap.config.as_ref()?.files.iter().find(|f| f.path == path)
}

fn shadowed_by_repositories(snap: &InspectionSnapshot, path: &str) -> bool {
    let row = config_row(snap, path).unwrap_or_else(|| panic!("a config row for {path}"));
    row.disposition.is_inventory() && row.shadowed_by == Some(ConfigShadow::Repositories)
}

fn set_repo(id: &str, include: bool) -> RefinementOp {
    RefinementOp::SetInclude {
        item_id: ItemId::Repo { path: id.into() },
        include,
    }
}

fn set_config(path: &str, include: bool) -> RefinementOp {
    RefinementOp::SetInclude {
        item_id: ItemId::Config { path: path.into() },
        include,
    }
}

fn set_package(name: &str, include: bool) -> RefinementOp {
    RefinementOp::SetInclude {
        item_id: ItemId::Package {
            name: name.into(),
            arch: "x86_64".into(),
        },
        include,
    }
}

/// The warnings the refine export of `s` gives.
fn export_warnings(s: &RefineSession) -> Vec<ExportWarning> {
    let packed = tempfile::tempdir().unwrap();
    s.export_plan(s.generation())
        .unwrap()
        .render(&packed.path().join("x.tar.gz"))
        .unwrap()
}

/// What an export tree stages under `config/` at `path`.
fn staged(tree: &Path, path: &str) -> Option<String> {
    std::fs::read_to_string(tree.join("config").join(&path[1..])).ok()
}

fn containerfile(tree: &Path) -> String {
    std::fs::read_to_string(tree.join("Containerfile")).expect("Containerfile")
}

/// Every file under `config/<dir>` of an export tree, relative to `config/`.
fn files_under(tree: &Path, dir: &str) -> BTreeSet<String> {
    let root = tree.join("config");
    let mut out = BTreeSet::new();
    let mut stack = vec![root.join(dir)];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.insert(p.strip_prefix(&root).unwrap().display().to_string());
            }
        }
    }
    out
}

fn repo_files_and_keys(tree: &Path) -> BTreeSet<String> {
    let mut all = files_under(tree, "etc/yum.repos.d");
    all.extend(files_under(tree, "etc/pki/rpm-gpg"));
    all
}

fn export_state<'a>(session: &'a RefineSession, path: &str) -> &'a RepoOwnedExport {
    let dest = Destination::new(path).unwrap();
    &session
        .repo_owned()
        .iter()
        .find(|e| e.destination == dest.as_str())
        .unwrap_or_else(|| panic!("a repo_owned entry for {path}"))
        .export
}

// ---------------------------------------------------------------------
// T1. One parser.
// ---------------------------------------------------------------------

#[test]
fn t1_collection_and_ownership_read_one_parse() {
    let snap = scan_unredacted(with_parser_inputs(base_host()));
    let keys = key_paths(&snap);
    for named in [
        "/etc/pki/rpm-gpg/A",
        "/etc/pki/rpm-gpg/B",
        "/etc/pki/rpm-gpg/NEW",
        "/etc/pki/rpm-gpg/BARE",
        EPEL_KEY,
        CUSTOM_KEY,
        "/etc/pki/rpm-gpg/RPM-GPG-KEY-notpgp",
        "/etc/pki/rpm-gpg/RPM-GPG-KEY-priv",
    ] {
        assert!(keys.contains(named), "{named} is collected: {keys:?}");
    }
    for unnamed in ["/etc/pki/rpm-gpg/OLD", "/etc/pki/rpm-gpg/PRE", ORPHAN_KEY] {
        assert!(!keys.contains(unnamed), "{unnamed} is not a repo key");
    }
    assert!(
        rpm(&snap)
            .repo_files
            .iter()
            .all(|f| f.path != "/etc/yum.repos.d/empty.repo"),
        "a file that defines no section is not a repo definition"
    );

    // The invariant: every repo file defines a section, and every key is
    // named by a section of some repo file.
    let ownership = repo_ownership(rpm(&snap));
    for (dest, entry) in ownership.entries() {
        assert!(!entry.owners.is_empty(), "{dest} has an owner");
    }

    for (path, why) in [
        ("/etc/pki/rpm-gpg/RPM-GPG-KEY-notpgp", KeyRejection::NotPgp),
        (
            "/etc/pki/rpm-gpg/RPM-GPG-KEY-priv",
            KeyRejection::PrivateKeyMaterial,
        ),
    ] {
        let key = rpm(&snap).gpg_keys.iter().find(|k| k.path == path).unwrap();
        assert_eq!(key.rejected, Some(why), "{path}");
        assert!(!key.disposition.is_included(), "{path}");
        assert!(key.content.starts_with("# REDACTED"), "{path}: placeholder");
    }
    assert_eq!(
        RepoIndex::build(&snap).provenance(&inspectah_core::types::repo::RepoId::new("rej")),
        RepoProvenance::Incomplete
    );
}

// ---------------------------------------------------------------------
// T2. The ownership pass on a scan.
// ---------------------------------------------------------------------

#[test]
fn t2_config_twins_are_decided_under_repositories() {
    let snap = scan(with_parser_inputs(base_host()));
    for path in [
        CUSTOM_REPO,
        EPEL_REPO,
        CUSTOM_KEY,
        "/etc/pki/rpm-gpg/A",
        "/etc/pki/rpm-gpg/B",
        "/etc/pki/rpm-gpg/BARE",
    ] {
        assert!(shadowed_by_repositories(&snap, path), "{path}");
    }
    for path in [
        ORPHAN_KEY,
        RPMNEW,
        "/etc/yum.repos.d/empty.repo",
        "/etc/pki/rpm-gpg/OLD",
        "/etc/pki/rpm-gpg/PRE",
    ] {
        let row = config_row(&snap, path).unwrap_or_else(|| panic!("a config row for {path}"));
        assert_eq!(row.shadowed_by, None, "{path}");
        assert!(!row.disposition.is_inventory(), "{path} keeps its control");
    }
    check_repo_ownership(&snap).expect("the scan holds the invariants");
}

#[test]
fn t2_a_key_named_with_a_double_slash_is_still_shadowed() {
    let snap = scan(UnitHost::new().base_package("bash").file(
        CUSTOM_REPO,
        "[custom]\nbaseurl=https://custom.example/\ngpgkey=file:///etc/pki/rpm-gpg//RPM-GPG-KEY-custom\n",
    ).file(CUSTOM_KEY, PGP));
    assert!(shadowed_by_repositories(&snap, CUSTOM_KEY));
    check_repo_ownership(&snap).expect("settled");
}

// ---------------------------------------------------------------------
// T3. Four-state contract and reach, refine.
// ---------------------------------------------------------------------

/// The repo switch is the one control: the config row refuses both
/// directions, every accepted repo op is obeyed by the staged tree and
/// the Containerfile, and only a repo op moves a repo file or key.
#[test]
fn t3_the_repo_switch_is_the_one_control() {
    let snap = reload(&scan(with_parser_inputs(base_host())));
    for (repo, path) in [
        ("custom", CUSTOM_REPO),
        ("epel", EPEL_REPO),
        ("custom", CUSTOM_KEY),
    ] {
        let mut s = session(&snap);
        for include in [false, true] {
            assert!(
                matches!(
                    s.apply(set_config(path, include)),
                    Err(RefineError::InventoryNotToggleable(_))
                ),
                "{path}: the config row is read-only"
            );
        }
        for include in [false, true, false] {
            s.apply(set_repo(repo, include))
                .unwrap_or_else(|e| panic!("{repo} include={include}: {e}"));
            let tree = export_session(&s);
            assert_eq!(
                staged(tree.path(), path).is_some(),
                include,
                "{path} {include}"
            );
            let file = containerfile(tree.path());
            if path.ends_with(".repo") {
                let repo_files = files_under(tree.path(), "etc/yum.repos.d");
                assert_eq!(file.contains(REPO_COPY), !repo_files.is_empty(), "{file}");
            }
            assert!(
                !file
                    .lines()
                    .any(|l| l.starts_with("# FIXME") && l.contains(path)),
                "nothing written twice or withheld:\n{file}"
            );
            let failures: Vec<_> = export_warnings(&s)
                .into_iter()
                .filter(|w| w.kind() == ExportWarningKind::Failure)
                .collect();
            assert!(
                failures.is_empty(),
                "{repo} include={include}: {failures:?}"
            );
            // Only repo ops moved repo dispositions: every entry that
            // differs from the scan's is one a repo op reaches.
            let projected = s.snapshot_projected();
            let index = s.repo_index();
            for (before, after) in rpm(&snap)
                .repo_files
                .iter()
                .chain(&rpm(&snap).gpg_keys)
                .zip(
                    rpm(&projected)
                        .repo_files
                        .iter()
                        .chain(&rpm(&projected).gpg_keys),
                )
            {
                if before.disposition != after.disposition {
                    let dest = Destination::new(&before.path).unwrap();
                    assert!(
                        index
                            .ownership()
                            .get(&dest)
                            .is_some_and(|e| e.owners.iter().any(|o| o.section.as_str() == repo)),
                        "{}: moved by {repo}",
                        before.path
                    );
                }
            }
        }
    }
}

#[test]
fn t3_every_carriable_entry_can_be_dropped_and_rejected_keys_never_ship() {
    let snap = reload(&scan(with_parser_inputs(base_host())));
    let probe = session(&snap);
    let mut owners: Vec<String> = probe
        .view()
        .repo_states
        .iter()
        .filter(|(_, s)| s.control != RepoControl::None)
        .map(|(id, _)| id.clone())
        .collect();
    owners.sort();
    for order in [owners.clone(), owners.iter().rev().cloned().collect()] {
        let mut s = session(&snap);
        for id in &order {
            s.apply(set_repo(id, false))
                .unwrap_or_else(|e| panic!("exclude {id}: {e}"));
        }
        let tree = export_session(&s);
        let owned: BTreeSet<String> = repo_ownership(rpm(&snap))
            .entries()
            .map(|(d, _)| d.as_str()[1..].to_owned())
            .collect();
        let left: BTreeSet<String> = repo_files_and_keys(tree.path())
            .intersection(&owned)
            .cloned()
            .collect();
        assert!(
            left.is_empty(),
            "excluding every switch leaves no repo-owned file or key: {left:?}"
        );
    }
    // Excluding then including [rej] leaves both rejected keys out.
    let mut s = session(&snap);
    s.apply(set_repo("rej", false)).unwrap();
    s.apply(set_repo("rej", true)).unwrap();
    let tree = export_session(&s);
    for name in ["RPM-GPG-KEY-notpgp", "RPM-GPG-KEY-priv"] {
        let path = format!("/etc/pki/rpm-gpg/{name}");
        assert!(staged(tree.path(), &path).is_none(), "{path}");
        assert!(matches!(
            export_state(&s, &path),
            RepoOwnedExport::Rejected { .. }
        ));
    }
}

// ---------------------------------------------------------------------
// T4. Zero-member repos and initial exclusions.
// ---------------------------------------------------------------------

/// Every file under `root`, with its bytes.
fn tree_files(root: &Path) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if let Ok(bytes) = std::fs::read(&path) {
                out.push((path.display().to_string(), bytes));
            }
        }
    }
    out
}

/// `secret` is in none of `artifacts`.
fn assert_absent_from(secret: &str, artifacts: &[(String, Vec<u8>)]) {
    for (name, bytes) in artifacts {
        assert!(
            !bytes.windows(secret.len()).any(|w| w == secret.as_bytes()),
            "{secret} is in {name}"
        );
    }
}

/// What a refine session shows and exports: `/api/view` as served, and
/// every file of the export tree.
fn refine_artifacts(s: &RefineSession) -> Vec<(String, Vec<u8>)> {
    let view = inspectah_web::adapter::build_web_view(s);
    let mut all = vec![(
        "/api/view".to_string(),
        serde_json::to_vec(&view).expect("the view serializes"),
    )];
    all.extend(tree_files(export_session(s).path()));
    all
}

/// A key file the scan refused to store reaches no snapshot, view or
/// export through its Configuration twin: a raw and a redacted scan, the
/// scan tarball, refine's view and export, and an aggregate with its
/// config alternatives, where another host's usable key at the same path
/// ships. The public key beside it still ships everywhere.
#[test]
fn t3_a_rejected_keys_bytes_reach_no_snapshot_view_or_export() {
    let host = || with_parser_inputs(base_host());
    let priv_key = "/etc/pki/rpm-gpg/RPM-GPG-KEY-priv";
    let notpgp = "/etc/pki/rpm-gpg/RPM-GPG-KEY-notpgp";
    for (via, snap) in [
        ("raw", scan_finished_raw(host())),
        ("redacted", scan_finished(host())),
    ] {
        for (path, why) in [
            (priv_key, KeyRejection::PrivateKeyMaterial),
            (notpgp, KeyRejection::NotPgp),
        ] {
            let twin = config_row(&snap, path).expect("the config walk records it");
            assert_eq!(twin.content, why.placeholder(path), "{via}: {path}");
            assert!(twin.diff_against_rpm.is_none(), "{via}: {path}");
        }
        let mut all = vec![(
            format!("{via} snapshot"),
            serde_json::to_vec(&snap).expect("the snapshot serializes"),
        )];
        let loaded = reload(&snap);
        let tree = render_export(&loaded);
        assert!(
            staged(tree.path(), CUSTOM_KEY).is_some(),
            "{via}: public key"
        );
        all.extend(tree_files(tree.path()));
        let s = session(&loaded);
        let refine = refine_artifacts(&s);
        assert!(
            refine
                .iter()
                .any(|(_, b)| b.windows(10).any(|w| w == b"mQENBFtest")),
            "{via}: the public key is in the refine export"
        );
        all.extend(refine);
        assert_absent_from(PRIVATE_MATERIAL, &all);
        assert_absent_from("plain text, not a key", &all);
    }

    // A fleet where one host refused the key and the other has a usable
    // one at the same path: the usable one is the representative, and
    // the refused host's config alternative is its placeholder.
    let usable = || {
        base_host()
            .file(
                "/etc/yum.repos.d/reject.repo",
                &format!("[rej]\nbaseurl=https://rej.example/\ngpgkey=file://{priv_key}\n"),
            )
            .file(priv_key, PGP)
    };
    let (merged, _) =
        merge_snapshots(vec![named(host(), "h1"), named(usable(), "h2")], None, None).unwrap();
    assert_fleet_invariants(&merged);
    let rows: Vec<&str> = merged
        .config
        .iter()
        .flat_map(|c| &c.files)
        .filter(|f| f.path == priv_key)
        .map(|f| f.content.as_str())
        .collect();
    assert_eq!(rows.len(), 2, "two alternatives: {rows:?}");
    assert!(
        rows.contains(
            &KeyRejection::PrivateKeyMaterial
                .placeholder(priv_key)
                .as_str()
        )
    );
    let mut all = vec![(
        "aggregate".to_string(),
        serde_json::to_vec(&merged).expect("the aggregate serializes"),
    )];
    all.extend(tree_files(render_export(&merged).path()));
    let s = session(&reload(&merged));
    let refine = refine_artifacts(&s);
    assert!(
        refine
            .iter()
            .any(|(name, _)| name.contains("aggregate/variants/")),
        "the refine export writes the config alternatives"
    );
    all.extend(refine);
    assert_absent_from(PRIVATE_MATERIAL, &all);
}

/// A credential in a repo URL is replaced in the authority on every
/// path a redacted scan writes: both snapshot copies, the scan tree, the
/// refine view and the refine export. What is left of each URL is the
/// host's, with `REDACTED` where the operator supplies the credential.
#[test]
fn t6_a_repo_url_credential_reaches_no_snapshot_view_or_export() {
    const CRED: &str = "/etc/yum.repos.d/cred.repo";
    let content = "[cred]\nname=Credentialed\n\
                   baseurl=https://build:orchid92@repo.example.invalid/rpm/\n\
                   \x20 https://mirror.example.invalid/rpm/,https://build:orchid92@alt.example.invalid/rpm/\n\
                   mirrorlist=https://ml:violet55@mirrors.example.invalid/list\n\
                   enabled=1\n\
                   gpgkey=https://keys:tulip17@keys.example.invalid/RPM-GPG-KEY-cred\n";
    let host = base_host()
        .file(CRED, content)
        .package_from("cred-tool", "cred");
    let snap = scan_finished(host);
    let carried = "[cred]\nname=Credentialed\n\
                   baseurl=https://REDACTED@repo.example.invalid/rpm/\n\
                   \x20 https://mirror.example.invalid/rpm/,https://REDACTED@alt.example.invalid/rpm/\n\
                   mirrorlist=https://REDACTED@mirrors.example.invalid/list\n\
                   enabled=1\n\
                   gpgkey=https://REDACTED@keys.example.invalid/RPM-GPG-KEY-cred\n";
    let repo = rpm(&snap)
        .repo_files
        .iter()
        .find(|f| f.path == CRED)
        .unwrap();
    assert_eq!(repo.content, carried);
    assert_eq!(config_row(&snap, CRED).unwrap().content, carried);
    assert!(
        snap.redactions
            .iter()
            .any(|r| r.path == CRED && r.pattern == "url_credential")
    );

    let loaded = reload(&snap);
    let tree = render_export(&loaded);
    assert_eq!(staged(tree.path(), CRED).as_deref(), Some(carried));
    let mut all = vec![(
        "snapshot".to_string(),
        serde_json::to_vec(&snap).expect("the snapshot serializes"),
    )];
    all.extend(tree_files(tree.path()));
    let s = session(&loaded);
    assert_eq!(
        staged(export_session(&s).path(), CRED).as_deref(),
        Some(carried)
    );
    all.extend(refine_artifacts(&s));
    for secret in ["orchid92", "violet55", "tulip17"] {
        assert_absent_from(secret, &all);
    }
}

/// A userinfo runs through every character RFC 3986 allows in it, so a
/// comma in a `mirrorlist=` password, a single-URL option, does not end
/// it, and neither does one in a URL of a `baseurl=` list. No part of
/// either password reaches the snapshot JSON, the scan tree, refine's
/// view or export, or a fleet aggregate and its config alternatives.
#[test]
fn t6_a_comma_in_a_url_password_leaks_nothing() {
    const CRED: &str = "/etc/yum.repos.d/cred.repo";
    let content = |list: &str| {
        format!(
            "[cred]\nname=Credentialed\nmirrorlist=https://build:orchid,92@repo.example.invalid/{list}\n\
             baseurl=https://a.example.invalid/r,https://u:tulip,17@b.example.invalid/r\n\
             enabled=1\ngpgcheck=0\n"
        )
    };
    let host = |list: &str| {
        base_host()
            .file(CRED, &content(list))
            .package_from("cred-tool", "cred")
    };
    let snap = scan_finished(host("list"));
    let stored = &rpm(&snap)
        .repo_files
        .iter()
        .find(|f| f.path == CRED)
        .unwrap()
        .content;
    assert_eq!(
        stored,
        "[cred]\nname=Credentialed\nmirrorlist=https://REDACTED@repo.example.invalid/list\n\
         baseurl=https://a.example.invalid/r,https://REDACTED@b.example.invalid/r\n\
         enabled=1\ngpgcheck=0\n"
    );

    let loaded = reload(&snap);
    let mut all = vec![(
        "snapshot".to_string(),
        serde_json::to_vec(&snap).expect("the snapshot serializes"),
    )];
    all.extend(tree_files(render_export(&loaded).path()));
    all.extend(refine_artifacts(&session(&loaded)));

    // A fleet whose hosts' copies differ: one representative, and the
    // other copy as a config alternative.
    let (merged, _) = merge_snapshots(
        vec![named(host("list"), "h1"), named(host("other"), "h2")],
        None,
        None,
    )
    .unwrap();
    let alternatives = merged
        .config
        .iter()
        .flat_map(|c| &c.files)
        .filter(|f| f.path == CRED)
        .count();
    assert_eq!(alternatives, 2, "two config alternatives");
    all.push((
        "aggregate".to_string(),
        serde_json::to_vec(&merged).expect("the aggregate serializes"),
    ));
    all.extend(tree_files(render_export(&merged).path()));
    all.extend(refine_artifacts(&session(&reload(&merged))));
    for secret in ["orchid", "tulip", ",92@", ",17@"] {
        assert_absent_from(secret, &all);
    }
}

/// A `gpgkey=` reference that names no readable key is named in the
/// scan's warning, which the snapshot holds before redaction runs, with
/// its credential replaced: `file://keys:pass@host/KEY` reads as the
/// path `keys:pass@host/KEY`, and a reference dnf's list split cut out
/// of a userinfo with a comma in it is named `REDACTED` whole. No part of
/// either credential reaches the snapshot JSON and its warnings, the scan
/// tree's reports, or refine's view and export.
#[test]
fn t6_a_failed_key_reference_is_named_without_its_credential() {
    const CRED: &str = "/etc/yum.repos.d/cred.repo";
    const SPLIT: &str = "/etc/yum.repos.d/split.repo";
    let host = base_host()
        .file(
            CRED,
            "[cred]\nname=Credentialed\nbaseurl=https://repo.example.invalid/rpm/\nenabled=1\n\
             gpgkey=file://keys:orchid92@keys.example.invalid/RPM-GPG-KEY\n",
        )
        .file(
            SPLIT,
            "[split]\nbaseurl=https://split.example.invalid/\n\
             gpgkey=file://keys:tulip,17,x@keys.example.invalid/K\n",
        )
        .package_from("cred-tool", "cred");
    let snap = scan_finished(host);
    let warned = |repo: &str| -> Vec<&str> {
        snap.warnings
            .iter()
            .filter(|w| w.message.contains(&format!("named in {repo} ")))
            .map(|w| w.message.as_str())
            .collect()
    };
    let cred = warned(CRED);
    assert!(
        cred.len() == 1
            && cred[0].starts_with(
                "GPG key REDACTED@keys.example.invalid/RPM-GPG-KEY named in \
                 /etc/yum.repos.d/cred.repo was not collected: \
                 REDACTED@keys.example.invalid/RPM-GPG-KEY could not be read."
            ),
        "{cred:?}"
    );
    let split = warned(SPLIT);
    assert!(
        !split.is_empty()
            && split
                .iter()
                .all(|w| w.starts_with(&format!("GPG key REDACTED named in {SPLIT} "))),
        "{split:?}"
    );

    let loaded = reload(&snap);
    let mut all = vec![(
        "snapshot".to_string(),
        serde_json::to_vec(&snap).expect("the snapshot serializes"),
    )];
    all.extend(tree_files(render_export(&loaded).path()));
    all.extend(refine_artifacts(&session(&loaded)));
    for secret in ["orchid92", "tulip", ",17,", "17,x"] {
        assert_absent_from(secret, &all);
    }
}

/// A key file that is an OpenPGP secret key and nothing else is withheld
/// by the read gate: no key entry, a withheld config row, the reason
/// recorded, and its bytes nowhere.
#[test]
fn t3_a_whole_file_secret_key_a_repo_names_is_withheld() {
    const WHOLE: &str = "/etc/pki/rpm-gpg/RPM-GPG-KEY-whole";
    let host = base_host()
        .file(
            "/etc/yum.repos.d/signed.repo",
            &format!("[signed]\nbaseurl=https://signed.example/\ngpgkey=file://{WHOLE}\n"),
        )
        .file(
            WHOLE,
            &format!(
                "-----BEGIN PGP PRIVATE KEY BLOCK-----\nComment: build <b@example.invalid>\n\n\
                 {PRIVATE_MATERIAL}\n=Xb7Q\n-----END PGP PRIVATE KEY BLOCK-----\n"
            ),
        );
    let snap = scan_finished_raw(host);
    assert!(!key_paths(&snap).contains(WHOLE));
    assert!(config_row(&snap, WHOLE).is_none());
    assert!(
        snap.config
            .as_ref()
            .unwrap()
            .withheld
            .iter()
            .any(|w| w.path == WHOLE)
    );
    let mut all = vec![(
        "snapshot".to_string(),
        serde_json::to_vec(&snap).expect("the snapshot serializes"),
    )];
    let loaded = reload(&snap);
    all.extend(tree_files(render_export(&loaded).path()));
    all.extend(refine_artifacts(&session(&loaded)));
    assert_absent_from(PRIVATE_MATERIAL, &all);
}

#[test]
fn t4_a_repo_whose_members_are_all_excluded_still_decides_its_files() {
    let snap = reload(&scan(base_host()));
    let mut s = session(&snap);
    s.apply(set_package("tool-a", false)).unwrap();
    s.apply(set_package("tool-b", false)).unwrap();
    let state = s.repo_state("custom");
    assert!(state.kept);
    assert_eq!((state.carriage_included, state.carriage_count), (2, 2));

    s.apply(set_repo("custom", false))
        .expect("the exclude moves the decision and the files");
    assert!(!s.repo_state("custom").kept);
    let tree = export_session(&s);
    assert!(staged(tree.path(), CUSTOM_REPO).is_none());
    assert!(staged(tree.path(), CUSTOM_KEY).is_none());

    s.apply(set_repo("custom", true)).expect("include");
    let tree = export_session(&s);
    assert!(staged(tree.path(), CUSTOM_REPO).is_some());
    assert!(staged(tree.path(), CUSTOM_KEY).is_some());
    let included: BTreeSet<String> = s
        .snapshot_projected()
        .rpm
        .unwrap()
        .packages_added
        .iter()
        .filter(|p| p.disposition.is_included())
        .map(|p| p.name.clone())
        .collect();
    assert!(included.contains("tool-a") && included.contains("tool-b"));

    s.undo().unwrap();
    assert!(staged(export_session(&s).path(), CUSTOM_REPO).is_none());
    s.redo().unwrap();
    assert!(staged(export_session(&s).path(), CUSTOM_REPO).is_some());
}

/// A bootstrap repo no package is attributed to, which serves the
/// `vendor-release` package that owns its file.
#[test]
fn t4_a_never_attributed_repo_has_a_row_and_a_switch() {
    const VENDOR_REPO: &str = "/etc/yum.repos.d/vendor.repo";
    const VENDOR_KEY: &str = "/etc/pki/rpm-gpg/RPM-GPG-KEY-vendor";
    let host = UnitHost::new()
        .base_package("bash")
        .owned_file(
            VENDOR_REPO,
            &format!(
                "[bootstrap]\nbaseurl=https://vendor.example/\nenabled=1\n\
                 gpgkey=file://{VENDOR_KEY}\n"
            ),
            "vendor-release",
        )
        .owned_file(VENDOR_KEY, PGP, "vendor-release")
        .package_from("vendor-release", "@commandline")
        .enabled_repos(&["bootstrap"])
        .available("vendor-release", "bootstrap");
    let snap = reload(&scan_finished(host));
    let vendor = rpm(&snap)
        .packages_added
        .iter()
        .find(|p| p.name == "vendor-release")
        .unwrap();
    assert_eq!(
        vendor
            .repo_enabling_from
            .iter()
            .map(|r| r.as_str())
            .collect::<Vec<_>>(),
        ["bootstrap"]
    );

    let mut s = session(&snap);
    // Refine opens a `@commandline` package excluded; the operator
    // includes it, and the export installs it from `bootstrap`.
    s.apply(set_package("vendor-release", true)).unwrap();
    let file = containerfile(export_session(&s).path());
    assert!(file.contains("    vendor-release"), "{file}");
    let row = inspectah_refine::projection::project_decisions(&s)
        .repo_groups
        .into_iter()
        .find(|g| g.section_id == "bootstrap")
        .expect("bootstrap has a row");
    assert_eq!(row.control, RepoControl::Full);
    assert!(row.kept);
    assert_eq!((row.carriage_included, row.carriage_count), (2, 2));

    s.apply(set_repo("bootstrap", false))
        .expect("excluding the bootstrap repo is accepted");
    let tree = export_session(&s);
    assert!(staged(tree.path(), VENDOR_REPO).is_none());
    assert!(staged(tree.path(), VENDOR_KEY).is_none());
    let file = containerfile(tree.path());
    assert!(
        file.contains("# FIXME: re-include repo bootstrap"),
        "vendor-release is demoted with the existing FIXME:\n{file}"
    );
}

#[test]
fn t4_a_repo_with_no_control_is_refused_with_nothing_to_decide() {
    let snap = reload(&scan(base_host()));
    let mut s = session(&snap);
    for include in [true, false] {
        assert!(matches!(
            s.apply(set_repo("nosuch", include)),
            Err(RefineError::NoopTarget(m)) if m.starts_with("nothing to decide in 'nosuch'")
        ));
    }
    // A kept repo, fully carried, with every member included.
    assert!(matches!(
        s.apply(set_repo("custom", true)),
        Err(RefineError::NoopTarget(m)) if m.contains("already kept")
    ));
}

#[test]
fn t4_a_default_repo_file_is_never_staged() {
    const REDHAT: &str = "/etc/yum.repos.d/redhat.repo";
    let host = base_host()
        .file(
            REDHAT,
            "[rhel-9-for-x86_64-baseos-rpms]\nbaseurl=https://cdn.example/baseos\n\
         [codeready-builder-for-rhel-9-x86_64-rpms]\nbaseurl=https://cdn.example/crb\nenabled=1\n",
        )
        .package_from("crb-tool", "codeready-builder-for-rhel-9-x86_64-rpms");
    let snap = reload(&scan(host));
    assert!(shadowed_by_repositories(&snap, REDHAT));
    assert!(staged(render_export(&snap).path(), REDHAT).is_none());
    let mut s = session(&snap);
    assert_eq!(
        s.repo_state("codeready-builder-for-rhel-9-x86_64-rpms")
            .carriage_count,
        0
    );
    s.apply(set_repo("codeready-builder-for-rhel-9-x86_64-rpms", false))
        .expect("moves the member");
    s.apply(set_repo("codeready-builder-for-rhel-9-x86_64-rpms", true))
        .expect("moves the member back");
    assert!(staged(export_session(&s).path(), REDHAT).is_none());
    assert!(matches!(
        export_state(&s, REDHAT),
        RepoOwnedExport::DefaultRepoFile
    ));
}

// ---------------------------------------------------------------------
// T5. Shared files.
// ---------------------------------------------------------------------

#[test]
fn t5_a_key_two_repos_name_ships_while_the_other_keeps_it() {
    const KEY: &str = "/etc/pki/rpm-gpg/RPM-GPG-KEY-shared";
    let repo = |id: &str| format!("[{id}]\nbaseurl=https://{id}.example/\ngpgkey=file://{KEY}\n");
    let snap = reload(&scan(
        UnitHost::new()
            .base_package("bash")
            .file("/etc/yum.repos.d/a.repo", &repo("a"))
            .file("/etc/yum.repos.d/b.repo", &repo("b"))
            .file(KEY, PGP),
    ));
    let mut s = session(&snap);
    let tree_before = export_session(&s);
    s.apply(set_repo("a", false))
        .expect("the decision moves though no staged byte of the key does");
    assert!(!s.repo_state("a").kept);
    match export_state(&s, KEY) {
        RepoOwnedExport::Ships { kept_by } => {
            assert_eq!(kept_by.len(), 1);
            assert_eq!(kept_by[0].section_id, "b");
            assert_eq!(kept_by[0].enablerepo_package, None);
        }
        other => panic!("the key ships for b: {other:?}"),
    }
    assert_eq!(
        staged(tree_before.path(), KEY),
        staged(export_session(&s).path(), KEY)
    );
    s.apply(set_repo("b", false)).unwrap();
    assert!(staged(export_session(&s).path(), KEY).is_none());
    s.undo().unwrap();
    assert!(staged(export_session(&s).path(), KEY).is_some());
    s.redo().unwrap();
    assert!(staged(export_session(&s).path(), KEY).is_none());
}

#[test]
fn t5_one_file_with_mixed_toggles_ships_whole_and_says_so() {
    const AB: &str = "/etc/yum.repos.d/ab.repo";
    let snap = reload(&scan(
        UnitHost::new()
            .base_package("bash")
            .file(
                AB,
                "[a]\nbaseurl=https://a.example/\n[b]\nbaseurl=https://b.example/\n\
                 [a-source]\nbaseurl=https://a.example/src\nenabled=0\n",
            )
            .package_from("pa", "a")
            .package_from("pb", "b"),
    ));
    let mut s = session(&snap);
    s.apply(set_repo("a", false)).unwrap();
    let tree = export_session(&s);
    assert!(staged(tree.path(), AB).is_some(), "the file ships whole");
    let file = containerfile(tree.path());
    let line = format!(
        "# {AB} also defines repo a, excluded in refine; the file is copied whole for repo b, \
         so a stays defined as the host had it"
    );
    assert!(file.lines().any(|l| l == line), "{file}");
    let packed = tempfile::tempdir().unwrap();
    let warnings = s
        .export_plan(s.generation())
        .unwrap()
        .render(&packed.path().join("x.tar.gz"))
        .unwrap();
    assert!(
        warnings
            .iter()
            .any(|w| w.text().starts_with(&format!("{AB} also defines repo a"))),
        "{warnings:?}"
    );
    s.apply(set_repo("b", false)).unwrap();
    assert!(
        staged(export_session(&s).path(), AB).is_none(),
        "the disabled [a-source] does not keep it"
    );
}

#[test]
fn t5_sections_that_differ_only_in_case_are_two_repos() {
    const SITE: &str = "/etc/yum.repos.d/site.repo";
    const K: &str = "/etc/pki/rpm-gpg/K";
    let lower = "[site]\nbaseurl=https://lower.example/\ngpgkey=file:///etc/pki/rpm-gpg/K\n";
    let upper = "[SITE]\nbaseurl=https://upper.example/\ngpgkey=file:///etc/pki/rpm-gpg/K\n";
    for content in [format!("{lower}{upper}"), format!("{upper}{lower}")] {
        let snap = reload(&scan(
            UnitHost::new()
                .base_package("bash")
                .file(SITE, &content)
                .file(K, PGP)
                .package_from("pkg", "SITE"),
        ));
        let ownership = repo_ownership(rpm(&snap));
        for path in [SITE, K] {
            let ids: Vec<&str> = ownership
                .get(&Destination::new(path).unwrap())
                .unwrap()
                .owners
                .iter()
                .map(|o| o.section.as_str())
                .collect();
            let ids: BTreeSet<&str> = ids.into_iter().collect();
            assert_eq!(ids, BTreeSet::from(["SITE", "site"]), "{path}");
        }
        let mut s = session(&snap);
        s.apply(set_repo("site", false)).unwrap();
        let pkg_included = s
            .snapshot_projected()
            .rpm
            .unwrap()
            .packages_added
            .iter()
            .any(|p| p.name == "pkg" && p.disposition.is_included());
        assert!(pkg_included, "pkg is SITE's, not site's");
        for path in [SITE, K] {
            match export_state(&s, path) {
                RepoOwnedExport::Ships { kept_by } => {
                    assert_eq!(kept_by[0].section_id, "SITE", "{path}")
                }
                other => panic!("{path}: {other:?}"),
            }
        }
        s.apply(set_repo("SITE", false)).unwrap();
        let tree = export_session(&s);
        assert!(staged(tree.path(), SITE).is_none());
        assert!(staged(tree.path(), K).is_none());
    }
}

#[test]
fn t5_a_disabled_repo_with_an_enablerepo_install_keeps_a_shared_key() {
    const KEY: &str = "/etc/pki/rpm-gpg/RPM-GPG-KEY-ad";
    let snap = reload(&scan_finished(
        UnitHost::new()
            .base_package("bash")
            .file(
                "/etc/yum.repos.d/a.repo",
                &format!("[a]\nbaseurl=https://a.example/\ngpgkey=file://{KEY}\n"),
            )
            .file(
                "/etc/yum.repos.d/d.repo",
                &format!("[d]\nbaseurl=https://d.example/\nenabled=0\ngpgkey=file://{KEY}\n"),
            )
            .file(KEY, PGP)
            .package_from("pd", "d")
            .enabled_repos(&["a"]),
    ));
    let mut s = session(&snap);
    assert_eq!(s.repo_state("d").control, RepoControl::CarriageOnly);
    let before = containerfile(export_session(&s).path());
    assert!(before.contains("--enablerepo=d"), "{before}");
    s.apply(set_repo("a", false)).unwrap();
    match export_state(&s, KEY) {
        RepoOwnedExport::Ships { kept_by } => {
            assert_eq!(kept_by[0].section_id, "d");
            assert_eq!(kept_by[0].enablerepo_package.as_deref(), Some("pd"));
        }
        other => panic!("{other:?}"),
    }
    assert!(containerfile(export_session(&s).path()).contains("--enablerepo=d"));

    // d's carriage-only switch drops d.repo and leaves pd alone.
    s.apply(set_repo("d", false)).unwrap();
    let projected = s.snapshot_projected();
    assert!(
        rpm(&projected)
            .packages_added
            .iter()
            .any(|p| p.name == "pd" && p.disposition.is_included()),
        "a carriage-only switch never moves members"
    );
    let tree = export_session(&s);
    assert!(staged(tree.path(), "/etc/yum.repos.d/d.repo").is_none());
    let file = containerfile(tree.path());
    assert!(
        !file
            .lines()
            .any(|l| !l.trim_start().starts_with('#') && l.contains("--enablerepo=d")),
        "no active install enables d once its file is gone:\n{file}"
    );
    assert!(!installs(&file, "pd"), "{file}");
}

/// Whether `containerfile` names `name` on an install line: an
/// instruction line, not a comment, that is the name alone.
fn installs(containerfile: &str, name: &str) -> bool {
    containerfile
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .any(|l| l.trim().trim_end_matches('\\').trim() == name)
}

const COMPANY_REPO: &str = "/etc/yum.repos.d/company.repo";
const COMPANY_KEY: &str = "/etc/pki/rpm-gpg/RPM-GPG-KEY-company";

/// A host whose `[baseos]` is an internal mirror in an unowned
/// `company.repo`, with its key: `baseos` gets a carriage-only switch.
fn company_host(key: &str) -> UnitHost {
    UnitHost::new()
        .base_package("bash")
        .file(
            COMPANY_REPO,
            &format!(
                "[baseos]\nname=BaseOS mirror\nbaseurl=https://mirror.corp.example/baseos/\n\
                 enabled=1\ngpgkey=file://{COMPANY_KEY}\n"
            ),
        )
        .file(COMPANY_KEY, key)
        .enabled_repos(&["baseos"])
}

/// The FIXME a package gets when a repo whose switch restores it is
/// named, `reach` saying what that switch brings back.
fn lost_route_fixme(name: &str, repo: &str, reach: &str) -> String {
    format!(
        "# FIXME: re-include repo {repo}, {reach}, or upload the RPM; {name} installs from repo \
         {repo}, whose definition this export does not carry"
    )
}

/// A package installed by name from a repo whose files the export drops
/// is not installed by name: its carriage-only switch keeps the package
/// included, and the package renders the demotion FIXME naming that
/// switch until its file and key are back. On the scan path, a repo whose
/// key the scan refused loses the route the same way, with no switch to
/// name.
#[test]
fn t5_a_package_whose_repo_the_export_drops_renders_the_fixme() {
    let snap = reload(&scan_finished(
        company_host(PGP).package_from("site-tool", "baseos"),
    ));
    let file = containerfile(render_export(&snap).path());
    assert!(
        installs(&file, "site-tool"),
        "the scan carries baseos:\n{file}"
    );

    let mut s = session(&snap);
    assert_eq!(s.repo_state("baseos").control, RepoControl::CarriageOnly);
    let file = containerfile(export_session(&s).path());
    assert!(installs(&file, "site-tool"), "{file}");
    s.apply(set_repo("baseos", false)).unwrap();
    let tree = export_session(&s);
    assert!(staged(tree.path(), COMPANY_REPO).is_none());
    assert!(staged(tree.path(), COMPANY_KEY).is_none());
    let projected = s.snapshot_projected();
    let site_tool = rpm(&projected)
        .packages_added
        .iter()
        .find(|p| p.name == "site-tool")
        .unwrap();
    assert!(
        site_tool.disposition.is_included(),
        "a carriage-only switch keeps the member"
    );
    assert!(
        !site_tool.repoless_annotation.is_empty(),
        "the view offers the upload"
    );
    let file = containerfile(tree.path());
    assert!(!installs(&file, "site-tool"), "{file}");
    let fixme = lost_route_fixme(
        "site-tool",
        "baseos",
        "which brings back its repo files and keys",
    );
    assert!(file.lines().any(|l| l == fixme), "{file}");

    s.apply(set_repo("baseos", true)).unwrap();
    let file = containerfile(export_session(&s).path());
    assert!(installs(&file, "site-tool"), "the route is back:\n{file}");
    assert!(!file.contains("site-tool installs from repo"), "{file}");

    // Scan path: the key is refused, so dnf in the image could not verify
    // baseos's packages, and no switch brings a refused key back.
    let refused = reload(&scan_finished(
        company_host("plain text, not a key\n").package_from("site-tool", "baseos"),
    ));
    for (via, file) in [
        ("scan", containerfile(render_export(&refused).path())),
        (
            "refine",
            containerfile(export_session(&session(&refused)).path()),
        ),
    ] {
        assert!(!installs(&file, "site-tool"), "{via}:\n{file}");
        assert!(
            file.contains(
                "# FIXME: upload the RPM; site-tool installs from repo baseos, whose \
                 definition this export does not carry, and re-including no repo in refine \
                 restores it"
            ),
            "{via}:\n{file}"
        );
    }
}

/// A repo-enabling package served by a distro repo whose switch is
/// carriage-only: excluding that repo demotes it, and the FIXME names the
/// switch, which restores it, as one that brings back files and keys.
#[test]
fn t5_a_carriage_only_switch_that_restores_a_package_is_named() {
    // `vendor-release`, from `@commandline`, owns `company.repo` and its
    // key, and the enabled `baseos` serves it.
    let host = UnitHost::new()
        .base_package("bash")
        .owned_file(
            COMPANY_REPO,
            &format!(
                "[baseos]\nname=BaseOS mirror\nbaseurl=https://mirror.corp.example/baseos/\n\
                 enabled=1\ngpgkey=file://{COMPANY_KEY}\n"
            ),
            "vendor-release",
        )
        .owned_file(COMPANY_KEY, PGP, "vendor-release")
        .package_from("vendor-release", "@commandline")
        .enabled_repos(&["baseos"])
        .available("vendor-release", "baseos");
    let snap = reload(&scan_finished(host));
    let mut s = session(&snap);
    assert_eq!(s.repo_state("baseos").control, RepoControl::CarriageOnly);
    s.apply(set_package("vendor-release", true)).unwrap();
    let file = containerfile(export_session(&s).path());
    assert!(installs(&file, "vendor-release"), "{file}");
    s.apply(set_repo("baseos", false)).unwrap();
    let file = containerfile(export_session(&s).path());
    assert!(!installs(&file, "vendor-release"), "{file}");
    let fixme = lost_route_fixme(
        "vendor-release",
        "baseos",
        "which brings back its repo files and keys",
    );
    assert!(file.lines().any(|l| l == fixme), "{file}");
    s.apply(set_repo("baseos", true)).unwrap();
    assert!(installs(
        &containerfile(export_session(&s).path()),
        "vendor-release"
    ));
}

/// What the scan tarball and the refine export opened from it each
/// render for `snap`, by path name.
fn both_containerfiles(snap: &InspectionSnapshot) -> [(&'static str, String); 2] {
    [
        ("scan", containerfile(render_export(snap).path())),
        (
            "refine",
            containerfile(export_session(&session(snap)).path()),
        ),
    ]
}

/// The FIXME a package gets when no repo switch restores its route.
fn unrestorable_fixme(name: &str, repo: &str) -> String {
    format!(
        "# FIXME: upload the RPM; {name} installs from repo {repo}, whose definition this \
         export does not carry, and re-including no repo in refine restores it"
    )
}

/// A repo id may spell a credential key, as a COPR id spells `token:`.
/// Redaction never rewrites a `[section]` header of a `.repo` file, so the
/// repo keeps its file, its key and its package's route through the scan
/// and both exports. And when a snapshot's stored file no longer defines
/// the section the collector attributed a package to, as when a header
/// was rewritten after the scan, the route is read from that
/// attribution, made before redaction ran: the file is dropped and the
/// package renders the FIXME on both exports instead of installing by
/// name from a repo the export does not carry.
#[test]
fn t5_a_repo_id_spelling_a_credential_key_keeps_its_route() {
    const VENDOR: &str = "/etc/yum.repos.d/vendor.repo";
    const VENDOR_KEY: &str = "/etc/pki/rpm-gpg/RPM-GPG-KEY-vendor";
    const ID: &str = "copr:copr.fedorainfracloud.org:token:vendor";
    let content = format!(
        "[{ID}]\nname=Vendor\nbaseurl=https://vendor.example/rpm/\nenabled=1\n\
         gpgkey=file://{VENDOR_KEY}\n"
    );
    let host = UnitHost::new()
        .base_package("bash")
        .file(VENDOR, &content)
        .file(VENDOR_KEY, PGP)
        .package_from("site-tool", ID)
        .enabled_repos(&[ID]);
    let snap = reload(&scan_finished(host));
    let stored = rpm(&snap)
        .repo_files
        .iter()
        .find(|f| f.path == VENDOR)
        .unwrap();
    assert_eq!(stored.content, content, "redaction leaves the header");
    assert!(stored.disposition.is_included());
    assert_eq!(config_row(&snap, VENDOR).unwrap().content, content);
    for (via, file) in both_containerfiles(&snap) {
        assert!(installs(&file, "site-tool"), "{via}:\n{file}");
    }
    let tree = render_export(&snap);
    assert_eq!(
        staged(tree.path(), VENDOR).as_deref(),
        Some(content.as_str())
    );
    assert!(staged(tree.path(), VENDOR_KEY).is_some());

    // The header rewritten after the scan: the stored file defines no
    // section, has no owner and is dropped, and the attribution says the
    // package's route went with it.
    let mut rewritten = snap.clone();
    let broken = content.replace(
        &format!("[{ID}]"),
        "[copr:copr.fedorainfracloud.org:token:X",
    );
    for file in &mut rewritten.rpm.as_mut().unwrap().repo_files {
        if file.path == VENDOR {
            file.content = broken.clone();
        }
    }
    for file in &mut rewritten.config.as_mut().unwrap().files {
        if file.path == VENDOR {
            file.content = broken.clone();
        }
    }
    settle_repo_ownership(&mut rewritten);
    let rewritten = reload(&rewritten);
    assert!(
        !rpm(&rewritten)
            .repo_files
            .iter()
            .any(|f| f.path == VENDOR && f.disposition.is_included())
    );
    for (via, file) in both_containerfiles(&rewritten) {
        assert!(!installs(&file, "site-tool"), "{via}:\n{file}");
        assert!(
            file.lines()
                .any(|l| l == unrestorable_fixme("site-tool", ID)),
            "{via}:\n{file}"
        );
    }
    assert!(staged(render_export(&rewritten).path(), VENDOR).is_none());
}

/// The base image's own repos are never lost: on a Fedora host whose
/// stock `fedora.repo` and `fedora-updates.repo` the base image's
/// `fedora-repos` owns unmodified, excluding `fedora`'s carriage-only
/// switch drops the host's copy and `tree` still installs by name. A
/// copy `rpm -Va` reports modified is the host's own, and its packages
/// lose their route with it.
#[test]
fn t5_a_repo_the_base_image_supplies_keeps_its_route_when_its_file_is_dropped() {
    const FEDORA: &str = "/etc/yum.repos.d/fedora.repo";
    const UPDATES: &str = "/etc/yum.repos.d/fedora-updates.repo";
    const FEDORA_KEY: &str = "/etc/pki/rpm-gpg/RPM-GPG-KEY-fedora-43-x86_64";
    let stock = |id: &str| {
        format!(
            "[{id}]\nname=Fedora $releasever - $basearch\n\
             metalink=https://mirrors.fedoraproject.org/metalink?repo={id}-$releasever&arch=$basearch\n\
             enabled=1\ngpgcheck=1\ngpgkey=file://{FEDORA_KEY}\n"
        )
    };
    let host = |modified: bool| {
        let host = UnitHost::new()
            .base_package("bash")
            .base_package("fedora-repos")
            .base_package("fedora-gpg-keys")
            .owned_file(FEDORA, &stock("fedora"), "fedora-repos")
            .owned_file(UPDATES, &stock("updates"), "fedora-repos")
            .owned_file(FEDORA_KEY, PGP, "fedora-gpg-keys")
            .package_from("tree", "fedora")
            .package_from("htop", "updates")
            .enabled_repos(&["fedora", "updates"]);
        if modified {
            host.rpm_va(&format!("S.5....T.  c {FEDORA}\n"), 1)
        } else {
            host
        }
    };

    let snap = reload(&scan_finished(host(false)));
    let mut s = session(&snap);
    assert_eq!(s.repo_state("fedora").control, RepoControl::CarriageOnly);
    for id in ["fedora", "updates"] {
        s.apply(set_repo(id, false)).unwrap();
    }
    let tree = export_session(&s);
    assert!(staged(tree.path(), FEDORA).is_none());
    assert!(staged(tree.path(), UPDATES).is_none());
    let file = containerfile(tree.path());
    for name in ["tree", "htop"] {
        assert!(installs(&file, name), "{name}:\n{file}");
        assert!(
            !file.contains(&format!("{name} installs from repo")),
            "{file}"
        );
    }
    let projected = s.snapshot_projected();
    assert!(
        rpm(&projected)
            .packages_added
            .iter()
            .all(|p| p.repoless_annotation.is_empty()),
        "no upload is offered"
    );

    // Modified on the host: the host's own definition, lost when dropped.
    let snap = reload(&scan_finished(host(true)));
    let mut s = session(&snap);
    s.apply(set_repo("fedora", false)).unwrap();
    let file = containerfile(export_session(&s).path());
    assert!(!installs(&file, "tree"), "{file}");
    assert!(
        file.lines().any(|l| l
            == lost_route_fixme(
                "tree",
                "fedora",
                "which brings back its repo files and keys"
            )),
        "{file}"
    );
    assert!(installs(&file, "htop"), "{file}");
}

/// `[site]` and `[SITE]`, in separate files, are two repos: `[site]`
/// names a local key the host lacks, `[SITE]` a remote one. Only the
/// lower-case section's package loses its route, in both file orders,
/// on both exports.
#[test]
fn t5_a_case_variant_repo_keeps_its_own_route() {
    let lower = "[site]\nname=lower\nbaseurl=https://lower.example/\nenabled=1\n\
                 gpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-missing\n";
    let upper = "[SITE]\nname=upper\nbaseurl=https://upper.example/\nenabled=1\n\
                 gpgkey=https://upper.example/RPM-GPG-KEY\n";
    for (lower_file, upper_file) in [("a.repo", "b.repo"), ("b.repo", "a.repo")] {
        let host = UnitHost::new()
            .base_package("bash")
            .file(&format!("/etc/yum.repos.d/{lower_file}"), lower)
            .file(&format!("/etc/yum.repos.d/{upper_file}"), upper)
            .package_from("lower-tool", "site")
            .package_from("upper-tool", "SITE")
            .enabled_repos(&["site", "SITE"]);
        let snap = reload(&scan_finished(host));
        for (via, file) in both_containerfiles(&snap) {
            let ctx = format!("{lower_file} then {upper_file}, {via}");
            assert!(installs(&file, "upper-tool"), "{ctx}:\n{file}");
            assert!(!installs(&file, "lower-tool"), "{ctx}:\n{file}");
            assert!(
                file.lines()
                    .any(|l| l == unrestorable_fixme("lower-tool", "site")),
                "{ctx}:\n{file}"
            );
            assert!(
                !file.contains("upper-tool installs from repo"),
                "{ctx}:\n{file}"
            );
        }
    }
}

/// A key a repo names is a prerequisite of its packages only when dnf
/// checks signatures for it. With `gpgcheck=0` and `repo_gpgcheck=0`, a
/// retired key the host no longer has costs `site-tool` nothing, and the
/// file is carried as the host wrote it; with `gpgcheck=1`, or with it
/// unset, which the base image's `dnf.conf` reads as on, the same missing
/// key loses the route.
#[test]
fn t5_a_missing_key_costs_a_route_only_when_dnf_checks_signatures() {
    const SITE: &str = "/etc/yum.repos.d/site.repo";
    let content = |check: &str| {
        format!(
            "[site]\nname=Site\nbaseurl=https://site.example/\nenabled=1\n{check}\
             gpgkey=file:///etc/pki/rpm-gpg/retired-key\n"
        )
    };
    for (check, lost) in [
        ("gpgcheck=0\nrepo_gpgcheck=0\n", false),
        ("gpgcheck=0\n", false),
        ("gpgcheck=1\nrepo_gpgcheck=0\n", true),
        ("gpgcheck=0\nrepo_gpgcheck=1\n", true),
        ("", true),
    ] {
        let host = UnitHost::new()
            .base_package("bash")
            .file(SITE, &content(check))
            .package_from("site-tool", "site")
            .enabled_repos(&["site"]);
        let snap = reload(&scan_finished(host));
        let tree = render_export(&snap);
        assert_eq!(
            staged(tree.path(), SITE).as_deref(),
            Some(content(check).as_str()),
            "{check:?}: carried as the host wrote it"
        );
        for (via, file) in both_containerfiles(&snap) {
            assert_eq!(
                !installs(&file, "site-tool"),
                lost,
                "{check:?}, {via}:\n{file}"
            );
            assert_eq!(
                file.lines()
                    .any(|l| l == unrestorable_fixme("site-tool", "site")),
                lost,
                "{check:?}, {via}:\n{file}"
            );
        }
    }
}

// ---------------------------------------------------------------------
// T6. CLI export.
// ---------------------------------------------------------------------

#[test]
fn t6_the_scan_tarball_stages_each_repo_file_and_key_once() {
    let snap = scan(with_parser_inputs(base_host()));
    let tree = render_export(&snap);
    let file = containerfile(tree.path());
    for path in [CUSTOM_REPO, EPEL_REPO, CUSTOM_KEY, EPEL_KEY] {
        assert!(staged(tree.path(), path).is_some(), "{path}");
    }
    assert_eq!(
        staged(tree.path(), EPEL_REPO).as_deref(),
        Some(epel_repo().as_str())
    );
    for path in [RPMNEW, "/etc/yum.repos.d/empty.repo", ORPHAN_KEY] {
        assert!(
            staged(tree.path(), path).is_some(),
            "{path}: an ordinary config file"
        );
    }
    for name in ["RPM-GPG-KEY-notpgp", "RPM-GPG-KEY-priv"] {
        assert!(staged(tree.path(), &format!("/etc/pki/rpm-gpg/{name}")).is_none());
    }
    assert!(
        file.contains(REPO_COPY) && file.contains(KEY_COPY),
        "{file}"
    );
}

#[test]
fn t6_the_scan_path_and_refine_stage_the_same_repo_tree() {
    let snap = reload(&scan(base_host()));
    let mut s = session(&snap);
    s.apply(set_repo("custom", false)).unwrap();
    let refine = export_session(&s);
    let cli = render_export(&s.snapshot_projected());
    assert!(staged(refine.path(), CUSTOM_REPO).is_none());
    assert!(staged(refine.path(), CUSTOM_KEY).is_none());
    assert_eq!(
        repo_files_and_keys(refine.path()),
        repo_files_and_keys(cli.path())
    );
}

#[test]
fn t6_an_aggregate_excludes_default_repo_files_and_rejected_keys() {
    let host = |name: &str| {
        reload(&scan(
            with_parser_inputs(base_host())
                .file(
                    "/etc/yum.repos.d/redhat.repo",
                    "[rhel-9-for-x86_64-baseos-rpms]\nbaseurl=https://cdn.example/\n",
                )
                .file("/etc/hostname", &format!("{name}\n")),
        ))
    };
    let (merged, _) = merge_snapshots(vec![host("h1"), host("h2")], None, None).unwrap();
    let merged = reload(&merged);
    for path in [
        "/etc/yum.repos.d/redhat.repo",
        "/etc/pki/rpm-gpg/RPM-GPG-KEY-notpgp",
        "/etc/pki/rpm-gpg/RPM-GPG-KEY-priv",
    ] {
        let entry = rpm(&merged)
            .repo_files
            .iter()
            .chain(&rpm(&merged).gpg_keys)
            .find(|e| e.path == path)
            .unwrap();
        assert!(!entry.disposition.is_included(), "{path}");
        assert!(
            staged(render_export(&merged).path(), path).is_none(),
            "{path}"
        );
    }
}

// ---------------------------------------------------------------------
// T7. Aggregate.
// ---------------------------------------------------------------------

fn named(host: UnitHost, name: &str) -> InspectionSnapshot {
    reload(&scan(host.file("/etc/hostname", &format!("{name}\n"))))
}

/// After every fleet case: the invariants hold on the merged snapshot and
/// on it reloaded, every config entry at a repo destination is decided
/// under Repositories, and excluding every repo with a control, in sorted
/// order and in reverse, leaves every carriable entry out of the refine
/// export.
fn assert_fleet_invariants(merged: &InspectionSnapshot) {
    check_repo_ownership(merged).expect("merged");
    let reloaded = reload(merged);
    check_repo_ownership(&reloaded).expect("reloaded");
    let ownership = repo_ownership(rpm(merged));
    for row in merged.config.iter().flat_map(|c| &c.files) {
        if Destination::new(&row.path).is_ok_and(|d| ownership.contains(&d))
            && !row.disposition.is_advisory()
        {
            assert_eq!(
                row.shadowed_by,
                Some(ConfigShadow::Repositories),
                "{}",
                row.path
            );
        }
    }
    let mut again = merged.clone();
    settle_repo_ownership(&mut again);
    assert_eq!(&again, merged, "settling twice changes nothing");

    let sorted: Vec<String> = ownership
        .sections()
        .into_iter()
        .map(|s| s.as_str().to_owned())
        .collect();
    let reversed: Vec<String> = sorted.iter().rev().cloned().collect();
    for order in [sorted, reversed] {
        let mut s = session(&reloaded);
        for id in &order {
            if s.repo_state(id).control != RepoControl::None {
                s.apply(set_repo(id, false))
                    .unwrap_or_else(|e| panic!("excluding {id} in {order:?}: {e}"));
            }
        }
        let tree = export_session(&s);
        for (kind, list) in [
            (OwnedKind::RepoFile, &rpm(merged).repo_files),
            (OwnedKind::GpgKey, &rpm(merged).gpg_keys),
        ] {
            for entry in list.iter().filter(|e| ownership.carriable(kind, e)) {
                assert!(
                    staged(tree.path(), &entry.path).is_none(),
                    "{} after excluding {order:?}",
                    entry.path
                );
            }
        }
    }
}

#[test]
fn t7_the_most_common_version_ships_with_a_line_naming_the_others() {
    let custom = |base: &str| {
        UnitHost::new().base_package("bash").file(
            CUSTOM_REPO,
            &format!("[custom]\nbaseurl=https://{base}.example/\n"),
        )
    };
    let (merged, _) = merge_snapshots(
        vec![
            named(custom("one"), "h1"),
            named(custom("one"), "h2"),
            named(custom("two"), "h3"),
        ],
        None,
        None,
    )
    .unwrap();
    assert_fleet_invariants(&merged);
    let row = &rpm(&merged).repo_files[0];
    let prevalence = row.aggregate.as_ref().unwrap();
    assert_eq!(
        prevalence.payload_hosts.as_deref(),
        Some(&["h1".to_string(), "h2".to_string()][..])
    );
    let tree = render_export(&merged);
    let file = containerfile(tree.path());
    let line = format!(
        "# {CUSTOM_REPO}: the export copies the version 2 of 3 hosts with this file have; a \
         different version is on h3"
    );
    assert!(file.lines().any(|l| l == line), "{file}");
}

#[test]
fn t7_a_key_whose_content_differs_between_hosts_gets_the_line() {
    let host = |key: &str| {
        UnitHost::new()
            .base_package("bash")
            .file(CUSTOM_REPO, &custom_repo())
            .file(CUSTOM_KEY, key)
    };
    let other = PGP.replace("mQENBFtest", "mQENBFother");
    let (merged, _) = merge_snapshots(
        vec![named(host(PGP), "h1"), named(host(&other), "h2")],
        None,
        None,
    )
    .unwrap();
    assert_fleet_invariants(&merged);
    let file = containerfile(render_export(&merged).path());
    assert!(
        file.lines().any(|l| l.starts_with(&format!(
            "# {CUSTOM_KEY}: the export copies the version 1 of 2"
        ))),
        "{file}"
    );
}

/// An unmodified, RPM-owned `epel.repo` that two `epel-release` versions
/// put on two hosts: no config walk records it, and the export still says
/// which hosts' version it does not copy.
#[test]
fn t7_an_unmodified_rpm_owned_repo_file_that_differs_gets_the_line() {
    let host = |base: &str| {
        UnitHost::new()
            .base_package("bash")
            .owned_file(
                EPEL_REPO,
                &format!("[epel]\nmetalink=https://{base}.example/epel\nenabled=1\n"),
                "epel-release",
            )
            .package("epel-release")
    };
    let (merged, _) = merge_snapshots(
        vec![named(host("one"), "h1"), named(host("two"), "h2")],
        None,
        None,
    )
    .unwrap();
    assert_fleet_invariants(&merged);
    assert!(
        config_row(&merged, EPEL_REPO).is_none(),
        "no config walk row"
    );
    let file = containerfile(render_export(&merged).path());
    let line = format!(
        "# {EPEL_REPO}: the export copies the version 1 of 2 hosts with this file have; a \
         different version is on h2"
    );
    assert!(file.lines().any(|l| l == line), "{file}");
}

/// The config rows of a fleet repo file are read-only: every config op on
/// them is refused, and the export stages the repo row's representative.
#[test]
fn t7_fleet_config_variants_of_a_repo_file_are_read_only() {
    let custom = |base: &str| {
        UnitHost::new().base_package("bash").file(
            CUSTOM_REPO,
            &format!("[custom]\nbaseurl=https://{base}.example/\n"),
        )
    };
    for (first, second) in [("alpha", "beta"), ("beta", "alpha")] {
        let (merged, _) = merge_snapshots(
            vec![
                named(custom(first), "host-a"),
                named(custom(second), "host-b"),
            ],
            None,
            None,
        )
        .unwrap();
        assert_fleet_invariants(&merged);
        let representative = rpm(&merged).repo_files[0].content.clone();
        let mut s = session(&reload(&merged));
        let hash = |content: &str| ContentHash::from_content(content.as_bytes());
        let ops = [
            set_config(CUSTOM_REPO, true),
            set_config(CUSTOM_REPO, false),
            RefinementOp::SelectVariant {
                item_id: ItemId::Config {
                    path: CUSTOM_REPO.into(),
                },
                target: hash(&representative),
            },
            RefinementOp::EditVariant {
                item_id: ItemId::Config {
                    path: CUSTOM_REPO.into(),
                },
                content: "[custom]\nbaseurl=https://edited.example/\n".into(),
                based_on: None,
            },
            RefinementOp::DiscardVariant {
                item_id: ItemId::Config {
                    path: CUSTOM_REPO.into(),
                },
                variant: hash(&representative),
            },
        ];
        for op in ops {
            assert!(
                matches!(
                    s.apply(op.clone()),
                    Err(RefineError::InventoryNotToggleable(_))
                ),
                "{op:?}"
            );
        }
        assert_eq!(s.timeline_len(), 0);
        assert_eq!(
            staged(export_session(&s).path(), CUSTOM_REPO).as_deref(),
            Some(representative.as_str())
        );
    }
}

const CORR_A: &str = "/etc/pki/rpm-gpg/A";
const CORR_B: &str = "/etc/pki/rpm-gpg/B";

/// Three hosts, named `names`, whose `x.repo`, `y.repo` and `z.repo` each
/// name key A on two hosts and key B on the third, a different third host
/// for each file: every representative names A, and B, which every host
/// collected, has no owner.
fn correlated_hosts(names: [&str; 3]) -> Vec<InspectionSnapshot> {
    let repo = |id: &str, key: &str| {
        format!("[{id}]\nbaseurl=https://{id}.example/\nenabled=1\ngpgkey=file://{key}\n")
    };
    let host = |x: &str, y: &str, z: &str| {
        UnitHost::new()
            .base_package("bash")
            .file("/etc/yum.repos.d/x.repo", &repo("x", x))
            .file("/etc/yum.repos.d/y.repo", &repo("y", y))
            .file("/etc/yum.repos.d/z.repo", &repo("z", z))
            .file(CORR_A, PGP)
            .file(CORR_B, PGP)
    };
    vec![
        named(host(CORR_B, CORR_A, CORR_A), names[0]),
        named(host(CORR_A, CORR_B, CORR_A), names[1]),
        named(host(CORR_A, CORR_A, CORR_B), names[2]),
    ]
}

/// What a merge decided for each repo file and key, without the host
/// names its prevalence records.
fn repo_decisions(snap: &InspectionSnapshot) -> Vec<(String, String, bool)> {
    rpm(snap)
        .repo_files
        .iter()
        .chain(&rpm(snap).gpg_keys)
        .map(|e| {
            (
                e.path.clone(),
                e.content.clone(),
                e.disposition.is_included(),
            )
        })
        .collect()
}

/// Correlated keys over three hosts: every representative names A, so B,
/// which every host collected, has no owner, is never staged, and the
/// export names it. Neither the order the hosts are given in nor the
/// order of their names changes what the merge decides.
#[test]
fn t7_a_key_no_copied_version_names_is_left_out_and_named() {
    const B: &str = CORR_B;
    const A: &str = CORR_A;
    let hosts = || correlated_hosts(["h1", "h2", "h3"]);
    let (merged, _) = merge_snapshots(hosts(), None, None).unwrap();
    assert_fleet_invariants(&merged);
    let mut rotated = hosts();
    rotated.rotate_left(1);
    let (merged_rotated, _) = merge_snapshots(rotated, None, None).unwrap();
    assert_eq!(merged.rpm, merged_rotated.rpm);
    assert_eq!(merged.config, merged_rotated.config);
    // The same layouts under names that sort the other way round.
    let (renamed, _) =
        merge_snapshots(correlated_hosts(["zeta", "mid", "alpha"]), None, None).unwrap();
    assert_fleet_invariants(&renamed);
    assert_eq!(repo_decisions(&renamed), repo_decisions(&merged));

    let b = rpm(&merged).gpg_keys.iter().find(|k| k.path == B).unwrap();
    assert!(!b.disposition.is_included());
    assert!(shadowed_by_repositories(&merged, B));
    let mut s = session(&reload(&merged));
    match export_state(&s, B) {
        RepoOwnedExport::NoOwner {
            cause: NoOwnerCause::Fleet { hosts, .. },
        } => assert_eq!(hosts, &["h1", "h2", "h3"]),
        other => panic!("{other:?}"),
    }
    for id in ["x", "y", "z"] {
        s.apply(set_repo(id, false)).unwrap();
    }
    assert!(repo_files_and_keys(export_session(&s).path()).is_empty());
    for id in ["x", "y", "z"] {
        s.apply(set_repo(id, true)).unwrap();
    }
    let tree = export_session(&s);
    assert!(staged(tree.path(), A).is_some());
    assert!(staged(tree.path(), B).is_none());
    let packed = tempfile::tempdir().unwrap();
    let warnings = s
        .export_plan(s.generation())
        .unwrap()
        .render(&packed.path().join("x.tar.gz"))
        .unwrap();
    assert!(
        warnings.iter().any(|w| w.text()
            == format!(
                "{B} is not copied: no version of a repo file that the export copies names it. \
                 h1, h2, h3 have it because a different version of a repo file names it there."
            )),
        "{warnings:?}"
    );
}

/// Two hosts' `vendor.repo` name different keys: the representative names
/// the first host's, which only that host has, so the aggregate narrows it
/// out while the repo file ships. A package from that repo is not
/// installed by name, on the aggregate tarball and the refine export,
/// until re-including the repo brings the key back.
#[test]
fn t7_a_package_whose_key_an_aggregate_narrowed_out_renders_the_fixme() {
    const VENDOR_REPO: &str = "/etc/yum.repos.d/vendor.repo";
    let host = |key: &str| {
        UnitHost::new()
            .base_package("bash")
            .file(
                VENDOR_REPO,
                &format!(
                    "[vendor]\nbaseurl=https://vendor.example/\nenabled=1\ngpgkey=file://{key}\n"
                ),
            )
            .file(key, PGP)
            .package_from("vend-tool", "vendor")
            .enabled_repos(&["vendor"])
    };
    let (k1, k2) = ("/etc/pki/rpm-gpg/K1", "/etc/pki/rpm-gpg/K2");
    let (merged, _) = merge_snapshots(
        vec![named(host(k1), "h1"), named(host(k2), "h2")],
        None,
        None,
    )
    .unwrap();
    assert_fleet_invariants(&merged);
    let repo_file = rpm(&merged)
        .repo_files
        .iter()
        .find(|f| f.path == VENDOR_REPO)
        .unwrap();
    assert!(repo_file.disposition.is_included());
    assert!(repo_file.content.contains(k1));
    let key = rpm(&merged).gpg_keys.iter().find(|k| k.path == k1).unwrap();
    assert!(!key.disposition.is_included(), "narrowed out");

    let fixme = lost_route_fixme("vend-tool", "vendor", "which also re-includes its packages");
    let file = containerfile(render_export(&merged).path());
    assert!(!installs(&file, "vend-tool"), "aggregate tarball:\n{file}");
    assert!(
        file.lines().any(|l| l == fixme),
        "aggregate tarball:\n{file}"
    );

    // Refine opens an aggregate's packages excluded here; the operator
    // includes this one, and its repo's switch is kept.
    let mut s = session(&reload(&merged));
    s.apply(set_package("vend-tool", true)).unwrap();
    let file = containerfile(export_session(&s).path());
    assert!(!installs(&file, "vend-tool"), "refine:\n{file}");
    assert!(file.lines().any(|l| l == fixme), "refine:\n{file}");
    s.apply(set_repo("vendor", true)).unwrap();
    let tree = export_session(&s);
    assert!(staged(tree.path(), k1).is_some());
    assert!(installs(&containerfile(tree.path()), "vend-tool"));
}

/// A key one host's repo owns and another host records as a plain config
/// file: the merged config row is decided under Repositories in both host
/// orders, and only `custom`'s switch stages the key.
#[test]
fn t7_a_key_one_hosts_repo_owns_is_decided_by_that_repo_in_both_orders() {
    const K: &str = "/etc/pki/rpm-gpg/RPM-GPG-KEY-k";
    let plain = || UnitHost::new().base_package("bash").file(K, PGP);
    let with_repo = || {
        plain().file(
            CUSTOM_REPO,
            &format!("[custom]\nbaseurl=https://custom.example/\nenabled=1\ngpgkey=file://{K}\n"),
        )
    };
    let mut seen_config = None;
    for (a, b) in [("host-a", "host-b"), ("host-b", "host-a")] {
        let (merged, _) =
            merge_snapshots(vec![named(plain(), a), named(with_repo(), b)], None, None).unwrap();
        assert_fleet_invariants(&merged);
        let rows: Vec<_> = merged
            .config
            .iter()
            .flat_map(|c| &c.files)
            .filter(|f| f.path == K)
            .cloned()
            .collect();
        assert_eq!(rows.len(), 1, "one variant");
        assert!(shadowed_by_repositories(&merged, K));
        if let Some(previous) = &seen_config {
            assert_eq!(&rows[0].content, previous);
        }
        seen_config = Some(rows[0].content.clone());

        let mut s = session(&reload(&merged));
        let state = s.repo_state("custom");
        assert_eq!(state.control, RepoControl::Full);
        assert!(state.kept);
        assert_eq!((state.carriage_included, state.carriage_count), (0, 2));
        assert!(staged(export_session(&s).path(), K).is_none());
        for include in [true, false] {
            assert!(matches!(
                s.apply(set_config(K, include)),
                Err(RefineError::InventoryNotToggleable(_))
            ));
        }
        s.apply(set_repo("custom", true)).unwrap();
        let tree = export_session(&s);
        assert!(staged(tree.path(), K).is_some());
        assert!(staged(tree.path(), CUSTOM_REPO).is_some());
        s.apply(set_repo("custom", false)).unwrap();
        let tree = export_session(&s);
        assert!(repo_files_and_keys(tree.path()).is_empty());
        s.undo().unwrap();
        assert!(staged(export_session(&s).path(), K).is_some());
        s.redo().unwrap();
        assert!(staged(export_session(&s).path(), K).is_none());
    }
}

// ---------------------------------------------------------------------
// T9. Load.
// ---------------------------------------------------------------------

#[test]
fn t9_load_refuses_each_violation() {
    let snap = scan(base_host());
    let json = serde_json::to_value(&snap).unwrap();
    InspectionSnapshot::load(&json.to_string()).expect("the unedited snapshot loads");
    let files = json["config"]["files"].as_array().unwrap();
    let at = |path: &str| files.iter().position(|f| f["path"] == path).unwrap();

    let mut unshadowed = json.clone();
    let i = at(CUSTOM_KEY);
    unshadowed["config"]["files"][i]["shadowed_by"] = serde_json::Value::Null;
    unshadowed["config"]["files"][i]["disposition"] =
        serde_json::to_value(FindingKind::included()).unwrap();
    let err = InspectionSnapshot::load(&unshadowed.to_string()).unwrap_err();
    assert!(err.to_string().contains(CUSTOM_KEY), "{err}");

    let mut stray = json.clone();
    let i = at(ORPHAN_KEY);
    stray["config"]["files"][i]["shadowed_by"] = serde_json::json!("repositories");
    stray["config"]["files"][i]["disposition"] =
        serde_json::to_value(FindingKind::inventory()).unwrap();
    let err = InspectionSnapshot::load(&stray.to_string()).unwrap_err();
    assert!(err.to_string().contains("no repo records it"), "{err}");
}

/// The correlated aggregate's ownerless key, made included in its JSON, is
/// refused at load as a key no repo op can carry.
#[test]
fn t9_load_refuses_an_included_key_no_copied_version_names() {
    let (merged, _) = merge_snapshots(correlated_hosts(["h1", "h2", "h3"]), None, None).unwrap();
    let json = serde_json::to_value(&merged).unwrap();
    InspectionSnapshot::load(&json.to_string()).expect("the unedited aggregate loads");
    let i = json["rpm"]["gpg_keys"]
        .as_array()
        .unwrap()
        .iter()
        .position(|k| k["path"] == CORR_B)
        .unwrap();
    let mut included = json.clone();
    included["rpm"]["gpg_keys"][i]["disposition"] =
        serde_json::to_value(FindingKind::included()).unwrap();
    let err = InspectionSnapshot::load(&included.to_string()).unwrap_err();
    assert!(
        err.to_string().contains(&format!(
            "{CORR_B}: a repo file or key no repo op can carry is included"
        )),
        "{err}"
    );
}

/// A key the scan refused cannot come back through edited JSON: dropping
/// its `rejected` marker leaves a placeholder recorded as a usable key,
/// and putting the refused bytes back into the key or its config row is
/// refused too.
#[test]
fn t9_load_refuses_a_rejected_key_made_usable_or_given_its_bytes() {
    const NOTPGP: &str = "/etc/pki/rpm-gpg/RPM-GPG-KEY-notpgp";
    const PRIV: &str = "/etc/pki/rpm-gpg/RPM-GPG-KEY-priv";
    let snap = scan(with_parser_inputs(base_host()));
    let json = serde_json::to_value(&snap).unwrap();
    InspectionSnapshot::load(&json.to_string()).expect("the unedited snapshot loads");
    let key_at = |path: &str| {
        json["rpm"]["gpg_keys"]
            .as_array()
            .unwrap()
            .iter()
            .position(|k| k["path"] == path)
            .unwrap()
    };
    let row_at = |path: &str| {
        json["config"]["files"]
            .as_array()
            .unwrap()
            .iter()
            .position(|f| f["path"] == path)
            .unwrap()
    };

    let mut unmarked = json.clone();
    unmarked["rpm"]["gpg_keys"][key_at(NOTPGP)]
        .as_object_mut()
        .unwrap()
        .remove("rejected");
    let err = InspectionSnapshot::load(&unmarked.to_string()).unwrap_err();
    assert!(
        err.to_string().contains(&format!(
            "{NOTPGP}: a key recorded as usable is not a PGP public key"
        )),
        "{err}"
    );

    let mut key_bytes = json.clone();
    key_bytes["rpm"]["gpg_keys"][key_at(PRIV)]["content"] =
        serde_json::json!(private_beside_public());
    let err = InspectionSnapshot::load(&key_bytes.to_string()).unwrap_err();
    assert!(
        err.to_string()
            .contains(&format!("{PRIV}: a key the scan refused")),
        "{err}"
    );

    let mut row_bytes = json.clone();
    row_bytes["config"]["files"][row_at(PRIV)]["content"] =
        serde_json::json!(private_beside_public());
    let err = InspectionSnapshot::load(&row_bytes.to_string()).unwrap_err();
    assert!(
        err.to_string()
            .contains(&format!("{PRIV}: a key the scan refused")),
        "{err}"
    );
}

// ---------------------------------------------------------------------
// T10. Redaction.
// ---------------------------------------------------------------------

#[test]
fn t10_a_key_whose_reference_redaction_rewrote_is_left_out_and_named() {
    const VEND: &str = "/etc/yum.repos.d/vend.repo";
    const KEY: &str = "/etc/pki/rpm-gpg/token=vendor";
    let host = || {
        UnitHost::new()
            .base_package("bash")
            .file(
                VEND,
                &format!("[vend]\nbaseurl=https://vend.example/\nenabled=1\ngpgkey=file://{KEY}\n"),
            )
            .file(KEY, PGP)
            .package_from("pkg", "vend")
    };

    // Collection: owned and shadowed.
    let collected = scan_unredacted(host());
    assert!(
        rpm(&collected)
            .gpg_keys
            .iter()
            .any(|k| k.path == KEY && k.disposition.is_included())
    );
    assert!(shadowed_by_repositories(&collected, KEY));

    // Redaction and finish_scan, and run_pipeline, which runs the same
    // sequence in one call, decide the same.
    let snap = scan_finished(host());
    let no_base = || {
        UnitHost::new()
            .file(
                VEND,
                &format!("[vend]\nbaseurl=https://vend.example/\nenabled=1\ngpgkey=file://{KEY}\n"),
            )
            .file(KEY, PGP)
            .package_from("pkg", "vend")
    };
    let piped = run_pipeline(no_base());
    let sequenced = scan_finished(no_base());
    assert_eq!(piped.rpm, sequenced.rpm);
    assert_eq!(piped.config, sequenced.config);
    assert_eq!(piped.redactions, sequenced.redactions);
    check_repo_ownership(&piped).expect("run_pipeline settles after redaction");
    let vend = rpm(&snap)
        .repo_files
        .iter()
        .find(|f| f.path == VEND)
        .unwrap();
    assert!(
        !vend.content.contains("token=vendor"),
        "the reference is redacted"
    );
    assert!(snap.redactions.iter().any(|r| r.path == VEND));
    let key = rpm(&snap).gpg_keys.iter().find(|k| k.path == KEY).unwrap();
    assert!(!key.disposition.is_included(), "no owner: excluded");
    assert!(shadowed_by_repositories(&snap, KEY));
    check_repo_ownership(&snap).expect("settled after redaction");

    // Serialization and load.
    let json = serde_json::to_string_pretty(&snap).unwrap();
    let loaded = InspectionSnapshot::load(&json).expect("loads");
    let mut again = loaded.clone();
    settle_repo_ownership(&mut again);
    assert_eq!(again, loaded);

    // The scan tarball stages vend.repo and not the key.
    let tree = render_export(&snap);
    assert!(staged(tree.path(), VEND).is_some());
    assert!(staged(tree.path(), KEY).is_none());

    // Refine.
    let mut s = session(&loaded);
    match export_state(&s, KEY) {
        RepoOwnedExport::NoOwner {
            cause: NoOwnerCause::Redacted { repo_files },
        } => assert_eq!(repo_files, &[VEND]),
        other => panic!("{other:?}"),
    }
    let state = s.repo_state("vend");
    assert_eq!(state.control, RepoControl::Full);
    assert_eq!((state.carriage_included, state.carriage_count), (1, 1));
    s.apply(set_repo("vend", false)).unwrap();
    assert!(staged(export_session(&s).path(), VEND).is_none());
    s.apply(set_repo("vend", true)).unwrap();
    let tree = export_session(&s);
    assert!(staged(tree.path(), VEND).is_some());
    assert!(staged(tree.path(), KEY).is_none());
    let packed = tempfile::tempdir().unwrap();
    let warnings = s
        .export_plan(s.generation())
        .unwrap()
        .render(&packed.path().join("x.tar.gz"))
        .unwrap();
    let expected = format!(
        "{KEY} is not copied: redaction changed a gpgkey= reference in {VEND}, so no repo file \
         the export copies names it. Review the redacted reference; if it named this key, \
         restore it and add the key to the image yourself."
    );
    assert!(
        warnings.iter().any(|w| w.text() == expected),
        "{warnings:?}"
    );

    // Aggregate of two copies: the cause is fleet, with the review sentence.
    let (merged, _) = merge_snapshots(
        vec![
            reload(&scan_finished(host().file("/etc/hostname", "h1\n"))),
            reload(&scan_finished(host().file("/etc/hostname", "h2\n"))),
        ],
        None,
        None,
    )
    .unwrap();
    check_repo_ownership(&merged).expect("merged");
    check_repo_ownership(&reload(&merged)).expect("reloaded");
    let s = session(&reload(&merged));
    assert!(matches!(
        export_state(&s, KEY),
        RepoOwnedExport::NoOwner {
            cause: NoOwnerCause::Fleet { .. }
        }
    ));
    let combined = format!(
        "{KEY} is not copied: no version of a repo file that the export copies names it. h1, h2 \
         have it because a different version of a repo file names it there. Review the redacted \
         reference; if it named this key, restore it and add the key to the image yourself."
    );
    let warnings = export_warnings(&s);
    assert!(
        warnings.iter().any(|w| w.text() == combined),
        "{warnings:?}"
    );

    // Without redaction (`--no-redaction` through finish_scan), the key
    // stays included and owned.
    let raw = scan_finished_raw(host());
    assert!(
        rpm(&raw)
            .gpg_keys
            .iter()
            .any(|k| k.path == KEY && k.disposition.is_included())
    );
    assert!(staged(render_export(&raw).path(), KEY).is_some());
}
