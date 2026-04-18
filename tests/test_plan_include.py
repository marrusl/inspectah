"""Plan item tests: include field defaults, CLI flag rejection, cross-cutting smoke test."""

import tempfile
from pathlib import Path

import pytest

from inspectah.schema import (
    ComposeFile,
    ConfigFileEntry,
    ConfigFileKind,
    ConfigSection,
    CronJob,
    FstabEntry,
    FirewallZone,
    GeneratedTimerUnit,
    InspectionSnapshot,
    KernelBootSection,
    KernelModule,
    QuadletUnit,
    SysctlOverride,
    NMConnection,
    NetworkSection,
    NonRpmItem,
    NonRpmSoftwareSection,
    OsRelease,
    PackageEntry,
    ProxyEntry,
    RepoFile,
    RpmSection,
    ScheduledTaskSection,
    SelinuxSection,
    ServiceSection,
    ServiceStateChange,
    StaticRouteFile,
    StorageSection,
    UserGroupSection,
)


class TestIncludeFieldDefaults:
    """Every toggleable model defaults include=True."""

    def test_package_entry_defaults_true(self):
        p = PackageEntry(name="x", version="1", release="1", arch="x86_64")
        assert p.include is True

    def test_config_file_entry_defaults_true(self):
        c = ConfigFileEntry(path="/etc/foo", kind=ConfigFileKind.UNOWNED)
        assert c.include is True

    def test_service_state_change_defaults_true(self):
        s = ServiceStateChange(unit="x.service", current_state="enabled", default_state="disabled", action="enable")
        assert s.include is True

    def test_cron_job_defaults_true(self):
        assert CronJob(path="etc/cron.d/x", source="cron.d").include is True

    def test_generated_timer_unit_defaults_true(self):
        assert GeneratedTimerUnit(name="x").include is True

    def test_non_rpm_item_defaults_true(self):
        assert NonRpmItem(path="/opt/x").include is True

    def test_sysctl_override_defaults_true(self):
        assert SysctlOverride(key="net.ipv4.ip_forward").include is True

    def test_kernel_module_defaults_true(self):
        assert KernelModule(name="vfat").include is True

    def test_quadlet_unit_defaults_true(self):
        assert QuadletUnit(path="/etc/containers/systemd/x.container", name="x.container").include is True

    def test_compose_file_defaults_true(self):
        assert ComposeFile(path="/opt/app/docker-compose.yml").include is True

    def test_repofile_include_defaults_true(self):
        assert RepoFile(path="etc/yum.repos.d/epel.repo", content="").include is True


def test_profile_flag_rejected():
    from inspectah.cli import parse_args
    with pytest.raises(SystemExit):
        parse_args(["--profile", "server"])


def test_comps_file_flag_rejected():
    from inspectah.cli import parse_args
    with pytest.raises(SystemExit):
        parse_args(["--comps-file", "/tmp/comps.xml"])


def test_all_features_render_together():
    """Exercises every new code path in a single rich snapshot."""
    snapshot = InspectionSnapshot(
        meta={"hostname": "test-host"},
        os_release=OsRelease(name="CentOS Stream", version_id="9", id="centos",
                             pretty_name="CentOS Stream 9"),
        rpm=RpmSection(
            packages_added=[PackageEntry(name="httpd", version="2.4", release="1.el9", arch="x86_64")],
            base_image="quay.io/centos-bootc/centos-bootc:stream9",
            baseline_package_names=["bash", "coreutils"],
        ),
        config=ConfigSection(files=[ConfigFileEntry(
            path="/etc/httpd/conf/httpd.conf", kind=ConfigFileKind.RPM_OWNED_MODIFIED,
            content="ServerRoot /etc/httpd\n", rpm_va_flags="S.5....T.", package="httpd",
            diff_against_rpm="--- rpm\n+++ current\n@@ -1 +1 @@\n-Listen 80\n+Listen 8080\n",
        )]),
        services=ServiceSection(
            state_changes=[ServiceStateChange(
                unit="httpd.service", current_state="enabled",
                default_state="disabled", action="enable",
            )],
            enabled_units=["httpd.service"],
        ),
        network=NetworkSection(
            connections=[NMConnection(name="eth0", method="static", path="etc/NM/eth0.nmconnection")],
            firewall_zones=[FirewallZone(name="public", path="etc/firewalld/zones/public.xml",
                                        content="<zone/>", services=["ssh"], ports=[], rich_rules=[])],
            static_routes=[StaticRouteFile(path="etc/sysconfig/network-scripts/route-eth0", name="route-eth0")],
            hosts_additions=["10.0.0.5 api-server"],
            proxy=[ProxyEntry(source="etc/environment", line="http_proxy=http://proxy:3128")],
            resolv_provenance="networkmanager",
        ),
        storage=StorageSection(fstab_entries=[
            FstabEntry(device="/dev/sda1", mount_point="/", fstype="xfs"),
            FstabEntry(device="nas:/data", mount_point="/data", fstype="nfs"),
        ]),
        scheduled_tasks=ScheduledTaskSection(
            cron_jobs=[CronJob(path="etc/cron.d/backup", source="cron.d")],
            generated_timer_units=[GeneratedTimerUnit(
                name="cron-backup",
                timer_content="[Timer]\nOnCalendar=*-*-* 02:00:00\n",
                service_content="[Service]\nExecStart=/usr/local/bin/backup.sh\n",
                cron_expr="0 2 * * *", source_path="etc/cron.d/backup",
                command="/usr/local/bin/backup.sh",
            )],
        ),
        non_rpm_software=NonRpmSoftwareSection(items=[
            NonRpmItem(name="cryptography", version="41.0.0", method="pip dist-info",
                       has_c_extensions=True, confidence="high",
                       path="usr/lib/python3.9/site-packages/cryptography-41.0.0.dist-info"),
            NonRpmItem(name="requests", version="2.32.5", method="pip dist-info",
                       confidence="high",
                       path="usr/lib/python3.9/site-packages/requests-2.32.5.dist-info"),
        ]),
        users_groups=UserGroupSection(
            users=[{"name": "mark", "uid": "1000", "gid": "1000",
                    "home": "/home/mark", "shell": "/bin/bash"}],
            passwd_entries=["mark:x:1000:1000::/home/mark:/bin/bash"],
            shadow_entries=["mark:!!:19700:0:99999:7:::"],
            group_entries=["mark:x:1000:"],
            gshadow_entries=["mark:!::"],
            sudoers_rules=["mark ALL=(ALL) NOPASSWD: ALL"],
            ssh_authorized_keys_refs=[{"user": "mark", "path": "/home/mark/.ssh/authorized_keys"}],
        ),
        kernel_boot=KernelBootSection(
            sysctl_overrides=[SysctlOverride(key="net.ipv4.ip_forward", runtime="1", default="0", source="operator")],
            non_default_modules=[KernelModule(name="br_netfilter", size="32768", used_by="")],
        ),
        selinux=SelinuxSection(
            mode="enforcing", custom_modules=["mypolicy"],
            boolean_overrides=[{"name": "httpd_can_network_connect", "current": "on",
                                "default": "off", "non_default": True}],
        ),
    )
    with tempfile.TemporaryDirectory() as tmp:
        from inspectah.renderers import run_all
        run_all(snapshot, Path(tmp))
        content = (Path(tmp) / "Containerfile").read_text()

    assert "AS builder" in content
    assert "# === Base Image ===" in content
    assert "# === Service Enablement ===" in content
    assert "# === Firewall Configuration (bake into image) ===" in content
    assert "# === Scheduled Tasks ===" in content
    assert "# === Non-RPM Software ===" in content
    assert "# === Users and Groups ===" in content
    assert "# === Kernel Configuration ===" in content
    assert "# === SELinux Customizations ===" in content
    assert "# === Network / Kickstart ===" in content
    assert "# === tmpfiles.d for /var structure ===" in content
