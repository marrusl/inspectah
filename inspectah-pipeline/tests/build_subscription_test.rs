//! Integration tests for the build pipeline with subscription data.
//!
//! Validates:
//! - Dry-run command includes correct `-v` mounts when subscription is present
//! - Missing subscription components produce preflight errors
//! - Tag validation works across build configs
//! - Archive safety: TarballExtractor rejects malicious entries

use inspectah_pipeline::build::extract::TarballExtractor;
use inspectah_pipeline::build::{BuildConfig, BuildOutcome, plan_and_execute};
use std::io::Write;
use std::path::Path;

// ---------------------------------------------------------------------------
// Tarball construction helpers
// ---------------------------------------------------------------------------

/// Build a .tar.gz with a Containerfile and a complete subscription bundle.
fn build_tarball_with_subscription(dir: &Path) -> std::path::PathBuf {
    let tarball_path = dir.join("test-output.tar.gz");
    let mut builder = tar::Builder::new(Vec::new());

    add_dir(&mut builder, "output/");
    add_file(
        &mut builder,
        "output/Containerfile",
        b"FROM registry.access.redhat.com/ubi9:latest\nRUN dnf install -y httpd",
    );

    // subscription/entitlement/
    add_dir(&mut builder, "output/subscription/");
    add_dir(&mut builder, "output/subscription/entitlement/");
    let cert =
        b"-----BEGIN CERTIFICATE-----\nMIIDxTCCAq2gAwIBAgIJANxyz123...\n-----END CERTIFICATE-----";
    add_file(
        &mut builder,
        "output/subscription/entitlement/12345.pem",
        cert,
    );
    let key =
        b"-----BEGIN RSA PRIVATE KEY-----\nMIIEpAIBAAKCAQEAtest...\n-----END RSA PRIVATE KEY-----";
    add_file(
        &mut builder,
        "output/subscription/entitlement/12345-key.pem",
        key,
    );

    // subscription/rhsm/
    add_dir(&mut builder, "output/subscription/rhsm/");
    add_file(
        &mut builder,
        "output/subscription/rhsm/rhsm.conf",
        b"[rhsm]\nbaseurl = https://cdn.redhat.com\nhostname = subscription.rhsm.redhat.com",
    );
    add_dir(&mut builder, "output/subscription/rhsm/ca/");
    add_file(
        &mut builder,
        "output/subscription/rhsm/ca/redhat-uep.pem",
        b"-----BEGIN CERTIFICATE-----\nCA_CERT_DATA...\n-----END CERTIFICATE-----",
    );

    // subscription/redhat.repo
    add_file(
        &mut builder,
        "output/subscription/redhat.repo",
        b"[rhel-9-for-x86_64-baseos-rpms]\nname=RHEL 9 BaseOS\nenabled=1",
    );

    finish_tarball(builder, &tarball_path);
    tarball_path
}

/// Build a .tar.gz with a Containerfile but no subscription bundle.
fn build_tarball_without_subscription(dir: &Path) -> std::path::PathBuf {
    let tarball_path = dir.join("test-nosub.tar.gz");
    let mut builder = tar::Builder::new(Vec::new());

    add_dir(&mut builder, "output/");
    add_file(&mut builder, "output/Containerfile", b"FROM ubi9:latest");

    finish_tarball(builder, &tarball_path);
    tarball_path
}

/// Build a .tar.gz with a Containerfile and an incomplete subscription bundle.
fn build_tarball_incomplete_subscription(dir: &Path) -> std::path::PathBuf {
    let tarball_path = dir.join("test-incomplete.tar.gz");
    let mut builder = tar::Builder::new(Vec::new());

    add_dir(&mut builder, "output/");
    add_file(&mut builder, "output/Containerfile", b"FROM ubi9:latest");

    // Only entitlement cert, no key, no rhsm.conf, no CA, no redhat.repo
    add_dir(&mut builder, "output/subscription/");
    add_dir(&mut builder, "output/subscription/entitlement/");
    add_file(
        &mut builder,
        "output/subscription/entitlement/999.pem",
        b"orphan-cert",
    );

    finish_tarball(builder, &tarball_path);
    tarball_path
}

fn add_dir(builder: &mut tar::Builder<Vec<u8>>, path: &str) {
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(tar::EntryType::Directory);
    header.set_path(path).expect("set path");
    header.set_size(0);
    header.set_mode(0o755);
    header.set_cksum();
    builder
        .append(&header, std::io::empty())
        .expect("append dir");
}

fn add_file(builder: &mut tar::Builder<Vec<u8>>, path: &str, content: &[u8]) {
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(tar::EntryType::Regular);
    header.set_path(path).expect("set path");
    header.set_size(content.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    builder.append(&header, content).expect("append file");
}

fn finish_tarball(builder: tar::Builder<Vec<u8>>, path: &std::path::PathBuf) {
    let tar_bytes = builder.into_inner().expect("finish tar");
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    gz.write_all(&tar_bytes).expect("compress");
    let gz_bytes = gz.finish().expect("finish gz");
    std::fs::write(path, gz_bytes).expect("write tarball");
}

// ===========================================================================
// Build pipeline tests
// ===========================================================================

/// Dry-run with a complete subscription bundle produces `-v` mount arguments.
#[test]
fn dry_run_with_subscription_includes_mounts() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let tarball = build_tarball_with_subscription(tmp.path());

    let config = BuildConfig {
        tarball,
        tag: "test-image:v1".into(),
        dry_run: true,
        keep_context: false,
        podman_args: vec![],
    };

    let (outcome, _warnings) = plan_and_execute(&config).expect("plan_and_execute");

    match outcome {
        BuildOutcome::DryRun { command } => {
            assert!(command.contains("build"), "should contain 'build'");
            assert!(command.contains("test-image:v1"), "should contain tag");
            assert!(
                command.contains("Containerfile"),
                "should reference Containerfile"
            );
            // On non-RHEL (macOS CI), the tarball bundle is used for mounts
            assert!(
                command.contains("/run/secrets/etc-pki-entitlement"),
                "should mount entitlement certs: {command}"
            );
            assert!(
                command.contains("/run/secrets/rhsm"),
                "should mount rhsm config: {command}"
            );
            assert!(
                command.contains("/run/secrets/redhat.repo"),
                "should mount redhat.repo: {command}"
            );
        }
        BuildOutcome::PodmanNotFound => {
            // Acceptable on dev machines without podman
        }
        other => panic!("expected DryRun or PodmanNotFound, got: {other:?}"),
    }
}

/// Dry-run without subscription does NOT include mount arguments.
#[test]
fn dry_run_without_subscription_no_mounts() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let tarball = build_tarball_without_subscription(tmp.path());

    let config = BuildConfig {
        tarball,
        tag: "test-image:v1".into(),
        dry_run: true,
        keep_context: false,
        podman_args: vec![],
    };

    let (outcome, _) = plan_and_execute(&config).expect("plan_and_execute");

    match outcome {
        BuildOutcome::DryRun { command } => {
            assert!(
                !command.contains("/run/secrets/etc-pki-entitlement"),
                "should NOT mount entitlement when no subscription: {command}"
            );
        }
        BuildOutcome::PodmanNotFound => {}
        other => panic!("expected DryRun or PodmanNotFound, got: {other:?}"),
    }
}

/// Incomplete subscription bundle in tarball produces a preflight error.
#[test]
fn incomplete_subscription_bundle_is_preflight_error() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let tarball = build_tarball_incomplete_subscription(tmp.path());

    let config = BuildConfig {
        tarball,
        tag: "test-image:v1".into(),
        dry_run: true,
        keep_context: false,
        podman_args: vec![],
    };

    let (outcome, _) = plan_and_execute(&config).expect("plan_and_execute");

    match outcome {
        BuildOutcome::PreflightFailed { reason } => {
            assert!(
                reason.contains("incomplete") || reason.contains("missing"),
                "should mention incomplete bundle: {reason}"
            );
        }
        BuildOutcome::PodmanNotFound => {}
        other => panic!("expected PreflightFailed or PodmanNotFound, got: {other:?}"),
    }
}

// ===========================================================================
// Archive safety tests
// ===========================================================================

/// Build a .tar.gz with the given entry specifications.
fn build_evil_tarball(
    dir: &Path,
    name: &str,
    entries: impl FnOnce(&mut tar::Builder<Vec<u8>>),
) -> std::path::PathBuf {
    let tarball_path = dir.join(format!("{name}.tar.gz"));
    let mut builder = tar::Builder::new(Vec::new());
    entries(&mut builder);
    let tar_bytes = builder.into_inner().expect("finish tar");
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    gz.write_all(&tar_bytes).expect("compress");
    std::fs::write(&tarball_path, gz.finish().expect("finish gz")).expect("write");
    tarball_path
}

/// Path traversal via `../` in entry path is rejected.
#[test]
fn archive_rejects_path_traversal() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let tarball = build_evil_tarball(tmp.path(), "traversal", |builder| {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_size(5);
        header.set_mode(0o644);
        let evil_path = b"prefix/sub/../../etc/passwd";
        let name_field = &mut header.as_gnu_mut().unwrap().name;
        name_field[..evil_path.len()].copy_from_slice(evil_path);
        header.set_cksum();
        builder.append(&header, b"evil!" as &[u8]).expect("append");
    });

    let extractor = TarballExtractor::new(tmp.path().join("out"));
    let result = extractor.extract(&tarball);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("path traversal"),
        "expected path traversal error, got: {err}"
    );
}

/// Special file types (char device, block device, FIFO) are rejected.
#[test]
fn archive_rejects_special_file_types() {
    let tmp = tempfile::tempdir().expect("tempdir");

    // Test char device rejection
    let tarball = build_evil_tarball(tmp.path(), "chardev", |builder| {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Char);
        header.set_path("prefix/dev/null").expect("set path");
        header.set_size(0);
        header.set_mode(0o666);
        header.set_cksum();
        builder.append(&header, std::io::empty()).expect("append");
    });

    let extractor = TarballExtractor::new(tmp.path().join("out-char"));
    let result = extractor.extract(&tarball);
    assert!(result.is_err());
    assert!(
        result.unwrap_err().to_string().contains("char device"),
        "should reject char device"
    );

    // Test FIFO rejection
    let tarball = build_evil_tarball(tmp.path(), "fifo", |builder| {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Fifo);
        header.set_path("prefix/evil-fifo").expect("set path");
        header.set_size(0);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append(&header, std::io::empty()).expect("append");
    });

    let extractor = TarballExtractor::new(tmp.path().join("out-fifo"));
    let result = extractor.extract(&tarball);
    assert!(result.is_err());
    assert!(
        result.unwrap_err().to_string().contains("FIFO"),
        "should reject FIFO"
    );
}

/// Duplicate file entries (same path, same type) are rejected.
#[test]
fn archive_rejects_duplicate_paths() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let tarball = build_evil_tarball(tmp.path(), "duplicates", |builder| {
        for _ in 0..2 {
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Regular);
            header.set_path("prefix/config.txt").expect("set path");
            header.set_size(3);
            header.set_mode(0o644);
            header.set_cksum();
            builder.append(&header, b"dup" as &[u8]).expect("append");
        }
    });

    let extractor = TarballExtractor::new(tmp.path().join("out-dup"));
    let result = extractor.extract(&tarball);
    assert!(result.is_err());
    assert!(
        result.unwrap_err().to_string().contains("duplicate entry"),
        "should reject duplicate entries"
    );
}

/// File-type replacement (file at same path as symlink) is rejected.
#[test]
fn archive_rejects_type_replacement() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let tarball = build_evil_tarball(tmp.path(), "replace", |builder| {
        // First entry: regular file
        let mut h1 = tar::Header::new_gnu();
        h1.set_entry_type(tar::EntryType::Regular);
        h1.set_path("prefix/target").expect("set path");
        h1.set_size(4);
        h1.set_mode(0o644);
        h1.set_cksum();
        builder.append(&h1, b"file" as &[u8]).expect("append");

        // Second entry: symlink at same path
        let mut h2 = tar::Header::new_gnu();
        h2.set_entry_type(tar::EntryType::Symlink);
        h2.set_path("prefix/target").expect("set path");
        h2.set_size(0);
        h2.set_mode(0o777);
        h2.set_link_name("elsewhere").expect("set link");
        h2.set_cksum();
        builder.append(&h2, std::io::empty()).expect("append");
    });

    let extractor = TarballExtractor::new(tmp.path().join("out-replace"));
    let result = extractor.extract(&tarball);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("type replacement"),
        "expected type replacement error, got: {err}"
    );
}

/// Symlink pointing outside extraction root is rejected.
#[test]
fn archive_rejects_symlink_escape() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let tarball = build_evil_tarball(tmp.path(), "symlink-escape", |builder| {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_path("prefix/evil-link").expect("set path");
        header.set_size(0);
        header.set_mode(0o777);
        header.set_link_name("/etc/shadow").expect("set link");
        header.set_cksum();
        builder.append(&header, std::io::empty()).expect("append");
    });

    let extractor = TarballExtractor::new(tmp.path().join("out-symlink"));
    let result = extractor.extract(&tarball);
    assert!(result.is_err());
    assert!(
        result.unwrap_err().to_string().contains("symlink escape"),
        "should reject symlink escape"
    );
}

/// All hardlinks are rejected (inspectah tarballs do not use them).
#[test]
fn archive_rejects_hardlink() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let tarball = build_evil_tarball(tmp.path(), "hardlink", |builder| {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Link);
        header.set_path("prefix/some-hardlink").expect("set path");
        header.set_size(0);
        header.set_mode(0o644);
        header
            .set_link_name("target.txt")
            .expect("set link");
        header.set_cksum();
        builder.append(&header, std::io::empty()).expect("append");
    });

    let extractor = TarballExtractor::new(tmp.path().join("out-hardlink"));
    let result = extractor.extract(&tarball);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("hardlinks are not supported"),
        "expected hardlink rejection error, got: {err}"
    );
}

/// Duplicate directory entries are allowed (common in real tarballs).
#[test]
fn archive_allows_duplicate_directories() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let tarball = build_evil_tarball(tmp.path(), "dup-dirs", |builder| {
        for _ in 0..2 {
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Directory);
            header.set_path("prefix/subdir/").expect("set path");
            header.set_size(0);
            header.set_mode(0o755);
            header.set_cksum();
            builder.append(&header, std::io::empty()).expect("append");
        }
    });

    let extractor = TarballExtractor::new(tmp.path().join("out-dupdir"));
    let result = extractor.extract(&tarball);
    assert!(result.is_ok(), "duplicate directories should be allowed");
}

/// Valid tarball extracts successfully and files are readable.
#[test]
fn archive_extracts_valid_tarball() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let tarball = build_tarball_with_subscription(tmp.path());

    let extract_dir = tmp.path().join("extracted");
    let extractor = TarballExtractor::new(extract_dir.clone());
    let result = extractor.extract(&tarball);
    assert!(
        result.is_ok(),
        "valid tarball extraction failed: {result:?}"
    );

    // Verify key files exist
    assert!(extract_dir.join("Containerfile").exists());
    assert!(
        extract_dir
            .join("subscription/entitlement/12345.pem")
            .exists()
    );
    assert!(
        extract_dir
            .join("subscription/entitlement/12345-key.pem")
            .exists()
    );
    assert!(extract_dir.join("subscription/rhsm/rhsm.conf").exists());
    assert!(
        extract_dir
            .join("subscription/rhsm/ca/redhat-uep.pem")
            .exists()
    );
    assert!(extract_dir.join("subscription/redhat.repo").exists());
}
