//! Integration tests for unmanaged file scanning.
//!
//! Tests `scan_unmanaged_files()` which catalogs files in /opt, /srv,
//! /usr/local that are not owned by RPM or Tier 1 language packages.

use inspectah_collect::executor::mock::MockExecutor;
use inspectah_collect::inspectors::nonrpm::scan_unmanaged_files;
use inspectah_core::traits::executor::ExecResult;
use inspectah_core::types::nonrpm::FileType;

// ── Helpers ─────────────────────────────────────────────────────────

/// Build a MockExecutor with common provenance signal fixtures.
///
/// Sets up /etc/machine-id ctime for install-date detection,
/// /proc/mounts for writable-mount detection, and empty systemd dirs
/// for service-workdir detection. Individual tests override as needed.
fn base_mock() -> MockExecutor {
    MockExecutor::new()
        // Install date: /etc/machine-id ctime.
        .with_command(
            "stat -c %Z /etc/machine-id",
            ExecResult {
                stdout: "1600000000\n".into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        // /proc/mounts: /opt mounted rw.
        .with_command(
            "cat /proc/mounts",
            ExecResult {
                stdout: "rootfs / rootfs rw 0 0\n/dev/sda1 /opt ext4 rw,relatime 0 0\n/dev/sda2 /srv ext4 rw,relatime 0 0\n".into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        // Systemd: no WorkingDirectory entries.
        .with_command(
            "grep -rh WorkingDirectory= /etc/systemd/system",
            ExecResult {
                exit_code: 1,
                ..Default::default()
            },
        )
        .with_command(
            "grep -rh WorkingDirectory= /usr/lib/systemd/system",
            ExecResult {
                exit_code: 1,
                ..Default::default()
            },
        )
        // Default: /srv and /usr/local are empty.
        .with_dir("/srv", vec![])
        .with_dir("/usr/local", vec![])
}

/// Mock stat output for a file with given size and mtime.
fn stat_result(size: u64, mtime: u64) -> ExecResult {
    ExecResult {
        stdout: format!("{size} {mtime} 0 0 755\n"),
        exit_code: 0,
        ..Default::default()
    }
}

/// Mock `file -b` output for an ELF binary.
fn file_elf_result() -> ExecResult {
    ExecResult {
        stdout: "ELF 64-bit LSB executable, x86-64\n".into(),
        exit_code: 0,
        ..Default::default()
    }
}

/// Mock `file -b` output for a config file.
fn file_config_result() -> ExecResult {
    ExecResult {
        stdout: "ASCII text\n".into(),
        exit_code: 0,
        ..Default::default()
    }
}

/// Mock `file -b` output for a script.
fn file_script_result() -> ExecResult {
    ExecResult {
        stdout: "Bourne-Again shell script, ASCII text executable\n".into(),
        exit_code: 0,
        ..Default::default()
    }
}

/// Mock `rpm -qf` for an unmanaged file (not RPM-owned).
fn rpm_not_owned() -> ExecResult {
    ExecResult {
        stdout: "file /some/path is not owned by any package\n".into(),
        exit_code: 1,
        ..Default::default()
    }
}

/// Mock `rpm -qf` for an RPM-owned file.
fn rpm_owned() -> ExecResult {
    ExecResult {
        stdout: "httpd24-httpd-2.4.34-1.el7.x86_64\n".into(),
        exit_code: 0,
        ..Default::default()
    }
}

// ── Test 1: ELF binaries are cataloged with correct file_type ────

#[test]
fn scan_unmanaged_catalogs_elf_binaries() {
    let exec = base_mock()
        .with_dir("/opt", vec!["splunk"])
        .with_dir("/opt/splunk", vec!["bin", "etc"])
        .with_dir("/opt/splunk/bin", vec!["splunkd"])
        .with_dir("/opt/splunk/etc", vec!["config.ini"])
        // splunkd: ELF binary, 50 MB
        .with_command(
            "stat -c %s %Y %u %g %a /opt/splunk/bin/splunkd",
            stat_result(52428800, 1700000000),
        )
        .with_command("file -b /opt/splunk/bin/splunkd", file_elf_result())
        .with_command("rpm -qf /opt/splunk/bin/splunkd", rpm_not_owned())
        // config.ini: text file, 2 KB
        .with_command(
            "stat -c %s %Y %u %g %a /opt/splunk/etc/config.ini",
            stat_result(2048, 1700000000),
        )
        .with_command("file -b /opt/splunk/etc/config.ini", file_config_result())
        .with_command("rpm -qf /opt/splunk/etc/config.ini", rpm_not_owned());

    let result = scan_unmanaged_files(&exec, &[], &[]);

    assert_eq!(result.total_count, 2, "should catalog 2 files");
    assert_eq!(
        result.total_size,
        52428800 + 2048,
        "total_size should be sum of file sizes"
    );

    let splunkd = result
        .items
        .iter()
        .find(|f| f.path.contains("splunkd"))
        .expect("should find splunkd");
    assert_eq!(splunkd.file_type, FileType::ElfBinary);

    let config = result
        .items
        .iter()
        .find(|f| f.path.contains("config.ini"))
        .expect("should find config.ini");
    assert_eq!(config.file_type, FileType::Config);
}

// ── Test 2: RPM-owned paths are excluded ─────────────────────────

#[test]
fn scan_unmanaged_excludes_rpm_owned_paths() {
    let exec = base_mock()
        .with_dir("/opt", vec!["rh", "myapp"])
        // RPM-owned: /opt/rh/httpd24/root/usr/sbin/httpd
        .with_dir("/opt/rh", vec!["httpd24"])
        .with_dir("/opt/rh/httpd24", vec!["root"])
        .with_dir("/opt/rh/httpd24/root", vec!["usr"])
        .with_dir("/opt/rh/httpd24/root/usr", vec!["sbin"])
        .with_dir("/opt/rh/httpd24/root/usr/sbin", vec!["httpd"])
        .with_command("rpm -qf /opt/rh/httpd24/root/usr/sbin/httpd", rpm_owned())
        // Not RPM-owned: /opt/myapp/server
        .with_dir("/opt/myapp", vec!["server"])
        .with_command("rpm -qf /opt/myapp/server", rpm_not_owned())
        .with_command(
            "stat -c %s %Y %u %g %a /opt/myapp/server",
            stat_result(1024, 1700000000),
        )
        .with_command("file -b /opt/myapp/server", file_elf_result());

    let result = scan_unmanaged_files(&exec, &[], &[]);

    assert_eq!(
        result.items.len(),
        1,
        "should only contain non-RPM-owned file"
    );
    assert!(
        result.items[0].path.contains("myapp/server"),
        "should contain /opt/myapp/server, got: {}",
        result.items[0].path
    );
}

// ── Test 3: Tier 1 language paths are excluded ───────────────────

#[test]
fn scan_unmanaged_excludes_tier1_language_paths() {
    let exec = base_mock()
        .with_dir("/opt", vec!["myapp"])
        .with_dir("/opt/myapp", vec!["venv", "server"])
        // venv dir contains a python binary -- claimed by Tier 1
        .with_dir("/opt/myapp/venv", vec!["bin"])
        .with_dir("/opt/myapp/venv/bin", vec!["python"])
        .with_command("rpm -qf /opt/myapp/venv/bin/python", rpm_not_owned())
        .with_command(
            "stat -c %s %Y %u %g %a /opt/myapp/venv/bin/python",
            stat_result(4096, 1700000000),
        )
        .with_command("file -b /opt/myapp/venv/bin/python", file_elf_result())
        // server: ELF binary -- unclaimed
        .with_command("rpm -qf /opt/myapp/server", rpm_not_owned())
        .with_command(
            "stat -c %s %Y %u %g %a /opt/myapp/server",
            stat_result(1024, 1700000000),
        )
        .with_command("file -b /opt/myapp/server", file_elf_result());

    // Tier 1 claims /opt/myapp/venv
    let language_env_paths = vec!["/opt/myapp/venv".to_string()];
    let result = scan_unmanaged_files(&exec, &language_env_paths, &[]);

    assert_eq!(result.items.len(), 1, "should only contain unclaimed file");
    assert!(
        result.items[0].path.contains("server"),
        "should contain server, got: {}",
        result.items[0].path
    );
}

// ── Test 4: --exclude-path filters are applied ───────────────────

#[test]
fn scan_unmanaged_applies_exclude_paths() {
    let exec = base_mock()
        .with_dir("/opt", vec!["splunk", "datadog"])
        .with_dir("/opt/splunk", vec!["bin"])
        .with_dir("/opt/splunk/bin", vec!["splunkd"])
        .with_command("rpm -qf /opt/splunk/bin/splunkd", rpm_not_owned())
        .with_command(
            "stat -c %s %Y %u %g %a /opt/splunk/bin/splunkd",
            stat_result(1024, 1700000000),
        )
        .with_command("file -b /opt/splunk/bin/splunkd", file_elf_result())
        .with_dir("/opt/datadog", vec!["agent"])
        .with_dir("/opt/datadog/agent", vec!["dd-agent"])
        .with_command("rpm -qf /opt/datadog/agent/dd-agent", rpm_not_owned())
        .with_command(
            "stat -c %s %Y %u %g %a /opt/datadog/agent/dd-agent",
            stat_result(2048, 1700000000),
        )
        .with_command("file -b /opt/datadog/agent/dd-agent", file_elf_result());

    let exclude = vec!["/opt/datadog".to_string()];
    let result = scan_unmanaged_files(&exec, &[], &exclude);

    assert_eq!(result.items.len(), 1, "should exclude /opt/datadog paths");
    assert!(
        result.items[0].path.contains("splunk"),
        "should only contain splunk files"
    );
}

// ── Test 5: /var is NOT a scan root ──────────────────────────────

#[test]
fn scan_unmanaged_does_not_scan_var() {
    // Even if /var/lib/myapp exists, it should not be scanned.
    // /var is not in UNMANAGED_SCAN_ROOTS.
    let exec = base_mock()
        .with_dir("/opt", vec![])
        // /var exists but should not be touched
        .with_dir("/var", vec!["lib"])
        .with_dir("/var/lib", vec!["myapp"])
        .with_dir("/var/lib/myapp", vec!["data.db"]);

    let result = scan_unmanaged_files(&exec, &[], &[]);

    assert!(
        result.items.is_empty(),
        "/var should not be scanned -- only /opt, /srv, /usr/local are scan roots"
    );
    assert_eq!(result.total_count, 0);
    assert_eq!(result.total_size, 0);
}

// ── Test 6: Script classification ────────────────────────────────

#[test]
fn scan_unmanaged_classifies_scripts() {
    let exec = base_mock()
        .with_dir("/opt", vec!["app"])
        .with_dir("/opt/app", vec!["run.sh"])
        .with_command("rpm -qf /opt/app/run.sh", rpm_not_owned())
        .with_command(
            "stat -c %s %Y %u %g %a /opt/app/run.sh",
            stat_result(512, 1700000000),
        )
        .with_command("file -b /opt/app/run.sh", file_script_result());

    let result = scan_unmanaged_files(&exec, &[], &[]);

    assert_eq!(result.items.len(), 1);
    assert_eq!(
        result.items[0].file_type,
        FileType::Script,
        "should classify as Script"
    );
}

// ── Test 7: Mutability signal ────────────────────────────────────

#[test]
fn scan_unmanaged_computes_mutability_signal() {
    // File mtime (1700000000) is newer than install date (1600000000)
    // => mutable == true
    let exec = base_mock()
        .with_dir("/opt", vec!["app"])
        .with_dir("/opt/app", vec!["data.log"])
        .with_command("rpm -qf /opt/app/data.log", rpm_not_owned())
        .with_command(
            "stat -c %s %Y %u %g %a /opt/app/data.log",
            stat_result(256, 1700000000), // mtime > install date (1600000000)
        )
        .with_command("file -b /opt/app/data.log", file_config_result());

    let result = scan_unmanaged_files(&exec, &[], &[]);

    assert_eq!(result.items.len(), 1);
    assert!(
        result.items[0].provenance.mutable,
        "file newer than install date should be mutable"
    );
}

// ── Test 8: Writable mount signal ────────────────────────────────

#[test]
fn scan_unmanaged_computes_writable_mount_signal() {
    // /proc/mounts has /opt mounted rw (from base_mock)
    let exec = base_mock()
        .with_dir("/opt", vec!["app"])
        .with_dir("/opt/app", vec!["server"])
        .with_command("rpm -qf /opt/app/server", rpm_not_owned())
        .with_command(
            "stat -c %s %Y %u %g %a /opt/app/server",
            stat_result(1024, 1700000000),
        )
        .with_command("file -b /opt/app/server", file_elf_result());

    let result = scan_unmanaged_files(&exec, &[], &[]);

    assert_eq!(result.items.len(), 1);
    assert!(
        result.items[0].provenance.writable_mount,
        "file on /opt (rw) should have writable_mount == true"
    );
}

// ── Test 9: Service working directory signal ─────────────────────

#[test]
fn scan_unmanaged_computes_service_working_dir_signal() {
    let exec = base_mock()
        // Override systemd grep to return a WorkingDirectory.
        .with_command(
            "grep -rh WorkingDirectory= /etc/systemd/system",
            ExecResult {
                stdout: "WorkingDirectory=/opt/myapp\n".into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_dir("/opt", vec!["myapp"])
        .with_dir("/opt/myapp", vec!["data.log"])
        .with_command("rpm -qf /opt/myapp/data.log", rpm_not_owned())
        .with_command(
            "stat -c %s %Y %u %g %a /opt/myapp/data.log",
            stat_result(128, 1700000000),
        )
        .with_command("file -b /opt/myapp/data.log", file_config_result());

    let result = scan_unmanaged_files(&exec, &[], &[]);

    assert_eq!(result.items.len(), 1);
    assert!(
        result.items[0].provenance.service_working_dir,
        "file under WorkingDirectory should have service_working_dir == true"
    );
}
