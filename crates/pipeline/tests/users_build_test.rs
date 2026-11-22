//! B1: the users block builds on a real bootc base, and stops where the
//! base or a package already uses a group differently.
//!
//! The unit tests in `users_render_test` prove the text of every line.
//! Only a real build proves that `getent`, `grep` and `groupadd` behave as
//! the group lines assume against the base's `/etc/group` and
//! `/usr/lib/group` and against groups a package creates, that `useradd
//! -M` succeeds, that `bootc container lint` passes, and that the
//! tmpfiles lines resolve. Every case here does that, so every case is
//! `#[ignore]`d and needs a Linux host with podman.
//!
//! # Running it
//!
//! On a subscribed RHEL host (or any Linux host whose podman can pull the
//! base and whose builds reach the base's dnf repositories), after
//! `podman login registry.redhat.io`:
//!
//! ```text
//! INSPECTAH_BUILD_TEST_BASE=registry.redhat.io/rhel10/rhel-bootc:10.2 \
//!     cargo test -p inspectah-pipeline --test integration users_build_test \
//!     -- --ignored --test-threads=1
//! ```
//!
//! and again with `registry.redhat.io/rhel9/rhel-bootc:9.8`.
//!
//! - `INSPECTAH_BUILD_TEST_BASE` is the image every case builds `FROM`: a
//!   RHEL `rhel-bootc` base, because the cases rely on its layout (most
//!   groups in `/usr/lib/group` read through nss-altfiles, `/home` a link
//!   to `/var/home`). Every case fails when it is unset.
//! - The two package cases run `dnf -y install httpd`, so the base's
//!   repositories must be reachable from the build; on RHEL that means a
//!   subscribed build host. They fail, not skip, when dnf cannot reach
//!   them.
//! - `bootc container lint` exits non-zero on an error and fails the
//!   build there, so a case that expects a build to succeed also proves
//!   the lint clean.
//! - `--test-threads=1` is not required, but keeps the podman logs of
//!   concurrent builds from interleaving.
//!
//! Each case renders a collector-built snapshot with `render_all`, takes
//! the Containerfile's users section (and, where the case says so, its
//! `/var` section), and builds it between `FROM <base>` (plus any earlier
//! `RUN` the case names) and `RUN bootc container lint`, using the render
//! directory as the build context. Gids the base or a package owns are
//! read from the base first, never hard-coded. The built image is removed
//! when the case ends.

use std::path::Path;
use std::process::{Command, Output};

use inspectah_collect::executor::mock::MockExecutor;
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::traits::renderer::RenderContext;

use crate::users_render_test::{collect_storage, collect_users, spec_host, useradd, with_var_lib};

/// The environment variable naming the base image every case builds from.
const BASE_ENV: &str = "INSPECTAH_BUILD_TEST_BASE";

/// The section headers this harness takes from the rendered Containerfile.
const USERS_SECTION: &str = "# === Users and Groups ===";
const VAR_SECTION: &str = "# === /var directory provisioning ===";

/// The earlier `RUN` of the package cases: `httpd` creates `apache`.
const INSTALL_HTTPD: &str = "RUN dnf -y install httpd";

/// The users tmpfiles file and the `/var` fragment, as installed.
const USERS_CONF: &str = "/usr/lib/tmpfiles.d/inspectah-users.conf";
const VAR_CONF: &str = "/usr/lib/tmpfiles.d/inspectah-var.conf";

/// What a group line prints when it stops the build.
const STOP_MESSAGE: &str = "the image already uses the name or the gid differently";

/// Offset that turns a gid the image uses into one it does not.
const GID_OFFSET: u32 = 1000;

fn base() -> String {
    std::env::var(BASE_ENV).unwrap_or_else(|_| {
        panic!(
            "set {BASE_ENV} to a rhel-bootc base image, e.g. \
             registry.redhat.io/rhel10/rhel-bootc:10.2; see this file's doc comment"
        )
    })
}

fn podman(args: &[&str]) -> Output {
    Command::new("podman")
        .args(args)
        .output()
        .expect("podman runs; this test needs podman on the host")
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// The gid of `group` in a fresh container of `image`, after `setup`
/// (a shell command, or `""`). Panics when `setup` fails or the group is
/// missing, so a case never runs on a gid it guessed.
fn gid_in(image: &str, setup: &str, group: &str) -> u32 {
    let script = if setup.is_empty() {
        format!("getent group {group}")
    } else {
        format!("{setup} >/dev/null && getent group {group}")
    };
    let out = podman(&["run", "--rm", image, "sh", "-c", &script]);
    assert!(
        out.status.success(),
        "reading group {group} from {image} failed:\n{}",
        text(&out)
    );
    let entry = String::from_utf8_lossy(&out.stdout);
    entry
        .trim()
        .split(':')
        .nth(2)
        .and_then(|g| g.parse().ok())
        .unwrap_or_else(|| panic!("no gid in `{}`", entry.trim()))
}

/// The gid the base gives `group`.
fn base_gid(group: &str) -> u32 {
    gid_in(&base(), "", group)
}

/// The gid `httpd`'s install gives `apache` on the base.
fn apache_gid() -> u32 {
    gid_in(&base(), "dnf -y install httpd", "apache")
}

/// The lines of `section` in a rendered Containerfile, header included,
/// up to the blank line that ends it.
fn section_of(containerfile: &str, header: &str) -> Vec<String> {
    let lines: Vec<&str> = containerfile.lines().collect();
    let start = lines
        .iter()
        .position(|l| *l == header)
        .unwrap_or_else(|| panic!("no `{header}` in:\n{containerfile}"));
    lines[start..]
        .iter()
        .take_while(|l| !l.is_empty())
        .map(|l| l.to_string())
        .collect()
}

/// A built image, removed when dropped.
struct Image {
    tag: String,
}

impl Image {
    /// Run `script` in a fresh container of the image.
    fn run(&self, script: &str) -> Output {
        podman(&["run", "--rm", &self.tag, "sh", "-c", script])
    }

    /// Run `script` and return its stdout, panicking when it fails.
    fn stdout(&self, script: &str) -> String {
        let out = self.run(script);
        assert!(out.status.success(), "`{script}` failed:\n{}", text(&out));
        String::from_utf8_lossy(&out.stdout).into_owned()
    }
}

impl Drop for Image {
    fn drop(&mut self) {
        let _ = podman(&["rmi", "-f", &self.tag]);
    }
}

/// A finished build: the image when it succeeded, and the whole log.
struct Build {
    image: Option<Image>,
    log: String,
}

/// Render `snap` and build its users section (and its `/var` section when
/// `with_var`) on the base, after the `earlier` lines.
fn build(case: &str, snap: &InspectionSnapshot, earlier: &[&str], with_var: bool) -> Build {
    let dir = tempfile::tempdir().expect("tempdir");
    inspectah_pipeline::render::render_all(snap, &RenderContext { target: None }, dir.path())
        .expect("render");
    let rendered = std::fs::read_to_string(dir.path().join("Containerfile")).expect("read");

    let mut lines = vec![format!("FROM {}", base())];
    lines.extend(earlier.iter().map(|l| l.to_string()));
    lines.extend(section_of(&rendered, USERS_SECTION));
    if with_var {
        lines.extend(section_of(&rendered, VAR_SECTION));
    }
    lines.push("RUN bootc container lint".into());
    let containerfile = dir.path().join("Containerfile.users-build");
    std::fs::write(&containerfile, lines.join("\n") + "\n").expect("write");

    let tag = format!(
        "localhost/inspectah-users-build-{case}-{}",
        std::process::id()
    );
    let out = podman(&[
        "build",
        "-f",
        path_str(&containerfile),
        "-t",
        &tag,
        path_str(dir.path()),
    ]);
    let log = format!(
        "{}\n--- Containerfile ---\n{}",
        text(&out),
        lines.join("\n")
    );
    let image = out.status.success().then_some(Image { tag });
    Build { image, log }
}

fn path_str(path: &Path) -> &str {
    path.to_str().expect("a UTF-8 temp path")
}

/// The build must succeed; returns its image.
fn built(build: Build) -> Image {
    match build.image {
        Some(image) => image,
        None => panic!("the build failed:\n{}", build.log),
    }
}

/// The build must stop at the line for `group` with `gid`.
fn stopped_at(build: Build, group: &str, gid: u32) {
    assert!(
        build.image.is_none(),
        "the build succeeded; it must stop at group {group}:\n{}",
        build.log
    );
    let message = format!("group {group} (gid {gid}): {STOP_MESSAGE}");
    assert!(
        build.log.contains(&message),
        "the build failed without `{message}`:\n{}",
        build.log
    );
}

/// Every accepted user of `snap`: its primary group exists with the host's
/// gid, each supplementary group is in its `id`, `/etc/group` holds one
/// line for each such group, and tmpfiles makes its home `drwx------`
/// owned by it and its primary group.
fn assert_users_in(image: &Image, snap: &InspectionSnapshot) {
    let users = &snap.users_groups.as_ref().expect("users").users;
    let useradd: Vec<&serde_json::Value> = users
        .iter()
        .filter(|u| u["containerfile_strategy"] == "useradd")
        .collect();
    assert!(!useradd.is_empty(), "the case creates no user");

    let mut script = format!("systemd-tmpfiles --create {USERS_CONF} || exit 90\n");
    for user in &useradd {
        let name = user["name"].as_str().expect("a name");
        script.push_str(&format!(
            "stat -c '%A %U %G' /var/home/{name} || echo 'no home'\n"
        ));
    }
    let homes = image.stdout(&script);
    let homes: Vec<&str> = homes.lines().collect();

    for (user, home) in useradd.iter().zip(homes) {
        let name = user["name"].as_str().expect("a name");
        let primary = user["primary_group"].as_str().expect("a primary group");
        let gid = &user["group_gids"][primary];
        let entry = image.stdout(&format!("getent group {primary}"));
        assert_eq!(
            entry.trim().split(':').nth(2),
            Some(gid.to_string().as_str()),
            "{name}: group {primary} is `{}`, not the host's gid {gid}",
            entry.trim()
        );
        let ids = image.stdout(&format!("id -nG {name}"));
        let supplementary = user["supplementary_groups"]
            .as_array()
            .expect("a list")
            .iter()
            .map(|g| g.as_str().expect("a name"));
        for group in supplementary {
            assert!(
                ids.split_whitespace().any(|g| g == group),
                "{name}: `id -nG` is `{}`, without {group}",
                ids.trim()
            );
            let count = image.stdout(&format!("grep -c '^{group}:' /etc/group"));
            assert_eq!(count.trim(), "1", "{name}: /etc/group lines for {group}");
        }
        assert_eq!(
            home,
            format!("drwx------ {name} {primary}"),
            "{name}: its home after tmpfiles"
        );
    }
}

/// U1's snapshot: `alice`, primary group `appgrp` the base lacks, in
/// `wheel`, which the base keeps in `/usr/lib/group`.
#[test]
#[ignore = "needs podman and INSPECTAH_BUILD_TEST_BASE; see the module docs"]
fn b1_positive_one_user() {
    let snap = useradd(collect_users(&spec_host(), false), &["alice"]);
    let image = built(build("one-user", &snap, &[], false));
    assert_eq!(image.stdout("getent group appgrp").trim(), "appgrp:x:1001:");
    assert_users_in(&image, &snap);
}

/// U2's snapshot: all four users of the spec's host.
#[test]
#[ignore = "needs podman and INSPECTAH_BUILD_TEST_BASE; see the module docs"]
fn b1_positive_four_users() {
    let snap = useradd(
        collect_users(&spec_host(), false),
        &["alice", "carol", "dave", "erin"],
    );
    let image = built(build("four-users", &snap, &[], false));
    assert_users_in(&image, &snap);
}

/// U9's snapshot, with the `/var` section: a directory the fragment owns
/// by the host's numeric gid belongs to the image's `appgrp`.
#[test]
#[ignore = "needs podman and INSPECTAH_BUILD_TEST_BASE; see the module docs"]
fn b1_numeric_ownership() {
    let exec = with_var_lib(
        spec_host(),
        &[("/var/lib/shared", "0770 500 1001 legacy appgrp\n")],
    );
    let mut snap = useradd(collect_users(&exec, false), &["alice"]);
    snap.storage = Some(collect_storage(&exec));
    let image = built(build("numeric", &snap, &[], true));
    let out = image.stdout(&format!(
        "systemd-tmpfiles --create {USERS_CONF} {VAR_CONF} || exit 90\n\
         stat -c %g /var/lib/shared\n\
         getent group appgrp | cut -d: -f3"
    ));
    let ids: Vec<&str> = out.lines().collect();
    assert_eq!(
        ids,
        vec!["1001", "1001"],
        "/var/lib/shared's gid, appgrp's gid"
    );
}

/// A host whose one user, `hostuser`, has primary group `group` with `gid`.
fn host_with_group(group: &str, gid: u32) -> InspectionSnapshot {
    let exec = MockExecutor::new()
        .with_file(
            "/etc/passwd",
            &format!("hostuser:x:1101:{gid}::/home/hostuser:/bin/bash\n"),
        )
        .with_file("/etc/group", &format!("{group}:x:{gid}:\n"));
    useradd(collect_users(&exec, false), &["hostuser"])
}

/// A host group whose name the image lacks, with a gid the image already
/// gives another group: in the base's `/etc/group` (`kvm`), only in its
/// `/usr/lib/group` (`users`), or created by a package (`apache`).
#[test]
#[ignore = "needs podman and INSPECTAH_BUILD_TEST_BASE; see the module docs"]
fn b1_missing_name_occupied_gid() {
    let kvm = base_gid("kvm");
    stopped_at(
        build("virt", &host_with_group("virt", kvm), &[], false),
        "virt",
        kvm,
    );

    let users = base_gid("users");
    stopped_at(
        build("staff", &host_with_group("staff", users), &[], false),
        "staff",
        users,
    );

    let apache = apache_gid();
    stopped_at(
        build(
            "web",
            &host_with_group("web", apache),
            &[INSTALL_HTTPD],
            false,
        ),
        "web",
        apache,
    );
}

/// A host group the image has by name with another gid: in the base's
/// `/etc/group` (`kvm`), only in its `/usr/lib/group` (`users`), or
/// created by a package (`apache`).
#[test]
#[ignore = "needs podman and INSPECTAH_BUILD_TEST_BASE; see the module docs"]
fn b1_same_name_different_gid() {
    let kvm = base_gid("kvm") + GID_OFFSET;
    stopped_at(
        build("kvm", &host_with_group("kvm", kvm), &[], false),
        "kvm",
        kvm,
    );

    // The spec's case: the host's `users` at 1100, not the base's 100.
    let users = base_gid("users") + GID_OFFSET;
    stopped_at(
        build("users", &host_with_group("users", users), &[], false),
        "users",
        users,
    );

    let apache = apache_gid() + GID_OFFSET;
    stopped_at(
        build(
            "apache",
            &host_with_group("apache", apache),
            &[INSTALL_HTTPD],
            false,
        ),
        "apache",
        apache,
    );
}
