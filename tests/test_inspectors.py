"""
Tests for inspectors using fixture data. No subprocess or real host required.
"""

from pathlib import Path
from unittest.mock import patch

import pytest

import yoinkc.preflight as preflight_mod
from yoinkc.executor import Executor, RunResult
from yoinkc.inspectors import run_all
from yoinkc.inspectors.rpm import _parse_nevr, _parse_rpm_qa, _parse_rpm_va
from yoinkc.schema import InspectionSnapshot, RpmSection


FIXTURES = Path(__file__).parent / "fixtures"


@pytest.fixture(autouse=True)
def _mock_user_namespace():
    """Pretend we are NOT in a user namespace so nsenter probe runs."""
    with patch.object(preflight_mod, "in_user_namespace", return_value=False):
        yield


def _fixture_executor(cmd, cwd=None):
    """Executor that returns fixture file content for known commands."""
    if cmd[-1] == "true" and "nsenter" in cmd:
        return RunResult(stdout="", stderr="", returncode=0)
    cmd_str = " ".join(cmd)
    if "podman" in cmd and "login" in cmd and "--get-login" in cmd:
        return RunResult(stdout="testuser\n", stderr="", returncode=0)
    if "podman" in cmd and "rpm" in cmd and "-qa" in cmd:
        return RunResult(stdout=(FIXTURES / "base_image_packages.txt").read_text(), stderr="", returncode=0)
    if "rpm" in cmd and "-qa" in cmd:
        return RunResult(stdout=(FIXTURES / "rpm_qa_output.txt").read_text(), stderr="", returncode=0)
    if "rpm" in cmd and "-Va" in cmd:
        return RunResult(stdout=(FIXTURES / "rpm_va_output.txt").read_text(), stderr="", returncode=0)
    if "dnf" in cmd and "repoquery" in cmd and "--userinstalled" in cmd:
        return RunResult(stdout="httpd\nrsync\n", stderr="", returncode=0)
    if "dnf" in cmd and "repoquery" in cmd and "--installed" in cmd and "--requires" not in cmd:
        repo_output = "\n".join([
            "httpd baseos",
            "nginx appstream",
            "htop epel",
            "bat epel",
        ])
        return RunResult(stdout=repo_output, stderr="", returncode=0)
    if "dnf" in cmd and "history" in cmd and "list" in cmd:
        return RunResult(stdout=(FIXTURES / "dnf_history_list.txt").read_text(), stderr="", returncode=0)
    if "dnf" in cmd and "history" in cmd and "info" in cmd and "4" in cmd:
        return RunResult(stdout=(FIXTURES / "dnf_history_info_4.txt").read_text(), stderr="", returncode=0)
    if "rpm" in cmd and "-ql" in cmd:
        return RunResult(stdout=(FIXTURES / "rpm_qla_output.txt").read_text(), stderr="", returncode=0)
    if "rpm" in cmd and "-qf" in cmd:
        assert "--root" not in cmd, (
            f"rpm -qf must use --dbpath, not --root (chroot fails in containers); got: {cmd}"
        )
        # Collect path args — skip flag values like the --dbpath argument.
        path_args = []
        skip_next = False
        for a in cmd:
            if skip_next:
                skip_next = False
                continue
            if a in ("--dbpath", "--root", "--queryformat"):
                skip_next = True
                continue
            if a.startswith("/"):
                path_args.append(a)
        _KNOWN_OWNERS = {"httpd.service": "httpd"}
        names = [_KNOWN_OWNERS.get(Path(p).name, "") for p in path_args]
        if any(names):
            return RunResult(stdout="\n".join(names) + "\n", stderr="", returncode=0)
        return RunResult(
            stdout="",
            stderr=f"file {path_args[-1] if path_args else '?'} is not owned by any package",
            returncode=1,
        )
    if "systemctl" in cmd and "list-unit-files" in cmd:
        return RunResult(stdout=(FIXTURES / "systemctl_list_unit_files.txt").read_text(), stderr="", returncode=0)
    if "semodule" in cmd and "-l" in cmd:
        return RunResult(stdout=(FIXTURES / "semodule_l_output.txt").read_text(), stderr="", returncode=0)
    if "semanage" in cmd and "boolean" in cmd:
        return RunResult(stdout=(FIXTURES / "semanage_boolean_l_output.txt").read_text(), stderr="", returncode=0)
    if "semanage" in cmd and "port" in cmd:
        return RunResult(stdout=(FIXTURES / "semanage_port_l_C_output.txt").read_text(), stderr="", returncode=0)
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


def test_scan_unit_files_from_fs(host_root):
    """Test the filesystem fallback for unit file state detection."""
    from yoinkc.inspectors.service import _scan_unit_files_from_fs
    units = _scan_unit_files_from_fs(host_root)

    assert units.get("test-installable.service") == "enabled", (
        "Unit in .wants/ should be enabled"
    )
    assert units.get("test-masked.service") == "masked", (
        "Symlink to /dev/null should be masked"
    )
    assert units.get("test-static.service") == "static", (
        "Vendor unit without [Install] should be static"
    )
    # fstrim.service has [Install] but is not in .wants/ -> disabled
    if "fstrim.service" in units:
        assert units["fstrim.service"] in ("disabled", "static")


def test_preset_glob_rules_applied(host_root, fixture_executor):
    """Glob preset rules like 'enable cloud-*' must set default_state correctly."""
    from yoinkc.inspectors.service import run as run_service

    preset_text = "enable cloud-*\ndisable *\n"
    section = run_service(
        host_root, fixture_executor, base_image_preset_text=preset_text,
    )
    changes = {s.unit: s for s in section.state_changes}

    cloud_init = changes.get("cloud-init.service")
    assert cloud_init is not None, (
        f"cloud-init.service not in state_changes; units: {list(changes)}"
    )
    assert cloud_init.default_state == "enabled", (
        f"expected default_state='enabled' via glob, got '{cloud_init.default_state}'"
    )


def test_preset_glob_first_match_wins(host_root, fixture_executor):
    """Glob rules use first-match-wins: earlier rules take precedence."""
    from yoinkc.inspectors.service import run as run_service

    # 'disable cloud-*' appears before 'enable cloud-*': disable should win
    preset_text = "disable cloud-*\nenable cloud-*\ndisable *\n"
    section = run_service(
        host_root, fixture_executor, base_image_preset_text=preset_text,
    )
    changes = {s.unit: s for s in section.state_changes}

    cloud_init = changes.get("cloud-init.service")
    assert cloud_init is not None, (
        f"cloud-init.service not in state_changes; units: {list(changes)}"
    )
    assert cloud_init.default_state == "disabled", (
        f"first-match-wins: 'disable cloud-*' should beat 'enable cloud-*', "
        f"got '{cloud_init.default_state}'"
    )


def test_service_inspector_resolves_owning_packages(host_root, fixture_executor):
    """Changed units should have owning_package populated via rpm -qf."""
    from yoinkc.inspectors.service import run as run_service
    section = run_service(host_root, fixture_executor)
    httpd = next((s for s in section.state_changes if s.unit == "httpd.service"), None)
    assert httpd is not None, "httpd.service must be in state_changes"
    assert httpd.owning_package == "httpd", (
        f"expected owning_package='httpd', got {httpd.owning_package!r}"
    )
    unchanged = [s for s in section.state_changes if s.action == "unchanged"]
    for s in unchanged:
        assert s.owning_package is None, (
            f"unchanged unit {s.unit} should not have owning_package set"
        )


def test_service_inspector_detects_drop_ins(host_root, fixture_executor):
    """Drop-in overrides under /etc/systemd/system/*.service.d/ are detected."""
    from yoinkc.inspectors.service import run as run_service
    section = run_service(host_root, fixture_executor)
    assert len(section.drop_ins) >= 1
    httpd_dropin = next(
        (d for d in section.drop_ins if d.unit == "httpd.service"), None,
    )
    assert httpd_dropin is not None, (
        f"expected httpd.service drop-in, got units: {[d.unit for d in section.drop_ins]}"
    )
    assert httpd_dropin.path.endswith("override.conf")
    assert "TimeoutStartSec=600" in httpd_dropin.content


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

    # --- DNF proxy ---
    dnf_proxy = [p for p in section.proxy if "dnf" in p.source]
    assert len(dnf_proxy) >= 1, "Expected DNF proxy entries from etc/dnf/dnf.conf"
    assert any("proxy.corp.example.com" in p.line for p in dnf_proxy)


def test_storage_inspector_with_fixtures(host_root, fixture_executor):
    from yoinkc.inspectors.storage import run as run_storage
    section = run_storage(host_root, fixture_executor)
    assert section is not None
    assert len(section.fstab_entries) >= 3
    assert any(e.mount_point == "/" for e in section.fstab_entries)
    assert any(e.fstype == "cifs" for e in section.fstab_entries)
    assert any(e.fstype == "nfs" for e in section.fstab_entries)

    # CIFS credential reference extraction
    assert len(section.credential_refs) >= 1
    cifs_cred = next((c for c in section.credential_refs if c.mount_point == "/mnt/nas"), None)
    assert cifs_cred is not None
    assert cifs_cred.credential_path == "/etc/samba/creds"


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

    # Quadlet units: .container, .volume, .network (system + user-level)
    assert len(section.quadlet_units) >= 5
    unit_map = {u.name: u for u in section.quadlet_units}

    # .volume and .network types
    assert "app-data.volume" in unit_map
    assert "[Volume]" in unit_map["app-data.volume"].content
    assert "internal.network" in unit_map
    assert "[Network]" in unit_map["internal.network"].content
    assert unit_map["app-data.volume"].image == ""
    assert unit_map["internal.network"].image == ""

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

    item_map = {i.path: i for i in section.items}
    methods = {i.method for i in section.items}

    # Unknown provenance (directory scan)
    assert any(i.name == "dummy" for i in section.items)

    # pip dist-info with version (system-level)
    pip_items = [i for i in section.items if i.method == "pip dist-info"]
    assert len(pip_items) >= 2
    flask = next((i for i in pip_items if i.name == "flask"), None)
    assert flask is not None and flask.version == "3.1.3"
    requests_ = next((i for i in pip_items if i.name == "requests"), None)
    assert requests_ is not None and requests_.version == "2.32.5"

    # npm with lockfile content
    npm_items = [i for i in section.items if i.method == "npm package-lock.json"]
    assert len(npm_items) >= 1
    assert npm_items[0].name == "myapp"
    assert npm_items[0].files and "package-lock.json" in npm_items[0].files

    # readelf: Go binary
    go_items = [i for i in section.items if i.lang == "go"]
    assert len(go_items) >= 1, f"Expected Go binary, methods: {methods}"
    go_item = go_items[0]
    assert go_item.method == "readelf (go)"
    assert go_item.confidence == "high"
    assert go_item.static is True

    # readelf: Rust binary
    rust_items = [i for i in section.items if i.lang == "rust"]
    assert len(rust_items) >= 1, f"Expected Rust binary, methods: {methods}"
    rust_item = rust_items[0]
    assert rust_item.method == "readelf (rust)"
    assert rust_item.static is False
    assert any("libc" in lib for lib in rust_item.shared_libs)

    # Venv without system-site-packages
    venv_items = [i for i in section.items if i.method == "python venv"]
    assert len(venv_items) >= 2, f"Expected 2 venvs, got {len(venv_items)}"
    webapp_venv = next((i for i in venv_items if "webapp" in i.path), None)
    assert webapp_venv is not None
    assert webapp_venv.system_site_packages is False
    assert len(webapp_venv.packages) >= 2
    pkg_names = {p.name for p in webapp_venv.packages}
    assert "Django" in pkg_names or "django" in pkg_names.union({n.lower() for n in pkg_names})

    # Venv with system-site-packages
    analytics_venv = next((i for i in venv_items if "analytics" in i.path), None)
    assert analytics_venv is not None
    assert analytics_venv.system_site_packages is True
    assert len(analytics_venv.packages) >= 1

    # Git-managed directory
    git_items = [i for i in section.items if i.method == "git repository"]
    assert len(git_items) >= 1, f"Expected git repo, methods: {methods}"
    git_item = git_items[0]
    assert "custom-tool" in git_item.name
    assert git_item.git_remote == "https://github.com/example/custom-tool.git"
    assert len(git_item.git_commit) >= 10
    assert git_item.git_branch == "main"


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
    loaded_names = {m.name for m in section.loaded_modules}
    assert "br_netfilter" in loaded_names
    assert "virtio_net" in loaded_names
    assert "wireguard" in loaded_names

    # --- non-default modules ---
    # virtio_net / virtio_blk: in usr/lib/modules-load.d/virtio.conf → default
    # bonding: in etc/modules-load.d/bonding.conf → explicitly configured
    # bridge/stp/llc/nf_conntrack/nf_defrag_*/fat/mbcache/jbd2: have used_by → dependencies
    # br_netfilter, overlay, ip_tables, vfat, ext4, wireguard: non-default
    nd_names = {m.name for m in section.non_default_modules}
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
    sysctl_keys = {s.key for s in section.sysctl_overrides}
    assert "net.ipv4.ip_forward" in sysctl_keys, "ip_forward differs from default"
    assert "vm.swappiness" in sysctl_keys, "swappiness differs from default"
    # kernel.panic is 0 in both default and runtime → should NOT appear
    assert "kernel.panic" not in sysctl_keys, "kernel.panic is at default value"
    # net.ipv6.conf.all.forwarding is 0 in both → should NOT appear
    assert "net.ipv6.conf.all.forwarding" not in sysctl_keys

    # Check values
    ip_fwd = next(s for s in section.sysctl_overrides if s.key == "net.ipv4.ip_forward")
    assert ip_fwd.runtime == "1"
    assert ip_fwd.default == "0"
    swap = next(s for s in section.sysctl_overrides if s.key == "vm.swappiness")
    assert swap.runtime == "10"
    assert swap.default == "30"


def test_kernel_boot_detects_tuned_profile(host_root, fixture_executor):
    """Tuned active profile and custom profiles are detected."""
    from yoinkc.inspectors.kernel_boot import run as run_kernel_boot
    section = run_kernel_boot(host_root, fixture_executor)
    assert section.tuned_active == "my-web-profile"
    assert len(section.tuned_custom_profiles) >= 1
    custom = next(
        (p for p in section.tuned_custom_profiles if "my-web-profile" in p.path),
        None,
    )
    assert custom is not None
    assert "net.core.somaxconn" in custom.content


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

    # Custom port labels from semanage port -l -C
    assert len(section.port_labels) == 2
    port_map = {(pl.protocol, pl.port): pl.type for pl in section.port_labels}
    assert port_map[("tcp", "2222")] == "ssh_port_t"
    assert port_map[("tcp", "8080")] == "http_port_t"


def test_non_rpm_inspector_detects_env_files(host_root, fixture_executor):
    from yoinkc.inspectors.non_rpm_software import run as run_non_rpm
    section = run_non_rpm(host_root, fixture_executor)
    assert section is not None

    env_paths = [ef.path for ef in section.env_files]
    assert any("myapp/.env" in p for p in env_paths), f"Expected myapp/.env in {env_paths}"

    # The .env entry should be an unowned ConfigFileEntry with content
    myapp_env = next(ef for ef in section.env_files if "myapp/.env" in ef.path)
    from yoinkc.schema import ConfigFileKind
    assert myapp_env.kind == ConfigFileKind.UNOWNED
    assert "API_KEY" in myapp_env.content


def test_env_files_are_redacted(host_root, fixture_executor):
    from yoinkc.inspectors.non_rpm_software import run as run_non_rpm
    from yoinkc.redact import redact_snapshot
    from yoinkc.schema import InspectionSnapshot

    non_rpm = run_non_rpm(host_root, fixture_executor)
    snapshot = InspectionSnapshot(non_rpm_software=non_rpm)
    redacted = redact_snapshot(snapshot)

    myapp_env = next(ef for ef in redacted.non_rpm_software.env_files if "myapp/.env" in ef.path)
    # API key value should be redacted
    assert "sk-fakekeyABCDEFGHIJKLMNOPQRSTUVWXYZ1234" not in myapp_env.content
    assert "REDACTED_" in myapp_env.content
    # At least one redaction entry should reference this path
    assert any("myapp/.env" in r["path"] for r in redacted.redactions)


def test_user_classification():
    from yoinkc.inspectors.users_groups import _classify_user, _STRATEGY_MAP

    # Service: nologin shell
    assert _classify_user({"shell": "/sbin/nologin", "home": "/var/lib/redis", "uid": 1001}) == "service"
    assert _STRATEGY_MAP["service"] == "sysusers"

    # Service: /bin/false
    assert _classify_user({"shell": "/bin/false", "home": "/home/nobody", "uid": 1002}) == "service"

    # Service: home under /var with nologin
    assert _classify_user({"shell": "/sbin/nologin", "home": "/var/lib/myapp", "uid": 1003}) == "service"

    # Human: real shell, /home, uid >= 1000
    assert _classify_user({"shell": "/bin/bash", "home": "/home/alice", "uid": 1000}) == "human"
    assert _STRATEGY_MAP["human"] == "kickstart"

    # Human: zsh
    assert _classify_user({"shell": "/bin/zsh", "home": "/home/bob", "uid": 1001}) == "human"

    # Ambiguous: real shell but home under /var
    assert _classify_user({"shell": "/bin/bash", "home": "/var/lib/myapp", "uid": 1004}) == "ambiguous"
    assert _STRATEGY_MAP["ambiguous"] == "useradd"

    # Ambiguous: unusual shell
    assert _classify_user({"shell": "/usr/local/bin/custom-shell", "home": "/home/custom", "uid": 1005}) == "ambiguous"


def test_user_classification_in_fixture(host_root, fixture_executor):
    from yoinkc.inspectors.users_groups import run as run_users_groups
    section = run_users_groups(host_root, fixture_executor)
    jdoe = next(u for u in section.users if u["name"] == "jdoe")
    assert jdoe["classification"] == "human"
    assert jdoe["strategy"] == "kickstart"

    # Group follows primary user
    jdoe_group = next(g for g in section.groups if g["name"] == "jdoe")
    assert jdoe_group["strategy"] == "kickstart"


def test_group_strategy_no_user(tmp_path):
    """Groups with no associated user default to sysusers — tested via the real run()."""
    from yoinkc.inspectors.users_groups import run as run_users_groups

    etc = tmp_path / "etc"
    etc.mkdir()
    # No non-system users (uid ≥ 1000) in passwd
    (etc / "passwd").write_text("root:x:0:0:root:/root:/bin/bash\n")
    # One group with gid 2000 and no matching user
    (etc / "group").write_text("mygroup:x:2000:\n")

    section = run_users_groups(tmp_path, executor=None)

    mygroup = next((g for g in section.groups if g["name"] == "mygroup"), None)
    assert mygroup is not None, "mygroup must be collected by the inspector"
    assert mygroup["strategy"] == "sysusers", (
        f"Group with no primary user must default to sysusers, got {mygroup['strategy']!r}"
    )


def test_group_strategy_first_user_wins_on_shared_gid(tmp_path):
    """When two users share a primary GID, the group inherits the first user's strategy."""
    from yoinkc.inspectors.users_groups import run as run_users_groups

    etc = tmp_path / "etc"
    etc.mkdir()
    # alice (uid 1000, gid 2000) is a human; bob (uid 1001, also gid 2000) is a service.
    # The appgroup (gid 2000) should follow alice, not bob.
    (etc / "passwd").write_text(
        "alice:x:1000:2000:Alice:/home/alice:/bin/bash\n"
        "bob:x:1001:2000:Bob:/var/lib/bob:/sbin/nologin\n"
    )
    (etc / "group").write_text("appgroup:x:2000:\n")

    section = run_users_groups(tmp_path, executor=None)

    alice = next(u for u in section.users if u["name"] == "alice")
    assert alice["strategy"] == "kickstart"

    appgroup = next(g for g in section.groups if g["name"] == "appgroup")
    assert appgroup["strategy"] == "kickstart", (
        f"appgroup should follow alice (first user with gid 2000), got {appgroup['strategy']!r}"
    )


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


def test_cross_major_warning_in_snapshot(host_root, fixture_executor):
    """Cross-major-version warning appears in snapshot.warnings."""
    snapshot = run_all(
        host_root,
        executor=fixture_executor,
        target_version="10.0",
    )
    cross_warnings = [
        w for w in snapshot.warnings
        if "Cross-major-version" in w.get("message", "")
    ]
    assert len(cross_warnings) == 1, (
        f"Expected exactly one cross-major warning, got {len(cross_warnings)}"
    )
    assert cross_warnings[0]["severity"] == "error"


def test_no_cross_major_warning_same_version(host_root, fixture_executor):
    """No cross-major warning when source and target are same major."""
    snapshot = run_all(
        host_root,
        executor=fixture_executor,
        target_version="9.8",
    )
    cross_warnings = [
        w for w in snapshot.warnings
        if "Cross-major-version" in w.get("message", "")
    ]
    assert len(cross_warnings) == 0


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


def test_classify_leaf_auto_falls_back_when_dnf_repoquery_fails(host_root):
    """When dnf repoquery is unavailable, _classify_leaf_auto falls back to rpm -qR."""
    from yoinkc.inspectors.rpm import _classify_leaf_auto
    from yoinkc.schema import PackageEntry, PackageState

    packages = [
        PackageEntry(name="httpd", epoch="0", version="2.4.62", release="1.el9", arch="x86_64", state=PackageState.ADDED),
        PackageEntry(name="mod_ssl", epoch="1", version="2.4.62", release="1.el9", arch="x86_64", state=PackageState.ADDED),
    ]

    def dnf_fail_executor(cmd, cwd=None):
        cmd_str = " ".join(cmd)
        if "dnf" in cmd and "repoquery" in cmd:
            return RunResult(stdout="", stderr="dnf: command not found", returncode=127)
        if "rpm" in cmd and "-qR" in cmd:
            if "httpd" in cmd_str:
                return RunResult(stdout="mod_ssl\nhttpd-core\n", stderr="", returncode=0)
            return RunResult(stdout="", stderr="", returncode=0)
        if "rpm" in cmd and "--whatprovides" in cmd:
            return RunResult(stdout="mod_ssl-1:2.4.62-1.el9.x86_64\n", stderr="", returncode=0)
        return RunResult(stdout="", stderr="", returncode=1)

    leaf, auto, dep_tree = _classify_leaf_auto(dnf_fail_executor, host_root, packages)

    assert "httpd" in leaf
    assert "mod_ssl" in auto
    assert dep_tree["httpd"] == ["mod_ssl"]


def test_classify_leaf_auto_uses_userinstalled(host_root):
    """When dnf repoquery --userinstalled succeeds, it determines the leaf set."""
    from yoinkc.inspectors.rpm import _classify_leaf_auto
    from yoinkc.schema import PackageEntry, PackageState

    packages = [
        PackageEntry(name="httpd", epoch="0", version="2.4.62", release="1.el9", arch="x86_64", state=PackageState.ADDED),
        PackageEntry(name="git", epoch="0", version="2.43.5", release="1.el9", arch="x86_64", state=PackageState.ADDED),
        PackageEntry(name="httpd-core", epoch="0", version="2.4.62", release="1.el9", arch="x86_64", state=PackageState.ADDED),
        PackageEntry(name="perl-interpreter", epoch="4", version="5.32.1", release="480.el9", arch="x86_64", state=PackageState.ADDED),
    ]

    def executor(cmd, cwd=None):
        if "--userinstalled" in cmd:
            return RunResult(stdout="httpd\ngit\n", stderr="", returncode=0)
        if "dnf" in cmd and "repoquery" in cmd and "--requires" in cmd:
            pkg = cmd[-1]
            dep_map = {
                "httpd": "httpd-core\n",
                "git": "perl-interpreter\n",
                "httpd-core": "",
                "perl-interpreter": "",
            }
            return RunResult(stdout=dep_map.get(pkg, ""), stderr="", returncode=0)
        return RunResult(stdout="", stderr="", returncode=1)

    leaf, auto, dep_tree = _classify_leaf_auto(executor, host_root, packages)

    assert leaf == ["git", "httpd"]
    assert auto == ["httpd-core", "perl-interpreter"]
    assert dep_tree["httpd"] == ["httpd-core"]
    assert dep_tree["git"] == ["perl-interpreter"]


def test_classify_leaf_auto_userinstalled_fallback_on_failure(host_root):
    """When --userinstalled fails but dnf dep queries work, graph-based classification is used."""
    from yoinkc.inspectors.rpm import _classify_leaf_auto
    from yoinkc.schema import PackageEntry, PackageState

    packages = [
        PackageEntry(name="httpd", epoch="0", version="2.4.62", release="1.el9", arch="x86_64", state=PackageState.ADDED),
        PackageEntry(name="httpd-core", epoch="0", version="2.4.62", release="1.el9", arch="x86_64", state=PackageState.ADDED),
    ]

    def executor(cmd, cwd=None):
        if "--userinstalled" in cmd:
            return RunResult(stdout="", stderr="error", returncode=1)
        if "dnf" in cmd and "repoquery" in cmd and "--requires" in cmd:
            pkg = cmd[-1]
            if pkg == "httpd":
                return RunResult(stdout="httpd-core\n", stderr="", returncode=0)
            return RunResult(stdout="", stderr="", returncode=0)
        return RunResult(stdout="", stderr="", returncode=1)

    leaf, auto, dep_tree = _classify_leaf_auto(executor, host_root, packages)

    assert leaf == ["httpd"]
    assert auto == ["httpd-core"]


def test_classify_leaf_auto_empty_userinstalled_falls_back(host_root):
    """When --userinstalled succeeds but has no overlap with added packages, fall back to graph."""
    from yoinkc.inspectors.rpm import _classify_leaf_auto
    from yoinkc.schema import PackageEntry, PackageState

    packages = [
        PackageEntry(name="httpd", epoch="0", version="2.4.62", release="1.el9", arch="x86_64", state=PackageState.ADDED),
        PackageEntry(name="httpd-core", epoch="0", version="2.4.62", release="1.el9", arch="x86_64", state=PackageState.ADDED),
    ]

    def executor(cmd, cwd=None):
        if "--userinstalled" in cmd:
            return RunResult(stdout="vim\nemacs\n", stderr="", returncode=0)
        if "dnf" in cmd and "repoquery" in cmd and "--requires" in cmd:
            pkg = cmd[-1]
            if pkg == "httpd":
                return RunResult(stdout="httpd-core\n", stderr="", returncode=0)
            return RunResult(stdout="", stderr="", returncode=0)
        return RunResult(stdout="", stderr="", returncode=1)

    leaf, auto, dep_tree = _classify_leaf_auto(executor, host_root, packages)

    assert leaf == ["httpd"]
    assert auto == ["httpd-core"]


# ===========================================================================
# Graceful degradation: inspectors must not crash when commands fail
# ===========================================================================


def _failing_executor(cmd, cwd=None):
    """Executor that returns non-zero for everything except nsenter probe."""
    if cmd[-1] == "true" and "nsenter" in cmd:
        return RunResult(stdout="", stderr="", returncode=0)
    return RunResult(stdout="", stderr="command not available", returncode=1)


class TestInspectorFailures:
    """Each inspector must return a valid (possibly empty) section when commands fail."""

    def test_service_falls_back_to_fs_scan(self, host_root):
        from yoinkc.inspectors.service import run as run_service
        section = run_service(host_root, _failing_executor)
        assert section is not None
        # Filesystem fallback should still find units even when systemctl fails
        assert isinstance(section.state_changes, list)

    def test_kernel_boot_empty_on_lsmod_failure(self, host_root):
        from yoinkc.inspectors.kernel_boot import run as run_kernel_boot
        section = run_kernel_boot(host_root, _failing_executor)
        assert section is not None
        assert section.loaded_modules == []

    def test_scheduled_tasks_skips_at_jobs(self, host_root):
        from yoinkc.inspectors.scheduled_tasks import run as run_scheduled_tasks
        section = run_scheduled_tasks(host_root, _failing_executor)
        assert section is not None
        assert isinstance(section.at_jobs, list)
        # Cron jobs come from the filesystem, not the executor, so may still appear
        assert isinstance(section.cron_jobs, list)

    def test_rpm_empty_on_failure(self, host_root):
        from yoinkc.inspectors.rpm import run as run_rpm
        section = run_rpm(host_root, _failing_executor)
        assert section is not None
        assert section.packages_added == []
        assert section.rpm_va == []

    def test_selinux_graceful_on_failure(self, host_root):
        from yoinkc.inspectors.selinux import run as run_selinux
        section = run_selinux(host_root, _failing_executor)
        assert section is not None
        # Filesystem-based module detection still works; executor-backed
        # fields (booleans, ports) should be empty/absent.
        assert isinstance(section.boolean_overrides, list)

    def test_network_empty_on_failure(self, host_root):
        from yoinkc.inspectors.network import run as run_network
        section = run_network(host_root, _failing_executor)
        assert section is not None
        assert isinstance(section.connections, list)


def test_rpm_inspector_captures_gpg_keys(host_root, fixture_executor):
    """GPG keys referenced by gpgkey=file:// in repo files are captured."""
    from yoinkc.inspectors.rpm import run as run_rpm
    section = run_rpm(host_root, fixture_executor)
    assert section.gpg_keys, "Expected at least one GPG key captured"
    key_paths = [k.path for k in section.gpg_keys]
    assert "etc/pki/rpm-gpg/RPM-GPG-KEY-TEST" in key_paths
    key = next(k for k in section.gpg_keys if "TEST" in k.path)
    assert "BEGIN PGP PUBLIC KEY BLOCK" in key.content


def test_collect_gpg_keys_resolves_dnf_vars(tmp_path):
    """gpgkey= paths containing $releasever_major are resolved before file lookup."""
    from yoinkc.inspectors.rpm import _collect_gpg_keys
    from yoinkc.schema import RepoFile

    # Set up a minimal host_root with os-release and the resolved key file
    etc = tmp_path / "etc"
    etc.mkdir()
    (etc / "os-release").write_text('VERSION_ID="10.0"\nID=rhel\n')

    gpg_dir = etc / "pki" / "rpm-gpg"
    gpg_dir.mkdir(parents=True)
    key_content = "-----BEGIN PGP PUBLIC KEY BLOCK-----\nFAKE\n-----END PGP PUBLIC KEY BLOCK-----\n"
    (gpg_dir / "RPM-GPG-KEY-TEST-10").write_text(key_content)

    # Repo file using $releasever_major in the gpgkey path
    repo = RepoFile(
        path="etc/yum.repos.d/test.repo",
        content=(
            "[test]\nbaseurl=http://example.com\ngpgcheck=1\n"
            "gpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-TEST-$releasever_major\n"
        ),
    )

    keys = _collect_gpg_keys(tmp_path, [repo])
    assert keys, "Expected the GPG key to be captured after variable resolution"
    assert keys[0].path == "etc/pki/rpm-gpg/RPM-GPG-KEY-TEST-10"
    assert "BEGIN PGP PUBLIC KEY BLOCK" in keys[0].content


def test_source_repo_populated_via_dnf_repoquery(host_root, fixture_executor):
    """source_repo is populated for added packages when dnf repoquery succeeds."""
    from yoinkc.inspectors.rpm import run as run_rpm
    section = run_rpm(host_root, fixture_executor)
    pkgs_with_repo = [p for p in section.packages_added if p.source_repo]
    assert len(pkgs_with_repo) > 0, "Expected at least one package with source_repo set"
    httpd = next((p for p in section.packages_added if p.name == "httpd"), None)
    assert httpd is not None, "httpd must be in packages_added"
    assert httpd.source_repo == "baseos"
