"""
Tests for inspectors using fixture data. No subprocess or real host required.
"""

from pathlib import Path
from unittest.mock import patch

import pytest

import yoinkc.baseline as baseline_mod
from yoinkc.executor import Executor, RunResult
from yoinkc.inspectors import run_all
from yoinkc.inspectors.rpm import _parse_nevr, _parse_rpm_qa, _parse_rpm_va
from yoinkc.schema import InspectionSnapshot, RpmSection


FIXTURES = Path(__file__).parent / "fixtures"


@pytest.fixture(autouse=True)
def _mock_user_namespace():
    """Pretend we are NOT in a user namespace so nsenter probe runs."""
    with patch.object(baseline_mod, "_in_user_namespace", return_value=False):
        yield


def _fixture_executor(cmd, cwd=None):
    """Executor that returns fixture file content for known commands."""
    if cmd[-1] == "true" and "nsenter" in cmd:
        return RunResult(stdout="", stderr="", returncode=0)
    cmd_str = " ".join(cmd)
    if "podman" in cmd and "rpm" in cmd and "-qa" in cmd:
        return RunResult(stdout=(FIXTURES / "base_image_packages.txt").read_text(), stderr="", returncode=0)
    if "rpm" in cmd and "-qa" in cmd:
        return RunResult(stdout=(FIXTURES / "rpm_qa_output.txt").read_text(), stderr="", returncode=0)
    if "rpm" in cmd and "-Va" in cmd:
        return RunResult(stdout=(FIXTURES / "rpm_va_output.txt").read_text(), stderr="", returncode=0)
    if "dnf" in cmd and "history" in cmd and "list" in cmd:
        return RunResult(stdout=(FIXTURES / "dnf_history_list.txt").read_text(), stderr="", returncode=0)
    if "dnf" in cmd and "history" in cmd and "info" in cmd and "4" in cmd:
        return RunResult(stdout=(FIXTURES / "dnf_history_info_4.txt").read_text(), stderr="", returncode=0)
    if "rpm" in cmd and "-ql" in cmd:
        return RunResult(stdout=(FIXTURES / "rpm_qla_output.txt").read_text(), stderr="", returncode=0)
    if "systemctl" in cmd and "list-unit-files" in cmd:
        return RunResult(stdout=(FIXTURES / "systemctl_list_unit_files.txt").read_text(), stderr="", returncode=0)
    if "semodule" in cmd and "-l" in cmd:
        return RunResult(stdout=(FIXTURES / "semodule_l_output.txt").read_text(), stderr="", returncode=0)
    if "semanage" in cmd and "boolean" in cmd:
        return RunResult(stdout=(FIXTURES / "semanage_boolean_l_output.txt").read_text(), stderr="", returncode=0)
    if "lsmod" in cmd:
        return RunResult(stdout=(FIXTURES / "lsmod_output.txt").read_text(), stderr="", returncode=0)
    if "ip" in cmd and "route" in cmd:
        return RunResult(stdout=(FIXTURES / "ip_route_output.txt").read_text(), stderr="", returncode=0)
    if "ip" in cmd and "rule" in cmd:
        return RunResult(stdout=(FIXTURES / "ip_rule_output.txt").read_text(), stderr="", returncode=0)
    if "podman" in cmd and "ps" in cmd:
        return RunResult(stdout=(FIXTURES / "podman_ps_output.json").read_text(), stderr="", returncode=0)
    if "podman" in cmd and "inspect" in cmd:
        return RunResult(stdout=(FIXTURES / "podman_inspect_output.json").read_text(), stderr="", returncode=0)
    if "readelf" in cmd and "-S" in cmd:
        if "go-server" in cmd_str:
            return RunResult(stdout=(FIXTURES / "readelf_go_sections.txt").read_text(), stderr="", returncode=0)
        if "rust-worker" in cmd_str:
            return RunResult(stdout=(FIXTURES / "readelf_rust_sections.txt").read_text(), stderr="", returncode=0)
        return RunResult(stdout="", stderr="not an ELF", returncode=1)
    if "readelf" in cmd and "-d" in cmd:
        if "go-server" in cmd_str:
            return RunResult(stdout=(FIXTURES / "readelf_go_dynamic.txt").read_text(), stderr="", returncode=0)
        if "rust-worker" in cmd_str:
            return RunResult(stdout=(FIXTURES / "readelf_rust_dynamic.txt").read_text(), stderr="", returncode=0)
        return RunResult(stdout="", stderr="not an ELF", returncode=1)
    if "file" in cmd and "-b" in cmd:
        if "go-server" in cmd_str or "rust-worker" in cmd_str:
            return RunResult(stdout="ELF 64-bit LSB executable", stderr="", returncode=0)
        return RunResult(stdout="ASCII text", stderr="", returncode=0)
    if "pip" in cmd and "list" in cmd and "--path" in cmd:
        if "webapp" in cmd_str:
            return RunResult(stdout=(FIXTURES / "pip_list_webapp.txt").read_text(), stderr="", returncode=0)
        if "analytics" in cmd_str:
            return RunResult(stdout=(FIXTURES / "pip_list_analytics.txt").read_text(), stderr="", returncode=0)
        return RunResult(stdout="", stderr="", returncode=1)
    return RunResult(stdout="", stderr="unknown command", returncode=1)


@pytest.fixture
def fixture_executor() -> Executor:
    return _fixture_executor


@pytest.fixture
def host_root() -> Path:
    return FIXTURES / "host_etc"


def test_parse_nevr():
    p = _parse_nevr("0:bash-5.2.15-2.el9.x86_64")
    assert p is not None
    assert p.name == "bash"
    assert p.version == "5.2.15"
    assert p.release == "2.el9"
    assert p.arch == "x86_64"

    # (none) epoch — most packages on RHEL/CentOS have no explicit epoch
    p2 = _parse_nevr("(none):coreutils-8.32-35.el9.aarch64")
    assert p2 is not None
    assert p2.name == "coreutils"
    assert p2.epoch == "0"
    assert p2.version == "8.32"
    assert p2.release == "35.el9"
    assert p2.arch == "aarch64"


def test_parse_rpm_qa():
    text = (FIXTURES / "rpm_qa_output.txt").read_text()
    packages = _parse_rpm_qa(text)
    assert len(packages) >= 30
    names = [p.name for p in packages]
    assert "bash" in names
    assert "httpd" in names
    # (none) epoch packages must be parsed
    assert "dnf" in names
    assert "rpm" in names
    assert "sudo" in names


def test_parse_rpm_va():
    text = (FIXTURES / "rpm_va_output.txt").read_text()
    entries = _parse_rpm_va(text)
    assert len(entries) == 5
    paths = [e.path for e in entries]
    assert "/etc/httpd/conf/httpd.conf" in paths
    assert "/etc/ssh/sshd_config" in paths


def test_rpm_inspector_with_fixtures(host_root, fixture_executor):
    """With executor that can query base image, baseline is applied via podman."""
    from yoinkc.inspectors.rpm import run as run_rpm
    section = run_rpm(host_root, fixture_executor)
    assert section is not None
    assert section.no_baseline is False
    assert section.baseline_package_names is not None
    assert "bash" in section.baseline_package_names
    added_names = [p.name for p in section.packages_added]
    assert "httpd" in added_names
    assert "bash" not in added_names
    assert len(section.rpm_va) == 5
    assert len(section.repo_files) >= 1
    assert "old-daemon" in section.dnf_history_removed


def test_rpm_inspector_with_baseline_file(host_root, fixture_executor):
    """With --baseline-packages, baseline is loaded from file."""
    from yoinkc.inspectors.rpm import run as run_rpm
    baseline_file = FIXTURES / "base_image_packages.txt"
    section = run_rpm(host_root, fixture_executor, baseline_packages_file=baseline_file)
    assert section is not None
    assert section.no_baseline is False
    assert section.baseline_package_names is not None
    assert "acl" in section.baseline_package_names
    assert "bash" in section.baseline_package_names
    added_names = [p.name for p in section.packages_added]
    assert "httpd" in added_names
    assert "acl" not in added_names
    assert "bash" not in added_names


def test_service_inspector_with_fixtures(host_root, fixture_executor):
    from yoinkc.inspectors.service import run as run_service
    section = run_service(host_root, fixture_executor)
    assert section is not None
    assert any(s.unit == "httpd.service" and s.action == "enable" for s in section.state_changes)
    assert "httpd.service" in section.enabled_units


def test_config_inspector_with_fixtures(host_root, fixture_executor):
    from yoinkc.inspectors.config import run as run_config
    from yoinkc.inspectors.rpm import run as run_rpm
    rpm_section = run_rpm(host_root, fixture_executor)
    rpm_owned = set((FIXTURES / "rpm_qla_output.txt").read_text().strip().splitlines())
    section = run_config(host_root, fixture_executor, rpm_section=rpm_section, rpm_owned_paths_override=rpm_owned)
    assert section is not None
    modified = [f for f in section.files if f.kind.value == "rpm_owned_modified"]
    assert len(modified) >= 2  # httpd.conf, sshd_config at least
    assert any("/etc/httpd/conf/httpd.conf" == f.path for f in modified)


def test_network_inspector_with_fixtures(host_root, fixture_executor):
    from yoinkc.inspectors.network import run as run_network
    section = run_network(host_root, fixture_executor)
    assert section is not None

    # --- Connection classification ---
    assert len(section.connections) >= 2
    conn_map = {c.name: c for c in section.connections}
    assert "eth0" in conn_map
    assert conn_map["eth0"].method == "dhcp"
    assert conn_map["eth0"].type == "802-3-ethernet"
    assert "mgmt0" in conn_map
    assert conn_map["mgmt0"].method == "static"

    # --- Firewall zones with rich rules ---
    assert len(section.firewall_zones) >= 2
    zone_map = {z.name: z for z in section.firewall_zones}
    pub = zone_map["public"]
    assert "ssh" in pub.services
    assert "8080/tcp" in pub.ports
    assert len(pub.rich_rules) == 2
    assert any("192.168.1.0/24" in r for r in pub.rich_rules)

    internal = zone_map["internal"]
    assert "mdns" in internal.services
    assert len(internal.rich_rules) == 1

    # --- Firewall direct rules ---
    assert len(section.firewall_direct_rules) == 2
    assert any("9090" in r.args for r in section.firewall_direct_rules)
    assert section.firewall_direct_rules[0].chain == "INPUT"

    # --- resolv.conf provenance ---
    assert section.resolv_provenance == "networkmanager"

    # --- ip route ---
    assert len(section.ip_routes) >= 3
    assert any("default via" in r for r in section.ip_routes)
    assert any("proto static" in r for r in section.ip_routes)

    # --- ip rule (non-default only) ---
    assert len(section.ip_rules) >= 1
    assert any("custom_table" in r for r in section.ip_rules)
    # Default tables (local, main, default) should be filtered out
    assert not any("lookup local" in r for r in section.ip_rules)
    assert not any("lookup main" in r for r in section.ip_rules)


def test_storage_inspector_with_fixtures(host_root, fixture_executor):
    from yoinkc.inspectors.storage import run as run_storage
    section = run_storage(host_root, fixture_executor)
    assert section is not None
    assert len(section.fstab_entries) >= 1
    assert any(e.mount_point == "/" for e in section.fstab_entries)


def test_scheduled_tasks_inspector_with_fixtures(host_root, fixture_executor):
    from yoinkc.inspectors.scheduled_tasks import run as run_scheduled_tasks
    section = run_scheduled_tasks(host_root, fixture_executor)
    assert section is not None

    # Cron jobs still detected
    assert any(j.path.endswith("hourly-job") for j in section.cron_jobs)
    assert len(section.generated_timer_units) >= 1
    assert "OnCalendar" in section.generated_timer_units[0].timer_content

    # Existing systemd timers scanned from /etc and /usr/lib
    timer_names = [t.name for t in section.systemd_timers]
    assert "certbot-renew" in timer_names, f"expected certbot-renew, got {timer_names}"
    assert "fstrim" in timer_names, f"expected fstrim, got {timer_names}"

    certbot = next(t for t in section.systemd_timers if t.name == "certbot-renew")
    assert certbot.source == "local"
    assert "00,12:00:00" in certbot.on_calendar
    assert "/usr/bin/certbot" in certbot.exec_start

    fstrim = next(t for t in section.systemd_timers if t.name == "fstrim")
    assert fstrim.source == "vendor"
    assert fstrim.on_calendar == "weekly"

    # At jobs parsed with command and user
    assert len(section.at_jobs) >= 1
    at_job = section.at_jobs[0]
    assert "cleanup-temp" in at_job.command
    assert at_job.user == "root"


def test_container_inspector_with_fixtures(host_root, fixture_executor):
    from yoinkc.inspectors.container import run as run_container

    # Without podman query
    section = run_container(host_root, fixture_executor, query_podman=False)
    assert section is not None

    # Quadlet units with image references (system + user-level)
    assert len(section.quadlet_units) >= 3
    unit_map = {u.name: u for u in section.quadlet_units}
    assert "nginx.container" in unit_map
    assert unit_map["nginx.container"].image == "docker.io/library/nginx:1.25-alpine"
    assert "redis.container" in unit_map
    assert unit_map["redis.container"].image == "registry.redhat.io/rhel9/redis-6:latest"
    assert unit_map["nginx.container"].content.strip().startswith("[Unit]")

    # User-level quadlet from ~/.config/containers/systemd/
    assert "dev-postgres.container" in unit_map
    assert unit_map["dev-postgres.container"].image == "docker.io/library/postgres:16"
    assert "home/jdoe" in unit_map["dev-postgres.container"].path

    # Compose files with parsed image references
    assert len(section.compose_files) >= 1
    compose = section.compose_files[0]
    assert "docker-compose" in compose.path
    svc_images = {img.service: img.image for img in compose.images}
    assert "web" in svc_images
    assert "python:3.11-slim" in svc_images["web"]
    assert "db" in svc_images
    assert "postgres:16-alpine" in svc_images["db"]

    # No running containers without query_podman
    assert len(section.running_containers) == 0

    # With podman query
    section2 = run_container(host_root, fixture_executor, query_podman=True)
    assert len(section2.running_containers) >= 2
    rc_map = {c.name: c for c in section2.running_containers}
    assert "nginx-proxy" in rc_map
    nginx = rc_map["nginx-proxy"]
    assert nginx.image == "docker.io/library/nginx:1.25-alpine"
    assert len(nginx.mounts) >= 1
    assert nginx.mounts[0].destination == "/usr/share/nginx/html"
    assert "podman" in nginx.networks
    assert nginx.networks["podman"]["ip"] == "10.88.0.2"
    assert len(nginx.env) >= 1

    redis = rc_map["redis-cache"]
    assert redis.image == "registry.redhat.io/rhel9/redis-6:latest"
    assert any("REDIS_PASSWORD" in e for e in redis.env)


def test_non_rpm_software_inspector_with_fixtures(host_root, fixture_executor):
    from yoinkc.inspectors.non_rpm_software import run as run_non_rpm_software
    section = run_non_rpm_software(host_root, fixture_executor, deep_binary_scan=False)
    assert section is not None

    item_map = {i.get("path", ""): i for i in section.items}
    methods = {i.get("method") for i in section.items}

    # Unknown provenance (directory scan)
    assert any(i.get("name") == "dummy" for i in section.items)

    # pip dist-info with version (system-level)
    pip_items = [i for i in section.items if i.get("method") == "pip dist-info"]
    assert len(pip_items) >= 2
    flask = next((i for i in pip_items if i["name"] == "flask"), None)
    assert flask is not None and flask["version"] == "2.3.2"
    requests_ = next((i for i in pip_items if i["name"] == "requests"), None)
    assert requests_ is not None and requests_["version"] == "2.31.0"

    # npm with lockfile content
    npm_items = [i for i in section.items if i.get("method") == "npm package-lock.json"]
    assert len(npm_items) >= 1
    assert npm_items[0]["name"] == "myapp"
    assert "package-lock.json" in npm_items[0].get("files", {})

    # readelf: Go binary
    go_items = [i for i in section.items if i.get("lang") == "go"]
    assert len(go_items) >= 1, f"Expected Go binary, methods: {methods}"
    go_item = go_items[0]
    assert go_item["method"] == "readelf (go)"
    assert go_item["confidence"] == "high"
    assert go_item["static"] is True

    # readelf: Rust binary
    rust_items = [i for i in section.items if i.get("lang") == "rust"]
    assert len(rust_items) >= 1, f"Expected Rust binary, methods: {methods}"
    rust_item = rust_items[0]
    assert rust_item["method"] == "readelf (rust)"
    assert rust_item["static"] is False
    assert any("libc" in lib for lib in rust_item.get("shared_libs", []))

    # Venv without system-site-packages
    venv_items = [i for i in section.items if i.get("method") == "python venv"]
    assert len(venv_items) >= 2, f"Expected 2 venvs, got {len(venv_items)}"
    webapp_venv = next((i for i in venv_items if "webapp" in i["path"]), None)
    assert webapp_venv is not None
    assert webapp_venv["system_site_packages"] is False
    assert len(webapp_venv["packages"]) >= 2
    pkg_names = {p["name"] for p in webapp_venv["packages"]}
    assert "Django" in pkg_names or "django" in pkg_names.union({n.lower() for n in pkg_names})

    # Venv with system-site-packages
    analytics_venv = next((i for i in venv_items if "analytics" in i["path"]), None)
    assert analytics_venv is not None
    assert analytics_venv["system_site_packages"] is True
    assert len(analytics_venv["packages"]) >= 1

    # Git-managed directory
    git_items = [i for i in section.items if i.get("method") == "git repository"]
    assert len(git_items) >= 1, f"Expected git repo, methods: {methods}"
    git_item = git_items[0]
    assert "custom-tool" in git_item["name"]
    assert git_item["git_remote"] == "https://github.com/example/custom-tool.git"
    assert len(git_item["git_commit"]) >= 10
    assert git_item["git_branch"] == "main"


def test_kernel_boot_inspector_with_fixtures(host_root, fixture_executor):
    from yoinkc.inspectors.kernel_boot import run as run_kernel_boot
    section = run_kernel_boot(host_root, fixture_executor)
    assert section is not None
    assert section.cmdline != ""
    assert "root=" in section.cmdline
    assert section.grub_defaults != ""
    assert "GRUB_CMDLINE_LINUX" in section.grub_defaults

    # --- lsmod ---
    assert len(section.loaded_modules) > 0
    loaded_names = {m["name"] for m in section.loaded_modules}
    assert "br_netfilter" in loaded_names
    assert "virtio_net" in loaded_names
    assert "wireguard" in loaded_names

    # --- non-default modules ---
    # virtio_net / virtio_blk: in usr/lib/modules-load.d/virtio.conf → default
    # bonding: in etc/modules-load.d/bonding.conf → explicitly configured
    # bridge/stp/llc/nf_conntrack/nf_defrag_*/fat/mbcache/jbd2: have used_by → dependencies
    # br_netfilter, overlay, ip_tables, vfat, ext4, wireguard: non-default
    nd_names = {m["name"] for m in section.non_default_modules}
    assert "virtio_net" not in nd_names, "virtio_net is in modules-load.d defaults"
    assert "virtio_blk" not in nd_names, "virtio_blk is in modules-load.d defaults"
    assert "bonding" not in nd_names, "bonding is explicitly configured"
    assert "bridge" not in nd_names, "bridge has used_by → dependency"
    assert "nf_conntrack" not in nd_names, "nf_conntrack has used_by → dependency"
    assert "fat" not in nd_names, "fat has used_by → dependency"
    assert "jbd2" not in nd_names, "jbd2 has used_by → dependency"
    assert "wireguard" in nd_names, "wireguard: not configured, no dependents"
    assert "overlay" in nd_names, "overlay: not configured, no dependents"
    assert "ip_tables" in nd_names, "ip_tables: not configured, no dependents"
    assert "br_netfilter" in nd_names, "br_netfilter: not configured, no dependents"
    assert "ext4" in nd_names, "ext4: not configured, no dependents"
    assert "vfat" in nd_names, "vfat: not configured, no dependents"

    # --- sysctl overrides (only non-default) ---
    sysctl_keys = {s["key"] for s in section.sysctl_overrides}
    assert "net.ipv4.ip_forward" in sysctl_keys, "ip_forward differs from default"
    assert "vm.swappiness" in sysctl_keys, "swappiness differs from default"
    # kernel.panic is 0 in both default and runtime → should NOT appear
    assert "kernel.panic" not in sysctl_keys, "kernel.panic is at default value"
    # net.ipv6.conf.all.forwarding is 0 in both → should NOT appear
    assert "net.ipv6.conf.all.forwarding" not in sysctl_keys

    # Check values
    ip_fwd = next(s for s in section.sysctl_overrides if s["key"] == "net.ipv4.ip_forward")
    assert ip_fwd["runtime"] == "1"
    assert ip_fwd["default"] == "0"
    swap = next(s for s in section.sysctl_overrides if s["key"] == "vm.swappiness")
    assert swap["runtime"] == "10"
    assert swap["default"] == "30"


def test_selinux_inspector_with_fixtures(host_root, fixture_executor):
    from yoinkc.inspectors.selinux import run as run_selinux
    section = run_selinux(host_root, fixture_executor)
    assert section is not None
    assert section.mode == "enforcing"
    assert any("99-foo" in p for p in section.audit_rules)

    # Custom modules: myapp is in priority 400 store
    assert "myapp" in section.custom_modules
    # Base modules should NOT appear in custom_modules
    assert "abrt" not in section.custom_modules

    # Boolean overrides from semanage boolean -l
    assert len(section.boolean_overrides) > 0
    names = {b["name"] for b in section.boolean_overrides}
    # Non-default booleans should be present
    assert "httpd_can_network_connect" in names
    assert "httpd_use_nfs" in names
    assert "virt_sandbox_use_all_caps" in names

    # Non-default booleans carry current/default values
    httpd_net = next(b for b in section.boolean_overrides if b["name"] == "httpd_can_network_connect")
    assert httpd_net["current"] == "on"
    assert httpd_net["default"] == "off"
    assert httpd_net["non_default"] is True

    # Unchanged booleans should also be in the list (for completeness) but marked non_default=False
    httpd_cgi = next(b for b in section.boolean_overrides if b["name"] == "httpd_enable_cgi")
    assert httpd_cgi["non_default"] is False


def test_users_groups_inspector_with_fixtures(host_root, fixture_executor):
    from yoinkc.inspectors.users_groups import run as run_users_groups
    section = run_users_groups(host_root, fixture_executor)
    assert section is not None
    assert any(u.get("name") == "jdoe" and u.get("uid") == 1000 for u in section.users)
    assert any(g.get("name") == "jdoe" and g.get("gid") == 1000 for g in section.groups)

    # Raw entry capture
    assert len(section.passwd_entries) >= 1
    assert any("jdoe" in e for e in section.passwd_entries)
    assert len(section.shadow_entries) >= 1
    assert any("jdoe" in e for e in section.shadow_entries)
    assert len(section.group_entries) >= 1
    assert any("jdoe" in e for e in section.group_entries)
    assert len(section.gshadow_entries) >= 1
    assert any("jdoe" in e for e in section.gshadow_entries)
    assert len(section.subuid_entries) >= 1
    assert any("jdoe" in e for e in section.subuid_entries)
    assert len(section.subgid_entries) >= 1
    assert any("jdoe" in e for e in section.subgid_entries)

    # System accounts excluded from raw entries
    assert not any("root" in e for e in section.passwd_entries)
    assert not any("nobody" in e for e in section.passwd_entries)


def test_run_all_with_fixtures(host_root, fixture_executor):
    """Full run with base image query → baseline applied, all inspectors run."""
    snapshot = run_all(
        host_root,
        executor=fixture_executor,
        config_diffs=False,
        deep_binary_scan=False,
        query_podman=False,
    )
    assert isinstance(snapshot, InspectionSnapshot)
    assert snapshot.os_release is not None
    assert snapshot.os_release.name == "Red Hat Enterprise Linux"
    assert snapshot.rpm is not None
    assert snapshot.rpm.no_baseline is False
    assert snapshot.rpm.baseline_package_names is not None
    assert len(snapshot.rpm.packages_added) > 0
    added_names = [p.name for p in snapshot.rpm.packages_added]
    assert "httpd" in added_names
    assert "bash" not in added_names
    assert snapshot.services is not None
    assert snapshot.config is not None
    assert snapshot.network is not None
    assert snapshot.storage is not None
    assert snapshot.scheduled_tasks is not None
    assert snapshot.containers is not None
    assert snapshot.non_rpm_software is not None
    assert snapshot.kernel_boot is not None
    assert snapshot.selinux is not None
    assert snapshot.users_groups is not None


def test_run_all_no_baseline_warning(host_root):
    """When podman fails, no-baseline warning is produced and tool continues."""
    def failing_executor(cmd, cwd=None):
        cmd_str = " ".join(cmd)
        if "podman" in cmd:
            return RunResult(stdout="", stderr="not available", returncode=127)
        if "rpm" in cmd and "-qa" in cmd:
            return RunResult(stdout=(FIXTURES / "rpm_qa_output.txt").read_text(), stderr="", returncode=0)
        if "rpm" in cmd and "-Va" in cmd:
            return RunResult(stdout=(FIXTURES / "rpm_va_output.txt").read_text(), stderr="", returncode=0)
        if "rpm" in cmd and "-ql" in cmd:
            return RunResult(stdout=(FIXTURES / "rpm_qla_output.txt").read_text(), stderr="", returncode=0)
        if "systemctl" in cmd:
            return RunResult(stdout=(FIXTURES / "systemctl_list_unit_files.txt").read_text(), stderr="", returncode=0)
        return RunResult(stdout="", stderr="", returncode=1)

    snapshot = run_all(host_root, executor=failing_executor)
    assert snapshot.rpm is not None
    assert snapshot.rpm.no_baseline is True
    rpm_warnings = [w for w in snapshot.warnings if w.get("source") == "rpm"]
    assert any("base image" in w.get("message", "").lower() for w in rpm_warnings)


def test_snapshot_roundtrip_with_baseline(host_root, fixture_executor):
    """Resolved baseline is in inspection-snapshot.json; --from-snapshot re-renders without network."""
    import tempfile
    from yoinkc.pipeline import load_snapshot, save_snapshot
    from yoinkc.renderers import run_all as run_all_renderers
    snapshot = run_all(
        host_root,
        executor=fixture_executor,
        config_diffs=False,
        deep_binary_scan=False,
        query_podman=False,
    )
    assert snapshot.rpm is not None
    assert snapshot.rpm.no_baseline is False
    assert snapshot.rpm.baseline_package_names is not None
    with tempfile.TemporaryDirectory() as tmp:
        out = Path(tmp)
        path = out / "inspection-snapshot.json"
        save_snapshot(snapshot, path)
        loaded = load_snapshot(path)
        assert loaded.rpm is not None
        assert loaded.rpm.baseline_package_names is not None
        assert loaded.rpm.no_baseline is False
        run_all_renderers(loaded, out)
        assert (out / "Containerfile").exists()
        assert (out / "audit-report.md").exists()
        assert (out / "report.html").exists()
