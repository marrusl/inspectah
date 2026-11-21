//! SELinux custom modules, end to end through the scan path.
//!
//! The host is mocked files in the real policy store layout
//! (`/var/lib/selinux/<policy>/active`), run through the real SELinux
//! inspector by the scan command's own calls, then through both export
//! paths: the scan tarball's `render_all` and a refine session's export.
//! Nothing here builds a `SelinuxSection` by hand, so a collector that
//! reads the wrong store fails these tests instead of passing over a
//! fixture that shares its assumption.

use crate::sysctl_hosts::{
    SysctlHost, export_session, reload, render_export, scan_selinux, session,
};
use inspectah_core::snapshot::InspectionSnapshot;

/// The active policy store on a package-mode RHEL 9 or 10 host.
const STORE: &str = "/var/lib/selinux/targeted/active";

/// An enforcing targeted host whose store holds a base module (100), a
/// packaged one (200), two local ones (400) and the module selinux-policy
/// generates at 400. Module directories hold the files libsemanage writes.
fn selinux_host() -> SysctlHost {
    let mut host = SysctlHost::new()
        .file("/sys/fs/selinux/enforce", "1\n")
        .file(
            "/etc/selinux/config",
            "SELINUX=enforcing\nSELINUXTYPE=targeted\n",
        );
    for (priority, module) in [
        ("100", "apache"),
        ("200", "container"),
        ("400", "myapp"),
        ("400", "permissive_rhcd_t"),
        ("400", "extra_varrun"),
    ] {
        let dir = format!("{STORE}/modules/{priority}/{module}");
        host = host
            .file(&format!("{dir}/cil"), "")
            .file(&format!("{dir}/lang_ext"), "pp");
    }
    host.file(&format!("{STORE}/modules/disabled/.keep"), "")
}

/// The Security & Access Control block of an export's Containerfile.
fn selinux_block(tree: &std::path::Path) -> Vec<String> {
    let containerfile = std::fs::read_to_string(tree.join("Containerfile"))
        .expect("the export has a Containerfile");
    let lines: Vec<&str> = containerfile.lines().collect();
    let start = lines
        .iter()
        .position(|l| l.contains("Security & Access Control"))
        .unwrap_or_else(|| panic!("no SELinux block in:\n{containerfile}"));
    lines[start..]
        .iter()
        .skip(1)
        .take_while(|l| !l.starts_with("# ==="))
        .map(|l| (*l).to_string())
        .collect()
}

fn scanned() -> InspectionSnapshot {
    reload(&scan_selinux(selinux_host()))
}

/// The collector lists the local modules from the store, and only those.
#[test]
fn custom_modules_are_collected_from_the_policy_store() {
    let snap = scanned();
    let section = snap
        .selinux
        .as_ref()
        .expect("the SELinux section is present");
    assert_eq!(section.custom_modules, ["myapp", "permissive_rhcd_t"]);
}

/// Both export paths name every module the image does not carry, with
/// the commands that carry it, and render the same block.
#[test]
fn both_exports_name_the_modules_they_cannot_carry() {
    let snap = scanned();
    let scan_tree = render_export(&snap);
    let refine_tree = export_session(&session(&snap));

    let scan_block = selinux_block(scan_tree.path());
    assert!(
        scan_block.contains(
            &"# FIXME: 2 custom SELinux policy module(s) not carried: myapp, permissive_rhcd_t"
                .to_string()
        ),
        "{scan_block:#?}"
    );
    assert!(
        scan_block
            .iter()
            .any(|l| l == "# RUN semodule -i /tmp/selinux/*.cil && rm -rf /tmp/selinux/"),
        "{scan_block:#?}"
    );
    assert_eq!(selinux_block(refine_tree.path()), scan_block);
}
