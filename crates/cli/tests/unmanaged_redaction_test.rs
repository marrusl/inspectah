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

const NUL_ENV: &str = "/opt/nul/.env";
const NUL_LOG: &str = "/opt/logs/nul.log";
const PROXY_ENV: &str = "/opt/proxy/.env";
const PROXY_CONF: &str = "/opt/proxy/dnf.conf";
const ELF_BIN: &str = "/opt/tools/bin/tool";
const GZIP_DATA: &str = "/opt/tools/share/data.gz";

/// The start of an x86-64 ELF executable: its 64-byte header and its
/// first instructions.
const ELF: &[u8] = b"\x7fELF\x02\x01\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00\
    \x02\x00\x3e\x00\x01\x00\x00\x00\x40\x10\x40\x00\x00\x00\x00\x00\
    \x40\x00\x00\x00\x00\x00\x00\x00\x98\x3a\x00\x00\x00\x00\x00\x00\
    \x00\x00\x00\x00\x40\x00\x38\x00\x0d\x00\x40\x00\x1f\x00\x1e\x00\
    \xf3\x0f\x1e\xfa\x31\xed\x49\x89\xd1";

/// `hello` compressed by gzip.
const GZIP: &[u8] = b"\x1f\x8b\x08\x00\x00\x00\x00\x00\x00\x03\xcb\x48\xcd\xc9\xc9\x07\x00\
    \x86\xa6\x10\x36\x05\x00\x00\x00";

/// The round-two reproducers: a NUL byte in a valid UTF-8 `.env` and in
/// an oversized log, and a proxy password opening a private key whose
/// body is split by spaces, in an `.env` the snapshot also holds and in
/// a file only the bundle holds. Controls: a real ELF and gzip header.
fn nul_and_proxy_reproducers() -> Vec<(&'static str, Vec<u8>)> {
    let nul_env = format!("#\x00\nAPI_TOKEN={TOKEN}\n");
    let mut nul_log = format!("password={TOKEN}\n\x00").into_bytes();
    nul_log.resize(HUGE_LEN, b'x');
    let proxy = format!(
        "proxy_password=-----BEGIN PRIVATE KEY-----\n{KEY_BODY}\n-----END PRIVATE KEY-----\n"
    );
    vec![
        (NUL_ENV, nul_env.into_bytes()),
        (NUL_LOG, nul_log),
        (PROXY_ENV, proxy.clone().into_bytes()),
        (PROXY_CONF, proxy.into_bytes()),
        (ELF_BIN, ELF.to_vec()),
        (GZIP_DATA, GZIP.to_vec()),
    ]
}

/// A NUL byte does not decide "binary", and a masker never hides a key
/// from the pattern pass: on both export paths no token or key material
/// is in `unmanaged/`, `env-files/` or the snapshot. The NUL `.env` and
/// the proxy `.env` ship the snapshot's own redacted copy, the oversized
/// log is withheld, and the ELF and gzip files ship byte for byte.
#[test]
fn nul_bytes_and_masked_key_markers_never_ship_a_credential() {
    let (exec, root) = host_of(&nul_and_proxy_reproducers());
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
    for path in [NUL_ENV, PROXY_ENV, PROXY_CONF] {
        assert_eq!(state(path), PayloadRedaction::Redacted, "{path}");
    }
    assert_eq!(
        state(NUL_LOG),
        PayloadRedaction::Withheld(UncheckedPayload::TooLarge)
    );
    for path in [ELF_BIN, GZIP_DATA] {
        assert_eq!(state(path), PayloadRedaction::Unchanged, "{path}");
    }

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
        let packed: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        for run in KEY_RUNS {
            assert!(!packed.contains(run), "{where_} holds key material");
        }
    }

    for (via, tree) in [("scan", scan_tree.path()), ("refine", refine_tree.path())] {
        crate::copy_source_exists_test::assert_every_copy_source_exists(via, tree);
        let bundled = |path: &str| tree.join("unmanaged").join(&path[1..]);
        for path in [NUL_ENV, PROXY_ENV] {
            assert_eq!(
                std::fs::read(bundled(path)).unwrap(),
                std::fs::read(tree.join("env-files").join(&path[1..])).unwrap(),
                "{via}: {path} ships the snapshot's copy"
            );
        }
        assert!(!bundled(NUL_LOG).exists(), "{via}: the NUL log is bundled");
        assert_eq!(std::fs::read(bundled(ELF_BIN)).unwrap(), ELF, "{via}");
        assert_eq!(std::fs::read(bundled(GZIP_DATA)).unwrap(), GZIP, "{via}");
    }
    assert!(session.is_sensitive(), "refine asks for the acknowledgment");
}

const URL_ENV: &str = "/opt/svc/.env";
const PEM_COMPOSE: &str = "/opt/pem/docker-compose.yml";
const PLAIN_ENV: &str = "/opt/plain/.env";
const PLAIN_COMPOSE: &str = "/opt/plain/docker-compose.yml";

/// The URL password the `.env` reproducer holds under a neutral name.
const URL_PASSWORD: &str = "orchid92";

/// The round-three reproducers, both in files the snapshot also holds: an
/// `.env` URL password under a name no hint reads, and a compose private
/// key whose `BEGIN` marker sits on a line the compose pass rewrites,
/// with its body split by spaces. Controls: an `.env` and a compose file
/// with no credential.
fn original_text_reproducers() -> Vec<(&'static str, Vec<u8>)> {
    let url_env = format!("SERVICE_URL=https://deploy:{URL_PASSWORD}@api.example.test/v1\n");
    let pem_compose = format!(
        "services:\n  app:\n    image: docker.io/library/nginx:1.27\n    environment:\n      \
         PRIVATE_KEY: -----BEGIN PRIVATE KEY-----\n        {KEY_BODY}\n        \
         -----END PRIVATE KEY-----\n"
    );
    vec![
        (URL_ENV, url_env.into_bytes()),
        (PEM_COMPOSE, pem_compose.into_bytes()),
        (PLAIN_ENV, b"PORT=8080\nLOG_LEVEL=info\n".to_vec()),
        (
            PLAIN_COMPOSE,
            b"services:\n  web:\n    image: docker.io/library/nginx:1.27\n    environment:\n      TZ: UTC\n"
                .to_vec(),
        ),
    ]
}

/// The snapshot pass matches on the original text, so neither reproducer
/// ships its credential on either export path: not in `unmanaged/`,
/// `env-files/`, the snapshot JSON (compose `raw_content` included) or
/// any `COPY` source. The controls ship byte for byte everywhere.
#[test]
fn snapshot_twins_match_secrets_on_the_original_text() {
    let reproducers = original_text_reproducers();
    let (exec, root) = host_of(&reproducers);
    let Exported {
        snap,
        scan_tree,
        session,
        _packed,
    } = export(&exec, root.path(), false);

    let original = |path: &str| {
        reproducers
            .iter()
            .find(|(p, _)| *p == path)
            .map(|(_, bytes)| String::from_utf8(bytes.clone()).unwrap())
            .unwrap()
    };
    let compose_copy = |path: &str| {
        snap.containers
            .iter()
            .flat_map(|c| &c.compose_files)
            .find(|c| c.path.trim_start_matches('/') == &path[1..])
            .and_then(|c| c.raw_content.clone())
            .unwrap_or_else(|| panic!("the snapshot holds {path}"))
    };
    let env_copy = |path: &str| {
        snap.non_rpm_software
            .iter()
            .flat_map(|n| &n.env_files)
            .find(|e| e.path.trim_start_matches('/') == &path[1..])
            .map(|e| e.content.clone())
            .unwrap_or_else(|| panic!("the snapshot holds {path}"))
    };
    assert_eq!(env_copy(PLAIN_ENV), original(PLAIN_ENV));
    assert_eq!(compose_copy(PLAIN_COMPOSE), original(PLAIN_COMPOSE));
    assert!(compose_copy(PEM_COMPOSE).contains("PRIVATE_KEY: "));

    let items = &snap.unmanaged_files.as_ref().expect("a section").items;
    let state = |path: &str| {
        items
            .iter()
            .find(|f| f.path == path)
            .unwrap_or_else(|| panic!("{path} is an item"))
            .payload_redaction
    };
    assert_eq!(state(URL_ENV), PayloadRedaction::Redacted);
    assert_eq!(state(PEM_COMPOSE), PayloadRedaction::Redacted);
    assert_eq!(state(PLAIN_ENV), PayloadRedaction::Unchanged);
    assert_eq!(state(PLAIN_COMPOSE), PayloadRedaction::Unchanged);

    let refine_tree = export_session(&session);
    let mut all = vec![(
        "snapshot".to_string(),
        serde_json::to_vec(&snap).expect("the snapshot serializes"),
    )];
    all.extend(files(scan_tree.path()));
    all.extend(files(refine_tree.path()));
    for (where_, bytes) in &all {
        let text = String::from_utf8_lossy(bytes);
        assert!(!text.contains(URL_PASSWORD), "{where_} holds the password");
        let packed: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        for run in KEY_RUNS {
            assert!(!packed.contains(run), "{where_} holds key material");
        }
    }

    for (via, tree) in [("scan", scan_tree.path()), ("refine", refine_tree.path())] {
        crate::copy_source_exists_test::assert_every_copy_source_exists(via, tree);
        let bundled = |path: &str| {
            std::fs::read_to_string(tree.join("unmanaged").join(&path[1..]))
                .unwrap_or_else(|e| panic!("{via}: {path} ships: {e}"))
        };
        let env_file = |path: &str| {
            std::fs::read_to_string(tree.join("env-files").join(&path[1..]))
                .unwrap_or_else(|e| panic!("{via}: {path} is in env-files: {e}"))
        };
        assert_eq!(bundled(URL_ENV), env_file(URL_ENV), "{via}");
        assert_eq!(bundled(URL_ENV), env_copy(URL_ENV), "{via}");
        assert_eq!(bundled(PEM_COMPOSE), compose_copy(PEM_COMPOSE), "{via}");
        for path in [PLAIN_ENV, PLAIN_COMPOSE] {
            assert_eq!(bundled(path), original(path), "{via}: {path} ships as read");
        }
        assert_eq!(env_file(PLAIN_ENV), original(PLAIN_ENV), "{via}");
    }
}

/// A compose file the snapshot holds no copy of: `/usr/local` is an
/// unmanaged walk root but not a compose search root.
const LONE_COMPOSE: &str = "/usr/local/deploy/docker-compose.yml";
const LONE_PLAIN_COMPOSE: &str = "/usr/local/web/compose.yaml";

/// A compose file only the bundle holds takes the URL credential maskers:
/// the password in a `command:` URL is in no export tree and not in the
/// snapshot, on either export path. A secret-free compose file ships byte
/// for byte.
#[test]
fn lone_compose_files_mask_url_credentials() {
    let lone = format!(
        "services:\n  app:\n    image: docker.io/library/nginx:1.27\n    \
         command: curl https://deploy:{URL_PASSWORD}@api.example.test/v1\n"
    );
    let plain = b"services:\n  web:\n    image: docker.io/library/nginx:1.27\n    environment:\n      TZ: UTC\n";
    let (exec, root) = host_of(&[
        (LONE_COMPOSE, lone.into_bytes()),
        (LONE_PLAIN_COMPOSE, plain.to_vec()),
    ]);
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
    assert_eq!(state(LONE_COMPOSE), PayloadRedaction::Redacted);
    assert_eq!(state(LONE_PLAIN_COMPOSE), PayloadRedaction::Unchanged);

    let refine_tree = export_session(&session);
    let mut all = vec![(
        "snapshot".to_string(),
        serde_json::to_vec(&snap).expect("the snapshot serializes"),
    )];
    all.extend(files(scan_tree.path()));
    all.extend(files(refine_tree.path()));
    for (where_, bytes) in &all {
        assert!(
            !contains(bytes, URL_PASSWORD),
            "{where_} holds the password"
        );
    }

    for (via, tree) in [("scan", scan_tree.path()), ("refine", refine_tree.path())] {
        let bundled = |path: &str| {
            std::fs::read(tree.join("unmanaged").join(&path[1..]))
                .unwrap_or_else(|e| panic!("{via}: {path} ships: {e}"))
        };
        assert!(
            contains(&bundled(LONE_COMPOSE), "api.example.test/v1"),
            "{via}"
        );
        assert_eq!(bundled(LONE_PLAIN_COMPOSE), plain, "{via}");
    }
}

/// The placeholder form an inline finding names, as `secrets-review.md`
/// gives it, without the per-run number.
fn placeholder_form(f: &inspectah_core::types::redaction::RedactionFinding) -> String {
    match f.source.as_str() {
        "pattern" => format!("REDACTED_{}_", f.pattern.to_uppercase()),
        "proxy_credential" | "token_credential" => "[REDACTED]".into(),
        other => panic!("{}: no placeholder form for source {other}: {f:#?}", f.path),
    }
}

/// One URL credential is one `secrets-review.md` row, and the row names a
/// placeholder the shipped file holds. A URL password the patterns have
/// no rule for is the URL masker's row alone; one the patterns also match
/// (a `mongodb://` password) is the pattern's row alone, because the
/// merged edit writes the pattern token and the masker's `[REDACTED]` is
/// never in the file.
#[test]
fn each_url_credential_is_one_row_naming_a_shipped_placeholder() {
    let url_env = format!("SERVICE_URL=https://deploy:{URL_PASSWORD}@api.example.test/v1\n");
    let (exec, root) = host_of(&[
        (URL_ENV, url_env.into_bytes()),
        (ENV_PATH, env_file().into_bytes()),
    ]);
    let Exported {
        snap,
        scan_tree,
        session,
        _packed,
    } = export(&exec, root.path(), false);
    let refine_tree = export_session(&session);

    let rows = |path: &str| -> Vec<_> {
        snap.redactions
            .iter()
            .filter(|f| f.path.trim_start_matches('/') == &path[1..])
            .collect()
    };
    let url_rows = rows(URL_ENV);
    assert_eq!(url_rows.len(), 1, "{url_rows:#?}");
    let mongo_line: Vec<_> = rows(ENV_PATH)
        .into_iter()
        .filter(|f| f.line == Some(2))
        .collect();
    assert_eq!(mongo_line.len(), 1, "{mongo_line:#?}");

    for (via, tree) in [("scan", scan_tree.path()), ("refine", refine_tree.path())] {
        for (path, findings) in [(URL_ENV, &url_rows), (ENV_PATH, &rows(ENV_PATH))] {
            let shipped = std::fs::read_to_string(tree.join("unmanaged").join(&path[1..]))
                .unwrap_or_else(|e| panic!("{via}: {path} ships: {e}"));
            let inline = findings
                .iter()
                .filter(|f| f.kind == inspectah_core::types::redaction::RedactionKind::Inline);
            for f in inline {
                let form = placeholder_form(f);
                assert!(
                    shipped.contains(&form),
                    "{via}: {path} row names {form}, which the file does not hold:\n{shipped}"
                );
            }
        }
    }
}

/// Two URL credentials on one line, each also matched by a pattern: a
/// `mongodb://` password and an AWS key standing as a user name. The
/// `.env` copy takes the snapshot pass, the `.conf` file the bundle's
/// payload pass.
const MIXED_ENV: &str = "/opt/mixed/.env";
const MIXED_CONF: &str = "/opt/mixed/app.conf";
const MIXED_PASSWORD: &str = "mixedpw4471";
const MIXED_AWS_KEY: &str = "AKIA0123456789ABCDEF";

/// Each credential on a line holding two is masked under its own
/// placeholder, and every inline row names a placeholder the shipped file
/// holds, on both export paths and through both redaction passes: the
/// URL maskers' rewrites are separate edits, so one never joins the two
/// pattern tokens into one span that writes only the first.
#[test]
fn mixed_url_credentials_on_one_line_each_name_a_shipped_placeholder() {
    let line = format!("mongodb://u:{MIXED_PASSWORD}@db https://{MIXED_AWS_KEY}@host/\n");
    let (exec, root) = host_of(&[
        (MIXED_ENV, line.clone().into_bytes()),
        (MIXED_CONF, line.into_bytes()),
    ]);
    let Exported {
        snap,
        scan_tree,
        session,
        _packed,
    } = export(&exec, root.path(), false);
    let refine_tree = export_session(&session);

    let mut all = vec![(
        "snapshot".to_string(),
        serde_json::to_vec(&snap).expect("the snapshot serializes"),
    )];
    all.extend(files(scan_tree.path()));
    all.extend(files(refine_tree.path()));
    for secret in [MIXED_PASSWORD, MIXED_AWS_KEY] {
        for (where_, bytes) in &all {
            assert!(!contains(bytes, secret), "{where_} holds {secret}");
        }
    }

    for path in [MIXED_ENV, MIXED_CONF] {
        let inline: Vec<_> = snap
            .redactions
            .iter()
            .filter(|f| f.path.trim_start_matches('/') == &path[1..])
            .filter(|f| f.kind == inspectah_core::types::redaction::RedactionKind::Inline)
            .collect();
        assert_eq!(
            inline.len(),
            2,
            "{path}: one row per credential: {inline:#?}"
        );
        for (via, tree) in [("scan", scan_tree.path()), ("refine", refine_tree.path())] {
            let shipped = std::fs::read_to_string(tree.join("unmanaged").join(&path[1..]))
                .unwrap_or_else(|e| panic!("{via}: {path} ships: {e}"));
            for f in &inline {
                let form = placeholder_form(f);
                assert!(
                    shipped.contains(&form),
                    "{via}: {path} row names {form}, which the file does not hold:\n{shipped}"
                );
            }
        }
    }
}

/// Several URL credentials on one line that no pattern has a rule for:
/// two `user:password@` URLs and a token standing as a user name. The
/// `.env` copy takes the snapshot pass, the `.conf` file the bundle's
/// payload pass.
const MULTI_ENV: &str = "/opt/multi/.env";
const MULTI_CONF: &str = "/opt/multi/mirrors.conf";
const MULTI_SECRETS: [&str; 3] = ["firstpw8812", "secondpw3307", "deploytoken0123456789xyz"];
/// A fourth credential on the same line: a password holding an `@`, as
/// its two halves. The masker once stopped at the first `@` and shipped
/// the tail.
const AT_PASSWORD: [&str; 2] = ["atpwhead4471", "atpwtail9940"];

/// Every URL credential on a line is masked, not only the first: no
/// secret survives in the snapshot, the scan export or the refine
/// export, and each is one inline row naming a placeholder the shipped
/// file holds.
#[test]
fn every_url_credential_on_one_line_ships_masked() {
    let [first, second, token] = MULTI_SECRETS;
    let [at_head, at_tail] = AT_PASSWORD;
    let line = format!(
        "MIRRORS=https://a:{first}@h1.example.test https://b:{second}@h2.example.test \
         https://{token}@git.example.test/repo https://c:{at_head}@{at_tail}@h3.example.test\n"
    );
    let credentials = MULTI_SECRETS.len() + 1;
    let (exec, root) = host_of(&[
        (MULTI_ENV, line.clone().into_bytes()),
        (MULTI_CONF, line.into_bytes()),
    ]);
    let Exported {
        snap,
        scan_tree,
        session,
        _packed,
    } = export(&exec, root.path(), false);
    let refine_tree = export_session(&session);

    let mut all = vec![(
        "snapshot".to_string(),
        serde_json::to_vec(&snap).expect("the snapshot serializes"),
    )];
    all.extend(files(scan_tree.path()));
    all.extend(files(refine_tree.path()));
    for secret in MULTI_SECRETS.into_iter().chain(AT_PASSWORD) {
        for (where_, bytes) in &all {
            assert!(!contains(bytes, secret), "{where_} holds {secret}");
        }
    }

    for path in [MULTI_ENV, MULTI_CONF] {
        let inline: Vec<_> = snap
            .redactions
            .iter()
            .filter(|f| f.path.trim_start_matches('/') == &path[1..])
            .filter(|f| f.kind == inspectah_core::types::redaction::RedactionKind::Inline)
            .collect();
        assert_eq!(
            inline.len(),
            credentials,
            "{path}: one row per credential: {inline:#?}"
        );
        for (via, tree) in [("scan", scan_tree.path()), ("refine", refine_tree.path())] {
            let shipped = std::fs::read_to_string(tree.join("unmanaged").join(&path[1..]))
                .unwrap_or_else(|e| panic!("{via}: {path} ships: {e}"));
            assert_eq!(
                shipped.matches("[REDACTED]").count(),
                credentials,
                "{via}: {path}:\n{shipped}"
            );
        }
    }
}

/// One host file per credential class the redaction rework closed, each
/// holding fake secrets no other test plants: a URL password in an
/// ordinary config file, which only the URL pass reads; database URL
/// passwords holding an `@`, in a config file and a compose file; a
/// compact lockfile with two credentialed members; and values that run
/// past a space, an INI password and a quoted Wi-Fi PSK.
fn credential_class_files() -> Vec<(&'static str, String)> {
    vec![
        (
            "/etc/e2e/proxy.conf",
            "HTTPS_PROXY=https://u:cfgurlpw5521@proxy.example.test:3128\n".into(),
        ),
        (
            "/etc/e2e/cache.conf",
            "CACHE_URL=redis://:p@redistail5521@cache.example.test/0\n".into(),
        ),
        (
            "/opt/e2e/docker-compose.yml",
            "services:\n  web:\n    environment:\n      \
             DATABASE_URL: postgres://u:p@pgtail5521@db.example.test/app\n"
                .into(),
        ),
        (
            "/opt/e2enpm/package-lock.json",
            r#"{"name":"e2enpm","lockfileVersion":3,"packages":{"node_modules/a":{"resolved":"https://u:lockfirst5521@npm.example.test/a.tgz"},"node_modules/b":{"resolved":"https://v:locksecond5521@npm.example.test/b.tgz"}}}"#
                .to_string()
                + "\n",
        ),
        (
            "/etc/e2e/app.ini",
            "[db]\npassword=inialpha5521 inibravo5521\n".into(),
        ),
        (
            "/opt/e2e/app.ini",
            "[db]\npassword=optalpha5521 optbravo5521\n".into(),
        ),
        (
            "/etc/wpa_supplicant/wpa_supplicant.conf",
            "network={\n  ssid=\"home\"\n  psk=\"pskcorrect5521 pskhorse5521\"\n}\n".into(),
        ),
    ]
}

/// Every fake secret [`credential_class_files`] plants.
const CREDENTIAL_CLASS_SECRETS: &[&str] = &[
    "cfgurlpw5521",
    "redistail5521",
    "pgtail5521",
    "lockfirst5521",
    "locksecond5521",
    "inialpha5521",
    "inibravo5521",
    "optalpha5521",
    "optbravo5521",
    "pskcorrect5521",
    "pskhorse5521",
];

/// No credential of any class the redaction rework closed survives in the
/// snapshot, the scan export or the refine export: each file goes through
/// the collectors that read it, redaction and both export paths.
#[test]
fn every_credential_class_ships_masked_on_both_export_paths() {
    let files = credential_class_files();
    let planted: Vec<(&str, Vec<u8>)> = files
        .iter()
        .map(|(path, text)| (*path, text.clone().into_bytes()))
        .collect();
    let (exec, root) = host_of(&planted);
    let Exported {
        snap,
        scan_tree,
        session,
        _packed,
    } = export(&exec, root.path(), false);
    let refine_tree = export_session(&session);

    let snapshot = serde_json::to_vec(&snap).expect("the snapshot serializes");
    // Each file reached the snapshot, so the sweep below reads them all.
    for (path, _) in &files {
        assert!(
            contains(&snapshot, &path[1..]),
            "{path} is not in the snapshot"
        );
    }
    let mut all = vec![("snapshot".to_string(), snapshot)];
    all.extend(self::files(scan_tree.path()));
    all.extend(self::files(refine_tree.path()));
    for secret in CREDENTIAL_CLASS_SECRETS {
        for (where_, bytes) in &all {
            assert!(!contains(bytes, secret), "{where_} holds {secret}");
        }
    }
}

/// A link alone in its directory, as driftify plants it: no regular file
/// beside it, so no `COPY` creates the directory.
const LONE_LINK: &str = "/opt/tool/bin/run";
/// The link's own target. On the release check host it is itself a link
/// to `/usr/bin/actual-tool`, which `readlink -f` recorded instead.
const LONE_LINK_TARGET: &str = "/usr/local/bin/run-tool";
/// A second link beside it whose literal target opens with `-`, which
/// `ln` would read as an option without `--`.
const DASH_LINK: &str = "/opt/tool/bin/odd";
const DASH_LINK_TARGET: &str = "--bad-target";

/// A lone unmanaged symlink builds: the Containerfile creates its
/// directory before `ln` runs, on both export paths, and recreates the
/// link with the host's own target rather than the end of its chain,
/// even a target that opens with `-`.
#[test]
fn lone_unmanaged_symlink_gets_its_directory_and_own_target() {
    let root = tempfile::tempdir().expect("a host root");
    let host = SysctlHost::new()
        .file(PLAIN_PATH, PLAIN)
        .link(LONE_LINK, LONE_LINK_TARGET)
        .command(&format!("test -L {LONE_LINK}"), 0, "", "")
        .link(DASH_LINK, DASH_LINK_TARGET)
        .command(&format!("test -L {DASH_LINK}"), 0, "", "");
    let on_disk = root.path().join(&PLAIN_PATH[1..]);
    std::fs::create_dir_all(on_disk.parent().unwrap()).unwrap();
    std::fs::write(on_disk, PLAIN).unwrap();
    let links = [(LONE_LINK, LONE_LINK_TARGET), (DASH_LINK, DASH_LINK_TARGET)];
    for (link, target) in links {
        let link_on_disk = root.path().join(&link[1..]);
        std::fs::create_dir_all(link_on_disk.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(target, &link_on_disk).unwrap();
    }
    let exec = host.build();

    let Exported {
        snap,
        scan_tree,
        session,
        _packed,
    } = export(&exec, root.path(), false);
    for (link, target) in links {
        let item = snap
            .unmanaged_files
            .as_ref()
            .expect("a section")
            .items
            .iter()
            .find(|f| f.path == link)
            .expect("the link is an item");
        assert_eq!(item.link_target, target);
    }

    // Unpacked with plain `tar` rather than `export_session`: that helper
    // uses `inspectah build`'s extractor, which refuses an absolute link
    // target (the open item "inspectah build likely rejects real scan
    // tarballs that hold an unmanaged symlink").
    let refine_tree = tempfile::tempdir().unwrap();
    let packed = refine_tree.path().join("export.tar.gz");
    session
        .export_plan(session.generation())
        .expect("the generation matches")
        .render(&packed)
        .expect("the export writes");
    let unpacked = refine_tree.path().join("unpacked");
    tar::Archive::new(flate2::read::GzDecoder::new(
        std::fs::File::open(&packed).unwrap(),
    ))
    .unpack(&unpacked)
    .expect("the export unpacks");
    let refine_root = std::fs::read_dir(&unpacked)
        .unwrap()
        .next()
        .expect("one top-level directory")
        .unwrap()
        .path();
    for (via, tree) in [
        ("scan", scan_tree.path()),
        ("refine", refine_root.as_path()),
    ] {
        crate::copy_source_exists_test::assert_every_copy_source_exists(via, tree);
        let containerfile = std::fs::read_to_string(tree.join("Containerfile")).unwrap();
        for (link, target) in links {
            let expected =
                format!("RUN mkdir -p -- '/opt/tool/bin' && ln -sf -- '{target}' '{link}'");
            assert!(
                containerfile.lines().any(|l| l == expected),
                "{via}: no `{expected}` in:\n{containerfile}"
            );
            let shipped = tree.join("unmanaged").join(&link[1..]);
            assert_eq!(
                std::fs::read_link(&shipped)
                    .unwrap_or_else(|e| panic!("{via}: {link} ships as a link: {e}")),
                std::path::Path::new(target),
                "{via}"
            );
        }
    }
}
