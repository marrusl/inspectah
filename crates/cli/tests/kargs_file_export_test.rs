//! The kernel arguments file, from a collector-produced snapshot on both
//! export paths: one top-level `kargs` array bootc can parse, with an
//! argument whose value redaction replaced left out of it and named as an
//! install-time step on the surfaces that name provisioning steps.

use crate::sysctl_hosts::{SysctlHost, export_session, reload, render_export, scan, session};

/// Where the export stages the kernel arguments file.
const KARGS_FILE: &str = "config/usr/lib/bootc/kargs.d/inspectah-migrated.toml";
/// The secret on the host's kernel command line.
const CANARY: &str = "canary-iscsi-pw";
/// How every surface names the owed argument.
const OWED: &str = "rd.iscsi.password=<value>";

/// A host booted with operator arguments (one quoted with a space), one
/// whose value is a secret, and the boot loader's own.
fn kargs_host() -> SysctlHost {
    SysctlHost::new().file(
        "/proc/cmdline",
        "BOOT_IMAGE=/vmlinuz root=/dev/mapper/rhel-root ro quiet nosmt=force \
         dyndbg=\"file x.c +p\" rd.iscsi.password=canary-iscsi-pw\n",
    )
}

fn read(tree: &tempfile::TempDir, name: &str) -> String {
    std::fs::read_to_string(tree.path().join(name))
        .unwrap_or_else(|e| panic!("the export's {name} reads: {e}"))
}

/// The `kargs` array of a kargs.d file, checked the way bootc reads it:
/// a TOML table whose only keys are `kargs` and `match-architectures`.
fn parse_kargs(file: &str) -> Vec<String> {
    let table: toml::Table =
        toml::from_str(file).unwrap_or_else(|e| panic!("not valid TOML: {e}\n{file}"));
    for key in table.keys() {
        assert!(
            key == "kargs" || key == "match-architectures",
            "bootc refuses the key {key:?}:\n{file}"
        );
    }
    table
        .get("kargs")
        .and_then(toml::Value::as_array)
        .unwrap_or_else(|| panic!("no top-level kargs array:\n{file}"))
        .iter()
        .map(|v| {
            v.as_str()
                .unwrap_or_else(|| panic!("a non-string karg:\n{file}"))
                .to_owned()
        })
        .collect()
}

/// Every check one export tree must pass.
fn assert_export(label: &str, tree: &tempfile::TempDir) {
    let kargs = read(tree, KARGS_FILE);
    assert_eq!(
        parse_kargs(&kargs),
        vec!["nosmt=force", "dyndbg=\"file x.c +p\""],
        "{label}"
    );
    assert!(!kargs.contains("REDACTED"), "{label}: {kargs}");
    assert!(!kargs.contains("iscsi"), "{label}: {kargs}");

    let containerfile = read(tree, "Containerfile");
    assert!(
        containerfile.contains(&format!("#   bootc install to-disk --karg {OWED}")),
        "{label}:\n{containerfile}"
    );
    // The refine export writes no README.md.
    for name in ["audit-report.md", "README.md"] {
        if !tree.path().join(name).exists() && label == "refine export" {
            continue;
        }
        let md = read(tree, name);
        assert!(md.contains(&format!("- `{OWED}`")), "{label} {name}:\n{md}");
    }
    let review = read(tree, "secrets-review.md");
    assert!(
        review.contains("A kernel argument whose value was redacted is left out of the image's"),
        "{label}:\n{review}"
    );

    for entry in walk(tree.path()) {
        let Ok(text) = std::fs::read_to_string(&entry) else {
            continue;
        };
        assert!(
            !text.contains(CANARY),
            "{label}: {} holds the secret",
            entry.display()
        );
    }
}

/// Every file under `root`.
fn walk(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut dirs = vec![root.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir).expect("the export tree reads") {
            let path = entry.expect("a directory entry").path();
            if path.is_dir() {
                dirs.push(path);
            } else {
                out.push(path);
            }
        }
    }
    out
}

/// The scan tarball and the refine export write the same valid kargs
/// file, leave the redacted argument out of it, and name it as owed.
#[test]
fn both_export_paths_write_bootc_kargs_and_name_the_redacted_one() {
    let snap = reload(&scan(kargs_host()));
    let from_scan = render_export(&snap);
    assert_export("scan export", &from_scan);
    let from_refine = export_session(&session(&snap));
    assert_export("refine export", &from_refine);
    assert_eq!(read(&from_scan, KARGS_FILE), read(&from_refine, KARGS_FILE));

    let notices: Vec<String> = inspectah_pipeline::render::machine_state::export_notices(&snap)
        .iter()
        .map(|n| n.text().to_owned())
        .collect();
    assert!(
        notices.contains(
            &"1 kernel argument left out: the value was redacted. At install time: bootc \
              install to-disk --karg."
                .to_owned()
        ),
        "{notices:?}"
    );
}
