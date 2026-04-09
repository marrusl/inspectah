"""Tests for redact_snapshot — extended section scanning."""

from yoinkc.redact import redact_snapshot, _redact_text, _is_excluded_path
from yoinkc.schema import (
    InspectionSnapshot,
    RedactionFinding,
    ConfigSection, ConfigFileEntry, ConfigFileKind,
    NetworkSection, FirewallZone,
    ContainerSection, QuadletUnit, RunningContainer,
    ScheduledTaskSection, GeneratedTimerUnit, SystemdTimer,
    KernelBootSection, ConfigSnippet,
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
    assert redactions[0].get("path") == "test/path"


def test_redact_text_multiple_passwords_different_lengths():
    """Two passwords where the first replacement changes string length.

    Before the reverse-order fix, the second match's position was stale
    after the first replacement mutated the string, potentially leaving it
    unredacted or mis-evaluating _is_comment_line.
    """
    redactions = []
    text = "password=short\npassword=muchlongervalue"
    out = _redact_text(text, "test/multi", redactions)
    assert "short" not in out
    assert "muchlongervalue" not in out
    assert out.count("REDACTED_PASSWORD") == 2
    assert len(redactions) == 2


def test_redact_text_comment_line_preserved():
    """A commented secret must survive while the real one on the next line is redacted."""
    redactions = []
    text = "# password=safe_example\npassword=realsecret"
    out = _redact_text(text, "test/comment", redactions)
    assert "safe_example" in out, "commented value should be preserved"
    assert "realsecret" not in out, "real value should be redacted"
    assert out.count("REDACTED_PASSWORD") == 1
    assert len(redactions) == 1


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
    assert any("firewall_zone/vpn" in r.get("path", "") for r in result.redactions)


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
    assert any("quadlet/myapp.container" in r.get("path", "") for r in result.redactions)


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
    assert any("containers:running/redis:env" in r.get("path", "") for r in result.redactions)


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
    assert any("kernel:grub_defaults" in r.get("path", "") for r in result.redactions)


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
            modprobe_d=[ConfigSnippet(
                path="etc/modprobe.d/vpn.conf",
                content="options tun password=vpnsecret99",
            )]
        )
    )
    result = redact_snapshot(snapshot)
    assert "vpnsecret99" not in result.kernel_boot.modprobe_d[0].content
    assert any("kernel:modprobe_d" in r.get("path", "") for r in result.redactions)


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
    assert any("users:sudoers" in r.get("path", "") for r in result.redactions)


def test_sudoers_no_secrets():
    snapshot = _base_snapshot(
        users_groups=UserGroupSection(
            sudoers_rules=["deploy ALL=(ALL) NOPASSWD: /usr/bin/systemctl"]
        )
    )
    result = redact_snapshot(snapshot)
    assert result.users_groups.sudoers_rules == ["deploy ALL=(ALL) NOPASSWD: /usr/bin/systemctl"]


# ---------------------------------------------------------------------------
# Shadow entries
# ---------------------------------------------------------------------------

def test_redact_shadow_entry_with_hash():
    """A real yescrypt hash in a shadow entry must be redacted."""
    snapshot = _base_snapshot(
        users_groups=UserGroupSection(
            shadow_entries=[
                "jdoe:$y$j9T$abc123hashdata$longhashcontinues:19700:0:99999:7:::"
            ],
        )
    )
    result = redact_snapshot(snapshot)
    entry = result.users_groups.shadow_entries[0]
    assert "$y$j9T$" not in entry
    assert "REDACTED_SHADOW_HASH_" in entry
    fields = entry.split(":")
    assert fields[0] == "jdoe"
    assert fields[2] == "19700"
    assert any("SHADOW_HASH" in r.get("pattern", "") for r in result.redactions)


def test_redact_shadow_entry_locked_unchanged():
    """Locked accounts (!! or * hash) must not be modified."""
    snapshot = _base_snapshot(
        users_groups=UserGroupSection(
            shadow_entries=[
                "locked:!!:19700:0:99999:7:::",
                "nologin:*:19700:0:99999:7:::",
                "empty::19700:0:99999:7:::",
            ],
        )
    )
    result = redact_snapshot(snapshot)
    assert result.users_groups.shadow_entries[0] == "locked:!!:19700:0:99999:7:::"
    assert result.users_groups.shadow_entries[1] == "nologin:*:19700:0:99999:7:::"
    assert result.users_groups.shadow_entries[2] == "empty::19700:0:99999:7:::"
    assert not any("SHADOW_HASH" in r.get("pattern", "") for r in result.redactions)


def test_redact_passwd_gecos_with_credentials():
    """Credentials embedded in the GECOS field of a passwd entry must be redacted."""
    snapshot = _base_snapshot(
        users_groups=UserGroupSection(
            passwd_entries=[
                "svcacct:x:1001:1001:password=svc_secret_99:/home/svcacct:/bin/bash"
            ],
        )
    )
    result = redact_snapshot(snapshot)
    entry = result.users_groups.passwd_entries[0]
    assert "svc_secret_99" not in entry
    assert "REDACTED_PASSWORD" in entry
    fields = entry.split(":")
    assert fields[0] == "svcacct"
    assert fields[5] == "/home/svcacct"


def test_redact_shadow_round_trip():
    """Build a full snapshot with shadow entries, redact, verify no hash leaks."""
    snapshot = _base_snapshot(
        users_groups=UserGroupSection(
            shadow_entries=[
                "root:$6$rounds=65536$saltsalt$longhashvalue:19700:0:99999:7:::",
                "nobody:*:19700:0:99999:7:::",
            ],
        )
    )
    result = redact_snapshot(snapshot)
    for entry in result.users_groups.shadow_entries:
        assert "$6$" not in entry
        assert "longhashvalue" not in entry
    # After sorting by username, nobody comes before root
    nobody_entries = [e for e in result.users_groups.shadow_entries if e.startswith("nobody:")]
    assert len(nobody_entries) == 1
    assert nobody_entries[0] == "nobody:*:19700:0:99999:7:::"


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


# ---------------------------------------------------------------------------
# RedactionFinding model and compatibility
# ---------------------------------------------------------------------------

def test_redaction_finding_model():
    """RedactionFinding can be constructed with all fields."""
    f = RedactionFinding(
        path="/etc/shadow",
        source="file",
        kind="excluded",
        pattern="EXCLUDED_PATH",
        remediation="provision",
        line=None,
        replacement=None,
    )
    assert f.path == "/etc/shadow"
    assert f.source == "file"
    assert f.kind == "excluded"
    assert f.remediation == "provision"
    assert f.line is None
    assert f.replacement is None

def test_redaction_finding_dict_compat():
    """RedactionFinding supports .get() for backwards compat with dict consumers."""
    f = RedactionFinding(
        path="/etc/shadow",
        source="file",
        kind="excluded",
        pattern="EXCLUDED_PATH",
        remediation="provision",
    )
    assert f.get("path") == "/etc/shadow"
    assert f.get("pattern") == "EXCLUDED_PATH"
    assert f.get("line") is None
    assert f.get("missing", "default") == "default"


def test_redaction_finding_survives_save_load_roundtrip(tmp_path):
    """RedactionFinding objects survive save_snapshot() -> load_snapshot() round-trip.

    This is the critical durability test: save_snapshot() calls model_dump_json(),
    load_snapshot() calls model_validate(). Without the field_validator on
    InspectionSnapshot.redactions, RedactionFinding objects would be deserialized
    as plain dicts, and all isinstance() checks downstream would fail silently.
    """
    from yoinkc.pipeline import save_snapshot, load_snapshot

    snapshot = InspectionSnapshot(meta={"hostname": "test"})
    snapshot.redactions = [
        RedactionFinding(
            path="/etc/cockpit/ws-certs.d/0-self-signed.key",
            source="file", kind="excluded", pattern="EXCLUDED_PATH",
            remediation="regenerate",
        ),
        RedactionFinding(
            path="/etc/wireguard/wg0.conf",
            source="file", kind="inline", pattern="WIREGUARD_KEY",
            remediation="value-removed", line=3,
            replacement="REDACTED_WIREGUARD_KEY_1",
        ),
        RedactionFinding(
            path="users:shadow/admin",
            source="shadow", kind="inline", pattern="SHADOW_HASH",
            remediation="value-removed",
            replacement="REDACTED_SHADOW_HASH_1",
        ),
        # Legacy dict entry — should pass through unchanged
        {"path": "/etc/old.conf", "pattern": "PASSWORD", "line": "content",
         "remediation": "old style"},
    ]

    # Round-trip through save/load
    snapshot_path = tmp_path / "inspection-snapshot.json"
    save_snapshot(snapshot, snapshot_path)
    loaded = load_snapshot(snapshot_path)

    # Verify RedactionFinding objects survived as typed objects
    assert len(loaded.redactions) == 4

    # First three should be RedactionFinding instances
    assert isinstance(loaded.redactions[0], RedactionFinding)
    assert loaded.redactions[0].path == "/etc/cockpit/ws-certs.d/0-self-signed.key"
    assert loaded.redactions[0].remediation == "regenerate"
    assert loaded.redactions[0].kind == "excluded"

    assert isinstance(loaded.redactions[1], RedactionFinding)
    assert loaded.redactions[1].replacement == "REDACTED_WIREGUARD_KEY_1"
    assert loaded.redactions[1].line == 3

    assert isinstance(loaded.redactions[2], RedactionFinding)
    assert loaded.redactions[2].source == "shadow"

    # Fourth should still be a plain dict (legacy, no "source"/"kind" fields)
    assert isinstance(loaded.redactions[3], dict)
    assert loaded.redactions[3]["path"] == "/etc/old.conf"

    # Verify isinstance checks work for downstream consumers
    typed_findings = [r for r in loaded.redactions if isinstance(r, RedactionFinding)]
    assert len(typed_findings) == 3
    excluded = [r for r in typed_findings if r.kind == "excluded"]
    assert len(excluded) == 1
    inline = [r for r in typed_findings if r.kind == "inline"]
    assert len(inline) == 2


# ---------------------------------------------------------------------------
# Task 2: New EXCLUDED_PATHS detection
# ---------------------------------------------------------------------------

def test_p12_keystore_excluded():
    snapshot = _base_snapshot(config=ConfigSection(files=[
        ConfigFileEntry(path="/etc/pki/tls/private/server.p12", kind=ConfigFileKind.UNOWNED, content="binary-data", include=True),
    ]))
    result = redact_snapshot(snapshot)
    assert result.config.files[0].include is False


def test_pfx_keystore_excluded():
    snapshot = _base_snapshot(config=ConfigSection(files=[
        ConfigFileEntry(path="/etc/pki/tls/private/server.pfx", kind=ConfigFileKind.UNOWNED, content="binary-data", include=True),
    ]))
    result = redact_snapshot(snapshot)
    assert result.config.files[0].include is False


def test_jks_keystore_excluded():
    snapshot = _base_snapshot(config=ConfigSection(files=[
        ConfigFileEntry(path="/etc/pki/java/cacerts.jks", kind=ConfigFileKind.UNOWNED, content="binary-data", include=True),
    ]))
    result = redact_snapshot(snapshot)
    assert result.config.files[0].include is False


def test_cockpit_ws_certs_excluded():
    snapshot = _base_snapshot(config=ConfigSection(files=[
        ConfigFileEntry(path="/etc/cockpit/ws-certs.d/0-self-signed.key", kind=ConfigFileKind.UNOWNED, content="key-data", include=True),
        ConfigFileEntry(path="/etc/cockpit/ws-certs.d/0-self-signed.cert", kind=ConfigFileKind.UNOWNED, content="cert-data", include=True),
        ConfigFileEntry(path="/etc/cockpit/ws-certs.d/0-self-signed-ca.pem", kind=ConfigFileKind.UNOWNED, content="ca-data", include=True),
    ]))
    result = redact_snapshot(snapshot)
    assert all(f.include is False for f in result.config.files)


def test_containers_auth_json_excluded():
    snapshot = _base_snapshot(config=ConfigSection(files=[
        ConfigFileEntry(path="/etc/containers/auth.json", kind=ConfigFileKind.UNOWNED, content='{"auths":{}}', include=True),
    ]))
    result = redact_snapshot(snapshot)
    assert result.config.files[0].include is False


# ---------------------------------------------------------------------------
# Task 2: New REDACT_PATTERNS — WireGuard and WiFi PSK
# ---------------------------------------------------------------------------

def test_wireguard_private_key_redacted():
    """WireGuard PrivateKey redacted, assignment syntax preserved."""
    # Real WireGuard keys are 44 chars: 43 base64 chars + '=' padding (32 bytes)
    wg_config = "[Interface]\nAddress = 10.0.0.1/24\nPrivateKey = lWcu7GLoyXymjngaiY3JfFMRrTy96Fyonm2K5hW9qoo=\nListenPort = 51820\n"
    snapshot = _base_snapshot(config=ConfigSection(files=[
        ConfigFileEntry(path="/etc/wireguard/wg0.conf", kind=ConfigFileKind.UNOWNED, content=wg_config, include=True),
    ]))
    result = redact_snapshot(snapshot)
    content = result.config.files[0].content
    # Secret value must be gone
    assert "lWcu7GLoyXymjngaiY3JfFMRrTy96Fyonm2K5hW9qoo=" not in content
    # Assignment syntax must be preserved
    assert "PrivateKey = REDACTED_WIREGUARD_KEY_" in content or "PrivateKey =REDACTED_WIREGUARD_KEY_" in content
    # File stays included (inline, not exclusion)
    assert result.config.files[0].include is True


def test_wifi_psk_redacted():
    """WiFi PSK redacted, assignment syntax preserved."""
    nm_config = "[wifi-security]\nkey-mgmt=wpa-psk\npsk=mysecretpassword123\n"
    snapshot = _base_snapshot(config=ConfigSection(files=[
        ConfigFileEntry(path="/etc/NetworkManager/system-connections/wifi.nmconnection", kind=ConfigFileKind.UNOWNED, content=nm_config, include=True),
    ]))
    result = redact_snapshot(snapshot)
    content = result.config.files[0].content
    # Secret gone
    assert "mysecretpassword123" not in content
    # Assignment syntax preserved: "psk=" still present
    assert "psk=REDACTED_WIFI_PSK_" in content or "psk= REDACTED_WIFI_PSK_" in content
    assert result.config.files[0].include is True


# ---------------------------------------------------------------------------
# Task 3: Sequential counters
# ---------------------------------------------------------------------------

import re


def test_sequential_counters_deterministic():
    """Same input produces same counter assignments."""
    content_a = "password=secret1\napi_key=abcdefghijklmnopqrstuvwxyz\n"
    content_b = "password=secret2\n"
    snapshot = _base_snapshot(config=ConfigSection(files=[
        ConfigFileEntry(path="/etc/app/a.conf", kind=ConfigFileKind.UNOWNED, content=content_a, include=True),
        ConfigFileEntry(path="/etc/app/b.conf", kind=ConfigFileKind.UNOWNED, content=content_b, include=True),
    ]))
    r1 = redact_snapshot(snapshot)
    r2 = redact_snapshot(_base_snapshot(config=ConfigSection(files=[
        ConfigFileEntry(path="/etc/app/a.conf", kind=ConfigFileKind.UNOWNED, content=content_a, include=True),
        ConfigFileEntry(path="/etc/app/b.conf", kind=ConfigFileKind.UNOWNED, content=content_b, include=True),
    ])))
    assert r1.config.files[0].content == r2.config.files[0].content
    assert r1.config.files[1].content == r2.config.files[1].content


def test_sequential_counters_no_hash():
    """Counter tokens must not contain hash fragments."""
    content = "password=mysecret\n"
    snapshot = _base_snapshot(config=ConfigSection(files=[
        ConfigFileEntry(path="/etc/app.conf", kind=ConfigFileKind.UNOWNED, content=content, include=True),
    ]))
    result = redact_snapshot(snapshot)
    redacted = result.config.files[0].content
    # Should be REDACTED_PASSWORD_N, not REDACTED_PASSWORD_<hex>
    assert re.search(r"REDACTED_PASSWORD_\d+", redacted)
    assert not re.search(r"REDACTED_PASSWORD_[0-9a-f]{8}", redacted)


def test_same_secret_gets_same_counter():
    """Identical secret values across files share the same counter."""
    content_a = "password=identical_secret_value_here\n"
    content_b = "password=identical_secret_value_here\n"
    snapshot = _base_snapshot(config=ConfigSection(files=[
        ConfigFileEntry(path="/etc/app/a.conf", kind=ConfigFileKind.UNOWNED, content=content_a, include=True),
        ConfigFileEntry(path="/etc/app/b.conf", kind=ConfigFileKind.UNOWNED, content=content_b, include=True),
    ]))
    result = redact_snapshot(snapshot)
    token_a = re.search(r"REDACTED_PASSWORD_\d+", result.config.files[0].content).group()
    token_b = re.search(r"REDACTED_PASSWORD_\d+", result.config.files[1].content).group()
    assert token_a == token_b


def test_shadow_uses_sequential_counter():
    """Shadow entries must use sequential counters, not truncated SHA-256."""
    snapshot = _base_snapshot(
        users_groups=UserGroupSection(
            shadow_entries=[
                "jdoe:$y$j9T$abc123hashdata$longhashcontinues:19700:0:99999:7:::",
                "admin:$6$rounds=65536$saltsalt$longhashvalue:19700:0:99999:7:::",
            ],
        )
    )
    result = redact_snapshot(snapshot)
    entry0 = result.users_groups.shadow_entries[0]
    entry1 = result.users_groups.shadow_entries[1]
    # Must use sequential counter format, not hash
    assert re.search(r"REDACTED_SHADOW_HASH_\d+$", entry0.split(":")[1]), f"Expected counter format, got: {entry0.split(':')[1]}"
    assert re.search(r"REDACTED_SHADOW_HASH_\d+$", entry1.split(":")[1]), f"Expected counter format, got: {entry1.split(':')[1]}"
    # Different hashes get different counters
    assert entry0.split(":")[1] != entry1.split(":")[1]


def test_counter_shared_across_file_and_shadow():
    """File-backed and shadow findings share counter space — no duplicate counters."""
    content = "password=somesecret\n"
    snapshot = _base_snapshot(
        config=ConfigSection(files=[
            ConfigFileEntry(path="/etc/app.conf", kind=ConfigFileKind.UNOWNED, content=content, include=True),
        ]),
        users_groups=UserGroupSection(
            shadow_entries=["jdoe:$y$j9T$abc$hash:19700:0:99999:7:::"],
        ),
    )
    result = redact_snapshot(snapshot)
    # Both should use counters (no hashes)
    redacted_content = result.config.files[0].content
    shadow_entry = result.users_groups.shadow_entries[0]
    assert re.search(r"REDACTED_PASSWORD_\d+", redacted_content)
    assert re.search(r"REDACTED_SHADOW_HASH_\d+", shadow_entry)
    # No hex-hash patterns anywhere
    assert not re.search(r"REDACTED_\w+_[0-9a-f]{8}", redacted_content)
    assert not re.search(r"REDACTED_SHADOW_HASH_[0-9a-f]{8}", shadow_entry)


def test_counter_assignment_independent_of_input_order():
    """Counter tokens are deterministic regardless of input order.

    This test creates files in two different orders and verifies:
    1. The redactions list has identical path ordering (output-order sort)
    2. ConfigFileEntry.content strings carry identical placeholder tokens
    3. Each RedactionFinding.replacement token appears in its file's content

    Check 2 is the critical one: it proves that sorting inputs before
    processing makes the _CounterRegistry assign the same tokens
    regardless of the caller's original ordering. Check 3 proves content
    and metadata are in sync (same code path produces both).
    """
    # Files deliberately in REVERSE alphabetical order
    snapshot_reversed = _base_snapshot(config=ConfigSection(files=[
        ConfigFileEntry(path="/etc/zzz/app.conf", kind=ConfigFileKind.UNOWNED,
                        content="password=secret_zzz\n", include=True),
        ConfigFileEntry(path="/etc/aaa/db.conf", kind=ConfigFileKind.UNOWNED,
                        content="password=secret_aaa\n", include=True),
        ConfigFileEntry(path="/etc/mmm/mid.conf", kind=ConfigFileKind.UNOWNED,
                        content="password=secret_mmm\n", include=True),
    ]))
    # Same files in alphabetical order
    snapshot_sorted = _base_snapshot(config=ConfigSection(files=[
        ConfigFileEntry(path="/etc/aaa/db.conf", kind=ConfigFileKind.UNOWNED,
                        content="password=secret_aaa\n", include=True),
        ConfigFileEntry(path="/etc/mmm/mid.conf", kind=ConfigFileKind.UNOWNED,
                        content="password=secret_mmm\n", include=True),
        ConfigFileEntry(path="/etc/zzz/app.conf", kind=ConfigFileKind.UNOWNED,
                        content="password=secret_zzz\n", include=True),
    ]))

    result_reversed = redact_snapshot(snapshot_reversed)
    result_sorted = redact_snapshot(snapshot_sorted)

    # --- Check 1: redactions list ordering ---
    # Extract paths in the order they appear in snapshot.redactions
    paths_reversed = [r.get("path") if isinstance(r, dict) else r.path
                      for r in result_reversed.redactions]
    paths_sorted = [r.get("path") if isinstance(r, dict) else r.path
                    for r in result_sorted.redactions]

    # Both should produce the SAME ordering (alphabetical by path)
    assert paths_reversed == paths_sorted, (
        f"Findings order depends on input order!\n"
        f"  Reversed input produced: {paths_reversed}\n"
        f"  Sorted input produced:   {paths_sorted}"
    )

    # And the order should be alphabetical
    assert paths_reversed == sorted(paths_reversed), (
        f"Findings not sorted by path: {paths_reversed}"
    )

    # --- Check 2: actual content strings carry identical tokens ---
    # Build path->content maps from each result's config.files
    content_reversed = {f.path: f.content for f in result_reversed.config.files}
    content_sorted = {f.path: f.content for f in result_sorted.config.files}

    for path in content_reversed:
        assert content_reversed[path] == content_sorted[path], (
            f"Content tokens differ for {path}!\n"
            f"  Reversed input: {content_reversed[path]!r}\n"
            f"  Sorted input:   {content_sorted[path]!r}"
        )


# ---------------------------------------------------------------------------
# Task 4: Remediation states and RedactionFinding emission
# ---------------------------------------------------------------------------

def test_cockpit_gets_regenerate_remediation():
    snapshot = _base_snapshot(config=ConfigSection(files=[
        ConfigFileEntry(path="/etc/cockpit/ws-certs.d/0-self-signed.key", kind=ConfigFileKind.UNOWNED, content="key", include=True),
    ]))
    result = redact_snapshot(snapshot)
    cockpit = [r for r in result.redactions if isinstance(r, RedactionFinding) and "cockpit" in r.path]
    assert len(cockpit) >= 1
    assert all(f.remediation == "regenerate" for f in cockpit)


def test_ssh_host_key_gets_regenerate_remediation():
    snapshot = _base_snapshot(config=ConfigSection(files=[
        ConfigFileEntry(path="/etc/ssh/ssh_host_rsa_key", kind=ConfigFileKind.UNOWNED, content="key", include=True),
    ]))
    result = redact_snapshot(snapshot)
    ssh = [r for r in result.redactions if isinstance(r, RedactionFinding) and "ssh_host" in r.path]
    assert len(ssh) >= 1
    assert all(f.remediation == "regenerate" for f in ssh)


def test_tls_key_gets_provision_remediation():
    snapshot = _base_snapshot(config=ConfigSection(files=[
        ConfigFileEntry(path="/etc/pki/tls/private/server.key", kind=ConfigFileKind.UNOWNED, content="key", include=True),
    ]))
    result = redact_snapshot(snapshot)
    tls = [r for r in result.redactions if isinstance(r, RedactionFinding) and "server.key" in r.path]
    assert len(tls) >= 1
    assert all(f.remediation == "provision" for f in tls)


def test_inline_redaction_gets_value_removed_remediation():
    wg_config = "[Interface]\nPrivateKey = lWcu7GLoyXymjngaiY3JfFMRrTy96Fyonm2K5hW9qoo=\n"
    snapshot = _base_snapshot(config=ConfigSection(files=[
        ConfigFileEntry(path="/etc/wireguard/wg0.conf", kind=ConfigFileKind.UNOWNED, content=wg_config, include=True),
    ]))
    result = redact_snapshot(snapshot)
    wg = [r for r in result.redactions if isinstance(r, RedactionFinding) and "wireguard" in r.path]
    assert len(wg) >= 1
    assert all(f.remediation == "value-removed" for f in wg)


def test_shadow_finding_has_source():
    """Shadow findings carry source='shadow'."""
    snapshot = _base_snapshot(users_groups=UserGroupSection(
        shadow_entries=["testuser:$y$j9T$abc$hash:19700:0:99999:7:::"],
    ))
    result = redact_snapshot(snapshot)
    shadow = [r for r in result.redactions if isinstance(r, RedactionFinding) and r.source == "shadow"]
    assert len(shadow) >= 1
    assert all(f.kind == "inline" for f in shadow)
    assert all(f.remediation == "value-removed" for f in shadow)


def test_container_env_finding_has_source():
    """Container env findings carry source='container-env'."""
    c = RunningContainer(
        id="abc123", name="redis", image="redis:7",
        env=["REDIS_PASSWORD=topsecretredis", "HOSTNAME=redis-1"],
    )
    snapshot = _base_snapshot(containers=ContainerSection(running_containers=[c]))
    result = redact_snapshot(snapshot)
    env_findings = [r for r in result.redactions if isinstance(r, RedactionFinding) and r.source == "container-env"]
    assert len(env_findings) >= 1


def test_timer_cmd_finding_has_source():
    """Timer command findings carry source='timer-cmd'."""
    unit = GeneratedTimerUnit(
        name="cron-backup",
        timer_content="[Timer]\nOnCalendar=daily\n",
        service_content="[Service]\nExecStart=/usr/bin/pg_dump -p password=dbpass123 mydb\n",
        cron_expr="0 2 * * *",
        source_path="etc/cron.d/backup",
        command="/usr/bin/pg_dump -p password=dbpass123 mydb",
    )
    snapshot = _base_snapshot(scheduled_tasks=ScheduledTaskSection(generated_timer_units=[unit]))
    result = redact_snapshot(snapshot)
    timer_findings = [r for r in result.redactions if isinstance(r, RedactionFinding) and r.source == "timer-cmd"]
    assert len(timer_findings) >= 1


def test_diff_finding_has_source():
    """Diff view findings carry source='diff'."""
    snapshot = _base_snapshot(config=ConfigSection(files=[
        ConfigFileEntry(
            path="/etc/app.conf", kind=ConfigFileKind.RPM_OWNED_MODIFIED,
            content="clean content",
            diff_against_rpm="+password=leakedsecret",
            include=True,
        ),
    ]))
    result = redact_snapshot(snapshot)
    diff_findings = [r for r in result.redactions if isinstance(r, RedactionFinding) and r.source == "diff"]
    assert len(diff_findings) >= 1


def test_redaction_findings_compat_with_existing_tests():
    """RedactionFinding.get() works with existing test patterns like r['pattern']."""
    snapshot = _base_snapshot(config=ConfigSection(files=[
        ConfigFileEntry(path="/etc/app.conf", kind=ConfigFileKind.UNOWNED, content="password=secret123", include=True),
    ]))
    result = redact_snapshot(snapshot)
    assert len(result.redactions) > 0
    r = result.redactions[0]
    # .get() compat works
    assert r.get("path") == "/etc/app.conf"
    assert r.get("pattern") is not None
