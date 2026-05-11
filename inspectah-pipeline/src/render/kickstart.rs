//! Kickstart renderer — produces kickstart-suggestion.ks for deploy-time settings.

use inspectah_core::snapshot::InspectionSnapshot;

/// Render a kickstart suggestion file from the snapshot.
pub fn render_kickstart(snap: &InspectionSnapshot) -> String {
    let mut lines = vec![
        "#version=RHEL9".into(),
        String::new(),
        "# Kickstart suggestion -- review and adapt for your environment".into(),
        "# These settings belong at deploy time, not baked into the image.".into(),
        String::new(),
    ];

    // Network
    if let Some(network) = &snap.network {
        let dhcp_conns: Vec<_> = network
            .connections
            .iter()
            .filter(|c| c.method == "auto" || c.method == "dhcp")
            .collect();
        let static_conns: Vec<_> = network
            .connections
            .iter()
            .filter(|c| c.method == "manual")
            .collect();

        if !dhcp_conns.is_empty() {
            lines.push("# --- DHCP connections ---".into());
            for c in &dhcp_conns {
                let name = if c.name.is_empty() { "eth0" } else { &c.name };
                lines.push(format!(
                    "network --bootproto=dhcp --device={name} --activate"
                ));
            }
            lines.push(String::new());
        }

        if !static_conns.is_empty() {
            lines.push("# --- Static connections ---".into());
            lines.push("# FIXME: fill in IP, netmask, gateway for each static connection".into());
            for c in &static_conns {
                lines.push(format!(
                    "# network --bootproto=static --device={} --ip=FIXME --netmask=FIXME --gateway=FIXME",
                    c.name
                ));
            }
            lines.push(String::new());
        }

        // Hostname
        let hostname = snap
            .meta
            .get("hostname")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if !hostname.is_empty() {
            lines.push(format!("network --hostname={hostname}"));
            lines.push(String::new());
        }

        // /etc/hosts additions
        if !network.hosts_additions.is_empty() {
            lines.push("# --- /etc/hosts additions ---".into());
            lines.push("%post".into());
            for h in &network.hosts_additions {
                lines.push(format!("echo \"{h}\" >> /etc/hosts"));
            }
            lines.push("%end".into());
            lines.push(String::new());
        }

        // Static routes
        if !network.static_routes.is_empty() {
            lines.push("# --- Static route files detected ---".into());
            lines.push(
                "# These files were present on the source host. Review each and translate".into(),
            );
            lines.push(
                "# to NM connection properties (+ipv4.routes) or kickstart route directives."
                    .into(),
            );
            for r in &network.static_routes {
                lines.push(format!(
                    "# FIXME: review {} and add equivalent route to NM connection or kickstart",
                    r.path
                ));
            }
            lines.push(String::new());
        }

        // IP policy rules
        let policy_rules: Vec<_> = network
            .ip_rules
            .iter()
            .filter(|r| !r.trim().is_empty())
            .collect();
        if !policy_rules.is_empty() {
            lines.push("# --- Policy routing rules detected ---".into());
            let limit = policy_rules.len().min(10);
            for r in &policy_rules[..limit] {
                lines.push(format!("# ip rule: {r}"));
            }
            lines.push(
                "# FIXME: translate ip rules to NM connection properties or dispatcher scripts"
                    .into(),
            );
            lines.push(String::new());
        }
    }

    // Users deferred to kickstart
    if let Some(ug) = &snap.users_groups {
        let ks_users: Vec<_> = ug
            .users
            .iter()
            .filter(|u| {
                let strategy = u.get("strategy").and_then(|v| v.as_str()).unwrap_or("");
                let include = u.get("include").and_then(|v| v.as_bool()).unwrap_or(true);
                strategy == "kickstart" && include
            })
            .collect();

        if !ks_users.is_empty() {
            lines.push("# --- Human users (deploy-time provisioning) ---".into());
            for u in &ks_users {
                let uname = u.get("name").and_then(|v| v.as_str()).unwrap_or("unknown");
                let uid = u.get("uid").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let gid = u.get("gid").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let shell = u.get("shell").and_then(|v| v.as_str()).unwrap_or("");
                let home = u.get("home").and_then(|v| v.as_str()).unwrap_or("");

                let mut opts = format!("--name={uname}");
                if uid > 0.0 {
                    opts.push_str(&format!(" --uid={}", uid as u32));
                }
                if gid > 0.0 {
                    opts.push_str(&format!(" --gid={}", gid as u32));
                }
                if !shell.is_empty() {
                    opts.push_str(&format!(" --shell={shell}"));
                }
                if !home.is_empty() {
                    opts.push_str(&format!(" --homedir={home}"));
                }
                lines.push(format!("user {opts}"));
            }
            lines.push("# Set passwords interactively or via --password/--iscrypted".into());
            lines.push(String::new());
        }
    }

    lines.push("# --- Examples ---".into());
    lines.push("# network --bootproto=dhcp --device=eth0".into());
    lines.push("# network --hostname=myhost.example.com".into());
    lines.push(
        "# network --bootproto=static --ip=192.168.1.10 --netmask=255.255.255.0 --gateway=192.168.1.1"
            .into(),
    );
    lines.push(String::new());

    // Storage: remote mounts
    if let Some(storage) = &snap.storage {
        let nfs_mounts: Vec<_> = storage
            .fstab_entries
            .iter()
            .filter(|e| e.fstype.to_lowercase().contains("nfs"))
            .collect();
        let cifs_mounts: Vec<_> = storage
            .fstab_entries
            .iter()
            .filter(|e| e.fstype.to_lowercase().contains("cifs"))
            .collect();

        if !nfs_mounts.is_empty() || !cifs_mounts.is_empty() {
            lines.push("# --- Remote filesystem mounts detected ---".into());
            for m in &nfs_mounts {
                lines.push(format!("# NFS: {} -> {}", m.device, m.mount_point));
                lines.push("#   FIXME: provide NFS credentials at deploy time".into());
            }
            for m in &cifs_mounts {
                lines.push(format!("# CIFS: {} -> {}", m.device, m.mount_point));
                lines.push(
                    "#   FIXME: provide CIFS credentials (username/password) at deploy time".into(),
                );
            }
            lines.push(String::new());
        }
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kickstart_renders() {
        let snap = InspectionSnapshot::new();
        let ks = render_kickstart(&snap);
        assert!(ks.contains("#version="), "must contain version header");
    }

    #[test]
    fn test_kickstart_with_network() {
        let mut snap = InspectionSnapshot::new();
        snap.network = Some(inspectah_core::types::network::NetworkSection {
            connections: vec![inspectah_core::types::network::NMConnection {
                name: "eth0".into(),
                method: "auto".into(),
                ..Default::default()
            }],
            ..Default::default()
        });
        let ks = render_kickstart(&snap);
        assert!(ks.contains("network --bootproto=dhcp --device=eth0"));
    }

    #[test]
    fn test_kickstart_examples_section() {
        let snap = InspectionSnapshot::new();
        let ks = render_kickstart(&snap);
        assert!(ks.contains("# --- Examples ---"));
    }
}
