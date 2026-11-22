//! Unmanaged payloads through redaction, end to end: a host with an
//! `.env` file and a compose file under `/opt`, scanned by the real
//! collectors, bundled with `--include-unmanaged`'s bundler, rendered,
//! packed, and exported again by refine.

use crate::sysctl_hosts::{
    SysctlHost, bundled_export, export_session, reload, scan_exec, scan_exec_raw,
};
use inspectah_collect::executor::mock::MockExecutor;
use inspectah_collect::inspectors::config::ConfigInspector;
use inspectah_collect::inspectors::containers::ContainersInspector;
use inspectah_collect::inspectors::nonrpm::NonRpmInspector;
use inspectah_collect::inspectors::rpm::RpmInspector;
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::traits::inspector::Inspector;
use inspectah_core::types::nonrpm::{PayloadRedaction, UncheckedPayload};
use inspectah_refine::session::RefineSession;

/// The token and the password the planted files hold.
const API_TOKEN: &str = "DRIFTIFY_FAKE_TOKEN_12345";
const MONGO_PASSWORD: &str = "m0ng0pass";
const PG_PASSWORD: &str = "DRIFTIFY_FAKE_pgpass123";

const ENV_PATH: &str = "/opt/myapp/.env";
const COMPOSE_PATH: &str = "/opt/myapp/docker-compose.yml";
const PLAIN_PATH: &str = "/opt/myapp/settings.ini";

fn env_file() -> String {
    format!(
        "API_TOKEN={API_TOKEN}\nMONGODB_URL=mongodb://admin:{MONGO_PASSWORD}@mongo.internal:27017/admin\n"
    )
}

fn compose_file() -> String {
    format!(
        "version: \"3.8\"\n\
         services:\n  \
           db:\n    \
             image: docker.io/library/postgres:16\n    \
             environment:\n      \
               POSTGRES_PASSWORD: {PG_PASSWORD}\n"
    )
}

/// A file with no credential: it ships byte for byte.
const PLAIN: &str = "[server]\nport = 8080\nworkers = 4\n";

/// The three payloads, in the mock host and on disk under the host root
/// the bundler copies from.
fn host() -> (MockExecutor, tempfile::TempDir) {
    host_of(&[
        (ENV_PATH, env_file().into_bytes()),
        (COMPOSE_PATH, compose_file().into_bytes()),
        (PLAIN_PATH, PLAIN.as_bytes().to_vec()),
    ])
}

/// `files` in the mock host and on disk under the host root the bundler
/// copies from.
fn host_of(files: &[(&str, Vec<u8>)]) -> (MockExecutor, tempfile::TempDir) {
    let root = tempfile::tempdir().expect("a host root");
    let mut host = SysctlHost::new();
    for (path, content) in files {
        host = host.file_bytes(path, content);
        let on_disk = root.path().join(path.trim_start_matches('/'));
        std::fs::create_dir_all(on_disk.parent().unwrap()).unwrap();
        std::fs::write(on_disk, content).unwrap();
    }
    (host.build(), root)
}

/// The scan with `--include-unmanaged`: the collectors that read these
/// files, redaction (none when `raw`, as `--no-redaction` runs), then
/// the unmanaged walk through the read gate.
fn scan(exec: &MockExecutor, raw: bool) -> InspectionSnapshot {
    let inspectors: Vec<Box<dyn Inspector>> = vec![
        Box::new(RpmInspector::new()),
        Box::new(ConfigInspector::new()),
        Box::new(NonRpmInspector::with_roots(vec!["/opt".into()])),
        Box::new(ContainersInspector::new()),
    ];
    let mut snap = if raw {
        scan_exec_raw(exec, inspectors)
    } else {
        scan_exec(exec, inspectors)
    };
    let gate = inspectah_core::machine_state::ScreenedExecutor::new(exec);
    snap.unmanaged_files = Some(inspectah_collect::inspectors::nonrpm::scan_unmanaged_files(
        &gate,
        &[],
        &[],
    ));
    reload(&snap)
}

/// What one scan produced: the snapshot as the bundler left it, the scan
/// export, and a refine session opened on the packed scan tarball, which
/// `_packed` keeps on disk for the session's export to read.
struct Exported {
    snap: InspectionSnapshot,
    scan_tree: tempfile::TempDir,
    session: RefineSession,
    _packed: tempfile::TempDir,
}

fn export(exec: &MockExecutor, root: &std::path::Path, raw: bool) -> Exported {
    let mut snap = scan(exec, raw);
    let scan_tree = bundled_export(&mut snap, exec, root);
    let packed = tempfile::tempdir().unwrap();
    let tarball = packed.path().join("scan.tar.gz");
    inspectah_pipeline::render::tarball::create_tarball(scan_tree.path(), &tarball, "host")
        .expect("the scan tarball packs");
    let mut session = inspectah_refine::tarball::from_tarball(&tarball).expect("refine opens it");
    session.set_tarball_path(tarball);
    Exported {
        snap,
        scan_tree,
        session,
        _packed: packed,
    }
}

/// Every file under `dir`, with its bytes.
fn files(dir: &std::path::Path) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    let mut dirs = vec![dir.to_path_buf()];
    while let Some(d) = dirs.pop() {
        for entry in std::fs::read_dir(&d).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path);
            } else if let Ok(bytes) = std::fs::read(&path) {
                out.push((path.display().to_string(), bytes));
            }
        }
    }
    out
}

fn contains(bytes: &[u8], needle: &str) -> bool {
    bytes.windows(needle.len()).any(|w| w == needle.as_bytes())
}

/// The release check's S1: no planted secret is in either export or the
/// snapshot; the bundled `.env` is the redacted copy `env-files/` holds;
/// the compose file ships redacted; `secrets-review.md` names each
/// shipped copy and does not call either unmatched; refine asks for the
/// acknowledgment. The control file ships byte for byte.
#[test]
fn unmanaged_credentials_ship_redacted_on_both_export_paths() {
    let (exec, root) = host();
    let Exported {
        snap,
        scan_tree,
        session,
        _packed,
    } = export(&exec, root.path(), false);

    let items = &snap.unmanaged_files.as_ref().expect("a section").items;
    let state = |path: &str| {
        items
            .iter()
            .find(|f| f.path == path)
            .unwrap_or_else(|| panic!("{path} is an item"))
            .payload_redaction
    };
    assert_eq!(state(ENV_PATH), PayloadRedaction::Redacted);
    assert_eq!(state(COMPOSE_PATH), PayloadRedaction::Redacted);
    assert_eq!(state(PLAIN_PATH), PayloadRedaction::Unchanged);

    let refine_tree = export_session(&session);
    let mut all = vec![(
        "snapshot".to_string(),
        serde_json::to_vec(&snap).expect("the snapshot serializes"),
    )];
    all.extend(files(scan_tree.path()));
    all.extend(files(refine_tree.path()));
    for secret in [API_TOKEN, MONGO_PASSWORD, PG_PASSWORD] {
        for (where_, bytes) in &all {
            assert!(!contains(bytes, secret), "{where_} holds {secret}");
        }
    }

    for (via, tree) in [("scan", scan_tree.path()), ("refine", refine_tree.path())] {
        crate::copy_source_exists_test::assert_every_copy_source_exists(via, tree);
        let shipped = |path: &str| {
            std::fs::read_to_string(tree.join("unmanaged").join(&path[1..]))
                .unwrap_or_else(|e| panic!("{via}: {path} ships: {e}"))
        };
        let env = shipped(ENV_PATH);
        assert!(env.contains("REDACTED_"), "{via}: {env}");
        assert_eq!(
            env,
            std::fs::read_to_string(tree.join("env-files/opt/myapp/.env")).unwrap(),
            "{via}: one redacted text for the .env file"
        );
        let compose = shipped(COMPOSE_PATH);
        assert!(compose.contains("POSTGRES_PASSWORD"), "{via}: {compose}");
        assert!(compose.contains("REDACTED"), "{via}: {compose}");
        assert_eq!(
            shipped(PLAIN_PATH),
            PLAIN,
            "{via}: the control ships as read"
        );

        let review = std::fs::read_to_string(tree.join("secrets-review.md")).unwrap();
        for path in [ENV_PATH, COMPOSE_PATH] {
            let section = review
                .split("\n## ")
                .find(|s| s.starts_with(&format!("`{path}`")))
                .unwrap_or_else(|| panic!("{via}: a section for {path}:\n{review}"));
            assert!(
                section.contains(&format!("`unmanaged{path}`")),
                "{via}: names the shipped copy:\n{section}"
            );
            assert!(section.contains("with placeholders in place"), "{section}");
            assert!(!section.contains("Not matched"), "{via}:\n{section}");
        }
        let cf = std::fs::read_to_string(tree.join("Containerfile")).unwrap();
        assert!(cf.contains("REDACTED_* placeholders"), "{via}:\n{cf}");
    }

    assert!(session.is_sensitive(), "refine asks for the acknowledgment");
    let view = inspectah_web::adapter::build_web_view(&session);
    assert!(serde_json::to_value(&view).unwrap()["session_is_sensitive"] == true);
}

/// `--no-redaction` keeps the raw bundle by design: every payload ships
/// byte for byte, nothing is marked redacted, and the session is
/// sensitive because the snapshot is.
#[test]
fn no_redaction_ships_unmanaged_payloads_raw() {
    let (exec, root) = host();
    let Exported {
        snap,
        scan_tree,
        session,
        _packed,
    } = export(&exec, root.path(), true);
    for (path, content) in [
        (ENV_PATH, env_file()),
        (COMPOSE_PATH, compose_file()),
        (PLAIN_PATH, PLAIN.to_string()),
    ] {
        let shipped =
            std::fs::read_to_string(scan_tree.path().join("unmanaged").join(&path[1..])).unwrap();
        assert_eq!(shipped, content, "{path}");
    }
    assert!(
        snap.unmanaged_files
            .as_ref()
            .unwrap()
            .items
            .iter()
            .all(|f| f.payload_redaction == PayloadRedaction::Unchanged)
    );
    assert!(snap.sensitive_snapshot);
    assert!(session.is_sensitive());
}

/// The token every reproducer holds, and a key body split into groups of
/// four by spaces, which the key screening passes and from which removing
/// the spaces recovers the key.
const TOKEN: &str = "supersecretvalue123";
const KEY_BODY: &str =
    "MC4C AQAw BQYD K2Vw BCIE IBQh 0WfR 6aXE BHdg fX4i x5qD YmYR BZGZ EmNF dzJ1 Owlp";
/// Two runs of `KEY_BODY` with the spaces taken out.
const KEY_RUNS: [&str; 2] = [
    "MC4CAQAwBQYDK2VwBCIEIBQh0WfR6aXE",
    "BHdgfX4ix5qDYmYRBZGZEmNFdzJ1Owlp",
];

const LATIN1_ENV: &str = "/opt/legacy/.env";
const KEYED_ENV: &str = "/opt/keyed/.env";
const TEMPLATE: &str = "/opt/tmpl/template.json";
const HUGE_LOG: &str = "/opt/logs/huge.log";
const CA_BUNDLE: &str = "/opt/trust/ca-bundle.pem";

/// A CA bundle of two certificates.
const CA_BUNDLE_TEXT: &str = "-----BEGIN CERTIFICATE-----\n\
    MIIFazCCA1OgAwIBAgIRAIIQz7DSQONZRGPgu2OCiwAwDQYJKoZIhvcNAQELBQAw\n\
    TzELMAkGA1UEBhMCVVMxKTAnBgNVBAoTIEludGVybmV0IFNlY3VyaXR5IFJlc2Vh\n\
    cmNoIEdyb3VwMRUwEwYDVQQDEwxJU1JHIFJvb3QgWDEwHhcNMTUwNjA0MTEwNDM4\n\
    -----END CERTIFICATE-----\n\
    -----BEGIN CERTIFICATE-----\n\
    MIICGzCCAaGgAwIBAgIQQdKd0XLq7qeAwSxs6S+HUjAKBggqhkjOPQQDAzBPMQsw\n\
    CQYDVQQGEwJVUzEpMCcGA1UEChMgSW50ZXJuZXQgU2VjdXJpdHkgUmVzZWFyY2gg\n\
    -----END CERTIFICATE-----\n";

/// The size of the oversized text file: over the 8 MiB the bundler reads
/// whole to redact.
const HUGE_LEN: usize = 8 * 1024 * 1024 + 1;

/// The reviewer's reproducers: a Latin-1 `.env` with a token; an `.env`
/// holding a space-split private key and a token; a JSON template with a
/// token between private-key markers and between certificate markers; an
/// oversized log with a password; and a real CA bundle.
fn reproducers() -> Vec<(&'static str, Vec<u8>)> {
    let mut latin1 = b"# caf\xe9\nAPI_TOKEN=".to_vec();
    latin1.extend_from_slice(TOKEN.as_bytes());
    latin1.push(b'\n');
    let keyed = format!(
        "TLS_KEY=\"-----BEGIN PRIVATE KEY----- {KEY_BODY} -----END PRIVATE KEY-----\"\n\
         API_TOKEN={TOKEN}\n"
    );
    let template = format!(
        "{{\n  \"key\": {{\n    \"begin\": \"-----BEGIN PRIVATE KEY-----\",\n    \
         \"api_token\": \"{TOKEN}\",\n    \"end\": \"-----END PRIVATE KEY-----\"\n  }},\n  \
         \"cert\": {{\n    \"begin\": \"-----BEGIN CERTIFICATE-----\",\n    \
         \"password\": \"{TOKEN}\",\n    \"end\": \"-----END CERTIFICATE-----\"\n  }}\n}}\n"
    );
    let mut huge = format!("password={TOKEN}\n").into_bytes();
    huge.resize(HUGE_LEN, b'x');
    vec![
        (LATIN1_ENV, latin1),
        (KEYED_ENV, keyed.into_bytes()),
        (TEMPLATE, template.into_bytes()),
        (HUGE_LOG, huge),
        (CA_BUNDLE, CA_BUNDLE_TEXT.as_bytes().to_vec()),
    ]
}

/// No reproducer ships a credential on either export path: the Latin-1
/// `.env` and the oversized log are withheld, each with a refine row
/// saying why, an excluded `secrets-review.md` entry, no `COPY` and no
/// bundled copy; the space-split key is gone from `unmanaged/`,
/// `env-files/` and the snapshot; the template's tokens are replaced with
/// its markers kept; the CA bundle ships byte for byte. Refine asks for
/// the acknowledgment.
#[test]
fn unmanaged_reproducers_never_ship_a_credential() {
    let (exec, root) = host_of(&reproducers());
    let Exported {
        snap,
        scan_tree,
        session,
        _packed,
    } = export(&exec, root.path(), false);

    let items = &snap.unmanaged_files.as_ref().expect("a section").items;
    let state = |path: &str| {
        items
            .iter()
            .find(|f| f.path == path)
            .unwrap_or_else(|| panic!("{path} is an item"))
            .payload_redaction
    };
    assert_eq!(
        state(LATIN1_ENV),
        PayloadRedaction::Withheld(UncheckedPayload::NotUtf8)
    );
    assert_eq!(
        state(HUGE_LOG),
        PayloadRedaction::Withheld(UncheckedPayload::TooLarge)
    );
    assert_eq!(state(KEYED_ENV), PayloadRedaction::Redacted);
    assert_eq!(state(TEMPLATE), PayloadRedaction::Redacted);
    assert_eq!(state(CA_BUNDLE), PayloadRedaction::Unchanged);

    let refine_tree = export_session(&session);
    let mut all = vec![(
        "snapshot".to_string(),
        serde_json::to_vec(&snap).expect("the snapshot serializes"),
    )];
    all.extend(files(scan_tree.path()));
    all.extend(files(refine_tree.path()));
    for (where_, bytes) in &all {
        let text = String::from_utf8_lossy(bytes);
        assert!(!text.contains(TOKEN), "{where_} holds the token");
        for run in KEY_RUNS {
            let packed: String = text.chars().filter(|c| !c.is_whitespace()).collect();
            assert!(!packed.contains(run), "{where_} holds key material");
        }
    }

    for (via, tree) in [("scan", scan_tree.path()), ("refine", refine_tree.path())] {
        crate::copy_source_exists_test::assert_every_copy_source_exists(via, tree);
        let bundled = |path: &str| tree.join("unmanaged").join(&path[1..]);
        let cf = std::fs::read_to_string(tree.join("Containerfile")).unwrap();
        let review = std::fs::read_to_string(tree.join("secrets-review.md")).unwrap();
        for path in [LATIN1_ENV, HUGE_LOG] {
            assert!(!bundled(path).exists(), "{via}: {path} is bundled");
            assert!(
                !cf.lines()
                    .any(|l| !l.starts_with('#') && l.contains(&path[1..])),
                "{via}: {path} is copied:\n{cf}"
            );
            let section = review
                .split("\n## ")
                .find(|s| s.starts_with(&format!("`{path}`")))
                .unwrap_or_else(|| panic!("{via}: a section for {path}:\n{review}"));
            assert!(
                section.contains("Left out of this export"),
                "{via}:\n{section}"
            );
            assert!(section.contains("whole file left out"), "{via}:\n{section}");
        }
        let keyed = std::fs::read_to_string(bundled(KEYED_ENV)).unwrap();
        assert!(keyed.contains("REDACTED_"), "{via}: {keyed}");
        assert_eq!(
            keyed,
            std::fs::read_to_string(tree.join("env-files/opt/keyed/.env")).unwrap(),
            "{via}: one redacted text for the .env file"
        );
        let template = std::fs::read_to_string(bundled(TEMPLATE)).unwrap();
        for marker in [
            "-----BEGIN PRIVATE KEY-----",
            "-----END PRIVATE KEY-----",
            "-----BEGIN CERTIFICATE-----",
            "-----END CERTIFICATE-----",
        ] {
            assert!(template.contains(marker), "{via}: {template}");
        }
        assert_eq!(
            std::fs::read_to_string(bundled(CA_BUNDLE)).unwrap(),
            CA_BUNDLE_TEXT,
            "{via}: the CA bundle ships as read"
        );
    }

    assert!(session.is_sensitive(), "refine asks for the acknowledgment");
    let view = serde_json::to_value(inspectah_web::adapter::build_web_view(&session)).unwrap();
    let row = |path: &str| {
        view["unmanaged_files"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|g| g["items"].as_array().unwrap())
            .find(|i| i["path"] == path)
            .unwrap_or_else(|| panic!("a refine row for {path}"))
            .clone()
    };
    for path in [LATIN1_ENV, HUGE_LOG] {
        let withheld = &row(path)["withheld"];
        assert_eq!(withheld["why"], "UncheckedPayload", "{path}: {withheld}");
        assert!(
            withheld["text"]
                .as_str()
                .unwrap()
                .contains("could not check this unmanaged file for credentials"),
            "{path}: {withheld}"
        );
    }
    assert!(row(CA_BUNDLE).get("withheld").is_none());
}

/// `--no-redaction` ships the reproducers as the host had them, the
/// oversized log and the Latin-1 file included, and records nothing
/// withheld.
#[test]
fn no_redaction_ships_the_reproducers_raw() {
    let files = reproducers();
    let (exec, root) = host_of(&files);
    let Exported {
        snap, scan_tree, ..
    } = export(&exec, root.path(), true);
    for (path, content) in &files {
        let shipped = std::fs::read(scan_tree.path().join("unmanaged").join(&path[1..])).unwrap();
        assert!(shipped == *content, "{path}");
    }
    assert!(
        snap.unmanaged_files
            .as_ref()
            .unwrap()
            .items
            .iter()
            .all(|f| f.payload_redaction == PayloadRedaction::Unchanged)
    );
}
