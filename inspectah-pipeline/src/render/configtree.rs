//! Config tree materialization — writes config files from snapshot to output.
//!
//! Materializes the following config artifacts:
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

/// Returns the set of service drop-in paths owned by the services renderer.
///
/// These paths live under `etc/systemd/system/*.service.d/` and must NOT be
/// materialized by the generic config tree — the services section handles
/// them via its own COPY/enable logic.
fn service_owned_paths(snap: &InspectionSnapshot) -> HashSet<String> {
    let mut paths = HashSet::new();
    if let Some(services) = &snap.services {
        for dropin in &services.drop_ins {
            let rel = dropin.path.trim_start_matches('/');
            if !rel.is_empty() {
                paths.insert(rel.to_string());
            }
        }
    }
    paths
}

/// Returns the set of quadlet unit paths owned by the containers renderer.
///
/// These paths live under `etc/containers/systemd/` and must NOT be
/// materialized by the generic config tree — the containers section handles
/// them via its own quadlet/ directory and COPY logic.
fn container_owned_paths(snap: &InspectionSnapshot) -> HashSet<String> {
    let mut paths = HashSet::new();
    if let Some(containers) = &snap.containers {
        for quadlet in &containers.quadlet_units {
            let rel = quadlet.path.trim_start_matches('/');
            if !rel.is_empty() {
                paths.insert(rel.to_string());
            }
        }
    }
    paths
}

/// Returns the set of sysctl source file paths owned by the sysctl renderer.
///
/// These are the per-file drop-ins (e.g., `/etc/sysctl.d/99-custom.conf`) that
/// individual `SysctlOverride` entries reference. The pipeline synthesizes a
/// single merged file instead, so the originals must NOT be materialized.
fn sysctl_source_paths(snap: &InspectionSnapshot) -> HashSet<String> {
    let mut paths = HashSet::new();
    if let Some(ref kb) = snap.kernel_boot {
        for so in &kb.sysctl_overrides {
            if !so.source.is_empty() {
                let rel = so.source.trim_start_matches('/');
                if !rel.is_empty() {
                    paths.insert(rel.to_string());
                }
            }
        }
    }
    paths
}

/// Tuned profile directory prefix — paths under this are owned by the
/// tuned renderer and must NOT be materialized by the config tree.
const TUNED_PREFIX: &str = "etc/tuned/";

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

    if let Some(parent) = dest.parent()
        && std::fs::create_dir_all(parent).is_err()
    {
        eprintln!(
            "inspectah: warning: skipping config file write -- parent path conflict: {}",
            dest.display()
        );
        return;
    }

    let _ = std::fs::write(dest, content);
}

/// Write all config files from snapshot to output_dir/config/ preserving
/// paths. Writes all artifact categories listed in the module doc.
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
    let svc_paths = service_owned_paths(snap);
    let ctr_paths = container_owned_paths(snap);
    let sysctl_paths = sysctl_source_paths(snap);

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
            if rel.starts_with(QUADLET_PREFIX) || ctr_paths.contains(rel) {
                continue;
            }
            if svc_paths.contains(rel) {
                continue;
            }
            if sysctl_paths.contains(rel) {
                continue;
            }
            if rel.starts_with(TUNED_PREFIX) {
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
        // Custom tuned profiles — written to tuned/ (promoted root),
        // not config/. Single-host snapshots (no fleet_meta) treat tuned
        // as included when a profile is active.
        let tuned_included =
            kb.tuned_include || (snap.fleet_meta.is_none() && !kb.tuned_active.is_empty());
        if tuned_included {
            for tp in &kb.tuned_custom_profiles {
                if !tp.path.is_empty() {
                    let rel = tp.path.trim_start_matches('/');
                    if validate_path(rel).is_ok() {
                        let tuned_dir = output_dir.join("tuned");
                        safe_write_file(&tuned_dir.join(rel), &tp.content);
                    }
                }
            }
        }
        // Synthesized sysctl conf — only included overrides
        let included_sysctls: Vec<_> = kb.sysctl_overrides.iter().filter(|s| s.include).collect();
        if !included_sysctls.is_empty() {
            let sysctl_dir = output_dir.join("sysctl/etc/sysctl.d");
            let _ = std::fs::create_dir_all(&sysctl_dir);
            let mut conf = String::new();
            conf.push_str("# Migrated sysctl overrides — generated by inspectah\n");
            for s in &included_sysctls {
                conf.push_str(&format!("{} = {}\n", s.key, s.runtime));
            }
            let _ = std::fs::write(sysctl_dir.join("99-inspectah-migrated.conf"), &conf);
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

    // Systemd drop-ins — write to drop-ins/ only. Service-owned paths
    // are excluded from config/ via the svc_paths filter above; the
    // services renderer handles their materialization.
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
            safe_write_file(&drop_ins_dir.join(rel), &di.content);
        }
    }

    // Timer units (generated and local)
    if let Some(ref st) = snap.scheduled_tasks
        && (!st.generated_timer_units.is_empty() || !st.systemd_timers.is_empty())
    {
        let systemd_dir = config_dir.join("etc/systemd/system");
        let _ = std::fs::create_dir_all(&systemd_dir);

        for u in &st.generated_timer_units {
            if !u.include {
                continue;
            }
            // @reboot entries have empty timer_content — only write a
            // .timer file when there is actual timer content to emit.
            if !u.timer_content.is_empty() {
                let _ = std::fs::write(
                    systemd_dir.join(format!("{}.timer", u.name)),
                    &u.timer_content,
                );
            }
            if !u.service_content.is_empty() {
                let _ = std::fs::write(
                    systemd_dir.join(format!("{}.service", u.name)),
                    &u.service_content,
                );
            }
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

    // Quadlet units — single-host snapshots (no fleet_meta) treat all
    // quadlets as included by default.
    let is_single_host = snap.fleet_meta.is_none();
    if let Some(ref containers) = snap.containers {
        for u in &containers.quadlet_units {
            if (!u.include && !is_single_host) || u.name.is_empty() || u.content.is_empty() {
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
            svc.push_str("Description=Provision Flatpak applications from inspectah manifest\n");
            svc.push_str("After=network-online.target\n");
            svc.push_str("Wants=network-online.target\n");
            svc.push_str("ConditionPathExists=!/var/lib/inspectah/.flatpak-provisioned\n");
            svc.push_str("\n[Service]\n");
            svc.push_str("Type=oneshot\n");
            svc.push_str("RemainAfterExit=yes\n");
            svc.push_str("Restart=on-failure\n");
            svc.push_str("RestartSec=30s\n\n");

            if !unreconstructable.is_empty() {
                svc.push_str(
                    "# WARNING: The following remote(s) could not be fully reconstructed\n",
                );
                svc.push_str("# because no URL was captured. You must configure them manually.\n");
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
            svc.push_str("ExecStartPost=/usr/bin/mkdir -p /var/lib/inspectah\n");
            svc.push_str("ExecStartPost=/usr/bin/touch /var/lib/inspectah/.flatpak-provisioned\n");
            svc.push_str("\n[Install]\n");
            svc.push_str("WantedBy=multi-user.target\n");
            svc.push_str("\n[Unit]\n");
            svc.push_str("StartLimitBurst=3\n");
            svc.push_str("StartLimitIntervalSec=300s\n");

            let _ = std::fs::write(flatpak_dir.join("flatpak-provision.service"), &svc);
        }
    }

    // SELinux carry-forward files: audit rules and PAM configs.
    // These are declarative admin customizations — no FIXME wrapping.
    if let Some(ref sel) = snap.selinux {
        for rule in &sel.audit_rules {
            if !rule.path.is_empty() && !rule.content.is_empty() {
                let rel = rule.path.trim_start_matches('/');
                if validate_path(rel).is_ok() {
                    safe_write_file(&config_dir.join(rel), &rule.content);
                }
            }
        }
        for pam in &sel.pam_configs {
            if !pam.path.is_empty() && !pam.content.is_empty() {
                let rel = pam.path.trim_start_matches('/');
                if validate_path(rel).is_ok() {
                    safe_write_file(&config_dir.join(rel), &pam.content);
                }
            }
        }
    }

    // Non-RPM env files are NOT written under config/ — they go to a separate
    // env-files/ directory via write_env_files(). See design decision: .env files
    // are high-probability secret carriers requiring operator review.

    // Stage subscription material (decoded from base64)
    if snap.preserved_subscription
        && let Some(ref sub) = snap.subscription
    {
        stage_subscription_files(output_dir, sub)?;
    }

    // Return the actual top-level directories materialized under config/
    Ok(config_copy_roots(&config_dir))
}

/// Stage subscription files into subscription/ directory, decoding from base64.
fn stage_subscription_files(
    output_dir: &Path,
    section: &inspectah_core::types::subscription::SubscriptionSection,
) -> Result<(), RenderError> {
    use base64::Engine;

    let sub_dir = output_dir.join("subscription");

    // Entitlement certs -> subscription/entitlement/
    let ent_dir = sub_dir.join("entitlement");
    for f in &section.entitlement_certs {
        let filename = Path::new(&f.path).file_name().unwrap_or_default();
        let dest = ent_dir.join(filename);
        std::fs::create_dir_all(dest.parent().unwrap())?;
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(&f.content)
            .map_err(|e| {
                RenderError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e))
            })?;
        std::fs::write(&dest, decoded)?;
    }

    // CA certs -> subscription/rhsm/ca/
    let ca_dir = sub_dir.join("rhsm/ca");
    for f in &section.ca_certs {
        let filename = Path::new(&f.path).file_name().unwrap_or_default();
        let dest = ca_dir.join(filename);
        std::fs::create_dir_all(dest.parent().unwrap())?;
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(&f.content)
            .map_err(|e| {
                RenderError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e))
            })?;
        std::fs::write(&dest, decoded)?;
    }

    // Config files (rhsm.conf -> subscription/rhsm/, redhat.repo -> subscription/)
    for f in &section.config_files {
        let dest = if f.path.contains("rhsm.conf") {
            sub_dir.join("rhsm/rhsm.conf")
        } else if f.path.contains("redhat.repo") {
            sub_dir.join("redhat.repo")
        } else {
            continue;
        };
        std::fs::create_dir_all(dest.parent().unwrap())?;
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(&f.content)
            .map_err(|e| {
                RenderError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e))
            })?;
        std::fs::write(&dest, decoded)?;
    }

    Ok(())
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

/// Write .env files to a separate `env-files/` directory under output_dir.
///
/// These are NOT written under `config/` because .env files are
/// high-probability secret carriers requiring operator review before
/// inclusion in a container image.
pub fn write_env_files(snap: &InspectionSnapshot, output_dir: &Path) -> Result<(), RenderError> {
    let nrs = match &snap.non_rpm_software {
        Some(n) => n,
        None => return Ok(()),
    };

    let included: Vec<_> = nrs
        .env_files
        .iter()
        .filter(|e| e.include && !e.path.is_empty() && !e.content.trim().is_empty())
        .collect();

    if included.is_empty() {
        return Ok(());
    }

    let env_dir = output_dir.join("env-files");
    std::fs::create_dir_all(&env_dir)?;

    for entry in &included {
        let rel = entry.path.trim_start_matches('/');
        if rel.is_empty() {
            continue;
        }
        if validate_path(rel).is_err() {
            continue;
        }
        safe_write_file(&env_dir.join(rel), &entry.content);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::types::config::{ConfigFileEntry, ConfigSection};
    use inspectah_core::types::containers::{ContainerSection, FlatpakApp, QuadletUnit};
    use inspectah_core::types::kernelboot::{ConfigSnippet, KernelBootSection, SysctlOverride};
    use inspectah_core::types::network::{FirewallZone, NetworkSection};
    use inspectah_core::types::nonrpm::NonRpmSoftwareSection;
    use inspectah_core::types::rpm::{RepoFile, RpmSection};
    use inspectah_core::types::scheduled::{
        GeneratedTimerUnit, ScheduledTaskSection, SystemdTimer,
    };
    use inspectah_core::types::selinux::{CarryForwardFile, SelinuxSection};
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
        let snap = snapshot_with_config("/etc/httpd/conf/httpd.conf", "ServerRoot /etc/httpd");
        let dir = TempDir::new().unwrap();
        write_config_tree(&snap, dir.path()).unwrap();
        assert!(dir.path().join("config/etc/httpd/conf/httpd.conf").exists());
        let content =
            std::fs::read_to_string(dir.path().join("config/etc/httpd/conf/httpd.conf")).unwrap();
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
        assert!(dir.path().join("config/etc/yum.repos.d/epel.repo").exists());
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
        assert!(
            dir.path()
                .join("config/etc/pki/rpm-gpg/RPM-GPG-KEY-test")
                .exists()
        );
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
            tuned_include: true,
            tuned_custom_profiles: vec![ConfigSnippet {
                path: "etc/tuned/my-profile/tuned.conf".to_string(),
                content: "[main]\nsummary=Custom profile".to_string(),
            }],
            cmdline: "quiet crashkernel=auto nosmt=force".to_string(),
            ..Default::default()
        });
        let dir = TempDir::new().unwrap();
        write_config_tree(&snap, dir.path()).unwrap();

        assert!(
            dir.path()
                .join("config/etc/modules-load.d/custom.conf")
                .exists()
        );
        assert!(
            dir.path()
                .join("config/etc/modprobe.d/blacklist.conf")
                .exists()
        );
        assert!(
            dir.path()
                .join("config/etc/dracut.conf.d/custom.conf")
                .exists()
        );
        assert!(
            dir.path()
                .join("tuned/etc/tuned/my-profile/tuned.conf")
                .exists()
        );
        assert!(
            dir.path()
                .join("config/usr/lib/bootc/kargs.d/inspectah-migrated.toml")
                .exists()
        );

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
    fn test_config_tree_dropin_only_in_dropins_dir() {
        // Service drop-ins are written to drop-ins/ only — NOT config/.
        // The services renderer owns these paths.
        let mut snap = InspectionSnapshot::new();
        snap.services = Some(ServiceSection {
            drop_ins: vec![SystemdDropIn {
                unit: "httpd.service".to_string(),
                path: "etc/systemd/system/httpd.service.d/override.conf".to_string(),
                content: "[Service]\nLimitNOFILE=65535".to_string(),
                include: true,
                ..Default::default()
            }],
            ..Default::default()
        });
        let dir = TempDir::new().unwrap();
        write_config_tree(&snap, dir.path()).unwrap();

        // Must NOT exist in config/ (service-owned)
        assert!(
            !dir.path()
                .join("config/etc/systemd/system/httpd.service.d/override.conf")
                .exists(),
            "service drop-in must NOT be materialized under config/"
        );

        // Must exist in drop-ins/
        assert!(
            dir.path()
                .join("drop-ins/etc/systemd/system/httpd.service.d/override.conf")
                .exists(),
            "service drop-in must be written to drop-ins/"
        );
        let content = std::fs::read_to_string(
            dir.path()
                .join("drop-ins/etc/systemd/system/httpd.service.d/override.conf"),
        )
        .unwrap();
        assert_eq!(content, "[Service]\nLimitNOFILE=65535");
    }

    #[test]
    fn test_config_tree_generated_timer_units() {
        let mut snap = InspectionSnapshot::new();
        snap.scheduled_tasks = Some(ScheduledTaskSection {
            generated_timer_units: vec![GeneratedTimerUnit {
                name: "backup".to_string(),
                timer_content: "[Timer]\nOnCalendar=*-*-* 02:00:00".to_string(),
                service_content: "[Service]\nExecStart=/usr/bin/backup".to_string(),
                include: true,
                ..Default::default()
            }],
            ..Default::default()
        });
        let dir = TempDir::new().unwrap();
        write_config_tree(&snap, dir.path()).unwrap();
        assert!(
            dir.path()
                .join("config/etc/systemd/system/backup.timer")
                .exists()
        );
        assert!(
            dir.path()
                .join("config/etc/systemd/system/backup.service")
                .exists()
        );
    }

    #[test]
    fn test_config_tree_local_systemd_timers() {
        let mut snap = InspectionSnapshot::new();
        snap.scheduled_tasks = Some(ScheduledTaskSection {
            systemd_timers: vec![SystemdTimer {
                name: "myjob".to_string(),
                source: "local".to_string(),
                timer_content: "[Timer]\nOnCalendar=daily".to_string(),
                service_content: "[Service]\nExecStart=/usr/local/bin/myjob".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        });
        let dir = TempDir::new().unwrap();
        write_config_tree(&snap, dir.path()).unwrap();
        assert!(
            dir.path()
                .join("config/etc/systemd/system/myjob.timer")
                .exists()
        );
        assert!(
            dir.path()
                .join("config/etc/systemd/system/myjob.service")
                .exists()
        );
    }

    #[test]
    fn test_config_tree_nonrpm_env_files_not_in_config() {
        // .env files are no longer written under config/ — they go to env-files/
        // via write_env_files(). Verify write_config_tree does NOT produce them.
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
        assert!(
            !dir.path()
                .join("config/etc/environment.d/99-custom.conf")
                .exists(),
            ".env files must NOT be written under config/"
        );
    }

    #[test]
    fn test_write_env_files_to_separate_dir() {
        // .env files now go to env-files/ via write_env_files()
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
        write_env_files(&snap, dir.path()).unwrap();
        assert!(
            dir.path()
                .join("env-files/etc/environment.d/99-custom.conf")
                .exists()
        );
        let content = std::fs::read_to_string(
            dir.path()
                .join("env-files/etc/environment.d/99-custom.conf"),
        )
        .unwrap();
        assert_eq!(content, "MY_VAR=value");
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
        assert!(
            dir.path()
                .join("config/etc/firewalld/zones/public.xml")
                .exists()
        );
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
            connections: vec![inspectah_core::types::network::NMConnection {
                path: "/etc/NetworkManager/system-connections/eth0.nmconnection".to_string(),
                method: "auto".to_string(),
                ..Default::default()
            }],
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
        assert!(
            !dir.path()
                .join("config/etc/NetworkManager/system-connections/eth0.nmconnection")
                .exists()
        );
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
        assert!(
            !dir.path()
                .join("config/etc/containers/systemd/myapp.container")
                .exists()
        );
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
        assert!(validate_symlink_target("../../../etc/shadow", "config/").is_err());
    }

    #[test]
    fn test_valid_paths_accepted() {
        assert!(validate_path("etc/httpd/conf/httpd.conf").is_ok());
        assert!(validate_tarball_entry("prefix/etc/httpd.conf").is_ok());
        // A symlink that goes up then back down within root is valid
        assert!(validate_symlink_target("sibling/file", "config/sub/").is_ok());
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

    #[test]
    fn test_config_tree_selinux_audit_rules_materialized() {
        let mut snap = InspectionSnapshot::new();
        snap.selinux = Some(SelinuxSection {
            audit_rules: vec![
                CarryForwardFile {
                    path: "etc/audit/rules.d/custom.rules".to_string(),
                    content: "-w /etc/shadow -p wa -k shadow".to_string(),
                },
                CarryForwardFile {
                    path: "etc/audit/rules.d/compliance.rules".to_string(),
                    content: "-a always,exit -F arch=b64".to_string(),
                },
            ],
            ..Default::default()
        });
        let dir = TempDir::new().unwrap();
        write_config_tree(&snap, dir.path()).unwrap();
        let custom = dir.path().join("config/etc/audit/rules.d/custom.rules");
        assert!(custom.exists());
        assert_eq!(
            std::fs::read_to_string(&custom).unwrap(),
            "-w /etc/shadow -p wa -k shadow"
        );
        let compliance = dir.path().join("config/etc/audit/rules.d/compliance.rules");
        assert!(compliance.exists());
        assert_eq!(
            std::fs::read_to_string(&compliance).unwrap(),
            "-a always,exit -F arch=b64"
        );
    }

    #[test]
    fn test_config_tree_selinux_pam_configs_materialized() {
        let mut snap = InspectionSnapshot::new();
        snap.selinux = Some(SelinuxSection {
            pam_configs: vec![CarryForwardFile {
                path: "etc/pam.d/custom-sshd".to_string(),
                content: "auth required pam_unix.so".to_string(),
            }],
            ..Default::default()
        });
        let dir = TempDir::new().unwrap();
        write_config_tree(&snap, dir.path()).unwrap();
        let pam = dir.path().join("config/etc/pam.d/custom-sshd");
        assert!(pam.exists());
        assert_eq!(
            std::fs::read_to_string(&pam).unwrap(),
            "auth required pam_unix.so"
        );
    }

    #[test]
    fn test_service_dropin_not_in_config_tree() {
        // Service-owned drop-in paths must NOT be materialized under config/
        // — the services renderer owns these paths.
        let mut snap = InspectionSnapshot::new();
        snap.services = Some(ServiceSection {
            drop_ins: vec![SystemdDropIn {
                unit: "sshd.service".to_string(),
                path: "etc/systemd/system/sshd.service.d/override.conf".to_string(),
                content: "[Service]\nPermitRootLogin=no".to_string(),
                include: true,
                ..Default::default()
            }],
            ..Default::default()
        });
        // Also add the same path as a config file entry to verify the
        // svc_paths exclusion catches it in the config files loop too.
        snap.config = Some(ConfigSection {
            files: vec![ConfigFileEntry {
                path: "/etc/systemd/system/sshd.service.d/override.conf".to_string(),
                content: "[Service]\nPermitRootLogin=no".to_string(),
                include: true,
                ..Default::default()
            }],
        });
        let dir = TempDir::new().unwrap();
        write_config_tree(&snap, dir.path()).unwrap();

        // Must NOT exist in config/
        assert!(
            !dir.path()
                .join("config/etc/systemd/system/sshd.service.d/override.conf")
                .exists(),
            "service-owned drop-in must NOT be materialized under config/"
        );
        // Must still exist in drop-ins/
        assert!(
            dir.path()
                .join("drop-ins/etc/systemd/system/sshd.service.d/override.conf")
                .exists(),
            "service-owned drop-in must still be written to drop-ins/"
        );
    }

    #[test]
    fn test_container_owned_quadlet_excluded_from_config_tree() {
        // Quadlet paths listed in snap.containers.quadlet_units must NOT be
        // materialized under config/ — the containers renderer owns them.
        let mut snap = InspectionSnapshot::new();
        snap.containers = Some(ContainerSection {
            quadlet_units: vec![QuadletUnit {
                path: "/etc/containers/systemd/myapp.container".to_string(),
                name: "myapp.container".to_string(),
                content: "[Container]\nImage=quay.io/test:latest".to_string(),
                include: true,
                ..Default::default()
            }],
            ..Default::default()
        });
        // Also add the same path as a config file to verify exclusion
        snap.config = Some(ConfigSection {
            files: vec![
                ConfigFileEntry {
                    path: "/etc/containers/systemd/myapp.container".to_string(),
                    content: "[Container]\nImage=quay.io/test:latest".to_string(),
                    include: true,
                    ..Default::default()
                },
                // A non-quadlet config file should still be materialized
                ConfigFileEntry {
                    path: "/etc/httpd/conf/httpd.conf".to_string(),
                    content: "ServerRoot /etc/httpd".to_string(),
                    include: true,
                    ..Default::default()
                },
            ],
        });
        let dir = TempDir::new().unwrap();
        write_config_tree(&snap, dir.path()).unwrap();

        // Quadlet path must NOT exist in config/
        assert!(
            !dir.path()
                .join("config/etc/containers/systemd/myapp.container")
                .exists(),
            "container-owned quadlet path must NOT be materialized under config/"
        );
        // Non-quadlet config file must still exist
        assert!(
            dir.path().join("config/etc/httpd/conf/httpd.conf").exists(),
            "non-quadlet config file must still be materialized"
        );
        // Quadlet must be written to quadlet/ directory instead
        assert!(
            dir.path().join("quadlet/myapp.container").exists(),
            "quadlet must be written to quadlet/ directory"
        );
    }

    #[test]
    fn test_excluded_quadlet_not_in_quadlet_dir() {
        // Quadlet with include=false must not be materialized in fleet mode.
        // Single-host snapshots override include=false for quadlets.
        let mut snap = InspectionSnapshot::new();
        snap.fleet_meta = Some(inspectah_core::types::fleet::FleetSnapshotMeta {
            label: "test".into(),
            host_count: 3,
            hostnames: vec![],
            merged_at: String::new(),
            baseline_provisional: false,
            section_host_counts: Default::default(),
        });
        snap.containers = Some(ContainerSection {
            quadlet_units: vec![QuadletUnit {
                path: "/etc/containers/systemd/excluded.container".to_string(),
                name: "excluded.container".to_string(),
                content: "[Container]\nImage=quay.io/test:latest".to_string(),
                include: false,
                ..Default::default()
            }],
            ..Default::default()
        });
        let dir = TempDir::new().unwrap();
        write_config_tree(&snap, dir.path()).unwrap();

        assert!(
            !dir.path().join("quadlet/excluded.container").exists(),
            "excluded quadlet must NOT be written to quadlet/"
        );
    }

    #[test]
    fn test_single_host_quadlet_materialized_by_default() {
        // Single-host snapshots (no fleet_meta) materialize quadlets
        // even when include=false (the raw serde default).
        let mut snap = InspectionSnapshot::new();
        snap.containers = Some(ContainerSection {
            quadlet_units: vec![QuadletUnit {
                path: "/etc/containers/systemd/app.container".to_string(),
                name: "app.container".to_string(),
                content: "[Container]\nImage=quay.io/test:latest".to_string(),
                include: false,
                ..Default::default()
            }],
            ..Default::default()
        });
        let dir = TempDir::new().unwrap();
        write_config_tree(&snap, dir.path()).unwrap();

        assert!(
            dir.path().join("quadlet/app.container").exists(),
            "single-host quadlet must be written to quadlet/"
        );
    }

    #[test]
    fn test_excluded_flatpak_not_materialized() {
        // Flatpak with include=false must not produce any output
        let mut snap = InspectionSnapshot::new();
        snap.containers = Some(ContainerSection {
            flatpak_apps: vec![FlatpakApp {
                app_id: "org.example.Excluded".to_string(),
                origin: "flathub".to_string(),
                branch: "stable".to_string(),
                include: false,
                remote: "flathub".to_string(),
                remote_url: "https://flathub.org/repo/".to_string(),
            }],
            ..Default::default()
        });
        let dir = TempDir::new().unwrap();
        write_config_tree(&snap, dir.path()).unwrap();

        assert!(
            !dir.path().join("flatpak").exists(),
            "excluded flatpak must NOT produce flatpak/ directory"
        );
    }

    #[test]
    fn test_config_tree_selinux_empty_content_skipped() {
        let mut snap = InspectionSnapshot::new();
        snap.selinux = Some(SelinuxSection {
            audit_rules: vec![CarryForwardFile {
                path: "etc/audit/rules.d/empty.rules".to_string(),
                content: String::new(),
            }],
            pam_configs: vec![CarryForwardFile {
                path: String::new(),
                content: "auth required pam_unix.so".to_string(),
            }],
            ..Default::default()
        });
        let dir = TempDir::new().unwrap();
        write_config_tree(&snap, dir.path()).unwrap();
        // Empty content should not be materialized
        assert!(
            !dir.path()
                .join("config/etc/audit/rules.d/empty.rules")
                .exists()
        );
        // Empty path should not be materialized
        assert!(!dir.path().join("config/etc/pam.d").exists());
    }

    #[test]
    fn test_sysctl_source_files_excluded_from_config_tree() {
        // Sysctl source files (e.g., /etc/sysctl.d/99-custom.conf) must NOT
        // be materialized under config/ — the pipeline synthesizes a merged file.
        let mut snap = InspectionSnapshot::new();
        snap.kernel_boot = Some(KernelBootSection {
            sysctl_overrides: vec![SysctlOverride {
                key: "net.ipv4.ip_forward".to_string(),
                runtime: "1".to_string(),
                source: "/etc/sysctl.d/99-custom.conf".to_string(),
                include: true,
                ..Default::default()
            }],
            ..Default::default()
        });
        // Add the same path as a config file entry
        snap.config = Some(ConfigSection {
            files: vec![ConfigFileEntry {
                path: "/etc/sysctl.d/99-custom.conf".to_string(),
                content: "net.ipv4.ip_forward = 1".to_string(),
                include: true,
                ..Default::default()
            }],
        });
        let dir = TempDir::new().unwrap();
        write_config_tree(&snap, dir.path()).unwrap();

        assert!(
            !dir.path()
                .join("config/etc/sysctl.d/99-custom.conf")
                .exists(),
            "sysctl source file must NOT be materialized under config/"
        );
    }

    #[test]
    fn test_sysctl_synthesized_file_from_included_keys() {
        let mut snap = InspectionSnapshot::new();
        snap.kernel_boot = Some(KernelBootSection {
            sysctl_overrides: vec![
                SysctlOverride {
                    key: "net.ipv4.ip_forward".to_string(),
                    runtime: "1".to_string(),
                    source: "/etc/sysctl.d/99-custom.conf".to_string(),
                    include: true,
                    ..Default::default()
                },
                SysctlOverride {
                    key: "vm.swappiness".to_string(),
                    runtime: "10".to_string(),
                    source: "/etc/sysctl.d/99-custom.conf".to_string(),
                    include: false, // excluded
                    ..Default::default()
                },
            ],
            ..Default::default()
        });
        let dir = TempDir::new().unwrap();
        write_config_tree(&snap, dir.path()).unwrap();

        let synth = dir
            .path()
            .join("sysctl/etc/sysctl.d/99-inspectah-migrated.conf");
        assert!(synth.exists(), "synthesized sysctl file must exist");
        let content = std::fs::read_to_string(&synth).unwrap();
        assert!(
            content.contains("net.ipv4.ip_forward = 1"),
            "included key must be in synthesized file"
        );
        assert!(
            !content.contains("vm.swappiness"),
            "excluded key must NOT be in synthesized file"
        );
    }

    #[test]
    fn test_tuned_profile_excluded_from_config_tree() {
        // Config entries under /etc/tuned/ must NOT be materialized —
        // the tuned renderer owns these paths.
        let mut snap = InspectionSnapshot::new();
        snap.config = Some(ConfigSection {
            files: vec![ConfigFileEntry {
                path: "/etc/tuned/my-profile/tuned.conf".to_string(),
                content: "[main]\nsummary=test".to_string(),
                include: true,
                ..Default::default()
            }],
        });
        let dir = TempDir::new().unwrap();
        write_config_tree(&snap, dir.path()).unwrap();

        assert!(
            !dir.path()
                .join("config/etc/tuned/my-profile/tuned.conf")
                .exists(),
            "tuned profile path must NOT be materialized under config/"
        );
    }

    #[test]
    fn test_tuned_excluded_skips_custom_profiles() {
        // When tuned_include is false, custom profiles must NOT be materialized.
        let mut snap = InspectionSnapshot::new();
        snap.kernel_boot = Some(KernelBootSection {
            tuned_active: "my-profile".to_string(),
            tuned_include: false,
            tuned_custom_profiles: vec![ConfigSnippet {
                path: "etc/tuned/my-profile/tuned.conf".to_string(),
                content: "[main]\nsummary=Custom".to_string(),
            }],
            ..Default::default()
        });
        let dir = TempDir::new().unwrap();
        write_config_tree(&snap, dir.path()).unwrap();

        assert!(
            !dir.path()
                .join("config/etc/tuned/my-profile/tuned.conf")
                .exists(),
            "tuned custom profile must NOT be materialized when tuned_include=false"
        );
    }

    #[test]
    fn test_tuned_included_materializes_custom_profiles() {
        // When tuned_include is true, custom profiles are materialized.
        let mut snap = InspectionSnapshot::new();
        snap.kernel_boot = Some(KernelBootSection {
            tuned_active: "my-profile".to_string(),
            tuned_include: true,
            tuned_custom_profiles: vec![ConfigSnippet {
                path: "etc/tuned/my-profile/tuned.conf".to_string(),
                content: "[main]\nsummary=Custom".to_string(),
            }],
            ..Default::default()
        });
        let dir = TempDir::new().unwrap();
        write_config_tree(&snap, dir.path()).unwrap();

        assert!(
            dir.path()
                .join("tuned/etc/tuned/my-profile/tuned.conf")
                .exists(),
            "tuned custom profile must be materialized when tuned_include=true"
        );
    }

    #[test]
    fn test_tuned_bundled_files_all_materialized() {
        // A custom tuned profile can have multiple files (tuned.conf + script.sh).
        // All bundled files must be materialized when tuned_include is true.
        let mut snap = InspectionSnapshot::new();
        snap.kernel_boot = Some(KernelBootSection {
            tuned_active: "my-profile".to_string(),
            tuned_include: true,
            tuned_custom_profiles: vec![
                ConfigSnippet {
                    path: "etc/tuned/my-profile/tuned.conf".to_string(),
                    content: "[main]\nsummary=Custom perf profile".to_string(),
                },
                ConfigSnippet {
                    path: "etc/tuned/my-profile/script.sh".to_string(),
                    content: "#!/bin/bash\necho tuned".to_string(),
                },
            ],
            ..Default::default()
        });
        let dir = TempDir::new().unwrap();
        write_config_tree(&snap, dir.path()).unwrap();

        assert!(
            dir.path()
                .join("tuned/etc/tuned/my-profile/tuned.conf")
                .exists(),
            "tuned.conf must be materialized"
        );
        assert!(
            dir.path()
                .join("tuned/etc/tuned/my-profile/script.sh")
                .exists(),
            "bundled script.sh must be materialized alongside tuned.conf"
        );
        let conf =
            std::fs::read_to_string(dir.path().join("tuned/etc/tuned/my-profile/tuned.conf"))
                .unwrap();
        assert!(
            conf.contains("Custom perf profile"),
            "tuned.conf content must be preserved"
        );
        let script =
            std::fs::read_to_string(dir.path().join("tuned/etc/tuned/my-profile/script.sh"))
                .unwrap();
        assert!(
            script.contains("#!/bin/bash"),
            "script.sh content must be preserved"
        );
    }

    #[test]
    fn test_subscription_dir_staged() {
        use base64::Engine;
        use inspectah_core::types::subscription::{SubscriptionFile, SubscriptionSection};

        let mut snap = InspectionSnapshot::new();
        snap.subscription = Some(SubscriptionSection {
            entitlement_certs: vec![
                SubscriptionFile {
                    path: "/etc/pki/entitlement/123.pem".into(),
                    content: base64::engine::general_purpose::STANDARD.encode("cert-data"),
                    size_bytes: 9,
                    cert_expiry: None,
                },
                SubscriptionFile {
                    path: "/etc/pki/entitlement/123-key.pem".into(),
                    content: base64::engine::general_purpose::STANDARD.encode("key-data"),
                    size_bytes: 8,
                    cert_expiry: None,
                },
            ],
            ca_certs: vec![SubscriptionFile {
                path: "/etc/rhsm/ca/redhat-uep.pem".into(),
                content: base64::engine::general_purpose::STANDARD.encode("ca-data"),
                size_bytes: 7,
                cert_expiry: None,
            }],
            config_files: vec![
                SubscriptionFile {
                    path: "/etc/rhsm/rhsm.conf".into(),
                    content: base64::engine::general_purpose::STANDARD.encode("[rhsm]"),
                    size_bytes: 6,
                    cert_expiry: None,
                },
                SubscriptionFile {
                    path: "/etc/yum.repos.d/redhat.repo".into(),
                    content: base64::engine::general_purpose::STANDARD.encode("[rhel]"),
                    size_bytes: 6,
                    cert_expiry: None,
                },
            ],
            ..Default::default()
        });
        snap.preserved_subscription = true;

        let dir = tempfile::TempDir::new().unwrap();
        write_config_tree(&snap, dir.path()).unwrap();

        assert!(dir.path().join("subscription/entitlement/123.pem").exists());
        assert!(
            dir.path()
                .join("subscription/entitlement/123-key.pem")
                .exists()
        );
        assert!(
            dir.path()
                .join("subscription/rhsm/ca/redhat-uep.pem")
                .exists()
        );
        assert!(dir.path().join("subscription/rhsm/rhsm.conf").exists());
        assert!(dir.path().join("subscription/redhat.repo").exists());
    }

    #[test]
    fn test_no_subscription_no_dir() {
        let snap = InspectionSnapshot::new();
        let dir = tempfile::TempDir::new().unwrap();
        write_config_tree(&snap, dir.path()).unwrap();
        assert!(!dir.path().join("subscription").exists());
    }
}
