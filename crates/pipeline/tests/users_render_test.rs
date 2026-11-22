//! Integration tests for the user/group renderers (kickstart, TOML, Containerfile).
//!
//! The Containerfile tests run the users inspector, and the storage
//! inspector where a case needs `/var` directories, over a
//! `MockExecutor`: every user and directory is collector output, never a
//! hand-written snapshot field.
//!
//! The § 4.4 host of the users block spec lives in
//! `testdata/fixtures/users-block/` with the sections it renders
//! (`alice.containerfile`, `four-users.containerfile`). A real build test
//! reuses [`spec_host`], [`useradd`] and those files: it puts the section
//! between `FROM <base>` and `RUN bootc container lint`.

use std::path::Path;

use inspectah_collect::executor::mock::MockExecutor;
use inspectah_collect::inspectors::storage::StorageInspector;
use inspectah_collect::inspectors::users::{UserGroupOptions, UsersGroupsInspector};
use inspectah_core::aggregate::merge::merge_usersgroups_sections;
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::traits::executor::ExecResult;
use inspectah_core::traits::inspector::{
    InspectionContext, Inspector, InspectorError, InspectorOutput,
};
use inspectah_core::traits::progress::NullProgress;
use inspectah_core::traits::renderer::RenderContext;
use inspectah_core::types::completeness::SectionData;
use inspectah_core::types::os::OsRelease;
use inspectah_core::types::storage::StorageSection;
use inspectah_core::types::system::SourceSystem;
use inspectah_core::types::users::UserGroupSection;
use inspectah_pipeline::render::users;

/// Build a test snapshot with the given users and groups.
fn snap_with(users: Vec<serde_json::Value>, groups: Vec<serde_json::Value>) -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.users_groups = Some(UserGroupSection {
        users,
        groups,
        ..Default::default()
    });
    snap
}

fn test_user() -> serde_json::Value {
    serde_json::json!({
        "name": "alice",
        "uid": 1001,
        "gid": 1001,
        "home": "/home/alice",
        "shell": "/bin/bash",
        "include": true,
        "containerfile_strategy": "useradd",
        "supplementary_groups": ["wheel", "docker"],
        "password_hash": "$6$rounds=65536$salt$hash",
        "ssh_keys": ["ssh-rsa AAAA... alice@host"]
    })
}

fn test_group() -> serde_json::Value {
    serde_json::json!({
        "name": "docker",
        "gid": 1010,
        "source": "custom",
        "include": true
    })
}

// ============================================================
// Kickstart
// ============================================================

#[test]
fn kickstart_renders_group_before_user() {
    let snap = snap_with(vec![test_user()], vec![test_group()]);
    let ks = users::render_kickstart(&snap);

    let group_pos = ks.find("group --name=docker").expect("group line missing");
    let user_pos = ks.find("user --name=alice").expect("user line missing");
    assert!(
        group_pos < user_pos,
        "group must come before user in kickstart output"
    );

    // Correct flags
    assert!(ks.contains("--gid=1010"), "group gid flag missing");
    assert!(ks.contains("--uid=1001"), "user uid flag missing");
    assert!(ks.contains("--gid=1001"), "user gid flag missing");
    assert!(
        ks.contains("--groups=wheel,docker"),
        "supplementary groups missing"
    );
    assert!(
        ks.contains("--iscrypted --password="),
        "password flag missing"
    );
    assert!(
        ks.contains("sshkey --username=alice"),
        "sshkey line missing"
    );
}

// ============================================================
// Blueprint TOML
// ============================================================

#[test]
fn toml_renders_group_and_user_blocks() {
    let snap = snap_with(vec![test_user()], vec![test_group()]);
    let toml = users::render_blueprint_toml(&snap);

    assert!(
        toml.contains("[[customizations.group]]"),
        "group block missing"
    );
    assert!(
        toml.contains("[[customizations.user]]"),
        "user block missing"
    );
    assert!(toml.contains("name = \"docker\""), "group name missing");
    assert!(toml.contains("gid = 1010"), "group gid missing");
    assert!(toml.contains("name = \"alice\""), "user name missing");
    assert!(toml.contains("uid = 1001"), "user uid missing");

    // Order: group block before user block
    let g_pos = toml.find("[[customizations.group]]").unwrap();
    let u_pos = toml.find("[[customizations.user]]").unwrap();
    assert!(g_pos < u_pos, "group block must precede user block");
}

#[test]
fn toml_multi_key_uses_first_with_comment() {
    let user = serde_json::json!({
        "name": "bob",
        "uid": 1002,
        "gid": 1002,
        "include": true,
        "containerfile_strategy": "useradd",
        "ssh_keys": ["ssh-rsa KEY1 bob@a", "ssh-rsa KEY2 bob@b", "ssh-ed25519 KEY3 bob@c"]
    });
    let snap = snap_with(vec![user], vec![]);
    let toml = users::render_blueprint_toml(&snap);

    assert!(
        toml.contains("key = \"ssh-rsa KEY1 bob@a\""),
        "first key missing"
    );
    assert!(
        toml.contains("# NOTE: 2 additional SSH key(s)"),
        "multi-key comment missing"
    );
}

// ============================================================
// Containerfile: collector-built fixtures
// ============================================================

const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../testdata/fixtures/users-block"
);

fn fixture(name: &str) -> String {
    std::fs::read_to_string(Path::new(FIXTURES).join(name))
        .unwrap_or_else(|e| panic!("fixture {name}: {e}"))
}

fn pkg_source() -> SourceSystem {
    SourceSystem::PackageBased {
        os_release: OsRelease {
            name: "Red Hat Enterprise Linux".into(),
            version_id: "9.6".into(),
            id: "rhel".into(),
            ..Default::default()
        },
    }
}

fn run(inspector: &dyn Inspector, exec: &MockExecutor) -> InspectorOutput {
    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &inspectah_core::machine_state::ScreenedExecutor::new(exec),
        rpm_state: None,
        baseline_data: None,
    };
    match inspector.inspect(&ctx, &NullProgress) {
        Ok(output) => output,
        Err(InspectorError::Degraded { partial, .. }) => *partial,
        Err(e) => panic!("inspector failed: {e}"),
    }
}

fn collect_users(exec: &MockExecutor, preserve_keys: bool) -> UserGroupSection {
    let inspector = UsersGroupsInspector::with_options(UserGroupOptions {
        preserve_ssh_keys: preserve_keys,
        ..Default::default()
    });
    match run(&inspector, exec).section {
        SectionData::UsersGroups(section) => section,
        other => panic!("expected the users section, got {other:?}"),
    }
}

fn collect_storage(exec: &MockExecutor) -> StorageSection {
    match run(&StorageInspector::new(), exec).section {
        SectionData::Storage(section) => section,
        other => panic!("expected the storage section, got {other:?}"),
    }
}

fn ok(stdout: &str) -> ExecResult {
    ExecResult {
        stdout: stdout.into(),
        exit_code: 0,
        ..Default::default()
    }
}

/// A host whose `/var/lib` walk finds `dirs`, each `(path, stat output)`.
fn with_var_lib(exec: MockExecutor, dirs: &[(&str, &str)]) -> MockExecutor {
    let found: String = dirs.iter().map(|(p, _)| format!("{p}\n")).collect();
    let mut exec = exec
        .with_file("/etc/fstab", "")
        .with_command("findmnt --json", ok(r#"{"filesystems": []}"#))
        .with_command(
            "find /var/lib -mindepth 1 -maxdepth 2 -type d ! -empty",
            ok(&found),
        );
    for (path, stat) in dirs {
        exec = exec.with_command(&format!("stat -c %04a %u %g %U %G {path}"), ok(stat));
    }
    exec
}

/// The § 4.4 host: `alice` (primary `appgrp`, in `wheel`, two keys),
/// `carol` (primary `users`, gid 100), `dave` (in `users`) and `erin` (in
/// `wheel`).
pub(crate) fn spec_host() -> MockExecutor {
    MockExecutor::new()
        .with_file("/etc/passwd", &fixture("passwd"))
        .with_file("/etc/group", &fixture("group"))
        .with_file(
            "/home/alice/.ssh/authorized_keys",
            &fixture("alice-authorized_keys"),
        )
}

/// A snapshot of `users`, with the named users set to useradd, as the
/// refine decision does; every other user stays Skip.
pub(crate) fn useradd(users: UserGroupSection, names: &[&str]) -> InspectionSnapshot {
    let mut users = users;
    for user in &mut users.users {
        if names.iter().any(|n| user["name"] == *n) {
            user["containerfile_strategy"] = serde_json::json!("useradd");
        }
    }
    let mut snap = InspectionSnapshot::new();
    snap.users_groups = Some(users);
    snap
}

fn section(snap: &InspectionSnapshot) -> String {
    users::render_containerfile_users(snap).join("\n")
}

/// The lines a build runs.
fn active(output: &str) -> Vec<&str> {
    output
        .lines()
        .filter(|l| !l.trim_start().starts_with('#') && !l.trim().is_empty())
        .collect()
}

/// U1: a primary group the base lacks, the L5 case. The section is the
/// spec's first example, line for line.
#[test]
fn u1_primary_group_the_base_lacks() {
    let snap = useradd(collect_users(&spec_host(), false), &["alice"]);
    assert_eq!(section(&snap), fixture("alice.containerfile"));
}

/// U2: all four users, the spec's second example: groups deduplicated in
/// first-appearance order, `users` in its long form because `dave` names
/// it in `-G`.
#[test]
fn u2_all_four_users() {
    let snap = useradd(
        collect_users(&spec_host(), false),
        &["alice", "carol", "dave", "erin"],
    );
    assert_eq!(section(&snap), fixture("four-users.containerfile"));
}

/// U3: the two group line forms. Rows: (group, gid, named in `-G`).
#[test]
fn u3_group_line_forms() {
    let exec = MockExecutor::new()
        .with_file(
            "/etc/passwd",
            "ann:x:1050:1050::/home/ann:/bin/bash\n\
             ben:x:1051:1051::/home/ben:/bin/bash\n",
        )
        .with_file(
            "/etc/group",
            "my.grp:x:1050:\nben:x:1051:\ndev.ops:x:1060:ann,ben\n",
        );
    let out = section(&useradd(collect_users(&exec, false), &["ann", "ben"]));
    let lines: Vec<&str> = out.lines().collect();
    let rows = [
        ("my.grp", "my\\.grp", 1050, false),
        ("ben", "ben", 1051, false),
        ("dev.ops", "dev\\.ops", 1060, true),
    ];
    for (group, escaped, gid, supplementary) in rows {
        let pattern = format!("'^{escaped}:[^:]*:{gid}:'");
        let expected = if supplementary {
            format!(
                "RUN grep -q {pattern} /etc/group || getent group {group} | grep {pattern} >> /etc/group || groupadd -g {gid} {group} || \\"
            )
        } else {
            format!(
                "RUN getent group {group} | grep -q {pattern} || groupadd -g {gid} {group} || \\"
            )
        };
        let at = lines
            .iter()
            .position(|l| *l == expected)
            .unwrap_or_else(|| panic!("{group}: no line `{expected}` in:\n{out}"));
        assert_eq!(
            lines[at + 1],
            format!(
                "    {{ echo 'group {group} (gid {gid}): the image already uses the name or the gid differently; see the comment above' >&2; exit 1; }}"
            ),
            "{group}: the message names the group as is"
        );
    }
    // The escaped form is in the patterns and nowhere else.
    assert!(!out.contains("groupadd -g 1050 my\\.grp"), "{out}");
    assert!(
        out.contains("RUN useradd -M -u 1050 -g my.grp -G dev.ops"),
        "{out}"
    );
}

/// U4: nothing under `/home` or `/var` at build time.
#[test]
fn u4_nothing_under_home_or_var_at_build_time() {
    let out = section(&useradd(
        collect_users(&spec_host(), true),
        &["alice", "carol", "dave", "erin"],
    ));
    let lines = active(&out);
    assert!(!out.contains("useradd -m"), "{out}");
    for line in lines.iter().filter(|l| l.contains("useradd")) {
        assert!(line.starts_with("RUN useradd -M "), "{line}");
    }
    for word in ["install ", "mkdir", "chown", "COPY"] {
        assert!(
            !lines.iter().any(|l| l.contains(word)),
            "`{word}` in the users block:\n{out}"
        );
    }
    for line in lines.iter().filter(|l| l.contains("/var")) {
        assert!(
            line.starts_with("    'd /var/home/"),
            "a /var string outside the printf argument: {line}"
        );
    }
}

/// U5: every home gets exactly one of a users line, a `/var` fragment
/// rule, or the "not created" comment, across the whole export.
#[test]
fn u5_homes_across_the_whole_export() {
    let exec = with_var_lib(
        MockExecutor::new()
            .with_file(
                "/etc/passwd",
                "bob:x:1005:1005::/home/staff/bob:/bin/bash\n\
                 vic:x:1006:1006::/var/home/vic:/bin/bash\n\
                 svc:x:1007:1007::/var/lib/svc:/sbin/nologin\n\
                 app:x:1008:1008::/var/opt/app:/sbin/nologin\n\
                 opt:x:1009:1009::/opt/app:/sbin/nologin\n\
                 rooty:x:1010:1010::/:/sbin/nologin\n",
            )
            .with_file(
                "/etc/group",
                "bob:x:1005:\nvic:x:1006:\nsvc:x:1007:\napp:x:1008:\nopt:x:1009:\nrooty:x:1010:\n",
            ),
        &[("/var/lib/svc", "0750 1007 1007 svc svc\n")],
    );
    let mut snap = useradd(
        collect_users(&exec, false),
        &["bob", "vic", "svc", "app", "opt", "rooty"],
    );
    snap.storage = Some(collect_storage(&exec));

    let dir = tempfile::tempdir().expect("tempdir");
    inspectah_pipeline::render::render_all(&snap, &RenderContext { target: None }, dir.path())
        .expect("render");
    let containerfile = std::fs::read_to_string(dir.path().join("Containerfile")).expect("read");
    let var_conf = std::fs::read_to_string(
        dir.path()
            .join("config/usr/lib/tmpfiles.d/inspectah-var.conf"),
    )
    .expect("the /var fragment");

    let users_lines: Vec<&str> = containerfile
        .lines()
        .filter_map(|l| l.trim().strip_prefix('\''))
        .filter_map(|l| l.split('\'').next())
        .filter(|l| l.starts_with("d "))
        .collect();
    assert_eq!(
        users_lines,
        vec![
            "d /var/home/staff/bob 0700 bob bob -",
            "d /var/home/vic 0700 vic vic -",
        ],
        "{containerfile}"
    );
    assert_eq!(
        var_conf
            .lines()
            .filter(|l| l.contains("/var/lib/svc"))
            .collect::<Vec<_>>(),
        vec!["d /var/lib/svc 0750 svc svc -"],
        "{var_conf}"
    );
    for (user, home) in [("app", "/var/opt/app"), ("opt", "/opt/app")] {
        assert!(
            containerfile.contains(&format!(
                "# {user}'s home, {home}, is not created by this image."
            )),
            "{user}: no comment:\n{containerfile}"
        );
        assert!(!var_conf.contains(home), "{user}: a rule in the fragment");
    }
    for user in ["bob", "vic", "svc", "rooty"] {
        assert!(
            !containerfile.contains(&format!("# {user}'s home")),
            "{user}: a comment beside a rule:\n{containerfile}"
        );
    }

    // No /home or /var/home home at all: no printf and no home comment.
    let only_var = useradd(collect_users(&exec, false), &["svc", "app"]);
    let out = section(&only_var);
    assert!(!out.contains("printf"), "{out}");
    assert!(!out.contains("Home directories are created"), "{out}");
}

/// U5b: the `/var` fragment names a materialized user's group only when a
/// group line guarantees it. tmpfiles rejects a line whose group the image
/// lacks, so a home owned by a group no accepted user is in keeps the
/// host's gid. Rows: (`homeowners` member list, expected fragment line).
#[test]
fn u5b_var_home_group_by_name_only_when_guaranteed() {
    let rows = [
        // Nobody accepted is in `homeowners`: no group line creates it.
        ("", "d /var/lib/svc 0750 svc 45001 -"),
        // `svc` is in it, so its group line guarantees the name and gid.
        ("svc", "d /var/lib/svc 0750 svc homeowners -"),
    ];
    for (members, expected) in rows {
        let exec = with_var_lib(
            MockExecutor::new()
                .with_file(
                    "/etc/passwd",
                    "svc:x:1007:1007::/var/lib/svc:/sbin/nologin\n",
                )
                .with_file(
                    "/etc/group",
                    &format!("svc:x:1007:\nhomeowners:x:45001:{members}\n"),
                ),
            &[("/var/lib/svc", "0750 1007 45001 svc homeowners\n")],
        );
        let mut snap = useradd(collect_users(&exec, false), &["svc"]);
        snap.storage = Some(collect_storage(&exec));

        let dir = tempfile::tempdir().expect("tempdir");
        inspectah_pipeline::render::render_all(&snap, &RenderContext { target: None }, dir.path())
            .expect("render");
        let var_conf = std::fs::read_to_string(
            dir.path()
                .join("config/usr/lib/tmpfiles.d/inspectah-var.conf"),
        )
        .expect("the /var fragment");
        assert_eq!(
            active(&var_conf),
            vec![expected],
            "members {members:?}: {var_conf}"
        );
        let containerfile =
            std::fs::read_to_string(dir.path().join("Containerfile")).expect("read");
        assert!(
            !containerfile.contains("# svc's home"),
            "members {members:?}: the fragment creates the home:\n{containerfile}"
        );
    }
}

/// U6: the keys comment.
#[test]
fn u6_keys_comment() {
    let exec = spec_host().with_file(
        "/home/erin/.ssh/authorized_keys",
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIErin erin@host\n",
    );
    for preserve in [false, true] {
        let out = section(&useradd(
            collect_users(&exec, preserve),
            &["alice", "carol", "erin"],
        ));
        assert_eq!(
            out.matches("# SSH keys are not part of this image.")
                .count(),
            1,
            "{out}"
        );
        assert!(
            out.contains("# alice had 2 keys in /home/alice/.ssh/authorized_keys on the host."),
            "{out}"
        );
        assert!(
            out.contains("# erin had 1 key in /home/erin/.ssh/authorized_keys on the host."),
            "{out}"
        );
        assert!(!out.contains("# carol had"), "{out}");
        assert_eq!(
            out.contains("# inspectah-users.ks has the sshkey lines"),
            preserve,
            "preserve = {preserve}: {out}"
        );
    }
    let out = section(&useradd(collect_users(&exec, true), &["carol", "dave"]));
    assert!(!out.contains("SSH keys"), "{out}");
}

/// U7: a refusal costs the whole user, and the `/var` fragment does not
/// name the refused account.
#[test]
fn u7_refusals_and_the_plan() {
    let exec = with_var_lib(
        spec_host()
            .with_file(
                "/etc/passwd",
                &format!(
                    "{}zed:x:1009:1009::/home/zed:/bin/bash\n",
                    fixture("passwd")
                ),
            )
            .with_file(
                "/home/zed/.ssh/authorized_keys",
                "ssh-ed25519 AAAAZed zed@host\n",
            ),
        &[("/var/lib/zed", "0700 1009 1009 zed UNKNOWN\n")],
    );
    let users = collect_users(&exec, true);
    let mut snap = useradd(users.clone(), &["zed", "carol"]);
    snap.users_groups.as_mut().expect("users").users[0]["password_hash"] =
        serde_json::json!("$6$salt$hash");
    snap.storage = Some(collect_storage(&exec));

    let out = section(&snap);
    assert!(
        out.contains(
            "# FIXME: user zed not created: its primary group (gid 1009) has no entry in the host's /etc/group"
        ),
        "{out}"
    );
    assert!(out.contains("RUN useradd -M -u 1002 -g users"), "{out}");
    let zed_lines: Vec<&str> = out
        .lines()
        .filter(|l| l.contains("zed") && !l.starts_with("# FIXME"))
        .collect();
    assert!(
        zed_lines.is_empty(),
        "lines for the refused user: {zed_lines:?}"
    );

    let dir = tempfile::tempdir().expect("tempdir");
    inspectah_pipeline::render::render_all(&snap, &RenderContext { target: None }, dir.path())
        .expect("render");
    let var_conf = std::fs::read_to_string(
        dir.path()
            .join("config/usr/lib/tmpfiles.d/inspectah-var.conf"),
    )
    .expect("the /var fragment");
    assert!(
        var_conf
            .contains("# account 'zed' not guaranteed in image\nd /var/lib/zed 0700 1009 1009 -"),
        "{var_conf}"
    );
    assert!(
        !active(&var_conf).iter().any(|l| l.contains(" zed ")),
        "zed in an owner field: {var_conf}"
    );

    // The kickstart and blueprint fragments do not read the new fields.
    let mut without = snap.clone();
    for user in &mut without.users_groups.as_mut().expect("users").users {
        let user = user.as_object_mut().expect("an object");
        user.remove("primary_group");
        user.remove("group_gids");
    }
    assert_eq!(
        users::render_kickstart(&snap),
        users::render_kickstart(&without)
    );
    assert_eq!(
        users::render_blueprint_toml(&snap),
        users::render_blueprint_toml(&without)
    );
}

/// U8: a group the operator excluded is still created for a user that
/// needs it.
#[test]
fn u8_excluded_custom_group() {
    let mut users = collect_users(&spec_host(), false);
    let appgrp = users
        .groups
        .iter_mut()
        .find(|g| g["name"] == "appgrp")
        .expect("appgrp is a custom group");
    appgrp["include"] = serde_json::json!(false);
    let out = section(&useradd(users, &["alice"]));
    assert!(
        out.contains("RUN getent group appgrp | grep -q '^appgrp:[^:]*:1001:' || groupadd -g 1001 appgrp || \\"),
        "{out}"
    );
}

/// U9: a group the export writes by number elsewhere keeps that number.
#[test]
fn u9_numeric_ownership() {
    let exec = with_var_lib(
        spec_host(),
        &[("/var/lib/shared", "0770 500 1001 legacy appgrp\n")],
    );
    let mut snap = useradd(collect_users(&exec, false), &["alice"]);
    snap.storage = Some(collect_storage(&exec));

    let dir = tempfile::tempdir().expect("tempdir");
    inspectah_pipeline::render::render_all(&snap, &RenderContext { target: None }, dir.path())
        .expect("render");
    let var_conf = std::fs::read_to_string(
        dir.path()
            .join("config/usr/lib/tmpfiles.d/inspectah-var.conf"),
    )
    .expect("the /var fragment");
    assert!(
        var_conf
            .lines()
            .any(|l| l == "d /var/lib/shared 0770 500 1001 -"),
        "{var_conf}"
    );
    let containerfile = std::fs::read_to_string(dir.path().join("Containerfile")).expect("read");
    assert!(
        containerfile.contains(
            "RUN getent group appgrp | grep -q '^appgrp:[^:]*:1001:' || groupadd -g 1001 appgrp || \\"
        ),
        "{containerfile}"
    );
}

/// Two hosts' users sections, merged as `aggregate` merges them.
fn fleet(hosts: &[MockExecutor]) -> UserGroupSection {
    let sections = hosts
        .iter()
        .map(|h| Some(collect_users(h, false)))
        .collect();
    let names: Vec<String> = (0..hosts.len()).map(|i| format!("host{i}")).collect();
    merge_usersgroups_sections(sections, hosts.len(), &names).expect("a merged section")
}

/// U10: in a fleet aggregate, two users can disagree on a group's gid. The
/// first renders; the second is refused.
#[test]
fn u10_conflicting_gids_across_users() {
    let host_a = MockExecutor::new()
        .with_file("/etc/passwd", "amy:x:1001:1001::/home/amy:/bin/bash\n")
        .with_file("/etc/group", "amy:x:1001:\ndevs:x:2000:amy\n");
    let host_b = MockExecutor::new()
        .with_file("/etc/passwd", "bea:x:1002:1002::/home/bea:/bin/bash\n")
        .with_file("/etc/group", "bea:x:1002:\ndevs:x:2001:bea\n");
    let out = section(&useradd(fleet(&[host_a, host_b]), &["amy", "bea"]));
    assert!(
        out.contains("RUN useradd -M -u 1001 -g amy -G devs"),
        "{out}"
    );
    assert!(
        out.contains(
            "# FIXME: user bea not created: group devs has gid 2001 here and 2000 for an earlier user"
        ),
        "{out}"
    );
    assert!(!out.contains("groupadd -g 2001"), "{out}");
}

/// F1: the fleet merge keeps every host's supplementary groups for a
/// user, each with a recorded gid.
#[test]
fn f1_fleet_merge_keeps_every_hosts_groups() {
    let host = |group: &str| {
        MockExecutor::new()
            .with_file("/etc/passwd", "alice:x:1001:1001::/home/alice:/bin/bash\n")
            .with_file("/etc/group", &format!("alice:x:1001:\n{group}:alice\n"))
    };
    let merged = fleet(&[host("wheel:x:10"), host("docker:x:975")]);
    let alice = &merged.users[0];
    assert_eq!(
        alice["supplementary_groups"],
        serde_json::json!(["docker", "wheel"])
    );
    assert_eq!(
        alice["group_gids"],
        serde_json::json!({"alice": 1001, "docker": 975, "wheel": 10})
    );
    let out = section(&useradd(merged, &["alice"]));
    assert!(
        out.contains("RUN useradd -M -u 1001 -g alice -G docker,wheel"),
        "{out}"
    );
}
