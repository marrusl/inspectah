//! Snapshot values never add an instruction, directive, key or command.
//!
//! The comment guard in `render::comment::join_lines` keeps a value inside
//! a comment line. It never touches an instruction line, so a value that
//! reaches one is refused or proved before it gets there. Each row below
//! poisons one field that reaches an instruction or directive line,
//! renders the whole file through the real render path for every
//! separator in the row's set, and checks the output split on every line
//! terminator: no line that is not a comment carries the payload, and the
//! row reached its site (a FIXME names the refused value or, for a field
//! the FIXME does not quote, a FIXME is present). Each group also has a
//! test that a benign value still renders its line. One field per row, because a fixture
//! that poisons two fields only reaches the first one the renderer
//! refuses.

use std::collections::HashMap;

use inspectah_core::baseline::{ResolutionStrategy, TargetImageIdentity};
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::FindingKind;
use inspectah_core::types::config::SYNTHESIZED_SYSCTL_FILE;
use inspectah_core::types::config::{ConfigFileEntry, ConfigSection};
use inspectah_core::types::containers::{ContainerSection, FlatpakApp};
use inspectah_core::types::group_render::{GroupRenderState, RenderContext};
use inspectah_core::types::kernelboot::{KernelBootSection, SysctlOverride};
use inspectah_core::types::network::{NMConnection, NetworkSection};
use inspectah_core::types::nonrpm::{FileType, UnmanagedFile, UnmanagedFileSection};
use inspectah_core::types::rpm::{InstalledGroup, PackageEntry, PackageState, RpmSection};
use inspectah_core::types::users::UserGroupSection;
use inspectah_pipeline::render::comment::LINE_TERMINATORS;
use inspectah_pipeline::render::configtree::write_config_tree;
use inspectah_pipeline::render::containerfile::render_containerfile;
use inspectah_pipeline::render::kickstart::render_kickstart;
use inspectah_pipeline::render::users;

/// What the hostile value tries to place on a line of its own.
const PAYLOAD: &str = "RUN echo pwned";

/// Every line terminator alone, and CRLF.
fn terminators() -> Vec<String> {
    LINE_TERMINATORS
        .iter()
        .map(char::to_string)
        .chain(std::iter::once("\r\n".to_string()))
        .collect()
}

/// The terminators plus what breaks a word or a quote, or starts a
/// substitution or a systemd specifier, on a single line.
fn word_breakers() -> Vec<String> {
    terminators()
        .into_iter()
        .chain(
            [" ", "'", "\"", "$(", "`", "%", "\\"]
                .iter()
                .map(|s| s.to_string()),
        )
        .collect()
}

/// The terminators plus what ends or substitutes inside a double-quoted
/// shell word. A space is left out: it is legal inside the quotes.
fn double_quote_breakers() -> Vec<String> {
    terminators()
        .into_iter()
        .chain(["\"", "$(", "`", "\\"].iter().map(|s| s.to_string()))
        .collect()
}

/// A value that would put the payload on its own line, or in its own
/// word, if the separator got through.
fn hostile(sep: &str) -> String {
    format!("x{sep}{PAYLOAD}")
}

/// How a row proves it reached its site rather than passing vacuously.
enum Reached {
    /// A comment line starting with this text carries the payload.
    Fixme(&'static str),
    /// A comment line starting with this text is present. For a field the
    /// FIXME does not quote (it names the user, not the refused home).
    Refused(&'static str),
}

struct Row {
    field: &'static str,
    render: fn(&str) -> String,
    reached: Reached,
}

fn is_comment(line: &str) -> bool {
    line.trim_start().starts_with('#')
}

/// Why `output` fails the row, or `None` when the value stayed out of
/// every instruction line.
fn failure(row: &Row, output: &str) -> Option<String> {
    let lines: Vec<&str> = output.split(LINE_TERMINATORS).collect();
    for line in &lines {
        if line.contains(PAYLOAD) && !is_comment(line) {
            return Some(format!("the value reached a non-comment line: {line:?}"));
        }
    }
    if let Reached::Fixme(prefix) = row.reached
        && !lines
            .iter()
            .any(|l| l.trim_start().starts_with(prefix) && l.contains(PAYLOAD))
    {
        return Some(format!("no `{prefix}` line names the refused value"));
    }
    if let Reached::Refused(prefix) = row.reached
        && !lines.iter().any(|l| l.trim_start().starts_with(prefix))
    {
        return Some(format!("no `{prefix}` line"));
    }
    None
}

fn check_rows(rows: &[Row], seps: &[String]) {
    let mut failures = Vec::new();
    for row in rows {
        for sep in seps {
            let output = (row.render)(&hostile(sep));
            if let Some(why) = failure(row, &output) {
                failures.push(format!("{} ({sep:?}): {why}\n{output}", row.field));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

// ---------------------------------------------------------------------------
// Unmanaged files and configuration roots (Containerfile COPY and RUN ln)
// ---------------------------------------------------------------------------

fn unmanaged(file: UnmanagedFile) -> UnmanagedFile {
    UnmanagedFile {
        disposition: FindingKind::included(),
        ..file
    }
}

fn with_unmanaged(items: Vec<UnmanagedFile>) -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.unmanaged_files = Some(UnmanagedFileSection {
        items: items.into_iter().map(unmanaged).collect(),
        ..Default::default()
    });
    snap
}

fn regular(path: String) -> UnmanagedFile {
    UnmanagedFile {
        path,
        file_type: FileType::ElfBinary,
        ..Default::default()
    }
}

fn symlink(path: String, target: String) -> UnmanagedFile {
    UnmanagedFile {
        path,
        file_type: FileType::Symlink,
        link_target: target,
        ..Default::default()
    }
}

fn with_config_file(path: &str) -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.config = Some(ConfigSection {
        files: vec![ConfigFileEntry {
            path: path.into(),
            content: "x=1\n".into(),
            disposition: FindingKind::included(),
            ..Default::default()
        }],
    });
    snap
}

/// The Containerfile from the roots `write_config_tree` materialized, as
/// the export renders it.
fn materialized_containerfile(snap: &InspectionSnapshot) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let roots = write_config_tree(snap, dir.path()).expect("write config tree");
    render_containerfile(snap, Some(&roots), None)
}

const COPY_ROWS: &[Row] = &[
    Row {
        field: "unmanaged single file path",
        render: |v| {
            render_containerfile(
                &with_unmanaged(vec![regular(format!("/opt/{v}"))]),
                None,
                None,
            )
        },
        reached: Reached::Fixme("# FIXME: unmanaged file"),
    },
    Row {
        field: "unmanaged directory of several files",
        render: |v| {
            render_containerfile(
                &with_unmanaged(vec![
                    regular(format!("/opt/{v}/a")),
                    regular(format!("/opt/{v}/b")),
                ]),
                None,
                None,
            )
        },
        reached: Reached::Fixme("# FIXME: unmanaged files under"),
    },
    Row {
        field: "unmanaged symlink path",
        render: |v| {
            render_containerfile(
                &with_unmanaged(vec![symlink(format!("/opt/{v}"), "/usr/bin/true".into())]),
                None,
                None,
            )
        },
        reached: Reached::Fixme("# FIXME: unmanaged symlink"),
    },
    Row {
        field: "unmanaged symlink target",
        render: |v| {
            render_containerfile(
                &with_unmanaged(vec![symlink("/opt/link".into(), v.to_string())]),
                None,
                None,
            )
        },
        reached: Reached::Fixme("# FIXME: unmanaged symlink"),
    },
    Row {
        field: "config file top-level directory (snapshot roots)",
        render: |v| render_containerfile(&with_config_file(&format!("/{v}/f.conf")), None, None),
        reached: Reached::Fixme("# FIXME: config"),
    },
    Row {
        field: "config file top-level directory (materialized roots)",
        render: |v| materialized_containerfile(&with_config_file(&format!("/{v}/f.conf"))),
        reached: Reached::Fixme("# FIXME: config"),
    },
];

#[test]
fn copy_lines_refuse_every_hostile_path() {
    check_rows(COPY_ROWS, &terminators());
}

#[test]
fn copy_lines_render_benign_paths() {
    let snap = with_unmanaged(vec![
        regular("/opt/app/bin/one".into()),
        symlink("/opt/app/current".into(), "../app-1.2".into()),
    ]);
    let out = render_containerfile(&snap, None, None);
    assert!(
        out.contains("COPY unmanaged/opt/app/bin/one /opt/app/bin/one"),
        "{out}"
    );
    assert!(
        out.contains("RUN ln -sf '../app-1.2' '/opt/app/current'"),
        "{out}"
    );
    let out = materialized_containerfile(&with_config_file("/etc/app/app.conf"));
    assert!(out.contains("COPY config/etc/ /etc/"), "{out}");
}

/// A `COPY` source is a wildcard pattern to the builder, so a path holding
/// `*`, `?` or `[` would copy every file it matched. No separator row
/// covers this: the hostile values above all hold a space too.
#[test]
fn copy_lines_refuse_wildcard_paths() {
    let mut failures = Vec::new();
    for segment in ["app*", "app?", "[ab]"] {
        // The config `COPY` names only the top-level directory, so the
        // wildcard goes there; the unmanaged one names the whole path.
        let cases = [
            (
                format!("/opt/{segment}"),
                render_containerfile(
                    &with_unmanaged(vec![regular(format!("/opt/{segment}"))]),
                    None,
                    None,
                ),
            ),
            (
                format!("/{segment}"),
                render_containerfile(&with_config_file(&format!("/{segment}/f.conf")), None, None),
            ),
            (
                format!("/{segment}"),
                materialized_containerfile(&with_config_file(&format!("/{segment}/f.conf"))),
            ),
        ];
        for (path, out) in cases {
            let path = path.as_str();
            let named = out
                .lines()
                .any(|l| l.starts_with("COPY") && l.contains(path));
            let fixme = out
                .lines()
                .any(|l| l.starts_with("# FIXME") && l.contains(path));
            if named || !fixme {
                failures.push(format!("{path}: named={named} fixme={fixme}\n{out}"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

// ---------------------------------------------------------------------------
// Flatpak provisioning unit (systemd ExecStartPre= / ExecStart=, root at boot)
// ---------------------------------------------------------------------------

fn flatpak_app() -> FlatpakApp {
    FlatpakApp {
        app_id: "org.example.App".into(),
        branch: "stable".into(),
        remote: "flathub".into(),
        remote_url: "https://dl.flathub.org/repo/".into(),
        disposition: FindingKind::included(),
        ..Default::default()
    }
}

fn flatpak_unit(app: FlatpakApp) -> String {
    let mut snap = InspectionSnapshot::new();
    snap.containers = Some(ContainerSection {
        flatpak_apps: vec![app],
        ..Default::default()
    });
    let dir = tempfile::tempdir().expect("tempdir");
    write_config_tree(&snap, dir.path()).expect("write config tree");
    std::fs::read_to_string(dir.path().join("flatpak/flatpak-provision.service"))
        .expect("unit written")
}

const FLATPAK_ROWS: &[Row] = &[
    Row {
        field: "flatpak remote name",
        render: |v| {
            flatpak_unit(FlatpakApp {
                remote: v.into(),
                ..flatpak_app()
            })
        },
        reached: Reached::Fixme("# FIXME: flatpak"),
    },
    Row {
        field: "flatpak remote URL",
        render: |v| {
            flatpak_unit(FlatpakApp {
                remote_url: format!("https://example.com/{v}"),
                ..flatpak_app()
            })
        },
        reached: Reached::Fixme("# FIXME: flatpak remote"),
    },
    Row {
        field: "flatpak app id",
        render: |v| {
            flatpak_unit(FlatpakApp {
                app_id: v.into(),
                ..flatpak_app()
            })
        },
        reached: Reached::Fixme("# FIXME: flatpak app"),
    },
    Row {
        field: "flatpak branch",
        render: |v| {
            flatpak_unit(FlatpakApp {
                branch: v.into(),
                ..flatpak_app()
            })
        },
        reached: Reached::Fixme("# FIXME: flatpak app"),
    },
];

#[test]
fn flatpak_unit_refuses_every_hostile_value() {
    check_rows(FLATPAK_ROWS, &word_breakers());
}

#[test]
fn flatpak_unit_renders_benign_values() {
    let unit = flatpak_unit(flatpak_app());
    assert!(
        unit.contains(
            "ExecStartPre=/usr/bin/flatpak remote-add --if-not-exists flathub https://dl.flathub.org/repo/\n"
        ),
        "{unit}"
    );
    assert!(
        unit.contains("ExecStart=/usr/bin/flatpak install -y --noninteractive flathub org.example.App//stable\n"),
        "{unit}"
    );
}

// ---------------------------------------------------------------------------
// Containerfile FROM and `dnf group install`
// ---------------------------------------------------------------------------

fn with_target_image(image_ref: &str) -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.target_image = Some(TargetImageIdentity {
        image_ref: image_ref.into(),
        strategy: ResolutionStrategy::CliOverride,
    });
    snap
}

/// A renderable package group named `name`, rendered through the real
/// Containerfile path with a render context that marks it renderable.
fn group_containerfile(name: &str) -> String {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "vim".into(),
            version: "1.0".into(),
            release: "1.el9".into(),
            arch: "x86_64".into(),
            state: PackageState::Added,
            disposition: FindingKind::included(),
            source_repo: "appstream".into(),
            ..Default::default()
        }],
        installed_groups: Some(vec![InstalledGroup {
            name: name.into(),
            members: vec!["vim".into()],
            optional_installed: Vec::new(),
        }]),
        ..Default::default()
    });
    let ctx = RenderContext {
        group_states: HashMap::from([(name.to_string(), GroupRenderState::Renderable)]),
        ..Default::default()
    };
    render_containerfile(&snap, None, Some(&ctx))
}

const FROM_ROWS: &[Row] = &[Row {
    field: "target image reference",
    render: |v| render_containerfile(&with_target_image(&format!("quay.io/{v}")), None, None),
    reached: Reached::Fixme("# FIXME: target image"),
}];

const GROUP_ROWS: &[Row] = &[Row {
    field: "package group name",
    render: group_containerfile,
    reached: Reached::Fixme("# FIXME: package group"),
}];

#[test]
fn from_line_refuses_every_hostile_image_reference() {
    check_rows(FROM_ROWS, &word_breakers());
}

#[test]
fn group_install_refuses_every_hostile_group_name() {
    check_rows(GROUP_ROWS, &double_quote_breakers());
}

#[test]
fn from_and_group_lines_render_benign_values() {
    let out = render_containerfile(
        &with_target_image("quay.io/centos-bootc/centos-bootc:stream10"),
        None,
        None,
    );
    assert!(
        out.contains("FROM quay.io/centos-bootc/centos-bootc:stream10"),
        "{out}"
    );
    let out = group_containerfile("Development Tools");
    assert!(out.contains("    \"Development Tools\" \\"), "{out}");
    // A refused group's member is installed on its own instead.
    let out = group_containerfile("x\nRUN y");
    assert!(out.lines().any(|l| l.trim() == "vim \\"), "{out}");
}

// ---------------------------------------------------------------------------
// Kickstart suggestion (anaconda directives and the %post shell)
// ---------------------------------------------------------------------------

fn kickstart_with(network: NetworkSection) -> String {
    let mut snap = InspectionSnapshot::new();
    snap.network = Some(network);
    render_kickstart(&snap)
}

fn dhcp_connection(name: &str) -> String {
    kickstart_with(NetworkSection {
        connections: vec![NMConnection {
            name: name.into(),
            method: "auto".into(),
            ..Default::default()
        }],
        ..Default::default()
    })
}

fn hosts_addition(line: &str) -> String {
    kickstart_with(NetworkSection {
        hosts_additions: vec![line.into()],
        ..Default::default()
    })
}

const KICKSTART_DEVICE_ROWS: &[Row] = &[Row {
    field: "DHCP connection name in network --device=",
    render: dhcp_connection,
    reached: Reached::Fixme("# FIXME: network"),
}];

const KICKSTART_HOSTS_ROWS: &[Row] = &[Row {
    field: "/etc/hosts addition in %post",
    render: hosts_addition,
    reached: Reached::Fixme("# FIXME: /etc/hosts"),
}];

#[test]
fn kickstart_network_refuses_every_hostile_device_name() {
    check_rows(KICKSTART_DEVICE_ROWS, &word_breakers());
}

#[test]
fn kickstart_hosts_refuses_every_line_break() {
    check_rows(KICKSTART_HOSTS_ROWS, &terminators());
}

#[test]
fn kickstart_renders_benign_network_values() {
    let out = dhcp_connection("eth0");
    assert!(
        out.contains("network --bootproto=dhcp --device=eth0 --activate"),
        "{out}"
    );
}

/// A hosts line keeps every character but a line break, so it is quoted
/// rather than refused. Run the rendered `printf` through `sh` and check
/// it prints the value back unchanged: nothing in it was substituted or
/// split, and no command ran.
#[test]
fn kickstart_hosts_line_reaches_the_shell_as_one_literal() {
    for sep in [" ", "\t", "'", "\"", "$(", "`", "\\", "%", ";", "&&", "#"] {
        let value = format!("10.0.0.1 db{sep}$(echo pwned) `echo pwned`");
        let out = hosts_addition(&value);
        let line = out
            .lines()
            .find(|l| l.starts_with("printf "))
            .unwrap_or_else(|| panic!("{sep:?}: no printf line\n{out}"));
        let command = line
            .strip_suffix(" >> /etc/hosts")
            .unwrap_or_else(|| panic!("{sep:?}: unexpected line {line:?}"));
        let printed = std::process::Command::new("sh")
            .arg("-c")
            .arg(command)
            .output()
            .expect("run sh");
        assert_eq!(
            String::from_utf8_lossy(&printed.stdout),
            format!("{value}\n"),
            "{sep:?}: {line}"
        );
    }
}

// ---------------------------------------------------------------------------
// Users kickstart (anaconda directives) and blueprint TOML
// ---------------------------------------------------------------------------

fn base_user() -> serde_json::Value {
    serde_json::json!({
        "name": "alice",
        "uid": 1001,
        "gid": 1001,
        "home": "/home/alice",
        "shell": "/bin/bash",
        "include": true,
        "supplementary_groups": ["wheel"],
        "password_hash": "$6$salt$hash",
        "ssh_keys": ["ssh-ed25519 AAAAC3Nz alice@host"]
    })
}

fn base_group() -> serde_json::Value {
    serde_json::json!({"name": "devs", "gid": 1010, "source": "custom", "include": true})
}

fn users_snap(user: serde_json::Value, group: serde_json::Value) -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.users_groups = Some(UserGroupSection {
        users: vec![user],
        groups: vec![group],
        ..Default::default()
    });
    snap
}

/// `base_user()` with `field` set to `value`.
fn user_with(field: &str, value: serde_json::Value) -> serde_json::Value {
    let mut u = base_user();
    u[field] = value;
    u
}

fn ks_user(field: &str, value: serde_json::Value) -> String {
    users::render_kickstart(&users_snap(user_with(field, value), base_group()))
}

const USERS_KS_ROWS: &[Row] = &[
    Row {
        field: "kickstart group name",
        render: |v| {
            let mut g = base_group();
            g["name"] = v.into();
            users::render_kickstart(&users_snap(base_user(), g))
        },
        reached: Reached::Fixme("# FIXME: group"),
    },
    Row {
        field: "kickstart user name",
        render: |v| ks_user("name", v.into()),
        reached: Reached::Fixme("# FIXME: user"),
    },
    Row {
        field: "kickstart supplementary group",
        render: |v| ks_user("supplementary_groups", serde_json::json!([v])),
        reached: Reached::Refused("# FIXME: user"),
    },
    Row {
        field: "kickstart home",
        render: |v| ks_user("home", format!("/home/{v}").into()),
        reached: Reached::Refused("# FIXME: user"),
    },
    Row {
        field: "kickstart shell",
        render: |v| ks_user("shell", format!("/bin/{v}").into()),
        reached: Reached::Refused("# FIXME: user"),
    },
    Row {
        field: "kickstart password hash",
        render: |v| ks_user("password_hash", format!("$6$s${v}").into()),
        reached: Reached::Refused("# FIXME: password"),
    },
];

/// An SSH key holds spaces legitimately, so its row is driven by what
/// ends or acts inside the double quotes it is written in.
const USERS_KS_KEY_ROWS: &[Row] = &[Row {
    field: "kickstart sshkey",
    render: |v| {
        ks_user(
            "ssh_keys",
            serde_json::json!([format!("ssh-ed25519 AAAA {v}")]),
        )
    },
    reached: Reached::Fixme("# FIXME: SSH key"),
}];

#[test]
fn users_kickstart_refuses_every_hostile_value() {
    check_rows(USERS_KS_ROWS, &word_breakers());
    check_rows(USERS_KS_KEY_ROWS, &double_quote_breakers());
}

#[test]
fn users_kickstart_renders_benign_values() {
    let out = users::render_kickstart(&users_snap(base_user(), base_group()));
    for line in [
        "group --name=devs --gid=1010",
        "user --name=alice --uid=1001 --gid=1001 --shell=/bin/bash --homedir=/home/alice --groups=wheel --iscrypted --password=$6$salt$hash",
        "sshkey --username=alice \"ssh-ed25519 AAAAC3Nz alice@host\"",
    ] {
        assert!(out.lines().any(|l| l == line), "missing {line:?}\n{out}");
    }
}

/// Builds the JSON value a user field holds from a hostile string.
type FieldValue = fn(&str) -> serde_json::Value;

/// Every string field of the blueprint, poisoned one at a time with every
/// terminator, CRLF, a quote and a backslash. The fragment must parse,
/// hold exactly the tables and keys a benign user and group produce, and
/// carry the hostile value back unchanged in its own field.
#[test]
fn users_blueprint_values_never_add_a_key() {
    let mut seps = terminators();
    seps.extend(
        ["\"", "\\", "'", "\"\"\"", "=", "[x]"]
            .iter()
            .map(|s| s.to_string()),
    );
    let user_fields: &[(&str, FieldValue)] = &[
        ("name", |v| v.into()),
        ("home", |v| v.into()),
        ("shell", |v| v.into()),
        ("password_hash", |v| v.into()),
        ("ssh_keys", |v| serde_json::json!([v])),
        ("supplementary_groups", |v| serde_json::json!([v])),
    ];
    let user_keys = [
        "gid", "groups", "home", "key", "name", "password", "shell", "uid",
    ];
    let mut failures = Vec::new();
    for sep in &seps {
        let value = format!("x{sep}injected = true");
        let mut cases: Vec<(String, InspectionSnapshot)> = user_fields
            .iter()
            .map(|(f, make)| {
                (
                    f.to_string(),
                    users_snap(user_with(f, make(&value)), base_group()),
                )
            })
            .collect();
        let mut g = base_group();
        g["name"] = value.clone().into();
        cases.push(("group name".into(), users_snap(base_user(), g)));

        for (field, snap) in cases {
            let out = users::render_blueprint_toml(&snap);
            let doc: toml::Table = match out.parse() {
                Ok(d) => d,
                Err(e) => {
                    failures.push(format!("{field} ({sep:?}): not TOML: {e}\n{out}"));
                    continue;
                }
            };
            let custom = doc["customizations"].as_table().expect("customizations");
            let keys: Vec<String> = doc.keys().cloned().collect();
            let user = custom["user"].as_array().expect("user")[0]
                .as_table()
                .expect("t");
            let group = custom["group"].as_array().expect("group")[0]
                .as_table()
                .expect("t");
            let mut ukeys: Vec<&str> = user.keys().map(String::as_str).collect();
            ukeys.sort();
            let mut gkeys: Vec<&str> = group.keys().map(String::as_str).collect();
            gkeys.sort();
            let mut ckeys: Vec<&str> = custom.keys().map(String::as_str).collect();
            ckeys.sort();
            if keys != ["customizations"]
                || ckeys != ["group", "user"]
                || ukeys != user_keys
                || gkeys != ["gid", "name"]
            {
                failures.push(format!("{field} ({sep:?}): keys changed\n{out}"));
                continue;
            }
            let carried = match field.as_str() {
                "name" => user["name"].as_str(),
                "home" => user["home"].as_str(),
                "shell" => user["shell"].as_str(),
                "password_hash" => user["password"].as_str(),
                "ssh_keys" => user["key"].as_str(),
                "supplementary_groups" => user["groups"][0].as_str(),
                _ => group["name"].as_str(),
            };
            if carried != Some(value.as_str()) {
                failures.push(format!(
                    "{field} ({sep:?}): value not carried: {carried:?}\n{out}"
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

// ---------------------------------------------------------------------------
// Synthesized sysctl drop-in (`key = value`, applied at boot)
// ---------------------------------------------------------------------------

fn sysctl_dropin(key: &str, runtime: &str) -> String {
    let mut snap = InspectionSnapshot::new();
    snap.kernel_boot = Some(KernelBootSection {
        sysctl_overrides: vec![SysctlOverride {
            key: key.into(),
            runtime: runtime.into(),
            disposition: FindingKind::included(),
            ..Default::default()
        }],
        ..Default::default()
    });
    let dir = tempfile::tempdir().expect("tempdir");
    write_config_tree(&snap, dir.path()).expect("write config tree");
    std::fs::read_to_string(
        dir.path()
            .join("sysctl/etc/sysctl.d")
            .join(SYNTHESIZED_SYSCTL_FILE),
    )
    .expect("drop-in written")
}

const SYSCTL_KEY_ROWS: &[Row] = &[Row {
    field: "sysctl key",
    render: |v| sysctl_dropin(&format!("net.ipv4.{v}"), "1"),
    reached: Reached::Fixme("# FIXME: sysctl"),
}];

/// The FIXME names the key, not the refused value.
const SYSCTL_VALUE_ROWS: &[Row] = &[Row {
    field: "sysctl value",
    render: |v| sysctl_dropin("net.ipv4.ip_forward", v),
    reached: Reached::Refused("# FIXME: sysctl"),
}];

#[test]
fn sysctl_dropin_refuses_every_hostile_key_and_value() {
    let mut key_seps = terminators();
    key_seps.extend([" ", "=", "#", ";"].iter().map(|s| s.to_string()));
    check_rows(SYSCTL_KEY_ROWS, &key_seps);
    check_rows(SYSCTL_VALUE_ROWS, &terminators());
}

#[test]
fn sysctl_dropin_renders_benign_values() {
    let out = sysctl_dropin("net.ipv4.ip_local_port_range", "32768\t60999");
    assert!(
        out.lines()
            .any(|l| l == "net.ipv4.ip_local_port_range = 32768\t60999"),
        "{out:?}"
    );
    let out = sysctl_dropin("net/ipv4/conf/eth0.100/rp_filter", "2");
    assert!(
        out.lines()
            .any(|l| l == "net/ipv4/conf/eth0.100/rp_filter = 2"),
        "{out:?}"
    );
}
