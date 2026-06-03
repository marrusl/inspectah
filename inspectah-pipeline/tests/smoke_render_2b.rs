//! Renderer smoke tests for Slice 2b sections: network, containers, users.
//!
//! These are SMOKE tests: they prove data REACHES the renderer, not that every
//! field is perfectly formatted. Each test builds a snapshot manually (no
//! inspector execution), calls the relevant renderer, and checks for key
//! markers in the output.
//!
//! Tests 1–9:   Containerfile renderer
//! Tests 10–12: Configtree renderer
//! Test  13:    Containerfile flatpak (cross-cutting with configtree)
//! Tests 14–17: Kickstart renderer
//! Test  18:    Readme renderer
//! Tests 19–20: Cross-cutting (empty/degraded)

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::completeness::{Completeness, InspectorId};
use inspectah_core::types::containers::{ComposeFile, ContainerSection, FlatpakApp, QuadletUnit};
use inspectah_core::types::network::{
    FirewallZone, NMConnection, NetworkSection, ProxyEntry, StaticRouteFile,
};
use inspectah_core::types::users::UserGroupSection;
use inspectah_pipeline::render::{configtree, containerfile, kickstart, readme};
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Helpers: snapshot builders
// ---------------------------------------------------------------------------

fn snapshot_with_firewall() -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.network = Some(NetworkSection {
        firewall_zones: vec![
            FirewallZone {
                path: "etc/firewalld/zones/public.xml".into(),
                name: "public".into(),
                content: "<zone><short>Public</short></zone>".into(),
                include: true,
                locked: false,
                ..Default::default()
            },
            FirewallZone {
                path: "etc/firewalld/zones/internal.xml".into(),
                name: "internal".into(),
                content: "<zone><short>Internal</short></zone>".into(),
                include: true,
                locked: false,
                ..Default::default()
            },
        ],
        ..Default::default()
    });
    snap
}

fn snapshot_with_routes_hosts_proxy() -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.network = Some(NetworkSection {
        static_routes: vec![StaticRouteFile {
            path: "/etc/sysconfig/network-scripts/route-eth0".into(),
            name: "route-eth0".into(),
        }],
        hosts_additions: vec![
            "192.168.1.100 db.internal".into(),
            "10.0.0.5 cache.local".into(),
        ],
        proxy: vec![ProxyEntry {
            source: "/etc/environment".into(),
            line: "http_proxy=http://proxy.corp:8080".into(),
        }],
        ..Default::default()
    });
    snap
}

fn snapshot_with_quadlets() -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.containers = Some(ContainerSection {
        quadlet_units: vec![
            QuadletUnit {
                name: "webapp.container".into(),
                content: "[Container]\nImage=quay.io/myorg/webapp:latest\n".into(),
                include: true,
                locked: false,
                ..Default::default()
            },
            QuadletUnit {
                name: "db.volume".into(),
                content: "[Volume]\nDevice=tmpfs\n".into(),
                include: true,
                locked: false,
                ..Default::default()
            },
        ],
        ..Default::default()
    });
    snap
}

fn snapshot_with_compose_only() -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.containers = Some(ContainerSection {
        compose_files: vec![ComposeFile {
            path: "/opt/app/docker-compose.yml".into(),
            include: true,
            locked: false,
            ..Default::default()
        }],
        ..Default::default()
    });
    snap
}

fn snapshot_with_flatpaks() -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.containers = Some(ContainerSection {
        flatpak_apps: vec![FlatpakApp {
            app_id: "org.mozilla.firefox".into(),
            remote: "flathub".into(),
            branch: "stable".into(),
            include: true,
            locked: false,
            ..Default::default()
        }],
        ..Default::default()
    });
    snap
}

fn snapshot_with_useradd_users() -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.users_groups = Some(UserGroupSection {
        users: vec![serde_json::json!({
            "name": "appuser",
            "uid": 1500,
            "gid": 1500,
            "include": true,
            "containerfile_strategy": "useradd"
        })],
        ..Default::default()
    });
    snap
}

fn snapshot_with_sysusers() -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.users_groups = Some(UserGroupSection {
        users: vec![
            serde_json::json!({
                "name": "dbus",
                "uid": 81,
                "include": true,
                "containerfile_strategy": "skip"
            }),
            serde_json::json!({
                "name": "polkitd",
                "uid": 998,
                "include": true,
                "containerfile_strategy": "skip"
            }),
        ],
        ..Default::default()
    });
    snap
}

fn snapshot_with_blueprint_users() -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.users_groups = Some(UserGroupSection {
        users: vec![serde_json::json!({
            "name": "admin",
            "uid": 1000,
            "include": true,
            "containerfile_strategy": "skip"
        })],
        ..Default::default()
    });
    snap
}

fn snapshot_with_kickstart_users() -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.users_groups = Some(UserGroupSection {
        users: vec![serde_json::json!({
            "name": "deploy",
            "uid": 2000,
            "gid": 2000,
            "shell": "/bin/bash",
            "home": "/home/deploy",
            "include": true,
            "strategy": "kickstart"
        })],
        ..Default::default()
    });
    snap
}

fn snapshot_with_dhcp_connections() -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.network = Some(NetworkSection {
        connections: vec![NMConnection {
            name: "ens192".into(),
            method: "auto".into(),
            ..Default::default()
        }],
        ..Default::default()
    });
    snap
}

fn snapshot_with_static_connections() -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.network = Some(NetworkSection {
        connections: vec![NMConnection {
            name: "bond0".into(),
            // Use "static" — this is what classify_connection() actually emits
            // for NM keyfile method=manual.
            method: "static".into(),
            ..Default::default()
        }],
        ..Default::default()
    });
    snap
}

fn snapshot_with_quadlets_and_compose() -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.containers = Some(ContainerSection {
        quadlet_units: vec![QuadletUnit {
            name: "app.container".into(),
            content: "[Container]\nImage=registry.io/app:v1\n".into(),
            include: true,
            locked: false,
            ..Default::default()
        }],
        compose_files: vec![ComposeFile {
            path: "/opt/stack/docker-compose.yml".into(),
            include: true,
            locked: false,
            ..Default::default()
        }],
        ..Default::default()
    });
    snap
}

// ---------------------------------------------------------------------------
// Test 1: containerfile_network_firewall_copy_comment
// Firewall zones produce zone count + COPY comment (NOT firewall-cmd).
// ---------------------------------------------------------------------------

#[test]
fn containerfile_network_firewall_copy_comment() {
    let snap = snapshot_with_firewall();
    let output = containerfile::render_containerfile(&snap, None);

    // Must mention zone count
    assert!(
        output.contains("2 custom firewall zone(s)"),
        "Containerfile must show the count of included firewall zones"
    );
    // Must reference COPY config/etc/ (the zones live in the config tree)
    assert!(
        output.contains("COPY config/etc/"),
        "Containerfile must reference COPY config/etc/ for firewall zones"
    );
    // Must NOT use firewall-cmd (zones are file-based, not command-based)
    assert!(
        !output.contains("firewall-cmd"),
        "Containerfile must NOT use firewall-cmd for zone configuration"
    );
}

// ---------------------------------------------------------------------------
// Test 2: containerfile_network_static_routes
// Static routes produce comment lines in Containerfile.
// ---------------------------------------------------------------------------

#[test]
fn containerfile_network_static_routes() {
    let snap = snapshot_with_routes_hosts_proxy();
    let output = containerfile::render_containerfile(&snap, None);

    assert!(
        output.contains("Static Routes"),
        "Containerfile must contain Static Routes section heading"
    );
    assert!(
        output.contains("route-eth0"),
        "Containerfile must reference the static route file name"
    );
}

// ---------------------------------------------------------------------------
// Test 3: containerfile_network_hosts_additions
// Hosts additions produce FIXME comments in Containerfile.
// ---------------------------------------------------------------------------

#[test]
fn containerfile_network_hosts_additions() {
    let snap = snapshot_with_routes_hosts_proxy();
    let output = containerfile::render_containerfile(&snap, None);

    assert!(
        output.contains("/etc/hosts Additions"),
        "Containerfile must contain /etc/hosts Additions heading"
    );
    assert!(
        output.contains("FIXME"),
        "Containerfile must include FIXME for hosts additions"
    );
    assert!(
        output.contains("192.168.1.100 db.internal"),
        "Containerfile must list the hosts entry"
    );
}

// ---------------------------------------------------------------------------
// Test 4: containerfile_network_proxy
// Proxy entries produce comment lines in Containerfile.
// ---------------------------------------------------------------------------

#[test]
fn containerfile_network_proxy() {
    let snap = snapshot_with_routes_hosts_proxy();
    let output = containerfile::render_containerfile(&snap, None);

    assert!(
        output.contains("Proxy Configuration"),
        "Containerfile must contain Proxy Configuration heading"
    );
    assert!(
        output.contains("/etc/environment"),
        "Containerfile must reference the proxy source"
    );
    assert!(
        output.contains("http_proxy=http://proxy.corp:8080"),
        "Containerfile must include the proxy line"
    );
}

// ---------------------------------------------------------------------------
// Test 5: containerfile_containers_quadlet_copy
// Quadlet units produce COPY quadlet/ line in Containerfile.
// ---------------------------------------------------------------------------

#[test]
fn containerfile_containers_quadlet_copy() {
    let snap = snapshot_with_quadlets();
    let output = containerfile::render_containerfile(&snap, None);

    assert!(
        output.contains("Container Workloads"),
        "Containerfile must contain Container Workloads heading"
    );
    assert!(
        output.contains("COPY quadlet/ /etc/containers/systemd/"),
        "Containerfile must have COPY quadlet/ line"
    );
}

// ---------------------------------------------------------------------------
// Test 6: containerfile_containers_no_compose_comments
// Compose files alone do NOT produce compose-image comments in Containerfile.
// This is a NEGATIVE assertion — compose rendering is deferred.
// ---------------------------------------------------------------------------

#[test]
fn containerfile_containers_no_compose_comments() {
    let snap = snapshot_with_compose_only();
    let output = containerfile::render_containerfile(&snap, None);

    // Compose-only section has no included quadlets and no included flatpaks,
    // so containers_section_lines returns empty (the guard checks both).
    assert!(
        !output.contains("Container Workloads"),
        "Containerfile must NOT produce a Container Workloads section for compose-only snapshots"
    );
}

// ---------------------------------------------------------------------------
// Test 7: containerfile_users_useradd_override
// Users with useradd strategy produce RUN useradd commands.
// ---------------------------------------------------------------------------

#[test]
fn containerfile_users_useradd_override() {
    let snap = snapshot_with_useradd_users();
    let output = containerfile::render_containerfile(&snap, None);

    assert!(
        output.contains("Users and Groups"),
        "Containerfile must contain Users and Groups heading"
    );
    assert!(
        output.contains("RUN useradd -m -u 1500 -g 1500 appuser"),
        "Containerfile must emit RUN useradd with uid/gid for useradd-strategy users"
    );
}

// ---------------------------------------------------------------------------
// Test 8: containerfile_users_sysusers_skip
// Users with containerfile_strategy=skip produce no user section output.
// ---------------------------------------------------------------------------

#[test]
fn containerfile_users_sysusers_comment() {
    let snap = snapshot_with_sysusers();
    let output = containerfile::render_containerfile(&snap, None);

    // Skip-strategy users produce no Users and Groups section
    assert!(
        !output.contains("Users and Groups"),
        "Containerfile must NOT produce Users and Groups section for skip-strategy users"
    );
}

// ---------------------------------------------------------------------------
// Test 9: containerfile_users_blueprint_skip
// Users with containerfile_strategy=skip produce no user section output.
// ---------------------------------------------------------------------------

#[test]
fn containerfile_users_blueprint_fixme() {
    let snap = snapshot_with_blueprint_users();
    let output = containerfile::render_containerfile(&snap, None);

    // Skip-strategy users produce no Users and Groups section
    assert!(
        !output.contains("Users and Groups"),
        "Containerfile must NOT produce Users and Groups section for skip-strategy users"
    );
}

// ---------------------------------------------------------------------------
// Test 10: configtree_firewall_zones_materialized
// Firewall zone XMLs are written under config/etc/firewalld/zones/.
// ---------------------------------------------------------------------------

#[test]
fn configtree_firewall_zones_materialized() {
    let snap = snapshot_with_firewall();
    let dir = TempDir::new().unwrap();

    configtree::write_config_tree(&snap, dir.path()).unwrap();

    let public_path = dir.path().join("config/etc/firewalld/zones/public.xml");
    assert!(
        public_path.exists(),
        "firewall zone public.xml must be materialized under config/etc/firewalld/zones/"
    );
    let content = std::fs::read_to_string(&public_path).unwrap();
    assert!(
        content.contains("Public"),
        "firewall zone content must match snapshot data"
    );

    let internal_path = dir.path().join("config/etc/firewalld/zones/internal.xml");
    assert!(
        internal_path.exists(),
        "firewall zone internal.xml must be materialized"
    );
}

// ---------------------------------------------------------------------------
// Test 11: configtree_quadlet_units_materialized
// Quadlet unit files are written under quadlet/ (top-level, NOT config/).
// ---------------------------------------------------------------------------

#[test]
fn configtree_quadlet_units_materialized() {
    let snap = snapshot_with_quadlets();
    let dir = TempDir::new().unwrap();

    configtree::write_config_tree(&snap, dir.path()).unwrap();

    let container_path = dir.path().join("quadlet/webapp.container");
    assert!(
        container_path.exists(),
        "quadlet unit webapp.container must be materialized under quadlet/"
    );
    let content = std::fs::read_to_string(&container_path).unwrap();
    assert!(
        content.contains("quay.io/myorg/webapp:latest"),
        "quadlet unit content must match snapshot data"
    );

    let volume_path = dir.path().join("quadlet/db.volume");
    assert!(
        volume_path.exists(),
        "quadlet unit db.volume must be materialized under quadlet/"
    );

    // Quadlet files must NOT be under config/etc/containers/systemd/
    let wrong_path = dir
        .path()
        .join("config/etc/containers/systemd/webapp.container");
    assert!(
        !wrong_path.exists(),
        "quadlet units must NOT be duplicated under config/etc/containers/systemd/"
    );
}

// ---------------------------------------------------------------------------
// Test 12: configtree_flatpak_manifest_and_service
// Flatpak apps produce flatpak-install.json + flatpak-provision.service
// under flatpak/.
// ---------------------------------------------------------------------------

#[test]
fn configtree_flatpak_manifest_and_service() {
    let snap = snapshot_with_flatpaks();
    let dir = TempDir::new().unwrap();

    configtree::write_config_tree(&snap, dir.path()).unwrap();

    let manifest_path = dir.path().join("flatpak/flatpak-install.json");
    assert!(
        manifest_path.exists(),
        "flatpak-install.json must be materialized under flatpak/"
    );
    let manifest = std::fs::read_to_string(&manifest_path).unwrap();
    assert!(
        manifest.contains("org.mozilla.firefox"),
        "flatpak manifest must contain the app ID"
    );
    assert!(
        manifest.contains("flathub"),
        "flatpak manifest must contain the remote"
    );

    let service_path = dir.path().join("flatpak/flatpak-provision.service");
    assert!(
        service_path.exists(),
        "flatpak-provision.service must be materialized under flatpak/"
    );
    let service_content = std::fs::read_to_string(&service_path).unwrap();
    assert!(
        service_content.contains("[Unit]") || service_content.contains("[Service]"),
        "flatpak-provision.service must be a valid systemd unit"
    );
}

// ---------------------------------------------------------------------------
// Test 13: containerfile_containers_flatpak_copy_and_enable
// Flatpak apps produce COPY flatpak/ + RUN systemctl enable lines.
// ---------------------------------------------------------------------------

#[test]
fn containerfile_containers_flatpak_copy_and_enable() {
    let snap = snapshot_with_flatpaks();
    let output = containerfile::render_containerfile(&snap, None);

    assert!(
        output.contains("COPY flatpak/"),
        "Containerfile must include COPY flatpak/ for flatpak provisioning"
    );
    assert!(
        output.contains("systemctl enable") && output.contains("flatpak-provision.service"),
        "Containerfile must enable the flatpak-provision service"
    );
}

// ---------------------------------------------------------------------------
// Test 14: kickstart_network_dhcp_connections
// DHCP connections produce network --bootproto=dhcp in kickstart.
// ---------------------------------------------------------------------------

#[test]
fn kickstart_network_dhcp_connections() {
    let snap = snapshot_with_dhcp_connections();
    let ks = kickstart::render_kickstart(&snap);

    assert!(
        ks.contains("--bootproto=dhcp"),
        "kickstart must contain --bootproto=dhcp for auto/dhcp connections"
    );
    assert!(
        ks.contains("--device=ens192"),
        "kickstart must reference the connection name as device"
    );
    assert!(
        ks.contains("--activate"),
        "kickstart must include --activate flag"
    );
}

// ---------------------------------------------------------------------------
// Test 15: kickstart_network_static_connections
// Static connections produce FIXME comments in kickstart.
// ---------------------------------------------------------------------------

#[test]
fn kickstart_network_static_connections() {
    let snap = snapshot_with_static_connections();
    let ks = kickstart::render_kickstart(&snap);

    assert!(
        ks.contains("Static connections"),
        "kickstart must contain Static connections heading"
    );
    assert!(
        ks.contains("FIXME"),
        "kickstart must include FIXME for static connections"
    );
    assert!(
        ks.contains("--bootproto=static"),
        "kickstart must reference --bootproto=static"
    );
    assert!(
        ks.contains("bond0"),
        "kickstart must reference the static connection name"
    );
}

// ---------------------------------------------------------------------------
// Test 16: kickstart_network_hosts_routes
// Hosts additions and static routes produce kickstart entries.
// ---------------------------------------------------------------------------

#[test]
fn kickstart_network_hosts_routes() {
    let snap = snapshot_with_routes_hosts_proxy();
    let ks = kickstart::render_kickstart(&snap);

    // Hosts additions → %post echo lines
    assert!(
        ks.contains("%post"),
        "kickstart must contain %post for hosts additions"
    );
    assert!(
        ks.contains("192.168.1.100 db.internal"),
        "kickstart must include hosts entry in %post block"
    );
    assert!(
        ks.contains("/etc/hosts"),
        "kickstart must reference /etc/hosts"
    );

    // Static routes → FIXME review comments
    assert!(
        ks.contains("route-eth0"),
        "kickstart must reference the static route file"
    );
    assert!(
        ks.contains("FIXME"),
        "kickstart must include FIXME for static routes"
    );
}

// ---------------------------------------------------------------------------
// Test 17: kickstart_users_override_kickstart
// Users with kickstart strategy produce user commands in kickstart.
// ---------------------------------------------------------------------------

#[test]
fn kickstart_users_override_kickstart() {
    let snap = snapshot_with_kickstart_users();
    let ks = kickstart::render_kickstart(&snap);

    assert!(
        ks.contains("user --name=deploy"),
        "kickstart must contain user --name= for kickstart-strategy users"
    );
    assert!(
        ks.contains("--uid=2000"),
        "kickstart must include --uid for the user"
    );
    assert!(
        ks.contains("--gid=2000"),
        "kickstart must include --gid for the user"
    );
    assert!(
        ks.contains("--shell=/bin/bash"),
        "kickstart must include --shell for the user"
    );
    assert!(
        ks.contains("--homedir=/home/deploy"),
        "kickstart must include --homedir for the user"
    );
}

// ---------------------------------------------------------------------------
// Test 18: readme_container_workload_summary
// Readme findings summary shows "{q} quadlet, {c} compose" row.
// ---------------------------------------------------------------------------

#[test]
fn readme_container_workload_summary() {
    let snap = snapshot_with_quadlets_and_compose();
    let md = readme::render_readme(&snap);

    assert!(
        md.contains("Container workloads"),
        "README must contain Container workloads row"
    );
    assert!(
        md.contains("1 quadlet, 1 compose"),
        "README must show correct quadlet and compose counts"
    );
}

// ---------------------------------------------------------------------------
// Test 19: containerfile_empty_sections
// Empty sections do not crash and produce no extraneous output.
// ---------------------------------------------------------------------------

#[test]
fn containerfile_empty_sections() {
    let mut snap = InspectionSnapshot::new();
    snap.network = Some(NetworkSection::default());
    snap.containers = Some(ContainerSection::default());
    snap.users_groups = Some(UserGroupSection::default());

    // Must not panic
    let output = containerfile::render_containerfile(&snap, None);

    // Empty sections must not produce section headings
    assert!(
        !output.contains("Firewall Configuration"),
        "empty network must not produce Firewall Configuration heading"
    );
    assert!(
        !output.contains("Container Workloads"),
        "empty containers must not produce Container Workloads heading"
    );
    assert!(
        !output.contains("Users and Groups"),
        "empty users must not produce Users and Groups heading"
    );
    assert!(
        !output.contains("Static Routes"),
        "empty network must not produce Static Routes heading"
    );
    assert!(
        !output.contains("Proxy Configuration"),
        "empty network must not produce Proxy Configuration heading"
    );
}

// ---------------------------------------------------------------------------
// Test 20: containerfile_degraded_sections
// Degraded completeness produces FIXME comments for affected sections.
// ---------------------------------------------------------------------------

#[test]
fn containerfile_degraded_sections() {
    let mut snap = InspectionSnapshot::new();
    snap.completeness = Completeness::Partial {
        degraded_sections: vec![
            InspectorId::Network,
            InspectorId::Containers,
            InspectorId::UsersGroups,
        ],
        reason: "test degradation".into(),
    };
    // Add minimal data so sections render
    snap.network = Some(NetworkSection {
        static_routes: vec![StaticRouteFile {
            path: "/etc/sysconfig/network-scripts/route-eth1".into(),
            name: "route-eth1".into(),
        }],
        ..Default::default()
    });

    let output = containerfile::render_containerfile(&snap, None);

    assert!(
        output.contains("FIXME: network data may be incomplete"),
        "degraded network must produce FIXME comment"
    );
    assert!(
        output.contains("FIXME: containers data may be incomplete"),
        "degraded containers must produce FIXME comment"
    );
    assert!(
        output.contains("FIXME: users_groups data may be incomplete"),
        "degraded users_groups must produce FIXME comment"
    );
}
