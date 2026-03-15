"""Audit report, kickstart, README, and secrets review renderer output tests."""

import tempfile
from pathlib import Path

from jinja2 import Environment

from yoinkc.renderers.containerfile import render as render_containerfile
from yoinkc.renderers.audit_report import render as render_audit_report
from yoinkc.renderers.kickstart import render as render_kickstart
from yoinkc.renderers.readme import render as render_readme
from yoinkc.schema import InspectionSnapshot, OsRelease


class TestAuditReport:

    def _md(self, outputs):
        return (outputs["dir"] / "audit-report.md").read_text()

    def test_file_exists(self, outputs_with_baseline):
        assert (outputs_with_baseline["dir"] / "audit-report.md").exists()

    def test_expected_section_headers(self, outputs_with_baseline):
        md = self._md(outputs_with_baseline)
        assert "# Audit Report" in md
        assert "## Executive Summary" in md

    def test_packages_section_present(self, outputs_with_baseline):
        md = self._md(outputs_with_baseline)
        snapshot = outputs_with_baseline["snapshot"]
        if snapshot.rpm and snapshot.rpm.packages_added:
            assert "Package" in md or "package" in md

    def test_storage_section_when_fstab(self, outputs_with_baseline):
        md = self._md(outputs_with_baseline)
        snapshot = outputs_with_baseline["snapshot"]
        if snapshot.storage and snapshot.storage.fstab_entries:
            assert "Storage" in md or "fstab" in md.lower() or "Migration" in md

    def test_no_baseline_warning(self, outputs_no_baseline):
        md = self._md(outputs_no_baseline)
        assert "baseline" in md.lower() or "No baseline" in md

    def test_firewall_offline_cmd_in_audit_report_not_containerfile(self):
        """firewall-offline-cmd lines must appear in audit report, not Containerfile."""
        from yoinkc.schema import (
            InspectionSnapshot, OsRelease, NetworkSection, FirewallZone,
        )
        snapshot = InspectionSnapshot(
            meta={"host_root": "/host"},
            os_release=OsRelease(name="RHEL", version_id="9.6", pretty_name="RHEL 9.6"),
            network=NetworkSection(firewall_zones=[
                FirewallZone(
                    name="public",
                    path="etc/firewalld/zones/public.xml",
                    content="<zone/>",
                    services=["http", "https"],
                    ports=["8080/tcp"],
                    rich_rules=["rule family=ipv4 source address=10.0.0.0/8 accept"],
                ),
            ]),
        )
        with tempfile.TemporaryDirectory() as tmp:
            render_containerfile(snapshot, Environment(), Path(tmp))
            render_audit_report(snapshot, Environment(), Path(tmp))
            cf = (Path(tmp) / "Containerfile").read_text()
            md = (Path(tmp) / "audit-report.md").read_text()

        assert "# RUN firewall-offline-cmd" not in cf, \
            "firewall-offline-cmd comments must not appear in Containerfile"
        assert "audit-report.md" in cf, "Containerfile should point to audit report"
        assert "firewall-offline-cmd --zone=public --add-service=http" in md
        assert "firewall-offline-cmd --zone=public --add-service=https" in md
        assert "firewall-offline-cmd --zone=public --add-port=8080/tcp" in md
        assert "--add-rich-rule=" in md
        assert "Alternative: firewall-offline-cmd" in md

    def test_firewall_direct_rule_priority_used(self):
        """Direct rule commands must use the rule's actual priority, not hardcoded 0."""
        from yoinkc.schema import (
            InspectionSnapshot, OsRelease, NetworkSection, FirewallDirectRule,
        )
        snapshot = InspectionSnapshot(
            meta={"host_root": "/host"},
            os_release=OsRelease(name="RHEL", version_id="9.6", pretty_name="RHEL 9.6"),
            network=NetworkSection(firewall_direct_rules=[
                FirewallDirectRule(ipv="ipv4", table="filter", chain="INPUT",
                                   priority="5", args="-j ACCEPT"),
            ]),
        )
        with tempfile.TemporaryDirectory() as tmp:
            render_audit_report(snapshot, Environment(), Path(tmp))
            md = (Path(tmp) / "audit-report.md").read_text()

        assert "firewall-offline-cmd --direct --add-rule ipv4 filter INPUT 5 -j ACCEPT" in md, \
            "direct rule command must use actual priority (5), not hardcoded 0"

    def test_excluded_firewall_zone_not_written_to_config_tree(self):
        """Excluded firewall zones must not be written to the config tree or appear in the Containerfile."""
        from yoinkc.schema import (
            InspectionSnapshot, OsRelease, NetworkSection, FirewallZone,
        )
        snapshot = InspectionSnapshot(
            meta={"host_root": "/host"},
            os_release=OsRelease(name="RHEL", version_id="9.6", pretty_name="RHEL 9.6"),
            network=NetworkSection(firewall_zones=[
                FirewallZone(
                    name="public",
                    path="etc/firewalld/zones/public.xml",
                    content="<zone/>",
                    services=["http"],
                    include=True,
                ),
                FirewallZone(
                    name="internal",
                    path="etc/firewalld/zones/internal.xml",
                    content="<zone/>",
                    services=["ssh"],
                    include=False,
                ),
            ]),
        )
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp)
            render_containerfile(snapshot, Environment(), out)
            cf = (out / "Containerfile").read_text()
            public_xml_exists = (out / "config" / "etc" / "firewalld" / "zones" / "public.xml").exists()
            internal_xml_exists = (out / "config" / "etc" / "firewalld" / "zones" / "internal.xml").exists()

        assert public_xml_exists, "Included zone file must be written to config tree"
        assert not internal_xml_exists, "Excluded zone file must not be written to config tree"
        assert "public" in cf, "Included zone must appear in Containerfile header"
        assert "internal" not in cf, "Excluded zone must not appear in Containerfile header"

    def test_excluded_firewall_direct_rule_not_written(self):
        """Excluded direct rules must not be written to direct.xml."""
        from yoinkc.schema import (
            InspectionSnapshot, OsRelease, NetworkSection, FirewallDirectRule,
        )
        snapshot = InspectionSnapshot(
            meta={"host_root": "/host"},
            os_release=OsRelease(name="RHEL", version_id="9.6", pretty_name="RHEL 9.6"),
            network=NetworkSection(firewall_direct_rules=[
                FirewallDirectRule(ipv="ipv4", chain="INPUT", args="-j ACCEPT", include=True),
                FirewallDirectRule(ipv="ipv4", chain="OUTPUT", args="-j DROP", include=False),
            ]),
        )
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp)
            render_containerfile(snapshot, Environment(), out)
            direct_xml = (out / "config" / "etc" / "firewalld" / "direct.xml").read_text()
            assert "-j ACCEPT" in direct_xml, "Included direct rule must appear in direct.xml"
            assert "-j DROP" not in direct_xml, "Excluded direct rule must not appear in direct.xml"

    def test_all_excluded_firewall_direct_rules_no_direct_xml(self):
        """If all direct rules are excluded, direct.xml must not be written at all."""
        from yoinkc.schema import (
            InspectionSnapshot, OsRelease, NetworkSection, FirewallDirectRule,
        )
        snapshot = InspectionSnapshot(
            meta={"host_root": "/host"},
            os_release=OsRelease(name="RHEL", version_id="9.6", pretty_name="RHEL 9.6"),
            network=NetworkSection(firewall_direct_rules=[
                FirewallDirectRule(ipv="ipv4", chain="INPUT", args="-j ACCEPT", include=False),
            ]),
        )
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp)
            render_containerfile(snapshot, Environment(), out)
            direct_xml_exists = (out / "config" / "etc" / "firewalld" / "direct.xml").exists()

        assert not direct_xml_exists, "direct.xml must not be written when all rules are excluded"


class TestKickstart:

    def _ks(self, outputs):
        return (outputs["dir"] / "kickstart-suggestion.ks").read_text()

    def test_file_exists(self, outputs_with_baseline):
        assert (outputs_with_baseline["dir"] / "kickstart-suggestion.ks").exists()

    def test_dhcp_connections_produce_network_directive(self, outputs_with_baseline):
        ks = self._ks(outputs_with_baseline)
        snapshot = outputs_with_baseline["snapshot"]
        if snapshot.network:
            dhcp = [c for c in snapshot.network.connections if c.method == "dhcp"]
            if dhcp:
                assert "network --bootproto=dhcp" in ks

    def test_static_connections_absent_from_kickstart(self, outputs_with_baseline):
        """Static connections are baked into the image, not in kickstart."""
        ks = self._ks(outputs_with_baseline)
        snapshot = outputs_with_baseline["snapshot"]
        if snapshot.network:
            static = [c for c in snapshot.network.connections if c.method == "static"]
            for c in static:
                assert f"network --bootproto=static --device={c.name}\n" not in ks

    def test_proxy_settings_when_present(self, outputs_with_baseline):
        ks = self._ks(outputs_with_baseline)
        snapshot = outputs_with_baseline["snapshot"]
        if snapshot.network and snapshot.network.proxy:
            for p in snapshot.network.proxy:
                if "=" in p.line:
                    assert p.line in ks

    def test_has_comment_header(self, outputs_with_baseline):
        ks = self._ks(outputs_with_baseline)
        assert "# Kickstart suggestion" in ks


class TestReadme:

    def _readme(self, outputs):
        return (outputs["dir"] / "README.md").read_text()

    def test_file_exists(self, outputs_with_baseline):
        assert (outputs_with_baseline["dir"] / "README.md").exists()

    def test_podman_build_command_present(self, outputs_with_baseline):
        readme = self._readme(outputs_with_baseline)
        assert "podman build" in readme

    def test_os_description_present(self, outputs_with_baseline):
        """README should mention the detected OS."""
        readme = self._readme(outputs_with_baseline)
        snapshot = outputs_with_baseline["snapshot"]
        if snapshot.os_release:
            os_name = snapshot.os_release.name or snapshot.os_release.pretty_name
            if os_name:
                assert os_name in readme or snapshot.os_release.version_id in readme

    def test_fixme_count_or_checklist(self, outputs_with_baseline):
        """README should contain a FIXME checklist or mention FIXMEs."""
        readme = self._readme(outputs_with_baseline)
        assert "FIXME" in readme or "checklist" in readme.lower() or "TODO" in readme


class TestAuditModifications:
    """Tests for the operator modifications section."""

    def test_modifications_section_with_edits(self):
        """Audit report includes Modifications section when files are edited."""
        from yoinkc.schema import ConfigFileEntry, ConfigFileKind, ConfigSection
        original = InspectionSnapshot(
            meta={"host_root": "/host"},
            os_release=OsRelease(name="RHEL", version_id="9.6", pretty_name="RHEL 9.6"),
            config=ConfigSection(files=[
                ConfigFileEntry(path="/etc/myapp/app.conf", kind=ConfigFileKind.RPM_OWNED_MODIFIED, content="original"),
            ]),
        )
        modified = InspectionSnapshot(
            meta={"host_root": "/host"},
            os_release=OsRelease(name="RHEL", version_id="9.6", pretty_name="RHEL 9.6"),
            config=ConfigSection(files=[
                ConfigFileEntry(path="/etc/myapp/app.conf", kind=ConfigFileKind.RPM_OWNED_MODIFIED, content="changed"),
            ]),
        )
        with tempfile.TemporaryDirectory() as tmp:
            render_audit_report(modified, Environment(), Path(tmp), original_snapshot=original)
            md = (Path(tmp) / "audit-report.md").read_text()
        assert "## Modifications" in md
        assert "Edited" in md
        assert "/etc/myapp/app.conf" in md

    def test_modifications_section_with_added_files(self):
        from yoinkc.schema import ConfigFileEntry, ConfigFileKind, ConfigSection
        original = InspectionSnapshot(
            meta={"host_root": "/host"},
            os_release=OsRelease(name="RHEL", version_id="9.6", pretty_name="RHEL 9.6"),
        )
        modified = InspectionSnapshot(
            meta={"host_root": "/host"},
            os_release=OsRelease(name="RHEL", version_id="9.6", pretty_name="RHEL 9.6"),
            config=ConfigSection(files=[
                ConfigFileEntry(path="/etc/new.conf", kind=ConfigFileKind.UNOWNED, content="new"),
            ]),
        )
        with tempfile.TemporaryDirectory() as tmp:
            render_audit_report(modified, Environment(), Path(tmp), original_snapshot=original)
            md = (Path(tmp) / "audit-report.md").read_text()
        assert "## Modifications" in md
        assert "Added" in md
        assert "/etc/new.conf" in md

    def test_no_modifications_section_when_unchanged(self):
        snap = InspectionSnapshot(
            meta={"host_root": "/host"},
            os_release=OsRelease(name="RHEL", version_id="9.6", pretty_name="RHEL 9.6"),
        )
        with tempfile.TemporaryDirectory() as tmp:
            render_audit_report(snap, Environment(), Path(tmp), original_snapshot=snap)
            md = (Path(tmp) / "audit-report.md").read_text()
        assert "## Modifications" not in md


class TestSecretsReview:

    def _sr(self, outputs):
        return (outputs["dir"] / "secrets-review.md").read_text()

    def test_file_exists(self, outputs_with_baseline):
        assert (outputs_with_baseline["dir"] / "secrets-review.md").exists()

    def test_redaction_entries_listed(self, outputs_with_baseline):
        sr = self._sr(outputs_with_baseline)
        snapshot = outputs_with_baseline["snapshot"]
        for r in snapshot.redactions[:5]:
            path = r.get("path", "")
            if path:
                assert path in sr

    def test_has_table_structure(self, outputs_with_baseline):
        sr = self._sr(outputs_with_baseline)
        snapshot = outputs_with_baseline["snapshot"]
        if snapshot.redactions:
            assert "|" in sr

    def test_remediation_text_present(self, outputs_with_baseline):
        sr = self._sr(outputs_with_baseline)
        snapshot = outputs_with_baseline["snapshot"]
        if snapshot.redactions:
            assert "secret store" in sr.lower() or "deploy time" in sr.lower() or "manually" in sr.lower()
