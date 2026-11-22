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

/// Parameter names the kargs file must never carry, however they are
/// quoted on the source host.
const FILTERED_NAMES: &[&str] = &["root", "rd.lvm.lv", "rd.luks.uuid", "console"];

/// The words of the password on the quoted hosts' command lines, each of
/// which must be absent on its own, not only the whole value. Spelled to
/// appear nowhere else: the report's stylesheet holds `alpha`.
const PASSWORD_WORDS: &[&str] = &["alpha7k", "bravo7k"];

/// Hosts whose kernel command line quotes its arguments both ways the
/// kernel reads quotes: around the whole argument and around the value.
/// Each carries only `nosmt=force` and owes `rd.iscsi.password=<value>`.
const QUOTED_CMDLINES: &[(&str, &str)] = &[
    (
        "whole-argument quotes",
        "BOOT_IMAGE=/vmlinuz ro \"root=UUID=old-host\" \"rd.lvm.lv=rhel/root\" \
         \"rd.luks.uuid=luks-old\" \"console=ttyS0,115200\" nosmt=force \
         \"rd.iscsi.password=alpha7k bravo7k\"\n",
    ),
    (
        "value quotes",
        "BOOT_IMAGE=/vmlinuz ro root=\"UUID=old-host\" rd.lvm.lv=\"rhel/root\" \
         rd.luks.uuid=\"luks-old\" console=\"ttyS0,115200\" nosmt=force \
         rd.iscsi.password=\"alpha7k bravo7k\"\n",
    ),
];

/// One quoted host's export tree: the kargs file carries no filtered
/// name, and no word of the password reaches any file in it.
fn assert_quoted_export(label: &str, tree: &tempfile::TempDir) {
    let kargs = read(tree, KARGS_FILE);
    assert_eq!(
        parse_kargs(&kargs),
        vec!["nosmt=force"],
        "{label}:\n{kargs}"
    );
    for name in FILTERED_NAMES {
        assert!(!kargs.contains(name), "{label}: {name} in\n{kargs}");
    }
    let containerfile = read(tree, "Containerfile");
    assert!(
        containerfile.contains(&format!("#   bootc install to-disk --karg {OWED}\n")),
        "{label}:\n{containerfile}"
    );
    let audit = read(tree, "audit-report.md");
    assert!(
        [
            "rd.iscsi.password=REDACTED_PASSWORD_1\"",
            "rd.iscsi.password=\"REDACTED_PASSWORD_1\""
        ]
        .iter()
        .any(|whole| audit.contains(whole)),
        "{label}: the audit's command line shows the whole value redacted:\n{audit}"
    );
    for entry in walk(tree.path()) {
        let Ok(text) = std::fs::read_to_string(&entry) else {
            continue;
        };
        for word in PASSWORD_WORDS {
            assert!(
                !text.contains(word),
                "{label}: {} holds {word:?}",
                entry.display()
            );
        }
    }
}

/// A quoted boot loader argument stays out of the kargs file, and a
/// quoted password with a space is redacted whole: neither word of it
/// reaches the snapshot, the notices or any file of either export.
#[test]
fn quoted_kernel_arguments_are_read_as_the_kernel_reads_them() {
    for (label, cmdline) in QUOTED_CMDLINES {
        let snap = reload(&scan(SysctlHost::new().file("/proc/cmdline", cmdline)));
        let json = serde_json::to_string(&snap).expect("the snapshot serializes");
        let notices: Vec<String> = inspectah_pipeline::render::machine_state::export_notices(&snap)
            .iter()
            .map(|n| n.text().to_owned())
            .collect();
        for word in PASSWORD_WORDS {
            assert!(!json.contains(word), "{label}: the snapshot holds {word:?}");
            assert!(
                notices.iter().all(|n| !n.contains(word)),
                "{label}: {notices:?}"
            );
        }
        assert_eq!(notices.len(), 1, "{label}: {notices:?}");

        let from_scan = render_export(&snap);
        assert_quoted_export(&format!("{label}, scan export"), &from_scan);
        let from_refine = export_session(&session(&snap));
        assert_quoted_export(&format!("{label}, refine export"), &from_refine);
    }
}

/// The password word on the exempt-word hosts' command lines.
const EXEMPT_WORD_SECRET: &str = "bravo7k";

/// The NSS keyword each exempt-word password's value starts with, as the
/// value of the password argument in every spelling a file can hold it:
/// raw, quoted, and quoted inside JSON.
const EXEMPT_WORD_VALUES: &[&str] = &["password=files", "password=\"files", "password=\\\"files"];

/// One host whose password value starts with a word the NSS and PAM
/// exemptions skip, or whose command line starts with the `#` a config
/// file comments with. Neither means anything to the kernel.
struct ExemptWordHost {
    label: &'static str,
    cmdline: &'static str,
    /// The kernel arguments file's whole array.
    kargs: &'static [&'static str],
    /// How the audit report's command line shows the redacted argument.
    audit_shows: &'static str,
}

const EXEMPT_WORD_HOSTS: &[ExemptWordHost] = &[
    ExemptWordHost {
        label: "whole-argument quotes, NSS first word",
        cmdline: "ro \"rd.iscsi.password=files bravo7k\" nosmt=force\n",
        kargs: &["nosmt=force"],
        audit_shows: "\"rd.iscsi.password=REDACTED_PASSWORD_1\"",
    },
    ExemptWordHost {
        label: "value quotes, NSS first word",
        cmdline: "ro rd.iscsi.password=\"files bravo7k\" nosmt=force\n",
        kargs: &["nosmt=force"],
        audit_shows: "rd.iscsi.password=\"REDACTED_PASSWORD_1\"",
    },
    ExemptWordHost {
        label: "NSS word as the whole value",
        cmdline: "ro rd.iscsi.password=files nosmt=force\n",
        kargs: &["nosmt=force"],
        audit_shows: "rd.iscsi.password=REDACTED_PASSWORD_1 ",
    },
    ExemptWordHost {
        label: "leading #",
        cmdline: "# ro rd.iscsi.password=bravo7k nosmt=force\n",
        kargs: &["#", "nosmt=force"],
        audit_shows: "rd.iscsi.password=REDACTED_PASSWORD_1 ",
    },
];

/// Panics if `text` holds the exempt-word password or its value.
fn assert_no_exempt_word_secret(label: &str, what: &str, text: &str) {
    assert!(
        !text.contains(EXEMPT_WORD_SECRET),
        "{label}: {what} holds {EXEMPT_WORD_SECRET:?}"
    );
    for value in EXEMPT_WORD_VALUES {
        assert!(!text.contains(value), "{label}: {what} holds {value:?}");
    }
}

/// One exempt-word host's export tree.
fn assert_exempt_word_export(host: &ExemptWordHost, label: &str, tree: &tempfile::TempDir) {
    let kargs = read(tree, KARGS_FILE);
    assert_eq!(parse_kargs(&kargs), host.kargs, "{label}:\n{kargs}");
    assert!(!kargs.contains("iscsi"), "{label}: {kargs}");
    let containerfile = read(tree, "Containerfile");
    assert!(
        containerfile.contains(&format!("#   bootc install to-disk --karg {OWED}\n")),
        "{label}:\n{containerfile}"
    );
    let audit = read(tree, "audit-report.md");
    assert!(
        audit.contains(host.audit_shows),
        "{label}: the audit's command line shows the whole value redacted:\n{audit}"
    );
    for entry in walk(tree.path()) {
        let Ok(text) = std::fs::read_to_string(&entry) else {
            continue;
        };
        assert_no_exempt_word_secret(label, &entry.display().to_string(), &text);
    }
}

/// The exemptions the redaction patterns make for nsswitch and PAM files
/// (a value that is an NSS or PAM keyword, a line starting with `#`) do
/// not apply to the kernel command line: each password here is redacted
/// whole, left out of the kernel arguments file and owed, and neither it
/// nor its first word as the value reaches the snapshot, the notices or
/// any file of either export.
#[test]
fn nss_pam_and_comment_exemptions_do_not_apply_to_kernel_arguments() {
    for host in EXEMPT_WORD_HOSTS {
        let label = host.label;
        let snap = reload(&scan(SysctlHost::new().file("/proc/cmdline", host.cmdline)));
        let json = serde_json::to_string(&snap).expect("the snapshot serializes");
        assert_no_exempt_word_secret(label, "the snapshot", &json);
        let notices: Vec<String> = inspectah_pipeline::render::machine_state::export_notices(&snap)
            .iter()
            .map(|n| n.text().to_owned())
            .collect();
        for notice in &notices {
            assert_no_exempt_word_secret(label, "a notice", notice);
        }
        assert!(
            notices.contains(
                &"1 kernel argument left out: the value was redacted. At install time: bootc \
                  install to-disk --karg."
                    .to_owned()
            ),
            "{label}: {notices:?}"
        );

        let from_scan = render_export(&snap);
        assert_exempt_word_export(host, &format!("{label}, scan export"), &from_scan);
        let from_refine = export_session(&session(&snap));
        assert_exempt_word_export(host, &format!("{label}, refine export"), &from_refine);
    }
}
