use inspectah_core::fleet::merge::FleetMergeable;
use inspectah_core::types::config::ConfigFileEntry;
use inspectah_core::types::containers::{ComposeFile, QuadletUnit};
use inspectah_core::types::kernelboot::{KernelModule, SysctlOverride};
use inspectah_core::types::network::{FirewallZone, NMConnection};
use inspectah_core::types::nonrpm::NonRpmItem;
use inspectah_core::types::rpm::{EnabledModuleStream, PackageEntry, RepoFile, VersionLockEntry};
use inspectah_core::types::scheduled::CronJob;
use inspectah_core::types::selinux::SelinuxPortLabel;
use inspectah_core::types::services::SystemdDropIn;

// ---------------------------------------------------------------------------
// PackageEntry
// ---------------------------------------------------------------------------

#[test]
fn test_package_entry_identity_key_is_name_dot_arch() {
    let pkg = PackageEntry {
        name: "httpd".into(),
        arch: "x86_64".into(),
        ..Default::default()
    };
    assert_eq!(pkg.identity_key().as_ref(), "httpd.x86_64");
}

#[test]
fn test_package_entry_has_no_variant_key() {
    assert!(PackageEntry::default().content_variant_key().is_none());
}

#[test]
fn test_package_entry_fleet_mut() {
    let mut pkg = PackageEntry::default();
    assert!(pkg.fleet_mut().is_none());
}

#[test]
fn test_package_entry_set_include() {
    let mut pkg = PackageEntry::default();
    assert!(!pkg.include);
    pkg.set_include(true);
    assert!(pkg.include);
}

// ---------------------------------------------------------------------------
// ConfigFileEntry (variant-capable)
// ---------------------------------------------------------------------------

#[test]
fn test_config_file_identity_key_is_path() {
    let entry = ConfigFileEntry {
        path: "/etc/foo.conf".into(),
        ..Default::default()
    };
    assert_eq!(entry.identity_key().as_ref(), "/etc/foo.conf");
}

#[test]
fn test_config_file_has_variant_key() {
    let entry = ConfigFileEntry {
        path: "/etc/foo.conf".into(),
        content: "val".into(),
        ..Default::default()
    };
    assert!(entry.content_variant_key().is_some());
}

#[test]
fn test_config_file_different_content_different_variant_key() {
    let a = ConfigFileEntry {
        content: "abc".into(),
        ..Default::default()
    };
    let b = ConfigFileEntry {
        content: "xyz".into(),
        ..Default::default()
    };
    assert_ne!(
        a.content_variant_key().unwrap().as_ref(),
        b.content_variant_key().unwrap().as_ref()
    );
}

#[test]
fn test_config_file_has_variant_selection_mut() {
    let mut entry = ConfigFileEntry::default();
    assert!(entry.variant_selection_mut().is_some());
}

// ---------------------------------------------------------------------------
// ComposeFile (variant-capable, uses images hash)
// ---------------------------------------------------------------------------

#[test]
fn test_compose_file_identity_key_is_path() {
    let cf = ComposeFile {
        path: "/opt/app/docker-compose.yml".into(),
        ..Default::default()
    };
    assert_eq!(cf.identity_key().as_ref(), "/opt/app/docker-compose.yml");
}

#[test]
fn test_compose_file_variant_key_uses_images() {
    let cf = ComposeFile {
        path: "/opt/app/docker-compose.yml".into(),
        images: vec![],
        ..Default::default()
    };
    assert!(cf.content_variant_key().is_some());
}

#[test]
fn test_compose_file_has_variant_selection_mut() {
    let mut cf = ComposeFile::default();
    assert!(cf.variant_selection_mut().is_some());
}

// ---------------------------------------------------------------------------
// SystemdDropIn (variant-capable)
// ---------------------------------------------------------------------------

#[test]
fn test_systemd_dropin_identity_key_is_path() {
    let d = SystemdDropIn {
        path: "/etc/systemd/system/httpd.service.d/override.conf".into(),
        ..Default::default()
    };
    assert_eq!(
        d.identity_key().as_ref(),
        "/etc/systemd/system/httpd.service.d/override.conf"
    );
}

#[test]
fn test_systemd_dropin_has_variant_key() {
    let d = SystemdDropIn {
        content: "[Service]\nLimitNOFILE=65535\n".into(),
        ..Default::default()
    };
    assert!(d.content_variant_key().is_some());
}

#[test]
fn test_systemd_dropin_has_variant_selection_mut() {
    let mut d = SystemdDropIn::default();
    assert!(d.variant_selection_mut().is_some());
}

// ---------------------------------------------------------------------------
// QuadletUnit (variant-capable)
// ---------------------------------------------------------------------------

#[test]
fn test_quadlet_unit_identity_key_is_path() {
    let q = QuadletUnit {
        path: "/etc/containers/systemd/myapp.container".into(),
        ..Default::default()
    };
    assert_eq!(
        q.identity_key().as_ref(),
        "/etc/containers/systemd/myapp.container"
    );
}

#[test]
fn test_quadlet_unit_has_variant_key() {
    let q = QuadletUnit {
        content: "[Container]\nImage=quay.io/myorg/myapp:latest\n".into(),
        ..Default::default()
    };
    assert!(q.content_variant_key().is_some());
}

#[test]
fn test_quadlet_unit_has_variant_selection_mut() {
    let mut q = QuadletUnit::default();
    assert!(q.variant_selection_mut().is_some());
}

// ---------------------------------------------------------------------------
// Types without variants
// ---------------------------------------------------------------------------

#[test]
fn test_repo_file_identity_is_path() {
    let r = RepoFile {
        path: "/etc/yum.repos.d/custom.repo".into(),
        ..Default::default()
    };
    assert_eq!(r.identity_key().as_ref(), "/etc/yum.repos.d/custom.repo");
}

#[test]
fn test_repo_file_has_no_variant_key() {
    assert!(RepoFile::default().content_variant_key().is_none());
}

#[test]
fn test_enabled_module_stream_identity() {
    let m = EnabledModuleStream {
        module_name: "nodejs".into(),
        stream: "18".into(),
        ..Default::default()
    };
    assert_eq!(m.identity_key().as_ref(), "nodejs:18");
}

#[test]
fn test_version_lock_entry_identity() {
    let v = VersionLockEntry {
        name: "kernel".into(),
        arch: "x86_64".into(),
        ..Default::default()
    };
    assert_eq!(v.identity_key().as_ref(), "kernel.x86_64");
}

#[test]
fn test_selinux_port_label_identity() {
    let s = SelinuxPortLabel {
        protocol: "tcp".into(),
        port: "8080".into(),
        ..Default::default()
    };
    assert_eq!(s.identity_key().as_ref(), "tcp:8080");
}

#[test]
fn test_nm_connection_identity_is_path() {
    let n = NMConnection {
        path: "/etc/NetworkManager/system-connections/eth0.nmconnection".into(),
        ..Default::default()
    };
    assert_eq!(
        n.identity_key().as_ref(),
        "/etc/NetworkManager/system-connections/eth0.nmconnection"
    );
}

#[test]
fn test_nm_connection_set_include_uses_option() {
    let mut n = NMConnection::default();
    assert!(n.include.is_none());
    n.set_include(true);
    assert_eq!(n.include, Some(true));
}

#[test]
fn test_firewall_zone_identity_is_path() {
    let f = FirewallZone {
        path: "/etc/firewalld/zones/public.xml".into(),
        ..Default::default()
    };
    assert_eq!(f.identity_key().as_ref(), "/etc/firewalld/zones/public.xml");
}

#[test]
fn test_kernel_module_identity_is_name() {
    let k = KernelModule {
        name: "vfio_pci".into(),
        ..Default::default()
    };
    assert_eq!(k.identity_key().as_ref(), "vfio_pci");
}

#[test]
fn test_sysctl_override_identity_is_key() {
    let s = SysctlOverride {
        key: "net.ipv4.ip_forward".into(),
        ..Default::default()
    };
    assert_eq!(s.identity_key().as_ref(), "net.ipv4.ip_forward");
}

#[test]
fn test_nonrpm_item_identity_is_name() {
    let n = NonRpmItem {
        name: "myapp".into(),
        ..Default::default()
    };
    assert_eq!(n.identity_key().as_ref(), "myapp");
}

#[test]
fn test_cron_job_identity_is_path() {
    let c = CronJob {
        path: "/etc/cron.d/backup".into(),
        ..Default::default()
    };
    assert_eq!(c.identity_key().as_ref(), "/etc/cron.d/backup");
}
