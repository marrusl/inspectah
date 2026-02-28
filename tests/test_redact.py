"""Tests for redact_snapshot — extended section scanning."""

from yoinkc.redact import redact_snapshot, _redact_text, _is_excluded_path
from yoinkc.schema import (
    InspectionSnapshot,
    NetworkSection, FirewallZone,
    ContainerSection, QuadletUnit, RunningContainer,
    ScheduledTaskSection, GeneratedTimerUnit, SystemdTimer,
    KernelBootSection,
    UserGroupSection,
)


def _base_snapshot(**kwargs) -> InspectionSnapshot:
    return InspectionSnapshot(meta={}, **kwargs)


# ---------------------------------------------------------------------------
# _is_excluded_path
# ---------------------------------------------------------------------------

def test_excluded_path_shadow():
    assert _is_excluded_path("/etc/shadow") is True
    assert _is_excluded_path("etc/shadow") is True


def test_excluded_path_ssh_host_key():
    assert _is_excluded_path("/etc/ssh/ssh_host_rsa_key") is True
    assert _is_excluded_path("/etc/ssh/ssh_host_ed25519_key") is True


def test_excluded_path_not_matching():
    assert _is_excluded_path("/etc/hosts") is False
    assert _is_excluded_path("/etc/httpd/conf/httpd.conf") is False


# ---------------------------------------------------------------------------
# _redact_text
# ---------------------------------------------------------------------------

def test_redact_text_password():
    redactions = []
    out = _redact_text("password=hunter2", "test/path", redactions)
    assert "hunter2" not in out
    assert "REDACTED_PASSWORD" in out
    assert len(redactions) == 1
    assert redactions[0]["path"] == "test/path"


def test_redact_text_false_positive():
    """PAM config lines should not be redacted."""
    redactions = []
    out = _redact_text("auth required pam_unix.so", "etc/pam.d/system-auth", redactions)
    assert out == "auth required pam_unix.so"
    assert redactions == []


def test_redact_text_private_key():
    key = "-----BEGIN RSA PRIVATE KEY-----\nMIIEow...\n-----END RSA PRIVATE KEY-----"
    redactions = []
    out = _redact_text(key, "test/key", redactions)
    assert "MIIEow" not in out
    assert "REDACTED_PRIVATE_KEY" in out


# ---------------------------------------------------------------------------
# Firewall zone content
# ---------------------------------------------------------------------------

def test_redact_firewall_zone_content():
    # NM connection profiles can end up in firewall zone content when admins
    # embed openvpn inline credentials.  Use a realistic password= pattern.
    zone = FirewallZone(
        path="etc/firewalld/zones/vpn.xml",
        name="vpn",
        # Some firewall zone configs store inline VPN credentials as XML text
        content='<zone><service name="openvpn"/>\nDB_PASSWORD=myvpnsecret123456789\n</zone>',
    )
    snapshot = _base_snapshot(network=NetworkSection(firewall_zones=[zone]))
    result = redact_snapshot(snapshot)
    assert result.network is not None
    new_zone = result.network.firewall_zones[0]
    assert "myvpnsecret123456789" not in new_zone.content
    assert any("firewall_zone/vpn" in r["path"] for r in result.redactions)


def test_redact_firewall_zone_no_secrets():
    zone = FirewallZone(
        path="etc/firewalld/zones/public.xml",
        name="public",
        content="<zone><service name='ssh'/></zone>",
    )
    snapshot = _base_snapshot(network=NetworkSection(firewall_zones=[zone]))
    result = redact_snapshot(snapshot)
    assert result.network.firewall_zones[0].content == zone.content
    assert not any("firewall_zone" in r.get("path", "") for r in result.redactions)


# ---------------------------------------------------------------------------
# Quadlet unit content
# ---------------------------------------------------------------------------

def test_redact_quadlet_content():
    unit = QuadletUnit(
        path="etc/containers/systemd/myapp.container",
        name="myapp.container",
        content="[Container]\nImage=myapp:latest\nEnvironment=DB_PASSWORD=supersecret99\n",
    )
    snapshot = _base_snapshot(containers=ContainerSection(quadlet_units=[unit]))
    result = redact_snapshot(snapshot)
    new_unit = result.containers.quadlet_units[0]
    assert "supersecret99" not in new_unit.content
    assert any("quadlet/myapp.container" in r["path"] for r in result.redactions)


# ---------------------------------------------------------------------------
# Running container env
# ---------------------------------------------------------------------------

def test_redact_running_container_env():
    c = RunningContainer(
        id="abc123",
        name="redis",
        image="redis:7",
        env=["REDIS_PASSWORD=topsecretredis", "HOSTNAME=redis-1"],
    )
    snapshot = _base_snapshot(containers=ContainerSection(running_containers=[c]))
    result = redact_snapshot(snapshot)
    new_c = result.containers.running_containers[0]
    assert not any("topsecretredis" in e for e in new_c.env)
    assert any("containers:running/redis:env" in r["path"] for r in result.redactions)


def test_running_container_env_no_secrets():
    c = RunningContainer(
        id="abc123",
        name="nginx",
        image="nginx:latest",
        env=["HOSTNAME=web-1", "PATH=/usr/bin:/bin"],
    )
    snapshot = _base_snapshot(containers=ContainerSection(running_containers=[c]))
    result = redact_snapshot(snapshot)
    assert result.containers.running_containers[0].env == c.env


# ---------------------------------------------------------------------------
# Generated timer service content and command
# ---------------------------------------------------------------------------

def test_redact_generated_timer_service_content():
    unit = GeneratedTimerUnit(
        name="cron-backup",
        timer_content="[Timer]\nOnCalendar=daily\n",
        service_content="[Service]\nExecStart=/usr/bin/pg_dump -U admin -p password=dbpass123 mydb\n",
        cron_expr="0 2 * * *",
        source_path="etc/cron.d/backup",
        command="/usr/bin/pg_dump -U admin -p password=dbpass123 mydb",
    )
    snapshot = _base_snapshot(
        scheduled_tasks=ScheduledTaskSection(generated_timer_units=[unit])
    )
    result = redact_snapshot(snapshot)
    new_unit = result.scheduled_tasks.generated_timer_units[0]
    assert "dbpass123" not in new_unit.service_content
    assert "dbpass123" not in new_unit.command


# ---------------------------------------------------------------------------
# Systemd timer service content (local only)
# ---------------------------------------------------------------------------

def test_redact_local_timer_service_content():
    timer = SystemdTimer(
        name="myapp-backup",
        source="local",
        path="etc/systemd/system/myapp-backup.timer",
        timer_content="[Timer]\nOnCalendar=daily\n",
        service_content="[Service]\nExecStart=/usr/bin/backup --password=secret456\n",
    )
    snapshot = _base_snapshot(
        scheduled_tasks=ScheduledTaskSection(systemd_timers=[timer])
    )
    result = redact_snapshot(snapshot)
    new_timer = result.scheduled_tasks.systemd_timers[0]
    assert "secret456" not in new_timer.service_content


def test_vendor_timer_not_redacted():
    """Vendor timers (shipped in base image) are not redacted — not operator-owned."""
    timer = SystemdTimer(
        name="logrotate",
        source="vendor",
        service_content="[Service]\nExecStart=/usr/sbin/logrotate /etc/logrotate.conf\n",
    )
    snapshot = _base_snapshot(
        scheduled_tasks=ScheduledTaskSection(systemd_timers=[timer])
    )
    result = redact_snapshot(snapshot)
    # service_content unchanged (no secrets and vendor-only skip)
    assert result.scheduled_tasks.systemd_timers[0].service_content == timer.service_content


# ---------------------------------------------------------------------------
# GRUB defaults
# ---------------------------------------------------------------------------

def test_redact_grub_defaults():
    snapshot = _base_snapshot(
        kernel_boot=KernelBootSection(
            grub_defaults='GRUB_CMDLINE_LINUX="rd.luks.passphrase=mylukspassword quiet"',
        )
    )
    result = redact_snapshot(snapshot)
    assert "mylukspassword" not in result.kernel_boot.grub_defaults
    assert any("kernel:grub_defaults" in r["path"] for r in result.redactions)


def test_grub_no_secrets():
    snapshot = _base_snapshot(
        kernel_boot=KernelBootSection(grub_defaults='GRUB_CMDLINE_LINUX="rhgb quiet"')
    )
    result = redact_snapshot(snapshot)
    assert result.kernel_boot.grub_defaults == 'GRUB_CMDLINE_LINUX="rhgb quiet"'


# ---------------------------------------------------------------------------
# Kernel module configs
# ---------------------------------------------------------------------------

def test_redact_modprobe_d_content():
    snapshot = _base_snapshot(
        kernel_boot=KernelBootSection(
            modprobe_d=[{
                "path": "etc/modprobe.d/vpn.conf",
                "content": "options tun password=vpnsecret99",
            }]
        )
    )
    result = redact_snapshot(snapshot)
    assert "vpnsecret99" not in result.kernel_boot.modprobe_d[0]["content"]
    assert any("kernel:modprobe_d" in r["path"] for r in result.redactions)


# ---------------------------------------------------------------------------
# Sudoers rules
# ---------------------------------------------------------------------------

def test_redact_sudoers_rules():
    snapshot = _base_snapshot(
        users_groups=UserGroupSection(
            sudoers_rules=[
                "deploy ALL=(ALL) NOPASSWD: ALL",
                "dbadmin ALL=(ALL) PASSWD:password=adminpass123 /usr/bin/mysql",
            ]
        )
    )
    result = redact_snapshot(snapshot)
    assert not any("adminpass123" in r for r in result.users_groups.sudoers_rules)
    assert any("users:sudoers" in r["path"] for r in result.redactions)


def test_sudoers_no_secrets():
    snapshot = _base_snapshot(
        users_groups=UserGroupSection(
            sudoers_rules=["deploy ALL=(ALL) NOPASSWD: /usr/bin/systemctl"]
        )
    )
    result = redact_snapshot(snapshot)
    assert result.users_groups.sudoers_rules == ["deploy ALL=(ALL) NOPASSWD: /usr/bin/systemctl"]


# ---------------------------------------------------------------------------
# Idempotency — calling redact_snapshot twice produces the same result
# ---------------------------------------------------------------------------

def test_redact_idempotent():
    unit = QuadletUnit(
        path="etc/containers/systemd/app.container",
        name="app.container",
        content="[Container]\nEnvironment=API_KEY=abc123def456ghi789jkl",
    )
    snapshot = _base_snapshot(containers=ContainerSection(quadlet_units=[unit]))
    once = redact_snapshot(snapshot)
    twice = redact_snapshot(once)
    assert once.containers.quadlet_units[0].content == twice.containers.quadlet_units[0].content
    assert len(once.redactions) == len(twice.redactions)
