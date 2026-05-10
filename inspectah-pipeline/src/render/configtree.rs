//! Config tree materialization — writes config files from snapshot to output.
//!
//! Implements the full `writeConfigTree()` contract from Go:
//! - Config files (snap.config.files where include=true)
//! - Repo files (snap.rpm.repo_files)
//! - GPG keys (snap.rpm.gpg_keys)
//! - Firewall zones (snap.network.firewall_zones)
//! - Kernel/boot snippets (modules-load.d, modprobe.d, dracut.conf.d, tuned, kargs.d)
//! - Systemd drop-ins (both config/ and drop-ins/)
//! - Generated/local timer units
//! - Quadlet units
//! - Flatpak manifests + provisioning service
//! - Non-RPM env files

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::traits::renderer::RenderError;
use std::collections::HashSet;
use std::path::Path;

use super::safety::operator_kargs;

/// Path prefix for quadlet unit files — excluded from config tree copy.
const QUADLET_PREFIX: &str = "etc/containers/systemd/";

/// Validate that a filesystem path is safe: no traversal, no NUL bytes,
/// no absolute paths.
pub fn validate_path(path: &str) -> Result<(), PathError> {
    if path.contains('\0') {
        return Err(PathError::NulByte);
    }
    if path.starts_with('/') {
        return Err(PathError::AbsolutePath);
    }
    for component in path.split('/') {
        if component == ".." {
            return Err(PathError::Traversal);
        }
    }
    Ok(())
}

/// Validate that a tarball entry path is safe for extraction.
pub fn validate_tarball_entry(path: &str) -> Result<(), PathError> {
    if path.contains('\0') {
        return Err(PathError::NulByte);
    }
    if path.starts_with('/') {
        return Err(PathError::AbsolutePath);
    }
    for component in path.split('/') {
        if component == ".." {
            return Err(PathError::Traversal);
        }
    }
    Ok(())
}

/// Validate that a symlink target does not escape the tarball root.
pub fn validate_symlink_target(target: &str, _root: &str) -> Result<(), PathError> {
    if target.contains('\0') {
        return Err(PathError::NulByte);
    }
    // Count how many levels up the target goes
    let mut depth: i32 = 0;
    for component in target.split('/') {
        if component == ".." {
            depth -= 1;
        } else if !component.is_empty() && component != "." {
            depth += 1;
        }
        // If depth goes negative, we've escaped the root
        if depth < 0 {
            return Err(PathError::SymlinkEscape);
        }
    }
    Ok(())
}

/// Path safety errors.
#[derive(Debug, thiserror::Error)]
pub enum PathError {
    #[error("path contains NUL byte")]
    NulByte,
    #[error("absolute path not allowed")]
    AbsolutePath,
    #[error("path traversal detected")]
    Traversal,
    #[error("symlink escapes root")]
    SymlinkEscape,
}

/// Returns the set of NM connection paths that use DHCP (method=auto),
/// which should be excluded from the config tree copy.
fn dhcp_connection_paths(snap: &InspectionSnapshot) -> HashSet<String> {
    let mut paths = HashSet::new();
    if let Some(ref network) = snap.network {
        for conn in &network.connections {
            if conn.method == "auto" && !conn.path.is_empty() {
                let rel = conn.path.trim_start_matches('/');
                paths.insert(rel.to_string());
            }
        }
    }
    paths
}

/// Write content to dest, creating parent directories. Handles
/// directory/file collisions gracefully (skips with warning).
fn safe_write_file(dest: &Path, content: &str) {
    if dest.exists() && dest.is_dir() {
        eprintln!(
            "inspectah: warning: skipping config file write -- path is already a directory: {}",
            dest.display()
        );
        return;
    }

    if let Some(parent) = dest.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            eprintln!(
                "inspectah: warning: skipping config file write -- parent path conflict: {}",
                dest.display()
            );
            return;
        }
    }

    let _ = std::fs::write(dest, content);
}

/// Write all config files from snapshot to output_dir/config/ preserving
/// paths. This implements the full Go `writeConfigTree()` contract.
///
/// Returns the sorted list of top-level directory names materialized under
/// `config/` so the Containerfile renderer can emit matching COPY lines.
/// This is the single source of truth — the Containerfile must not
/// compute its own directory list independently.
pub fn write_config_tree(
    snap: &InspectionSnapshot,
    output_dir: &Path,
) -> Result<Vec<String>, RenderError> {
    let config_dir = output_dir.join("config");
    std::fs::create_dir_all(&config_dir)?;
    let dhcp_paths = dhcp_connection_paths(snap);

    // Config files
    if let Some(ref config) = snap.config {
        for entry in &config.files {
            if !entry.include {
                continue;
            }
            let rel = entry.path.trim_start_matches('/');
            if rel.is_empty() {
                continue;
            }
            if dhcp_paths.contains(rel) {
                continue;
            }
            if rel.starts_with(QUADLET_PREFIX) {
                continue;
            }
            if validate_path(rel).is_err() {
                continue;
            }
            let dest = config_dir.join(rel);
            safe_write_file(&dest, &entry.content);
        }
    }

    // Repo files
    if let Some(ref rpm) = snap.rpm {
        for repo in &rpm.repo_files {
            if !repo.include || repo.path.is_empty() {
                continue;
            }
            let rel = repo.path.trim_start_matches('/');
            if validate_path(rel).is_err() {
                continue;
            }
            let dest = config_dir.join(rel);
            safe_write_file(&dest, &repo.content);
        }

        for key in &rpm.gpg_keys {
            if !key.include || key.path.is_empty() {
                continue;
            }
            let rel = key.path.trim_start_matches('/');
            if validate_path(rel).is_err() {
                continue;
            }
            let dest = config_dir.join(rel);
            safe_write_file(&dest, &key.content);
        }
    }

    // Firewall zones
    if let Some(ref network) = snap.network {
        for zone in &network.firewall_zones {
            if !zone.include || zone.path.is_empty() {
                continue;
            }
            let rel = zone.path.trim_start_matches('/');
            if validate_path(rel).is_err() {
                continue;
            }
            let dest = config_dir.join(rel);
            safe_write_file(&dest, &zone.content);
        }
    }

    // Kernel boot files
    if let Some(ref kb) = snap.kernel_boot {
        // modules-load.d
        for m in &kb.modules_load_d {
            if !m.path.is_empty() {
                let rel = m.path.trim_start_matches('/');
                if validate_path(rel).is_ok() {
                    safe_write_file(&config_dir.join(rel), &m.content);
                }
            }
        }
        // modprobe.d
        for m in &kb.modprobe_d {
            if !m.path.is_empty() {
                let rel = m.path.trim_start_matches('/');
                if validate_path(rel).is_ok() {
                    safe_write_file(&config_dir.join(rel), &m.content);
                }
            }
        }
        // dracut.conf.d
        for d in &kb.dracut_conf {
            if !d.path.is_empty() {
                let rel = d.path.trim_start_matches('/');
                if validate_path(rel).is_ok() {
                    safe_write_file(&config_dir.join(rel), &d.content);
                }
            }
        }
        // Custom tuned profiles
        for tp in &kb.tuned_custom_profiles {
            if !tp.path.is_empty() {
                let rel = tp.path.trim_start_matches('/');
                if validate_path(rel).is_ok() {
                    safe_write_file(&config_dir.join(rel), &tp.content);
                }
            }
        }
        // Kernel arguments drop-in
        let safe_kargs = operator_kargs(&kb.cmdline);
        if !safe_kargs.is_empty() {
            let kargs_dir = config_dir.join("usr/lib/bootc/kargs.d");
            let _ = std::fs::create_dir_all(&kargs_dir);
            let mut toml = String::new();
            toml.push_str("[kargs]\n");
            toml.push_str("# Migrated from kernel cmdline by inspectah\n");
            for k in &safe_kargs {
                toml.push_str(&format!("append = [\"{k}\"]\n"));
            }
            let _ = std::fs::write(kargs_dir.join("inspectah-migrated.toml"), &toml);
        }
    }

    // Systemd drop-ins -- write to both config/ and drop-ins/
    if let Some(ref services) = snap.services {
        let drop_ins_dir = output_dir.join("drop-ins");
        for di in &services.drop_ins {
            if !di.include {
                continue;
            }
            let rel = di.path.trim_start_matches('/');
            if validate_path(rel).is_err() {
                continue;
            }
            safe_write_file(&config_dir.join(rel), &di.content);
            safe_write_file(&drop_ins_dir.join(rel), &di.content);
        }
    }

    // Timer units (generated and local)
    if let Some(ref st) = snap.scheduled_tasks {
        if !st.generated_timer_units.is_empty() || !st.systemd_timers.is_empty() {
            let systemd_dir = config_dir.join("etc/systemd/system");
            let _ = std::fs::create_dir_all(&systemd_dir);

            for u in &st.generated_timer_units {
                if !u.include {
                    continue;
                }
                let _ = std::fs::write(
                    systemd_dir.join(format!("{}.timer", u.name)),
                    &u.timer_content,
                );
                let _ = std::fs::write(
                    systemd_dir.join(format!("{}.service", u.name)),
                    &u.service_content,
                );
            }
            for t in &st.systemd_timers {
                if t.source == "local" {
                    if !t.name.is_empty() && !t.timer_content.is_empty() {
                        let _ = std::fs::write(
                            systemd_dir.join(format!("{}.timer", t.name)),
                            &t.timer_content,
                        );
                    }
                    if !t.name.is_empty() && !t.service_content.is_empty() {
                        let _ = std::fs::write(
                            systemd_dir.join(format!("{}.service", t.name)),
                            &t.service_content,
                        );
                    }
                }
            }
        }
    }

    // Quadlet units
    if let Some(ref containers) = snap.containers {
        for u in &containers.quadlet_units {
            if !u.include || u.name.is_empty() || u.content.is_empty() {
                continue;
            }
            let quadlet_dir = output_dir.join("quadlet");
            let _ = std::fs::create_dir_all(&quadlet_dir);
            let _ = std::fs::write(quadlet_dir.join(&u.name), &u.content);
        }

        // Flatpak manifest and provisioning service
        let included_flatpaks: Vec<_> = containers
            .flatpak_apps
            .iter()
            .filter(|app| app.include)
            .collect();
        if !included_flatpaks.is_empty() {
            let flatpak_dir = output_dir.join("flatpak");
            let _ = std::fs::create_dir_all(&flatpak_dir);

            // Write JSON manifest
            #[derive(serde::Serialize)]
            struct FlatpakManifestEntry {
                app_id: String,
                remote: String,
                branch: String,
                #[serde(skip_serializing_if = "String::is_empty")]
                remote_url: String,
            }
            let manifest: Vec<FlatpakManifestEntry> = included_flatpaks
                .iter()
                .map(|app| FlatpakManifestEntry {
                    app_id: app.app_id.clone(),
                    remote: app.remote.clone(),
                    branch: app.branch.clone(),
                    remote_url: app.remote_url.clone(),
                })
                .collect();
            if let Ok(data) = serde_json::to_string_pretty(&manifest) {
                let _ = std::fs::write(flatpak_dir.join("flatpak-install.json"), &data);
            }

            // Build provisioning service file
            let mut seen = HashSet::new();
            let mut remotes = Vec::new();
            let mut unreconstructable = Vec::new();
            for app in &included_flatpaks {
                if app.remote.is_empty() || seen.contains(&app.remote) {
                    continue;
                }
                seen.insert(&app.remote);
                if app.remote_url.is_empty() {
                    unreconstructable.push(&app.remote);
                } else {
                    remotes.push((&app.remote, &app.remote_url));
                }
            }

            let mut svc = String::new();
            svc.push_str("[Unit]\n");
            svc.push_str(
                "Description=Provision Flatpak applications from inspectah manifest\n",
            );
            svc.push_str("After=network-online.target\n");
            svc.push_str("Wants=network-online.target\n");
            svc.push_str(
                "ConditionPathExists=!/var/lib/inspectah/.flatpak-provisioned\n",
            );
            svc.push_str("\n[Service]\n");
            svc.push_str("Type=oneshot\n");
            svc.push_str("RemainAfterExit=yes\n");
            svc.push_str("Restart=on-failure\n");
            svc.push_str("RestartSec=30s\n\n");

            if !unreconstructable.is_empty() {
                svc.push_str(
                    "# WARNING: The following remote(s) could not be fully reconstructed\n",
                );
                svc.push_str(
                    "# because no URL was captured. You must configure them manually.\n",
                );
                svc.push_str("# See: flatpak remote-modify --help\n");
                for name in &unreconstructable {
                    svc.push_str(&format!("# Remote '{name}': URL unknown\n"));
                }
                svc.push('\n');
            }

            for (name, url) in &remotes {
                svc.push_str(&format!(
                    "ExecStartPre=/usr/bin/flatpak remote-add --if-not-exists {name} {url}\n"
                ));
            }
            for app in &included_flatpaks {
                svc.push_str(&format!(
                    "ExecStart=/usr/bin/flatpak install -y --noninteractive {} {}//{}\n",
                    app.remote, app.app_id, app.branch
                ));
            }
            svc.push_str(
                "ExecStartPost=/usr/bin/mkdir -p /var/lib/inspectah\n",
            );
            svc.push_str(
                "ExecStartPost=/usr/bin/touch /var/lib/inspectah/.flatpak-provisioned\n",
            );
            svc.push_str("\n[Install]\n");
            svc.push_str("WantedBy=multi-user.target\n");
            svc.push_str("\n[Unit]\n");
            svc.push_str("StartLimitBurst=3\n");
            svc.push_str("StartLimitIntervalSec=300s\n");

            let _ = std::fs::write(
                flatpak_dir.join("flatpak-provision.service"),
                &svc,
            );
        }
    }

    // Non-RPM env files
    if let Some(ref nrs) = snap.non_rpm_software {
        for entry in &nrs.env_files {
            if !entry.include {
                continue;
            }
            let rel = entry.path.trim_start_matches('/');
            if rel.is_empty() {
                continue;
            }
            if validate_path(rel).is_err() {
                continue;
            }
            let dest = config_dir.join(rel);
            safe_write_file(&dest, &entry.content);
        }
    }

    // Return the actual top-level directories materialized under config/
    Ok(config_copy_roots(&config_dir))
}

/// Returns the sorted list of top-level directory names under config_dir
/// that contain files (excluding tmp/).
pub fn config_copy_roots(config_dir: &Path) -> Vec<String> {
    let entries = match std::fs::read_dir(config_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    let mut roots = Vec::new();
    for entry in entries.flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name == "tmp" {
            continue;
        }
        // Check if directory has any files
        let has_files = walkdir::WalkDir::new(entry.path())
            .into_iter()
            .flatten()
            .any(|e| e.file_type().is_file());
        if has_files {
            roots.push(name);
        }
    }
    roots.sort();
    roots
}

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::types::config::{ConfigFileEntry, ConfigSection};
    use inspectah_core::types::kernelboot::{ConfigSnippet, KernelBootSection};
    use inspectah_core::types::network::{FirewallZone, NetworkSection};
    use inspectah_core::types::nonrpm::NonRpmSoftwareSection;
    use inspectah_core::types::rpm::{RepoFile, RpmSection};
    use inspectah_core::types::scheduled::{
        GeneratedTimerUnit, ScheduledTaskSection, SystemdTimer,
    };
    use inspectah_core::types::services::{ServiceSection, SystemdDropIn};
    use tempfile::TempDir;

    fn snapshot_with_config(path: &str, content: &str) -> InspectionSnapshot {
        let mut snap = InspectionSnapshot::new();
        snap.config = Some(ConfigSection {
            files: vec![ConfigFileEntry {
                path: path.to_string(),
                content: content.to_string(),
                include: true,
                ..Default::default()
            }],
        });
        snap
    }

    fn snapshot_with_repo(path: &str, content: &str) -> InspectionSnapshot {
        let mut snap = InspectionSnapshot::new();
        snap.rpm = Some(RpmSection {
            repo_files: vec![RepoFile {
                path: path.to_string(),
                content: content.to_string(),
                include: true,
                ..Default::default()
            }],
            ..Default::default()
        });
        snap
    }

    #[test]
    fn test_config_tree_materializes_etc() {
        let snap = snapshot_with_config(
            "/etc/httpd/conf/httpd.conf",
            "ServerRoot /etc/httpd",
        );
        let dir = TempDir::new().unwrap();
        write_config_tree(&snap, dir.path()).unwrap();
        assert!(dir
            .path()
            .join("config/etc/httpd/conf/httpd.conf")
            .exists());
        let content = std::fs::read_to_string(
            dir.path().join("config/etc/httpd/conf/httpd.conf"),
        )
        .unwrap();
        assert_eq!(content, "ServerRoot /etc/httpd");
    }

    #[test]
    fn test_config_tree_includes_repo_files() {
        let snap = snapshot_with_repo(
            "etc/yum.repos.d/epel.repo",
            "[epel]\nbaseurl=https://example.com",
        );
        let dir = TempDir::new().unwrap();
        write_config_tree(&snap, dir.path()).unwrap();
        assert!(dir
            .path()
            .join("config/etc/yum.repos.d/epel.repo")
            .exists());
    }

    #[test]
    fn test_config_tree_includes_gpg_keys() {
        let mut snap = InspectionSnapshot::new();
        snap.rpm = Some(RpmSection {
            gpg_keys: vec![RepoFile {
                path: "etc/pki/rpm-gpg/RPM-GPG-KEY-test".to_string(),
                content: "-----BEGIN PGP PUBLIC KEY BLOCK-----".to_string(),
                include: true,
                ..Default::default()
            }],
            ..Default::default()
        });
        let dir = TempDir::new().unwrap();
        write_config_tree(&snap, dir.path()).unwrap();
        assert!(dir
            .path()
            .join("config/etc/pki/rpm-gpg/RPM-GPG-KEY-test")
            .exists());
    }

    #[test]
    fn test_config_tree_kernel_boot_snippets() {
        let mut snap = InspectionSnapshot::new();
        snap.kernel_boot = Some(KernelBootSection {
            modules_load_d: vec![ConfigSnippet {
                path: "etc/modules-load.d/custom.conf".to_string(),
                content: "br_netfilter".to_string(),
            }],
            modprobe_d: vec![ConfigSnippet {
                path: "etc/modprobe.d/blacklist.conf".to_string(),
                content: "blacklist nouveau".to_string(),
            }],
            dracut_conf: vec![ConfigSnippet {
                path: "etc/dracut.conf.d/custom.conf".to_string(),
                content: "add_drivers+=\" vfio \"".to_string(),
            }],
            tuned_custom_profiles: vec![ConfigSnippet {
                path: "etc/tuned/my-profile/tuned.conf".to_string(),
                content: "[main]\nsummary=Custom profile".to_string(),
            }],
            cmdline: "quiet crashkernel=auto nosmt=force".to_string(),
            ..Default::default()
        });
        let dir = TempDir::new().unwrap();
        write_config_tree(&snap, dir.path()).unwrap();

        assert!(dir
            .path()
            .join("config/etc/modules-load.d/custom.conf")
            .exists());
        assert!(dir
            .path()
            .join("config/etc/modprobe.d/blacklist.conf")
            .exists());
        assert!(dir
            .path()
            .join("config/etc/dracut.conf.d/custom.conf")
            .exists());
        assert!(dir
            .path()
            .join("config/etc/tuned/my-profile/tuned.conf")
            .exists());
        assert!(dir
            .path()
            .join("config/usr/lib/bootc/kargs.d/inspectah-migrated.toml")
            .exists());

        let kargs = std::fs::read_to_string(
            dir.path()
                .join("config/usr/lib/bootc/kargs.d/inspectah-migrated.toml"),
        )
        .unwrap();
        assert!(kargs.contains("nosmt=force"));
        assert!(!kargs.contains("quiet"));
        assert!(!kargs.contains("crashkernel"));
    }

    #[test]
    fn test_config_tree_dropin_mirroring() {
        let mut snap = InspectionSnapshot::new();
        snap.services = Some(ServiceSection {
            drop_ins: vec![SystemdDropIn {
                unit: "httpd.service".to_string(),
                path: "etc/systemd/system/httpd.service.d/override.conf"
                    .to_string(),
                content: "[Service]\nLimitNOFILE=65535".to_string(),
                include: true,
                ..Default::default()
            }],
            ..Default::default()
        });
        let dir = TempDir::new().unwrap();
        write_config_tree(&snap, dir.path()).unwrap();

        // Must exist in both config/ and drop-ins/
        assert!(dir
            .path()
            .join(
                "config/etc/systemd/system/httpd.service.d/override.conf"
            )
            .exists());
        assert!(dir
            .path()
            .join(
                "drop-ins/etc/systemd/system/httpd.service.d/override.conf"
            )
            .exists());

        // Content must match
        let config_content = std::fs::read_to_string(
            dir.path()
                .join("config/etc/systemd/system/httpd.service.d/override.conf"),
        )
        .unwrap();
        let dropin_content = std::fs::read_to_string(
            dir.path()
                .join("drop-ins/etc/systemd/system/httpd.service.d/override.conf"),
        )
        .unwrap();
        assert_eq!(config_content, dropin_content);
    }

    #[test]
    fn test_config_tree_generated_timer_units() {
        let mut snap = InspectionSnapshot::new();
        snap.scheduled_tasks = Some(ScheduledTaskSection {
            generated_timer_units: vec![GeneratedTimerUnit {
                name: "backup".to_string(),
                timer_content: "[Timer]\nOnCalendar=*-*-* 02:00:00"
                    .to_string(),
                service_content: "[Service]\nExecStart=/usr/bin/backup"
                    .to_string(),
                include: true,
                ..Default::default()
            }],
            ..Default::default()
        });
        let dir = TempDir::new().unwrap();
        write_config_tree(&snap, dir.path()).unwrap();
        assert!(dir
            .path()
            .join("config/etc/systemd/system/backup.timer")
            .exists());
        assert!(dir
            .path()
            .join("config/etc/systemd/system/backup.service")
            .exists());
    }

    #[test]
    fn test_config_tree_local_systemd_timers() {
        let mut snap = InspectionSnapshot::new();
        snap.scheduled_tasks = Some(ScheduledTaskSection {
            systemd_timers: vec![SystemdTimer {
                name: "myjob".to_string(),
                source: "local".to_string(),
                timer_content: "[Timer]\nOnCalendar=daily".to_string(),
                service_content: "[Service]\nExecStart=/usr/local/bin/myjob"
                    .to_string(),
                ..Default::default()
            }],
            ..Default::default()
        });
        let dir = TempDir::new().unwrap();
        write_config_tree(&snap, dir.path()).unwrap();
        assert!(dir
            .path()
            .join("config/etc/systemd/system/myjob.timer")
            .exists());
        assert!(dir
            .path()
            .join("config/etc/systemd/system/myjob.service")
            .exists());
    }

    #[test]
    fn test_config_tree_nonrpm_env_files() {
        let mut snap = InspectionSnapshot::new();
        snap.non_rpm_software = Some(NonRpmSoftwareSection {
            env_files: vec![ConfigFileEntry {
                path: "/etc/environment.d/99-custom.conf".to_string(),
                content: "MY_VAR=value".to_string(),
                include: true,
                ..Default::default()
            }],
            ..Default::default()
        });
        let dir = TempDir::new().unwrap();
        write_config_tree(&snap, dir.path()).unwrap();
        assert!(dir
            .path()
            .join("config/etc/environment.d/99-custom.conf")
            .exists());
    }

    #[test]
    fn test_config_tree_firewall_zones() {
        let mut snap = InspectionSnapshot::new();
        snap.network = Some(NetworkSection {
            firewall_zones: vec![FirewallZone {
                path: "etc/firewalld/zones/public.xml".to_string(),
                name: "public".to_string(),
                content: "<zone>...</zone>".to_string(),
                include: true,
                ..Default::default()
            }],
            ..Default::default()
        });
        let dir = TempDir::new().unwrap();
        write_config_tree(&snap, dir.path()).unwrap();
        assert!(dir
            .path()
            .join("config/etc/firewalld/zones/public.xml")
            .exists());
    }

    #[test]
    fn test_excluded_files_not_materialized() {
        let mut snap = InspectionSnapshot::new();
        snap.config = Some(ConfigSection {
            files: vec![ConfigFileEntry {
                path: "/etc/excluded.conf".to_string(),
                content: "secret".to_string(),
                include: false, // not included
                ..Default::default()
            }],
        });
        let dir = TempDir::new().unwrap();
        write_config_tree(&snap, dir.path()).unwrap();
        assert!(!dir.path().join("config/etc/excluded.conf").exists());
    }

    #[test]
    fn test_dhcp_connections_excluded() {
        let mut snap = InspectionSnapshot::new();
        snap.network = Some(NetworkSection {
            connections: vec![
                inspectah_core::types::network::NMConnection {
                    path: "/etc/NetworkManager/system-connections/eth0.nmconnection".to_string(),
                    method: "auto".to_string(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        });
        snap.config = Some(ConfigSection {
            files: vec![ConfigFileEntry {
                path: "/etc/NetworkManager/system-connections/eth0.nmconnection".to_string(),
                content: "[connection]".to_string(),
                include: true,
                ..Default::default()
            }],
        });
        let dir = TempDir::new().unwrap();
        write_config_tree(&snap, dir.path()).unwrap();
        assert!(!dir
            .path()
            .join("config/etc/NetworkManager/system-connections/eth0.nmconnection")
            .exists());
    }

    #[test]
    fn test_quadlet_prefix_excluded_from_config() {
        let mut snap = InspectionSnapshot::new();
        snap.config = Some(ConfigSection {
            files: vec![ConfigFileEntry {
                path: "/etc/containers/systemd/myapp.container".to_string(),
                content: "[Container]".to_string(),
                include: true,
                ..Default::default()
            }],
        });
        let dir = TempDir::new().unwrap();
        write_config_tree(&snap, dir.path()).unwrap();
        assert!(!dir
            .path()
            .join("config/etc/containers/systemd/myapp.container")
            .exists());
    }

    // Path safety tests

    #[test]
    fn test_reject_path_traversal() {
        assert!(validate_path("../../etc/passwd").is_err());
    }

    #[test]
    fn test_reject_nul_bytes() {
        assert!(validate_path("etc/config\0.txt").is_err());
    }

    #[test]
    fn test_reject_absolute_paths_in_tarball() {
        assert!(validate_tarball_entry("/etc/passwd").is_err());
    }

    #[test]
    fn test_reject_symlink_escape() {
        assert!(validate_symlink_target("../../../etc/shadow", "config/")
            .is_err());
    }

    #[test]
    fn test_valid_paths_accepted() {
        assert!(validate_path("etc/httpd/conf/httpd.conf").is_ok());
        assert!(validate_tarball_entry("prefix/etc/httpd.conf").is_ok());
        // A symlink that goes up then back down within root is valid
        assert!(validate_symlink_target("sibling/file", "config/sub/")
            .is_ok());
        // Relative within same level
        assert!(validate_symlink_target("./other", "config/").is_ok());
    }

    #[test]
    fn test_config_copy_roots() {
        let dir = TempDir::new().unwrap();
        let config = dir.path().join("config");
        std::fs::create_dir_all(config.join("etc/httpd")).unwrap();
        std::fs::write(config.join("etc/httpd/conf"), "test").unwrap();
        std::fs::create_dir_all(config.join("usr/lib")).unwrap();
        std::fs::write(config.join("usr/lib/test"), "test").unwrap();
        // tmp/ should be excluded
        std::fs::create_dir_all(config.join("tmp")).unwrap();
        std::fs::write(config.join("tmp/junk"), "test").unwrap();

        let roots = config_copy_roots(&config);
        assert!(roots.contains(&"etc".to_string()));
        assert!(roots.contains(&"usr".to_string()));
        assert!(!roots.contains(&"tmp".to_string()));
        // Verify sorted
        assert_eq!(roots, vec!["etc", "usr"]);
    }
}
