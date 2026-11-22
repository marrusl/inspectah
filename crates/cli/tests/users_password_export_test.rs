//! A user the Containerfile's users block refuses keeps its password hash
//! out of the image build on the refine export path too.
//!
//! The pipeline's U7 test covers the scan export (`render_all`). Refine
//! renders the same snapshot through its own export, after its own
//! `UserStrategy` op, so the same host runs through it here: `zed` (no
//! host group for its gid, so refused) and `carol` (accepted), both with
//! a hash the scan kept.

use std::path::Path;

use inspectah_collect::executor::mock::MockExecutor;
use inspectah_collect::inspectors::users::{UserGroupOptions, UsersGroupsInspector};
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::traits::inspector::{InspectionContext, Inspector, InspectorError};
use inspectah_core::traits::progress::NullProgress;
use inspectah_core::types::completeness::SectionData;
use inspectah_core::types::os::OsRelease;
use inspectah_core::types::system::SourceSystem;
use inspectah_core::types::users::UserContainerfileStrategy;
use inspectah_refine::session::RefineSession;
use inspectah_refine::types::RefinementOp;

const ZED_HASH: &str = "$6$zedsalt$ZedHashForTheRefusedUser";
const CAROL_HASH: &str = "$6$carolsalt$CarolHashForTheAcceptedUser";

/// The export files that carry every included user's kept hash, whatever
/// the Containerfile does with the user.
const HASH_HOLDERS: [&str; 3] = [
    "inspectah-users.ks",
    "inspectah-users.toml",
    "inspection-snapshot.json",
];

const PASSWD: &str = "carol:x:1002:100::/home/carol:/bin/bash\n\
                      zed:x:1009:1009::/home/zed:/bin/bash\n";
/// No entry for gid 1009, so the users block refuses `zed`.
const GROUP: &str = "users:x:100:\n";

/// The users section the collector records for the host, with password
/// hashes kept, as `scan --preserve passwords` records it.
fn scanned_users() -> InspectionSnapshot {
    let exec = MockExecutor::new()
        .with_file("/etc/passwd", PASSWD)
        .with_file("/etc/group", GROUP)
        .with_file(
            "/etc/shadow",
            &format!("carol:{CAROL_HASH}:19000:0:99999:7:::\nzed:{ZED_HASH}:19000:0:99999:7:::\n"),
        );
    let source = SourceSystem::PackageBased {
        os_release: OsRelease {
            name: "Red Hat Enterprise Linux".into(),
            version_id: "9.6".into(),
            id: "rhel".into(),
            ..Default::default()
        },
    };
    let ctx = InspectionContext {
        source_system: &source,
        executor: &inspectah_core::machine_state::ScreenedExecutor::new(&exec),
        rpm_state: None,
        baseline_data: None,
    };
    let inspector = UsersGroupsInspector::with_options(UserGroupOptions {
        preserve_password_hashes: true,
        ..Default::default()
    });
    let output = match inspector.inspect(&ctx, &NullProgress) {
        Ok(output) => output,
        Err(InspectorError::Degraded { partial, .. }) => *partial,
        Err(e) => panic!("inspector failed: {e}"),
    };
    let SectionData::UsersGroups(users) = output.section else {
        panic!("expected the users section");
    };
    let mut snap = InspectionSnapshot::new();
    snap.users_groups = Some(users);
    snap
}

/// Every regular file in a `.tar.gz` whose content holds `needle`, by
/// its path below the archive's single top-level directory.
fn tarball_files_containing(tarball: &Path, needle: &str) -> Vec<String> {
    let file = std::fs::File::open(tarball).expect("the tarball opens");
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(file));
    let mut out = Vec::new();
    for entry in archive.entries().expect("the tarball reads") {
        let mut entry = entry.expect("an entry");
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let path = entry.path().expect("a path").to_string_lossy().into_owned();
        let mut content = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut content).expect("read an entry");
        if content
            .windows(needle.len())
            .any(|w| w == needle.as_bytes())
        {
            let rel = path.split_once('/').map_or(path.as_str(), |(_, rest)| rest);
            out.push(rel.to_string());
        }
    }
    out.sort();
    out
}

/// Read one file from a `.tar.gz` by its path below the top-level directory.
fn tarball_file(tarball: &Path, rel: &str) -> String {
    let file = std::fs::File::open(tarball).expect("the tarball opens");
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(file));
    for entry in archive.entries().expect("the tarball reads") {
        let mut entry = entry.expect("an entry");
        let path = entry.path().expect("a path").to_string_lossy().into_owned();
        if path.split_once('/').is_some_and(|(_, rest)| rest == rel) {
            let mut content = String::new();
            std::io::Read::read_to_string(&mut entry, &mut content).expect("read an entry");
            return content;
        }
    }
    panic!("{rel} is not in the export");
}

#[test]
fn a_refused_users_hash_stays_out_of_the_refine_export_build() {
    let snap = scanned_users();
    for (name, hash) in [("zed", ZED_HASH), ("carol", CAROL_HASH)] {
        let user = snap
            .users_groups
            .as_ref()
            .and_then(|ug| ug.users.iter().find(|u| u["name"] == name))
            .unwrap_or_else(|| panic!("the collector records {name}"));
        assert_eq!(user["password_hash"], hash, "the scan keeps {name}'s hash");
    }

    let mut session = RefineSession::new(snap);
    for name in ["zed", "carol"] {
        session
            .apply(RefinementOp::UserStrategy {
                username: name.into(),
                strategy: UserContainerfileStrategy::Useradd,
            })
            .unwrap_or_else(|e| panic!("{name} can be set to useradd: {e:?}"));
    }
    let dir = tempfile::tempdir().expect("tempdir");
    let tarball = dir.path().join("export.tar.gz");
    session
        .export_plan(session.generation())
        .expect("the generation matches")
        .render(&tarball)
        .expect("the export writes");

    let containerfile = tarball_file(&tarball, "Containerfile");
    assert!(
        containerfile.contains("# FIXME: user zed not created"),
        "zed is refused:\n{containerfile}"
    );
    let chpasswd: Vec<&str> = containerfile
        .lines()
        .filter(|l| l.contains("chpasswd"))
        .collect();
    assert_eq!(
        chpasswd,
        vec![format!("RUN printf '%s\\n' 'carol:{CAROL_HASH}' | chpasswd -e").as_str()],
        "{containerfile}"
    );
    assert!(!containerfile.contains(ZED_HASH), "{containerfile}");

    // Outside the Containerfile, the hash is only where the scan keeps it
    // for every included user: the snapshot, and the kickstart and
    // blueprint fragments, which do not use the users block's plan.
    assert_eq!(
        tarball_files_containing(&tarball, ZED_HASH),
        HASH_HOLDERS,
        "zed's hash outside the snapshot and the fragments"
    );
    let mut carol_files = HASH_HOLDERS.to_vec();
    carol_files.insert(0, "Containerfile");
    assert_eq!(tarball_files_containing(&tarball, CAROL_HASH), carol_files);
}
