"""Single-inspector tests for: config, network, storage, scheduled_tasks, container,
non_rpm_software (+ env_files, redaction), kernel_boot (+ tuned), selinux."""

from pathlib import Path

FIXTURES = Path(__file__).parent / "fixtures"


def test_config_inspector_with_fixtures(host_root, fixture_executor):
    from inspectah.inspectors.config import run as run_config
    from inspectah.inspectors.rpm import run as run_rpm
    rpm_section = run_rpm(host_root, fixture_executor)
    rpm_owned = set((FIXTURES / "rpm_qla_output.txt").read_text().strip().splitlines())
    section = run_config(host_root, fixture_executor, rpm_section=rpm_section, rpm_owned_paths_override=rpm_owned)
    assert section is not None
    modified = [f for f in section.files if f.kind.value == "rpm_owned_modified"]
    assert len(modified) >= 2
    assert any("/etc/httpd/conf/httpd.conf" == f.path for f in modified)


def test_network_inspector_with_fixtures(host_root, fixture_executor):
    from inspectah.inspectors.network import run as run_network
    section = run_network(host_root, fixture_executor)
    assert section is not None

    assert len(section.connections) >= 2
    conn_map = {c.name: c for c in section.connections}
    assert "eth0" in conn_map
    assert conn_map["eth0"].method == "dhcp"
    assert conn_map["eth0"].type == "802-3-ethernet"
    assert "mgmt0" in conn_map
    assert conn_map["mgmt0"].method == "static"

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

    assert len(section.firewall_direct_rules) == 2
    assert any("9090" in r.args for r in section.firewall_direct_rules)
    assert section.firewall_direct_rules[0].chain == "INPUT"

    assert section.resolv_provenance == "networkmanager"

    assert len(section.ip_routes) >= 3
    assert any("default via" in r for r in section.ip_routes)
    assert any("proto static" in r for r in section.ip_routes)

    assert len(section.ip_rules) >= 1
    assert any("custom_table" in r for r in section.ip_rules)
    assert not any("lookup local" in r for r in section.ip_rules)
    assert not any("lookup main" in r for r in section.ip_rules)

    dnf_proxy = [p for p in section.proxy if "dnf" in p.source]
    assert len(dnf_proxy) >= 1, "Expected DNF proxy entries from etc/dnf/dnf.conf"
    assert any("proxy.corp.example.com" in p.line for p in dnf_proxy)


def test_storage_inspector_with_fixtures(host_root, fixture_executor):
    from inspectah.inspectors.storage import run as run_storage
    section = run_storage(host_root, fixture_executor)
    assert section is not None
    assert len(section.fstab_entries) >= 3
    assert any(e.mount_point == "/" for e in section.fstab_entries)
    assert any(e.fstype == "cifs" for e in section.fstab_entries)
    assert any(e.fstype == "nfs" for e in section.fstab_entries)

    assert len(section.credential_refs) >= 1
    cifs_cred = next((c for c in section.credential_refs if c.mount_point == "/mnt/nas"), None)
    assert cifs_cred is not None
    assert cifs_cred.credential_path == "/etc/samba/creds"


def test_scheduled_tasks_inspector_with_fixtures(host_root, fixture_executor):
    from inspectah.inspectors.scheduled_tasks import run as run_scheduled_tasks
    section = run_scheduled_tasks(host_root, fixture_executor)
    assert section is not None

    assert any(j.path.endswith("hourly-job") for j in section.cron_jobs)
    assert len(section.generated_timer_units) >= 1
    assert "OnCalendar" in section.generated_timer_units[0].timer_content

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

    assert len(section.at_jobs) >= 1
    at_job = section.at_jobs[0]
    assert "cleanup-temp" in at_job.command
    assert at_job.user == "root"


def test_container_inspector_with_fixtures(host_root, fixture_executor):
    from inspectah.inspectors.container import run as run_container

    section = run_container(host_root, fixture_executor, query_podman=False)
    assert section is not None

    assert len(section.quadlet_units) >= 5
    unit_map = {u.name: u for u in section.quadlet_units}

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

    assert "dev-postgres.container" in unit_map
    assert unit_map["dev-postgres.container"].image == "docker.io/library/postgres:16"
    assert "home/jdoe" in unit_map["dev-postgres.container"].path

    assert len(section.compose_files) >= 1
    compose = section.compose_files[0]
    assert "docker-compose" in compose.path
    svc_images = {img.service: img.image for img in compose.images}
    assert "web" in svc_images
    assert "python:3.11-slim" in svc_images["web"]
    assert "db" in svc_images
    assert "postgres:16-alpine" in svc_images["db"]

    assert len(section.running_containers) == 0

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
    from inspectah.inspectors.non_rpm_software import run as run_non_rpm_software
    section = run_non_rpm_software(host_root, fixture_executor, deep_binary_scan=False)
    assert section is not None

    item_map = {i.path: i for i in section.items}
    methods = {i.method for i in section.items}

    assert any(i.name == "dummy" for i in section.items)

    pip_items = [i for i in section.items if i.method == "pip dist-info"]
    assert len(pip_items) >= 2
    flask = next((i for i in pip_items if i.name == "flask"), None)
    assert flask is not None and flask.version == "3.1.3"
    requests_ = next((i for i in pip_items if i.name == "requests"), None)
    assert requests_ is not None and requests_.version == "2.32.5"

    npm_items = [i for i in section.items if i.method == "npm package-lock.json"]
    assert len(npm_items) >= 1
    assert npm_items[0].name == "myapp"
    assert npm_items[0].files and "package-lock.json" in npm_items[0].files

    go_items = [i for i in section.items if i.lang == "go"]
    assert len(go_items) >= 1, f"Expected Go binary, methods: {methods}"
    go_item = go_items[0]
    assert go_item.method == "readelf (go)"
    assert go_item.confidence == "high"
    assert go_item.static is True

    rust_items = [i for i in section.items if i.lang == "rust"]
    assert len(rust_items) >= 1, f"Expected Rust binary, methods: {methods}"
    rust_item = rust_items[0]
    assert rust_item.method == "readelf (rust)"
    assert rust_item.static is False
    assert any("libc" in lib for lib in rust_item.shared_libs)

    venv_items = [i for i in section.items if i.method == "python venv"]
    assert len(venv_items) >= 2, f"Expected 2 venvs, got {len(venv_items)}"
    webapp_venv = next((i for i in venv_items if "webapp" in i.path), None)
    assert webapp_venv is not None
    assert webapp_venv.system_site_packages is False
    assert len(webapp_venv.packages) >= 2
    pkg_names = {p.name for p in webapp_venv.packages}
    assert "Django" in pkg_names or "django" in pkg_names.union({n.lower() for n in pkg_names})

    analytics_venv = next((i for i in venv_items if "analytics" in i.path), None)
    assert analytics_venv is not None
    assert analytics_venv.system_site_packages is True
    assert len(analytics_venv.packages) >= 1

    git_items = [i for i in section.items if i.method == "git repository"]
    assert len(git_items) >= 1, f"Expected git repo, methods: {methods}"
    git_item = git_items[0]
    assert "custom-tool" in git_item.name
    assert git_item.git_remote == "https://github.com/example/custom-tool.git"
    assert len(git_item.git_commit) >= 10
    assert git_item.git_branch == "main"


def test_kernel_boot_inspector_with_fixtures(host_root, fixture_executor):
    from inspectah.inspectors.kernel_boot import run as run_kernel_boot
    section = run_kernel_boot(host_root, fixture_executor)
    assert section is not None
    assert section.cmdline != ""
    assert "root=" in section.cmdline
    assert section.grub_defaults != ""
    assert "GRUB_CMDLINE_LINUX" in section.grub_defaults

    assert len(section.loaded_modules) > 0
    loaded_names = {m.name for m in section.loaded_modules}
    assert "br_netfilter" in loaded_names
    assert "virtio_net" in loaded_names
    assert "wireguard" in loaded_names

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

    sysctl_keys = {s.key for s in section.sysctl_overrides}
    assert "net.ipv4.ip_forward" in sysctl_keys, "ip_forward differs from default"
    assert "vm.swappiness" in sysctl_keys, "swappiness differs from default"
    assert "kernel.panic" not in sysctl_keys, "kernel.panic is at default value"
    assert "net.ipv6.conf.all.forwarding" not in sysctl_keys

    ip_fwd = next(s for s in section.sysctl_overrides if s.key == "net.ipv4.ip_forward")
    assert ip_fwd.runtime == "1"
    assert ip_fwd.default == "0"
    swap = next(s for s in section.sysctl_overrides if s.key == "vm.swappiness")
    assert swap.runtime == "10"
    assert swap.default == "30"


def test_kernel_boot_detects_tuned_profile(host_root, fixture_executor):
    """Tuned active profile and custom profiles are detected."""
    from inspectah.inspectors.kernel_boot import run as run_kernel_boot
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
    from inspectah.inspectors.selinux import run as run_selinux
    section = run_selinux(host_root, fixture_executor)
    assert section is not None
    assert section.mode == "enforcing"
    assert any("99-foo" in p for p in section.audit_rules)

    assert "myapp" in section.custom_modules
    assert "abrt" not in section.custom_modules

    # Only non-default booleans should be captured (default-value ones are filtered out)
    assert len(section.boolean_overrides) == 3
    names = {b["name"] for b in section.boolean_overrides}
    assert "httpd_can_network_connect" in names
    assert "httpd_use_nfs" in names
    assert "virt_sandbox_use_all_caps" in names

    httpd_net = next(b for b in section.boolean_overrides if b["name"] == "httpd_can_network_connect")
    assert httpd_net["current"] == "on"
    assert httpd_net["default"] == "off"
    assert httpd_net["non_default"] is True

    # Default-value booleans should be excluded from the snapshot
    assert "httpd_enable_cgi" not in names

    assert len(section.port_labels) == 2
    port_map = {(pl.protocol, pl.port): pl.type for pl in section.port_labels}
    assert port_map[("tcp", "2222")] == "ssh_port_t"
    assert port_map[("tcp", "8080")] == "http_port_t"


def test_non_rpm_inspector_detects_env_files(host_root, fixture_executor):
    from inspectah.inspectors.non_rpm_software import run as run_non_rpm
    section = run_non_rpm(host_root, fixture_executor)
    assert section is not None

    env_paths = [ef.path for ef in section.env_files]
    assert any("myapp/.env" in p for p in env_paths), f"Expected myapp/.env in {env_paths}"

    myapp_env = next(ef for ef in section.env_files if "myapp/.env" in ef.path)
    from inspectah.schema import ConfigFileKind
    assert myapp_env.kind == ConfigFileKind.UNOWNED
    assert "API_KEY" in myapp_env.content


def test_env_files_are_redacted(host_root, fixture_executor):
    from inspectah.inspectors.non_rpm_software import run as run_non_rpm
    from inspectah.redact import redact_snapshot
    from inspectah.schema import InspectionSnapshot

    non_rpm = run_non_rpm(host_root, fixture_executor)
    snapshot = InspectionSnapshot(non_rpm_software=non_rpm)
    redacted = redact_snapshot(snapshot)

    myapp_env = next(ef for ef in redacted.non_rpm_software.env_files if "myapp/.env" in ef.path)
    assert "sk-fakekeyABCDEFGHIJKLMNOPQRSTUVWXYZ1234" not in myapp_env.content
    assert "REDACTED_" in myapp_env.content
    assert any("myapp/.env" in r.get("path", "") for r in redacted.redactions)


# ---------------------------------------------------------------------------
# System properties detection (locale, timezone, alternatives)
# ---------------------------------------------------------------------------


def test_detect_locale(tmp_path):
    from inspectah.inspectors.kernel_boot import run

    locale_conf = tmp_path / "etc" / "locale.conf"
    locale_conf.parent.mkdir(parents=True)
    locale_conf.write_text("LANG=en_US.UTF-8\n")

    result = run(host_root=tmp_path, executor=None)
    assert result.locale == "en_US.UTF-8"


def test_detect_locale_missing(tmp_path):
    from inspectah.inspectors.kernel_boot import run

    (tmp_path / "etc").mkdir(parents=True)
    result = run(host_root=tmp_path, executor=None)
    assert result.locale is None


def test_detect_locale_quoted(tmp_path):
    from inspectah.inspectors.kernel_boot import run

    locale_conf = tmp_path / "etc" / "locale.conf"
    locale_conf.parent.mkdir(parents=True)
    locale_conf.write_text('LANG="C.UTF-8"\nLC_MESSAGES=POSIX\n')

    result = run(host_root=tmp_path, executor=None)
    assert result.locale == "C.UTF-8"


def test_detect_timezone(tmp_path):
    from inspectah.inspectors.kernel_boot import run

    etc = tmp_path / "etc"
    etc.mkdir(parents=True)
    zoneinfo = tmp_path / "usr" / "share" / "zoneinfo" / "America" / "New_York"
    zoneinfo.parent.mkdir(parents=True)
    zoneinfo.write_text("")
    localtime = etc / "localtime"
    localtime.symlink_to(zoneinfo)

    result = run(host_root=tmp_path, executor=None)
    assert result.timezone == "America/New_York"


def test_detect_timezone_relative_symlink(tmp_path):
    from inspectah.inspectors.kernel_boot import run

    etc = tmp_path / "etc"
    etc.mkdir(parents=True)
    zoneinfo = tmp_path / "usr" / "share" / "zoneinfo" / "America" / "Los_Angeles"
    zoneinfo.parent.mkdir(parents=True)
    zoneinfo.write_text("")
    localtime = etc / "localtime"
    localtime.symlink_to("../../usr/share/zoneinfo/America/Los_Angeles")

    result = run(host_root=tmp_path, executor=None)
    assert result.timezone == "America/Los_Angeles"


def test_detect_timezone_missing(tmp_path):
    from inspectah.inspectors.kernel_boot import run

    (tmp_path / "etc").mkdir(parents=True)
    result = run(host_root=tmp_path, executor=None)
    assert result.timezone is None


def test_detect_alternatives_auto(tmp_path):
    from inspectah.inspectors.kernel_boot import run

    alt_dir = tmp_path / "etc" / "alternatives"
    alt_dir.mkdir(parents=True)
    target = tmp_path / "usr" / "bin" / "java-17"
    target.parent.mkdir(parents=True)
    target.write_text("")
    (alt_dir / "java").symlink_to(target)

    var_alt = tmp_path / "var" / "lib" / "alternatives"
    var_alt.mkdir(parents=True)
    (var_alt / "java").write_text("auto\n/usr/bin/java\n")

    result = run(host_root=tmp_path, executor=None)
    assert len(result.alternatives) == 1
    assert result.alternatives[0].name == "java"
    assert result.alternatives[0].status == "auto"
    assert str(target) in result.alternatives[0].path


def test_detect_alternatives_manual(tmp_path):
    from inspectah.inspectors.kernel_boot import run

    alt_dir = tmp_path / "etc" / "alternatives"
    alt_dir.mkdir(parents=True)
    target = tmp_path / "usr" / "bin" / "python3.11"
    target.parent.mkdir(parents=True)
    target.write_text("")
    (alt_dir / "python3").symlink_to(target)

    var_alt = tmp_path / "var" / "lib" / "alternatives"
    var_alt.mkdir(parents=True)
    (var_alt / "python3").write_text("manual\n/usr/bin/python3\n")

    result = run(host_root=tmp_path, executor=None)
    assert len(result.alternatives) == 1
    assert result.alternatives[0].name == "python3"
    assert result.alternatives[0].status == "manual"


def test_detect_alternatives_empty(tmp_path):
    from inspectah.inspectors.kernel_boot import run

    (tmp_path / "etc").mkdir(parents=True)
    result = run(host_root=tmp_path, executor=None)
    assert result.alternatives == []
