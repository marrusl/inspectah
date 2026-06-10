//! Integration tests for NetworkInspector.
//!
//! Runs the actual Rust inspector on fixture data via MockExecutor and
//! verifies output is structurally correct. Follows the same pattern as
//! parity_test.rs (Slice 2a inspectors).

use inspectah_collect::executor::mock::MockExecutor;
use inspectah_collect::inspectors::network::NetworkInspector;
use inspectah_core::traits::executor::ExecResult;
use inspectah_core::traits::inspector::{InspectionContext, Inspector, InspectorError};
use inspectah_core::traits::progress::NullProgress;
use inspectah_core::types::completeness::SectionData;
use inspectah_core::types::network::NetworkSection;
use inspectah_core::types::os::OsRelease;
use inspectah_core::types::system::SourceSystem;

// ── Shared helpers ──────────────────────────────────────────────────

fn pkg_source() -> SourceSystem {
    SourceSystem::PackageBased {
        os_release: OsRelease {
            name: "Red Hat Enterprise Linux".into(),
            version_id: "9.4".into(),
            id: "rhel".into(),
            ..Default::default()
        },
    }
}

// ── Fixtures ────────────────────────────────────────────────────────

const NM_CONN_FIXTURE: &str = include_str!("../../../testdata/fixtures/network/eth0.nmconnection");
const PUBLIC_ZONE_FIXTURE: &str =
    include_str!("../../../testdata/fixtures/network/public-zone.xml");
const DIRECT_FIXTURE: &str = include_str!("../../../testdata/fixtures/network/direct.xml");
const HOSTS_FIXTURE: &str = include_str!("../../../testdata/fixtures/network/hosts");
const RESOLV_FIXTURE: &str = include_str!("../../../testdata/fixtures/network/resolv-nm.conf");
const PROXY_ENV_FIXTURE: &str =
    include_str!("../../../testdata/fixtures/network/proxy-environment");
const DNF_PROXY_FIXTURE: &str = include_str!("../../../testdata/fixtures/network/dnf-proxy.conf");
const IP_ROUTE_FIXTURE: &str = include_str!("../../../testdata/fixtures/network/ip-route.txt");
const IP_RULE_FIXTURE: &str = include_str!("../../../testdata/fixtures/network/ip-rule.txt");

// ── Mock builder ────────────────────────────────────────────────────

fn network_happy_mock() -> MockExecutor {
    MockExecutor::new()
        // NM connections
        .with_dir(
            "/etc/NetworkManager/system-connections",
            vec!["eth0.nmconnection"],
        )
        .with_file(
            "/etc/NetworkManager/system-connections/eth0.nmconnection",
            NM_CONN_FIXTURE,
        )
        // Firewall zones
        .with_dir("/etc/firewalld/zones", vec!["public.xml"])
        .with_file("/etc/firewalld/zones/public.xml", PUBLIC_ZONE_FIXTURE)
        // Firewall direct rules
        .with_file("/etc/firewalld/direct.xml", DIRECT_FIXTURE)
        // resolv.conf
        .with_file("/etc/resolv.conf", RESOLV_FIXTURE)
        // /etc/hosts
        .with_file("/etc/hosts", HOSTS_FIXTURE)
        // Static routes (network-scripts dir)
        .with_dir("/etc/sysconfig/network-scripts", vec!["route-eth0"])
        .with_file(
            "/etc/sysconfig/network-scripts/route-eth0",
            "10.0.0.0/8 via 192.168.1.1\n",
        )
        // NM connection dir for static routes (empty)
        .with_dir(
            "/etc/NetworkManager/system-connections",
            vec!["eth0.nmconnection"],
        )
        // ip route / ip rule
        .with_command(
            "ip route",
            ExecResult {
                stdout: IP_ROUTE_FIXTURE.into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_command(
            "ip rule",
            ExecResult {
                stdout: IP_RULE_FIXTURE.into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        // Proxy environment
        .with_file("/etc/environment", PROXY_ENV_FIXTURE)
        // DNF proxy
        .with_file("/etc/dnf/dnf.conf", DNF_PROXY_FIXTURE)
}

// ── Tests ───────────────────────────────────────────────────────────

/// Happy path: all sub-collectors produce data.
#[test]
fn test_network_inspector_happy_path() {
    let exec = network_happy_mock();
    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
        baseline_data: None,
    };

    let result = NetworkInspector::new().inspect(&ctx, &NullProgress);

    // The DNF proxy fixture contains proxy_password, which emits a redaction
    // hint, and that is fine. Extract the section regardless of Ok/Degraded.
    let output = match result {
        Ok(o) => o,
        Err(InspectorError::Degraded { partial, .. }) => *partial,
        Err(e) => panic!("unexpected error: {e}"),
    };

    let section = match &output.section {
        SectionData::Network(s) => s,
        other => panic!("expected SectionData::Network, got {:?}", other),
    };

    // NM connections
    assert!(
        !section.connections.is_empty(),
        "inspector must find NM connections from fixture"
    );
    assert_eq!(section.connections[0].name, "eth0");

    // Firewall zones
    assert!(
        !section.firewall_zones.is_empty(),
        "inspector must find firewall zones from fixture"
    );
    assert_eq!(section.firewall_zones[0].name, "public");
    assert!(
        !section.firewall_zones[0].services.is_empty(),
        "public zone must have services"
    );

    // Firewall direct rules
    assert!(
        !section.firewall_direct_rules.is_empty(),
        "inspector must find firewall direct rules from fixture"
    );

    // resolv.conf provenance
    assert_eq!(
        section.resolv_provenance, "networkmanager",
        "resolv.conf fixture is NM-managed"
    );

    // Hosts additions
    assert!(
        !section.hosts_additions.is_empty(),
        "inspector must find non-localhost hosts additions"
    );

    // Static routes
    assert!(
        !section.static_routes.is_empty(),
        "inspector must find static route files"
    );

    // ip routes
    assert!(
        !section.ip_routes.is_empty(),
        "inspector must capture ip route output"
    );

    // Proxy entries
    assert!(
        !section.proxy.is_empty(),
        "inspector must find proxy entries from environment and dnf.conf"
    );
}

/// No NM directory: inspector still succeeds with empty connections.
#[test]
fn test_network_inspector_nm_not_found() {
    // Minimal mock: no NM dir, no firewall, no hosts, no proxy.
    // ip route/rule return empty.
    let exec = MockExecutor::new()
        .with_command(
            "ip route",
            ExecResult {
                stdout: String::new(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_command(
            "ip rule",
            ExecResult {
                stdout: String::new(),
                exit_code: 0,
                ..Default::default()
            },
        );

    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
        baseline_data: None,
    };

    let output = NetworkInspector::new()
        .inspect(&ctx, &NullProgress)
        .expect("inspector should succeed when NM is not installed");

    let section = match &output.section {
        SectionData::Network(s) => s,
        other => panic!("expected SectionData::Network, got {:?}", other),
    };

    assert!(
        section.connections.is_empty(),
        "no NM dir means no connections"
    );
    assert!(
        section.firewall_zones.is_empty(),
        "no firewalld dir means no zones"
    );
}

/// PermissionDenied on NM connections dir -> Degraded output.
#[test]
fn test_network_inspector_degraded_permissions() {
    let exec = MockExecutor::new()
        .with_dir_error(
            "/etc/NetworkManager/system-connections",
            std::io::ErrorKind::PermissionDenied,
        )
        .with_dir_error("/etc/firewalld/zones", std::io::ErrorKind::PermissionDenied)
        .with_command(
            "ip route",
            ExecResult {
                stdout: String::new(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_command(
            "ip rule",
            ExecResult {
                stdout: String::new(),
                exit_code: 0,
                ..Default::default()
            },
        );

    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
        baseline_data: None,
    };

    let result = NetworkInspector::new().inspect(&ctx, &NullProgress);

    match result {
        Err(InspectorError::Degraded { partial, reason }) => {
            assert!(
                reason.contains("Permission denied"),
                "degraded reason should mention permission denied, got: {reason}"
            );
            assert!(
                matches!(partial.section, SectionData::Network(_)),
                "partial output should still contain a NetworkSection"
            );
        }
        Ok(_) => panic!("expected Degraded error when NM dir is PermissionDenied"),
        Err(e) => panic!("expected Degraded, got: {e}"),
    }
}

/// Output round-trips through NetworkSection type.
#[test]
fn test_network_inspector_json_roundtrip() {
    let exec = network_happy_mock();
    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
        baseline_data: None,
    };

    let result = NetworkInspector::new().inspect(&ctx, &NullProgress);

    let output = match result {
        Ok(o) => o,
        Err(InspectorError::Degraded { partial, .. }) => *partial,
        Err(e) => panic!("unexpected error: {e}"),
    };

    let section = match &output.section {
        SectionData::Network(s) => s,
        other => panic!("expected SectionData::Network, got {:?}", other),
    };

    let rust_json = serde_json::to_string_pretty(section).unwrap();
    let roundtrip: NetworkSection =
        serde_json::from_str(&rust_json).expect("inspector output must be valid JSON");
    let roundtrip_json = serde_json::to_string_pretty(&roundtrip).unwrap();
    assert_eq!(
        rust_json, roundtrip_json,
        "inspector output must round-trip faithfully through NetworkSection"
    );
}
