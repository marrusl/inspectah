"""
Comprehensive renderer output tests.

Two fixture variants are tested:
  - with_baseline: snapshot has a resolved base image package list
  - no_baseline:   snapshot has no_baseline=True (all packages listed)

All five renderers are exercised:
  Containerfile, HTML report, audit report, kickstart, README, secrets review.
"""

import re
import tempfile
from pathlib import Path
from typing import Optional
from unittest.mock import patch

import pytest
from jinja2 import Environment

import yoinkc.preflight as preflight_mod
from yoinkc.executor import RunResult
from yoinkc.inspectors import run_all as run_all_inspectors
from yoinkc.redact import redact_snapshot
from yoinkc.renderers import run_all as run_all_renderers
from yoinkc.renderers import html_report
from yoinkc.renderers.containerfile import render as render_containerfile
from yoinkc.renderers.html_report import render as render_html_report
from yoinkc.renderers.audit_report import render as render_audit_report
from yoinkc.renderers.kickstart import render as render_kickstart
from yoinkc.renderers.readme import render as render_readme
from yoinkc.schema import (
    FleetPrevalence,
    InspectionSnapshot,
    OsRelease,
    PackageEntry,
    RpmSection,
)
FIXTURES = Path(__file__).parent / "fixtures"


# ---------------------------------------------------------------------------
# Executor + fixture infrastructure
# ---------------------------------------------------------------------------

def _make_executor(pkg_list: Optional[str] = None):
    """Return a fixture executor.  If pkg_list is None, the baseline podman call fails."""
    def executor(cmd, cwd=None):
        if cmd[-1] == "true" and "nsenter" in cmd:
            return RunResult(stdout="", stderr="", returncode=0)
        c = " ".join(cmd)
        if "podman" in c and "login" in c and "--get-login" in c:
            return RunResult(stdout="testuser\n", stderr="", returncode=0)
        if "podman" in c and "image" in c and "exists" in c:
            return RunResult(stdout="", stderr="", returncode=0)
        if "podman" in c and "rpm" in c:
            if pkg_list is not None:
                return RunResult(stdout=pkg_list, stderr="", returncode=0)
            return RunResult(stdout="", stderr="Error: podman unavailable", returncode=1)
        if "rpm" in c and "-qa" in c:
            return RunResult(stdout=(FIXTURES / "rpm_qa_output.txt").read_text(), stderr="", returncode=0)
        if "rpm" in c and "-Va" in c:
            return RunResult(stdout=(FIXTURES / "rpm_va_output.txt").read_text(), stderr="", returncode=0)
        if "dnf" in c and "list" in c:
            return RunResult(stdout=(FIXTURES / "dnf_history_list.txt").read_text(), stderr="", returncode=0)
        if "dnf" in c and "info" in c and "4" in c:
            return RunResult(stdout=(FIXTURES / "dnf_history_info_4.txt").read_text(), stderr="", returncode=0)
        if "rpm" in c and "-ql" in c:
            return RunResult(stdout=(FIXTURES / "rpm_qla_output.txt").read_text(), stderr="", returncode=0)
        if "systemctl" in c:
            return RunResult(stdout=(FIXTURES / "systemctl_list_unit_files.txt").read_text(), stderr="", returncode=0)
        if "semodule" in c and "-l" in c:
            return RunResult(stdout=(FIXTURES / "semodule_l_output.txt").read_text(), stderr="", returncode=0)
        if "semanage" in c and "boolean" in c:
            return RunResult(stdout=(FIXTURES / "semanage_boolean_l_output.txt").read_text(), stderr="", returncode=0)
        if "lsmod" in c:
            return RunResult(stdout=(FIXTURES / "lsmod_output.txt").read_text(), stderr="", returncode=0)
        if "ip" in c and "route" in c:
            return RunResult(stdout=(FIXTURES / "ip_route_output.txt").read_text(), stderr="", returncode=0)
        if "ip" in c and "rule" in c:
            return RunResult(stdout=(FIXTURES / "ip_rule_output.txt").read_text(), stderr="", returncode=0)
        return RunResult(stdout="", stderr="", returncode=1)
    return executor


def _build_snapshot(with_baseline: bool):
    pkg_list = (FIXTURES / "base_image_packages.txt").read_text() if with_baseline else None
    with patch.object(preflight_mod, "in_user_namespace", return_value=False):
        snapshot = run_all_inspectors(
            FIXTURES / "host_etc",
            executor=_make_executor(pkg_list),
            no_baseline_opt_in=not with_baseline,
        )
    return redact_snapshot(snapshot)


@pytest.fixture(scope="module")
def outputs_with_baseline(tmp_path_factory):
    """Full renderer outputs built with baseline resolved."""
    tmp = tmp_path_factory.mktemp("with_baseline")
    snapshot = _build_snapshot(with_baseline=True)
    run_all_renderers(snapshot, tmp)
    return {"snapshot": snapshot, "dir": tmp}


@pytest.fixture(scope="module")
def outputs_no_baseline(tmp_path_factory):
    """Full renderer outputs built without baseline (no_baseline=True)."""
    tmp = tmp_path_factory.mktemp("no_baseline")
    snapshot = _build_snapshot(with_baseline=False)
    run_all_renderers(snapshot, tmp)
    return {"snapshot": snapshot, "dir": tmp}


# ===========================================================================
# Containerfile tests
# ===========================================================================

class TestContainerfile:

    def _cf(self, outputs):
        return (outputs["dir"] / "Containerfile").read_text()

    def test_file_exists(self, outputs_with_baseline):
        assert (outputs_with_baseline["dir"] / "Containerfile").exists()

    def test_layer_ordering(self, outputs_with_baseline):
        """Section headers must appear in design-doc order."""
        cf = self._cf(outputs_with_baseline)
        order = [
            "# === Base Image ===",
            "# === Package Installation ===",
            "# === Service Enablement ===",
            "# === Configuration Files ===",
        ]
        positions = [cf.index(h) for h in order if h in cf]
        assert positions == sorted(positions), "Layer ordering violated"

    def test_from_line_present(self, outputs_with_baseline):
        cf = self._cf(outputs_with_baseline)
        assert re.search(r"^FROM ", cf, re.MULTILINE), "No FROM line found"

    def test_dnf_install_has_packages(self, outputs_with_baseline):
        """dnf install block must include known added packages."""
        cf = self._cf(outputs_with_baseline)
        assert "RUN dnf install -y \\" in cf

    def test_no_per_file_copy_etc(self, outputs_with_baseline):
        """No individual COPY config/etc/specific/file lines — must be consolidated."""
        cf = self._cf(outputs_with_baseline)
        # Per-file COPYs look like: COPY config/etc/httpd/conf/httpd.conf /etc/httpd/...
        # The consolidated form is: COPY config/etc/ /etc/
        # Intentional early directory COPYs (before a specific RUN) are excluded:
        #   - /rpm-gpg/ (GPG keys before dnf install)
        #   - /systemd/system/ (timer units before systemctl enable)
        per_file = [
            line for line in
            re.findall(r"^COPY config/etc/[^\s/]+/[^\s]+\s+/etc/[^\s]+$", cf, re.MULTILINE)
            if "/rpm-gpg/" not in line and "/systemd/system/" not in line
        ]
        assert len(per_file) == 0, f"Found per-file COPY lines: {per_file[:5]}"

    def test_consolidated_copy_etc_present(self, outputs_with_baseline):
        """COPY config/etc/ /etc/ must be present."""
        cf = self._cf(outputs_with_baseline)
        assert "COPY config/etc/ /etc/" in cf

    def test_user_strategy_in_containerfile(self, outputs_with_baseline):
        """Users must be rendered according to their strategy."""
        cf = self._cf(outputs_with_baseline)
        snapshot = outputs_with_baseline["snapshot"]
        ug = snapshot.users_groups
        if ug and ug.users:
            assert "Users and Groups" in cf

    def test_no_baseline_mode_comment(self, outputs_no_baseline):
        """No-baseline mode should include a comment about all packages."""
        cf = self._cf(outputs_no_baseline)
        assert "No baseline" in cf

    def test_no_baseline_includes_packages(self, outputs_no_baseline):
        """No-baseline mode must still have a dnf install block."""
        cf = self._cf(outputs_no_baseline)
        assert "RUN dnf install -y \\" in cf

    def test_config_tree_etc_matches_copy(self, outputs_with_baseline):
        """config/etc/ must exist and be non-empty (matches the COPY source)."""
        config_etc = outputs_with_baseline["dir"] / "config" / "etc"
        assert config_etc.is_dir()
        files = list(config_etc.rglob("*"))
        assert any(f.is_file() for f in files), "config/etc/ is empty"

    def test_tmpfiles_written_to_config_etc(self, outputs_with_baseline):
        """yoinkc-var.conf must exist inside config/etc/tmpfiles.d/."""
        tmpfiles = outputs_with_baseline["dir"] / "config/etc/tmpfiles.d/yoinkc-var.conf"
        assert tmpfiles.exists()

    def test_fixme_comments_present(self, outputs_with_baseline):
        """FIXME comments must be present for items needing manual attention."""
        cf = self._cf(outputs_with_baseline)
        assert "FIXME" in cf

    def test_quadlet_copy_present(self, outputs_with_baseline):
        """Quadlet units must be copied via COPY quadlet/ /etc/containers/systemd/."""
        cf = self._cf(outputs_with_baseline)
        snapshot = outputs_with_baseline["snapshot"]
        if snapshot.containers and snapshot.containers.quadlet_units:
            assert "COPY quadlet/ /etc/containers/systemd/" in cf


# ===========================================================================
# HTML report tests
# ===========================================================================

class TestHtmlReport:

    def _html(self, outputs):
        return (outputs["dir"] / "report.html").read_text()

    def test_file_exists(self, outputs_with_baseline):
        assert (outputs_with_baseline["dir"] / "report.html").exists()

    def test_valid_html_structure(self, outputs_with_baseline):
        html = self._html(outputs_with_baseline)
        assert "<!DOCTYPE html>" in html or html.lstrip().startswith("<!DOCTYPE html>")
        assert "</html>" in html
        assert "<head>" in html and "<body>" in html

    def test_all_section_ids_present(self, outputs_with_baseline):
        html = self._html(outputs_with_baseline)
        for section in ["summary", "packages", "services", "config", "network",
                        "storage", "scheduled_tasks", "containers", "non_rpm",
                        "kernel_boot", "selinux", "users_groups", "warnings",
                        "containerfile", "output_files", "audit"]:
            assert f'id="section-{section}"' in html, f"Missing section: {section}"

    def test_warnings_panel_populated(self, outputs_with_baseline):
        """If there are warnings, the warnings tab alert-group should appear."""
        html = self._html(outputs_with_baseline)
        snapshot = outputs_with_baseline["snapshot"]
        if snapshot.warnings:
            assert "warnings-list" in html

    def test_redacted_tokens_in_report(self, outputs_with_baseline):
        """Redacted secrets should appear in the report as REDACTED_... tokens."""
        html = self._html(outputs_with_baseline)
        snapshot = outputs_with_baseline["snapshot"]
        if snapshot.redactions:
            # The warnings panel lists redactions
            assert "Redacted:" in html or "EXCLUDED_PATH" in html or "redaction" in html

    def test_containerfile_section_has_from(self, outputs_with_baseline):
        html = self._html(outputs_with_baseline)
        assert "containerfile-pre" in html
        assert "FROM " in html

    def test_reset_button_present(self, outputs_with_baseline):
        """Reset button should be in the toolbar, disabled by default."""
        html = self._html(outputs_with_baseline)
        assert 'id="btn-reset"' in html
        assert "disabled" in html.split('id="btn-reset"')[1].split(">")[0]

    def test_original_snapshot_embedded(self, outputs_with_baseline):
        """Page JS should deep-copy the snapshot for reset support."""
        html = self._html(outputs_with_baseline)
        assert "var originalSnapshot = JSON.parse(JSON.stringify(snapshot));" in html


# ===========================================================================
# Audit report tests
# ===========================================================================

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
            # Should mention packages
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


# ===========================================================================
# Kickstart tests
# ===========================================================================

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
                # They may appear as a comment reference but not as active directives
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


# ===========================================================================
# README tests
# ===========================================================================

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


# ===========================================================================
# Secrets review tests
# ===========================================================================

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
            # Should have a markdown table with | separators
            assert "|" in sr

    def test_remediation_text_present(self, outputs_with_baseline):
        sr = self._sr(outputs_with_baseline)
        snapshot = outputs_with_baseline["snapshot"]
        if snapshot.redactions:
            assert "secret store" in sr.lower() or "deploy time" in sr.lower() or "manually" in sr.lower()


# ===========================================================================
# Containerfile quality checks (moved from test_renderers.py)
# ===========================================================================

class TestContainerfileQuality:

    def test_copy_targets_exist(self, outputs_with_baseline):
        """Every COPY source in the Containerfile must exist on disk."""
        output_dir = outputs_with_baseline["dir"]
        cf = (output_dir / "Containerfile").read_text()
        for i, line in enumerate(cf.splitlines(), 1):
            if line.startswith("#"):
                continue
            m = re.match(r"^COPY\s+(config/\S+|quadlet/\S*)", line)
            if m:
                src = m.group(1)
                src_path = output_dir / src
                assert src_path.exists(), f"COPY source missing at line {i}: {src}"

    def test_fixme_comments_are_actionable(self, outputs_with_baseline):
        """Every FIXME comment must explain what the operator needs to do."""
        cf = (outputs_with_baseline["dir"] / "Containerfile").read_text()
        for i, line in enumerate(cf.splitlines(), 1):
            if "FIXME" in line:
                after = line.split("FIXME", 1)[1].strip().lstrip(":").strip()
                assert len(after) > 10, (
                    f"FIXME at line {i} is not actionable (too short): {line.strip()!r}"
                )

    def test_syntax_valid(self, outputs_with_baseline):
        """Containerfile uses only valid Dockerfile instructions."""
        cf = (outputs_with_baseline["dir"] / "Containerfile").read_text()
        VALID = {"FROM", "RUN", "COPY", "ADD", "ENV", "ARG", "LABEL", "EXPOSE",
                 "ENTRYPOINT", "CMD", "VOLUME", "USER", "WORKDIR", "ONBUILD",
                 "STOPSIGNAL", "HEALTHCHECK", "SHELL"}
        in_continuation = False
        had_from = False
        for i, line in enumerate(cf.splitlines(), 1):
            stripped = line.strip()
            if not stripped or stripped.startswith("#"):
                in_continuation = False
                continue
            if in_continuation:
                in_continuation = stripped.endswith("\\")
                continue
            m = re.match(r"^([A-Z]+)\s", stripped)
            if m:
                instr = m.group(1)
                assert instr in VALID, f"Unknown instruction at line {i}: {instr}"
                if instr == "FROM":
                    had_from = True
                in_continuation = stripped.endswith("\\")
            else:
                assert line[0] in (" ", "\t"), (
                    f"Line {i} is not a valid instruction or continuation: {stripped[:80]!r}"
                )
                in_continuation = stripped.endswith("\\")
        assert had_from, "Containerfile is missing a FROM instruction"

    def test_non_rpm_provenance(self, outputs_with_baseline):
        """Known-provenance items get real directives; unknown get commented stubs."""
        cf = (outputs_with_baseline["dir"] / "Containerfile").read_text()
        output_dir = outputs_with_baseline["dir"]

        # pip packages produce real RUN pip install
        assert re.search(r"^RUN pip install", cf, re.MULTILINE)
        assert "flask==3.1.3" in cf
        assert "requests==2.32.5" in cf

        # npm lockfiles produce real COPY + RUN
        assert re.search(r"^COPY config/opt/myapp/", cf, re.MULTILINE)
        assert re.search(r"^RUN cd /opt/myapp && npm ci", cf, re.MULTILINE)
        assert (output_dir / "config" / "opt" / "myapp" / "package-lock.json").exists()


# ===========================================================================
# Kernel kargs.d migration
# ===========================================================================

class TestKernelKargs:
    """Tests for the bootc-native kargs.d migration (replaces rpm-ostree kargs).

    The fixture cmdline is:
        BOOT_IMAGE=... root=/dev/vda1 ro crashkernel=auto rhgb quiet
        hugepagesz=2M transparent_hugepage=never
    Only hugepagesz and transparent_hugepage are operator-added; the rest are
    standard bootloader/installer parameters and must be excluded from the TOML.
    """

    def test_kargs_toml_generated(self, outputs_with_baseline):
        """TOML drop-in is written for operator-added kargs."""
        toml_path = (outputs_with_baseline["dir"]
                     / "config/usr/lib/bootc/kargs.d/yoinkc-migrated.toml")
        assert toml_path.exists(), "kargs TOML not written"

    def test_kargs_toml_contains_operator_args(self, outputs_with_baseline):
        """Operator-added kargs from the fixture appear in the TOML array."""
        toml_path = (outputs_with_baseline["dir"]
                     / "config/usr/lib/bootc/kargs.d/yoinkc-migrated.toml")
        content = toml_path.read_text()
        assert '"hugepagesz=2M"' in content
        assert '"transparent_hugepage=never"' in content

    def test_kargs_toml_excludes_bootloader_params(self, outputs_with_baseline):
        """Standard bootloader/installer parameters are NOT written to the TOML."""
        toml_path = (outputs_with_baseline["dir"]
                     / "config/usr/lib/bootc/kargs.d/yoinkc-migrated.toml")
        content = toml_path.read_text()
        for excluded in ("BOOT_IMAGE", "root=", '"ro"', '"rhgb"', '"quiet"', "crashkernel"):
            assert excluded not in content, (
                f"Bootloader param {excluded!r} should not appear in kargs TOML:\n{content}"
            )

    def test_kargs_toml_format(self, outputs_with_baseline):
        """TOML content uses the correct kargs array format."""
        toml_path = (outputs_with_baseline["dir"]
                     / "config/usr/lib/bootc/kargs.d/yoinkc-migrated.toml")
        content = toml_path.read_text()
        assert re.search(r'^kargs = \[".+"\]', content, re.MULTILINE), (
            f"kargs TOML does not have expected array format:\n{content}"
        )

    def test_containerfile_uses_kargs_copy(self, outputs_with_baseline):
        """Containerfile references the kargs TOML via COPY, not rpm-ostree kargs."""
        cf = (outputs_with_baseline["dir"] / "Containerfile").read_text()
        assert "rpm-ostree kargs" not in cf, "Containerfile still references rpm-ostree kargs"
        assert "COPY config/usr/lib/bootc/kargs.d/yoinkc-migrated.toml /usr/lib/bootc/kargs.d/" in cf
        assert "RUN mkdir -p /usr/lib/bootc/kargs.d" in cf

    def test_kargs_section_header_in_containerfile(self, outputs_with_baseline):
        """Containerfile contains the bootc-native kargs section header."""
        cf = (outputs_with_baseline["dir"] / "Containerfile").read_text()
        assert "Kernel Arguments (bootc-native kargs.d)" in cf

    def test_no_kargs_toml_when_no_cmdline(self):
        """No TOML file and no kargs section when kernel_boot has no cmdline."""
        from yoinkc.schema import InspectionSnapshot, OsRelease, KernelBootSection
        snapshot = InspectionSnapshot(
            meta={"host_root": "/host"},
            os_release=OsRelease(name="RHEL", version_id="9.6"),
            kernel_boot=KernelBootSection(cmdline=""),
        )
        with tempfile.TemporaryDirectory() as tmp:
            output_dir = Path(tmp)
            render_containerfile(snapshot, Environment(), output_dir)
            toml_path = output_dir / "config/usr/lib/bootc/kargs.d/yoinkc-migrated.toml"
            assert not toml_path.exists(), "TOML written for empty cmdline"
            cf = (output_dir / "Containerfile").read_text()
            assert "kargs.d" not in cf
            assert "rpm-ostree kargs" not in cf

    def test_no_kargs_toml_when_only_bootloader_params(self):
        """No TOML file or kargs section when cmdline contains only standard boot params."""
        from yoinkc.schema import InspectionSnapshot, OsRelease, KernelBootSection
        snapshot = InspectionSnapshot(
            meta={"host_root": "/host"},
            os_release=OsRelease(name="RHEL", version_id="9.6"),
            kernel_boot=KernelBootSection(
                cmdline="BOOT_IMAGE=/vmlinuz root=/dev/sda1 ro crashkernel=auto rhgb quiet"
            ),
        )
        with tempfile.TemporaryDirectory() as tmp:
            output_dir = Path(tmp)
            render_containerfile(snapshot, Environment(), output_dir)
            toml_path = output_dir / "config/usr/lib/bootc/kargs.d/yoinkc-migrated.toml"
            assert not toml_path.exists(), "TOML written for bootloader-only cmdline"
            cf = (output_dir / "Containerfile").read_text()
            assert "kargs.d" not in cf

    def test_no_kargs_toml_when_no_kernel_boot(self):
        """No TOML file and no kargs section when kernel_boot is absent."""
        from yoinkc.schema import InspectionSnapshot, OsRelease
        snapshot = InspectionSnapshot(
            meta={"host_root": "/host"},
            os_release=OsRelease(name="RHEL", version_id="9.6"),
        )
        with tempfile.TemporaryDirectory() as tmp:
            output_dir = Path(tmp)
            render_containerfile(snapshot, Environment(), output_dir)
            toml_path = output_dir / "config/usr/lib/bootc/kargs.d/yoinkc-migrated.toml"
            assert not toml_path.exists()
            cf = (output_dir / "Containerfile").read_text()
            assert "kargs.d" not in cf

    def test_multiple_kargs_combined_in_single_toml(self):
        """Multiple operator kargs from cmdline are collected into a single TOML array."""
        from yoinkc.schema import InspectionSnapshot, OsRelease, KernelBootSection
        snapshot = InspectionSnapshot(
            meta={"host_root": "/host"},
            os_release=OsRelease(name="RHEL", version_id="9.6"),
            kernel_boot=KernelBootSection(
                # Mix of bootloader params (excluded) and operator params (included)
                cmdline=(
                    "BOOT_IMAGE=/vmlinuz root=/dev/sda1 ro rhgb quiet "
                    "hugepagesz=2M transparent_hugepage=never mitigations=off"
                ),
            ),
        )
        with tempfile.TemporaryDirectory() as tmp:
            output_dir = Path(tmp)
            render_containerfile(snapshot, Environment(), output_dir)
            toml_path = output_dir / "config/usr/lib/bootc/kargs.d/yoinkc-migrated.toml"
            assert toml_path.exists()
            content = toml_path.read_text()
            # Operator kargs present
            assert '"hugepagesz=2M"' in content
            assert '"transparent_hugepage=never"' in content
            assert '"mitigations=off"' in content
            # Bootloader params absent
            assert "BOOT_IMAGE" not in content
            assert '"ro"' not in content
            assert '"rhgb"' not in content
            # Exactly one kargs line
            kargs_lines = [ln for ln in content.splitlines() if ln.startswith("kargs =")]
            assert len(kargs_lines) == 1, f"Expected single kargs line, got: {kargs_lines}"
            # Containerfile has exactly one COPY for kargs.d
            cf = (output_dir / "Containerfile").read_text()
            copies = [ln for ln in cf.splitlines()
                      if "kargs.d/yoinkc-migrated.toml" in ln and ln.startswith("COPY")]
            assert len(copies) == 1, f"Expected 1 COPY for kargs TOML, got: {copies}"


# ===========================================================================
# Baseline mode variations (moved from test_renderers.py)
# ===========================================================================

class TestBaselineModes:

    def test_baseline_available_wording(self, outputs_with_baseline):
        """With baseline, audit and Containerfile use 'beyond base image' wording."""
        cf = (outputs_with_baseline["dir"] / "Containerfile").read_text()
        audit = (outputs_with_baseline["dir"] / "audit-report.md").read_text()
        assert "added beyond base image" in cf
        assert "No baseline" not in cf
        assert "beyond base image" in audit or "Baseline:" in audit


# ===========================================================================
# Edge cases and minimal snapshots (moved from test_renderers.py)
# ===========================================================================

class TestEdgeCases:

    def test_minimal_snapshot_no_crash(self):
        """Renderers must not crash when all sections are None."""
        from yoinkc.schema import InspectionSnapshot, OsRelease
        minimal = InspectionSnapshot(
            meta={"host_root": "/host"},
            os_release=OsRelease(name="RHEL", version_id="9.6"),
        )
        with tempfile.TemporaryDirectory() as tmp:
            output_dir = Path(tmp)
            run_all_renderers(minimal, output_dir)
            assert (output_dir / "Containerfile").exists()
            assert (output_dir / "audit-report.md").exists()
            assert (output_dir / "report.html").exists()
            assert (output_dir / "README.md").exists()
            assert (output_dir / "secrets-review.md").exists()
            assert (output_dir / "kickstart-suggestion.ks").exists()

    def test_none_and_empty_values_no_literal_none(self):
        """No literal 'None' string in any rendered output."""
        from yoinkc.schema import (
            InspectionSnapshot, OsRelease, ServiceSection, ServiceStateChange,
        )
        services = ServiceSection(
            state_changes=[
                ServiceStateChange(unit="foo.service", current_state="enabled",
                                   default_state="enabled", action="unchanged"),
            ],
        )
        edge = InspectionSnapshot(
            meta={"host_root": "/host"},
            os_release=OsRelease(name="RHEL", version_id="9.6", pretty_name="RHEL 9.6"),
            services=services,
            warnings=[{"message": None}, {"message": ""}, {}, {"message": "Real warning"}],
            redactions=[
                {"path": None, "pattern": None, "remediation": None},
                {"path": "", "pattern": "PASSWORD", "remediation": "use secret"},
            ],
        )
        with tempfile.TemporaryDirectory() as tmp:
            output_dir = Path(tmp)
            run_all_renderers(edge, output_dir)
            for name in ("audit-report.md", "report.html", "README.md", "secrets-review.md"):
                content = (output_dir / name).read_text()
                assert "None" not in content, f"{name} must not contain literal None"


# ===========================================================================
# HTML report structural integrity (moved from test_renderers.py)
# ===========================================================================

class TestHtmlStructure:

    def test_section_ids_match_template(self, outputs_with_baseline):
        """Every card data-section and tab data-tab has a matching section element.

        Instead of hardcoding a list, extract card/tab references from the HTML
        itself and verify each has a corresponding section div.
        """
        html = (outputs_with_baseline["dir"] / "report.html").read_text()
        card_sections = re.findall(r'data-section="([^"]+)"', html)
        tab_sections = re.findall(r'data-tab="([^"]+)"', html)
        all_refs = set(card_sections) | set(tab_sections)
        for ref in all_refs:
            assert f'id="section-{ref}"' in html, f"No section for data-section/data-tab={ref}"

    def test_summary_is_default_visible(self, outputs_with_baseline):
        html = (outputs_with_baseline["dir"] / "report.html").read_text()
        assert 'id="section-summary"' in html
        assert 'class="section visible"' in html

    def test_service_snap_index_matches_unfiltered_array(self):
        """data-snap-index for each rendered service row must equal its position in
        the full state_changes array, not in the filtered set of changed units."""
        import re as _re
        import tempfile
        from yoinkc.schema import (
            InspectionSnapshot, OsRelease, ServiceSection, ServiceStateChange,
        )
        from yoinkc.renderers import run_all as run_all_renderers

        # index 0: unchanged — must not render a row
        # index 1: enable — row with data-snap-index="1"
        # index 2: unchanged — must not render a row
        # index 3: mask   — row with data-snap-index="3"
        services = ServiceSection(
            state_changes=[
                ServiceStateChange(unit="a.service", current_state="enabled",
                                   default_state="enabled", action="unchanged"),
                ServiceStateChange(unit="b.service", current_state="enabled",
                                   default_state="disabled", action="enable"),
                ServiceStateChange(unit="c.service", current_state="disabled",
                                   default_state="disabled", action="unchanged"),
                ServiceStateChange(unit="d.service", current_state="masked",
                                   default_state="disabled", action="mask"),
            ],
        )
        snapshot = InspectionSnapshot(
            meta={"host_root": "/host"},
            os_release=OsRelease(name="RHEL", version_id="9.6", pretty_name="RHEL 9.6"),
            services=services,
        )
        with tempfile.TemporaryDirectory() as tmp:
            run_all_renderers(snapshot, Path(tmp))
            html = (Path(tmp) / "report.html").read_text()

        # Extract (unit-name, snap-index) pairs from the services table rows
        rows = _re.findall(
            r'data-snap-section="services"[^>]*data-snap-index="(\d+)"[^>]*>'
            r'.*?<td>([^<]+)</td>',
            html,
        )
        index_by_unit = {unit: int(idx) for idx, unit in rows}

        assert "a.service" not in index_by_unit, "unchanged rows must not be rendered"
        assert "c.service" not in index_by_unit, "unchanged rows must not be rendered"
        assert index_by_unit.get("b.service") == 1, (
            f"b.service should have snap-index=1, got {index_by_unit}"
        )
        assert index_by_unit.get("d.service") == 3, (
            f"d.service should have snap-index=3, got {index_by_unit}"
        )

    def test_config_snap_index_matches_unfiltered_array(self):
        """data-snap-index for each config row must equal its position in the full
        config.files array, not in the filtered set (which excludes quadlet files)."""
        import re as _re
        import tempfile
        from yoinkc.schema import (
            InspectionSnapshot, OsRelease, ConfigSection, ConfigFileEntry, ConfigFileKind,
        )
        from yoinkc.renderers import run_all as run_all_renderers

        # index 0: regular config file — row with data-snap-index="0"
        # index 1: quadlet file — must not render a config row
        # index 2: regular config file — row with data-snap-index="2"
        snapshot = InspectionSnapshot(
            meta={"host_root": "/host"},
            os_release=OsRelease(name="RHEL", version_id="9.6", pretty_name="RHEL 9.6"),
            config=ConfigSection(files=[
                ConfigFileEntry(path="/etc/myapp/app.conf", kind=ConfigFileKind.RPM_OWNED_MODIFIED),
                ConfigFileEntry(path="/etc/containers/systemd/myapp.container", kind=ConfigFileKind.UNOWNED),
                ConfigFileEntry(path="/etc/myapp/extra.conf", kind=ConfigFileKind.UNOWNED),
            ]),
        )
        with tempfile.TemporaryDirectory() as tmp:
            run_all_renderers(snapshot, Path(tmp))
            html = (Path(tmp) / "report.html").read_text()

        rows = _re.findall(
            r'data-snap-section="config"[^>]*data-snap-index="(\d+)"[^>]*>'
            r'.*?<td><code>([^<]+)</code></td>',
            html,
            _re.DOTALL,
        )
        index_by_path = {path.strip(): int(idx) for idx, path in rows}

        assert "/etc/containers/systemd/myapp.container" not in index_by_path, \
            "quadlet file must not appear in config table"
        assert index_by_path.get("/etc/myapp/app.conf") == 0, (
            f"app.conf should have snap-index=0, got {index_by_path}"
        )
        assert index_by_path.get("/etc/myapp/extra.conf") == 2, (
            f"extra.conf should have snap-index=2, got {index_by_path}"
        )

    def test_config_file_count_excludes_quadlets(self):
        """_config_file_count must not count quadlet files."""
        from yoinkc.schema import (
            InspectionSnapshot, OsRelease, ConfigSection, ConfigFileEntry, ConfigFileKind,
        )
        from yoinkc.renderers._triage import _config_file_count

        snapshot = InspectionSnapshot(
            meta={"host_root": "/host"},
            os_release=OsRelease(name="RHEL", version_id="9.6", pretty_name="RHEL 9.6"),
            config=ConfigSection(files=[
                ConfigFileEntry(path="/etc/myapp/app.conf", kind=ConfigFileKind.RPM_OWNED_MODIFIED),
                ConfigFileEntry(path="/etc/containers/systemd/myapp.container", kind=ConfigFileKind.UNOWNED),
                ConfigFileEntry(path="/etc/myapp/extra.conf", kind=ConfigFileKind.UNOWNED),
            ]),
        )
        assert _config_file_count(snapshot) == 2

    def test_triage_counts_exclude_quadlets(self):
        """compute_triage automatic count must not include quadlet files."""
        from yoinkc.schema import (
            InspectionSnapshot, OsRelease, ConfigSection, ConfigFileEntry, ConfigFileKind,
        )
        from yoinkc.renderers._triage import compute_triage_detail

        snapshot = InspectionSnapshot(
            meta={"host_root": "/host"},
            os_release=OsRelease(name="RHEL", version_id="9.6", pretty_name="RHEL 9.6"),
            config=ConfigSection(files=[
                ConfigFileEntry(path="/etc/myapp/app.conf", kind=ConfigFileKind.RPM_OWNED_MODIFIED),
                ConfigFileEntry(path="/etc/containers/systemd/myapp.container", kind=ConfigFileKind.UNOWNED),
            ]),
        )
        import tempfile
        with tempfile.TemporaryDirectory() as tmp:
            triage = compute_triage_detail(snapshot, Path(tmp))
        config_item = next((t for t in triage if t["label"] == "Config files"), None)
        assert config_item is not None, "expected a Config files triage entry"
        assert config_item["count"] == 1, (
            f"expected 1 config file (quadlet excluded), got {config_item['count']}"
        )

    def test_snapshot_json_script_tag_injection_escaped(self):
        """</script> inside snapshot values must not terminate the embedded <script> block."""
        import tempfile
        from yoinkc.schema import (
            InspectionSnapshot, OsRelease, ConfigSection, ConfigFileEntry, ConfigFileKind,
        )
        from yoinkc.renderers import run_all as run_all_renderers

        payload = '</script><img src=x onerror=alert(1)>'
        snapshot = InspectionSnapshot(
            meta={"host_root": "/host"},
            os_release=OsRelease(name="RHEL", version_id="9.6", pretty_name="RHEL 9.6"),
            config=ConfigSection(files=[
                ConfigFileEntry(
                    path="/etc/myapp/config.conf",
                    kind=ConfigFileKind.RPM_OWNED_MODIFIED,
                    content=payload,
                )
            ]),
        )
        with tempfile.TemporaryDirectory() as tmp:
            run_all_renderers(snapshot, Path(tmp))
            html = (Path(tmp) / "report.html").read_text()

        # The payload must not appear verbatim inside the embedded JSON blob.
        # Legitimate </script> tags close the page's own script blocks, so we
        # check the payload-specific string rather than banning the tag globally.
        assert '</script><img' not in html, (
            "Injection payload must not appear unescaped in the HTML report"
        )
        assert "<\\/" in html, (
            "The escaped form <\\/ must be present in the embedded JSON"
        )

    def test_readme_detailed(self, outputs_with_baseline):
        """README includes build command, deploy, findings summary, artifacts, FIXMEs."""
        readme = (outputs_with_baseline["dir"] / "README.md").read_text()
        assert "Findings summary" in readme
        assert "podman build" in readme
        assert "bootc switch" in readme
        assert "audit-report.md" in readme
        assert "FIXME" in readme


def test_gpg_key_copy_precedes_repo_copy():
    """GPG key COPY must appear before repo COPY which must appear before dnf install."""
    from yoinkc.schema import InspectionSnapshot, RpmSection, PackageEntry, PackageState, RepoFile

    snap = InspectionSnapshot()
    snap.rpm = RpmSection()
    snap.rpm.packages_added = [
        PackageEntry(name="httpd", epoch="0", version="2.4.62", release="1.el9", arch="x86_64",
                     state=PackageState.ADDED, include=True),
    ]
    snap.rpm.leaf_packages = ["httpd"]
    snap.rpm.auto_packages = []
    snap.rpm.leaf_dep_tree = {"httpd": []}
    repo = RepoFile(path="etc/yum.repos.d/custom.repo",
                    content="[custom]\nbaseurl=http://example.com\ngpgkey=file:///etc/pki/rpm-gpg/KEY\n")
    snap.rpm.repo_files = [repo]
    snap.rpm.gpg_keys = [
        RepoFile(path="etc/pki/rpm-gpg/KEY", content="-----BEGIN PGP PUBLIC KEY BLOCK-----\nFAKE\n-----END PGP PUBLIC KEY BLOCK-----\n"),
    ]

    with tempfile.TemporaryDirectory() as td:
        out = Path(td)
        render_containerfile(snap, Environment(), out)
        cf = (out / "Containerfile").read_text()

    gpg_idx  = cf.find("COPY config/etc/pki/rpm-gpg/")
    repo_idx = cf.find("COPY config/etc/yum.repos.d/")
    dnf_idx  = cf.find("RUN dnf install")
    assert gpg_idx  != -1, "Expected COPY for GPG keys"
    assert repo_idx != -1, "Expected COPY for repos"
    assert dnf_idx  != -1, "Expected RUN dnf install"
    assert gpg_idx < repo_idx < dnf_idx, (
        f"Order must be: GPG keys ({gpg_idx}) < repos ({repo_idx}) < dnf install ({dnf_idx})"
    )


def test_systemd_timer_copy_precedes_enable():
    """Timer unit COPY must appear before RUN systemctl enable *.timer."""
    from yoinkc.schema import InspectionSnapshot, ScheduledTaskSection, SystemdTimer

    snap = InspectionSnapshot()
    snap.scheduled_tasks = ScheduledTaskSection()
    snap.scheduled_tasks.systemd_timers = [
        SystemdTimer(name="myapp-report", source="local", on_calendar="daily",
                     timer_content="[Timer]\nOnCalendar=daily\n",
                     service_content="[Service]\nExecStart=/usr/local/bin/report.sh\n"),
    ]

    with tempfile.TemporaryDirectory() as td:
        out = Path(td)
        render_containerfile(snap, Environment(), out)
        cf = (out / "Containerfile").read_text()

    copy_idx   = cf.find("COPY config/etc/systemd/system/")
    enable_idx = cf.find("RUN systemctl enable myapp-report.timer")
    assert copy_idx   != -1, "Expected COPY for systemd/system/"
    assert enable_idx != -1, "Expected RUN systemctl enable"
    assert copy_idx < enable_idx, (
        f"COPY config/etc/systemd/system/ (pos {copy_idx}) must come before "
        f"RUN systemctl enable (pos {enable_idx})"
    )


def test_repo_copy_precedes_dnf_install():
    """Repo COPY directives must appear before RUN dnf install so repos exist when packages are installed."""
    from yoinkc.schema import InspectionSnapshot, RpmSection, PackageEntry, PackageState, RepoFile

    snap = InspectionSnapshot()
    snap.rpm = RpmSection()
    snap.rpm.packages_added = [
        PackageEntry(name="httpd", epoch="0", version="2.4.62", release="1.el9", arch="x86_64",
                     state=PackageState.ADDED, include=True),
    ]
    snap.rpm.leaf_packages = ["httpd"]
    snap.rpm.auto_packages = []
    snap.rpm.leaf_dep_tree = {"httpd": []}
    repo = RepoFile(path="etc/yum.repos.d/custom.repo", content="[custom]\nbaseurl=http://repo.example.com\n")
    snap.rpm.repo_files = [repo]

    with tempfile.TemporaryDirectory() as td:
        out = Path(td)
        render_containerfile(snap, Environment(), out)
        cf = (out / "Containerfile").read_text()

    copy_idx = cf.find("COPY config/etc/yum.repos.d/")
    dnf_idx  = cf.find("RUN dnf install")
    assert copy_idx != -1, "Expected a COPY directive for etc/yum.repos.d/"
    assert dnf_idx  != -1, "Expected a RUN dnf install directive"
    assert copy_idx < dnf_idx, (
        f"COPY config/etc/yum.repos.d/ (pos {copy_idx}) must come before "
        f"RUN dnf install (pos {dnf_idx})"
    )


def test_config_tree_timers_excluded_from_services_enable():
    """Config-tree timer units must not appear in the services RUN systemctl enable line."""
    from yoinkc.renderers.containerfile import render as render_containerfile
    from yoinkc.schema import (
        InspectionSnapshot, ServiceSection, ScheduledTaskSection, SystemdTimer,
        )
    from jinja2 import Environment
    import tempfile

    snap = InspectionSnapshot()
    # A package-installed service (enabled after dnf install)
    snap.services = ServiceSection()
    snap.services.enabled_units = ["httpd.service", "myapp-report.timer", "myapp-report.service"]
    # A local timer whose unit file comes from the config tree
    snap.scheduled_tasks = ScheduledTaskSection()
    snap.scheduled_tasks.systemd_timers = [
        SystemdTimer(
            name="myapp-report", source="local", on_calendar="daily",
            timer_content="[Timer]\nOnCalendar=daily\n",
            service_content="[Service]\nExecStart=/bin/true\n",
        ),
    ]

    with tempfile.TemporaryDirectory() as td:
        out = Path(td)
        render_containerfile(snap, Environment(), out)
        cf = (out / "Containerfile").read_text()

    # Services enable line must contain httpd.service but NOT the timer or its .service
    services_enable_line = next(
        (l for l in cf.splitlines() if l.startswith("RUN systemctl enable") and "httpd" in l),
        "",
    )
    assert "httpd.service" in services_enable_line, "httpd.service must be in services enable"
    assert "myapp-report" not in services_enable_line, (
        "myapp-report must be excluded from services enable (it's a config-tree unit)"
    )

    # Timer COPY must precede the timer's enable in the scheduled tasks section
    copy_idx   = cf.find("COPY config/etc/systemd/system/")
    enable_idx = cf.find("RUN systemctl enable myapp-report.timer")
    assert copy_idx   != -1, "Expected COPY for systemd/system/"
    assert enable_idx != -1, "Expected RUN systemctl enable for myapp-report.timer"
    assert copy_idx < enable_idx


class TestServicePackageFiltering:
    """systemctl enable/disable must skip units whose package won't be installed."""

    @staticmethod
    def _render_cf(snap) -> str:
        from yoinkc.renderers.containerfile import render as render_containerfile
        from jinja2 import Environment
        with tempfile.TemporaryDirectory() as td:
            render_containerfile(snap, Environment(), Path(td))
            return (Path(td) / "Containerfile").read_text()

    def _make_snap(self, enabled=None, disabled=None, state_changes=None,
                   leaf=None, auto=None, baseline=None, dep_tree=None):
        from yoinkc.schema import (
            InspectionSnapshot, RpmSection, ServiceSection, ServiceStateChange,
            PackageEntry, PackageState,
        )
        services = ServiceSection(
            enabled_units=enabled or [],
            disabled_units=disabled or [],
            state_changes=state_changes or [],
        )
        rpm = RpmSection(
            packages_added=[PackageEntry(name=n, version="1.0", release="1.el9", arch="x86_64")
                            for n in (leaf or [])],
            leaf_packages=leaf,
            auto_packages=auto,
            leaf_dep_tree=dep_tree,
            baseline_package_names=baseline,
        )
        return InspectionSnapshot(services=services, rpm=rpm)

    def test_leaf_package_service_included(self):
        """Service from a leaf package must appear in RUN systemctl enable."""
        from yoinkc.schema import ServiceStateChange
        snap = self._make_snap(
            enabled=["httpd.service"],
            state_changes=[ServiceStateChange(
                unit="httpd.service", current_state="enabled",
                default_state="disabled", action="enable",
                owning_package="httpd",
            )],
            leaf=["httpd"], baseline=["bash"],
        )
        cf = self._render_cf(snap)
        assert "RUN systemctl enable httpd.service" in cf

    def test_baseline_package_service_included(self):
        """Service from a base image package must appear in RUN systemctl enable."""
        from yoinkc.schema import ServiceStateChange
        snap = self._make_snap(
            enabled=["sshd.service"],
            state_changes=[ServiceStateChange(
                unit="sshd.service", current_state="enabled",
                default_state="disabled", action="enable",
                owning_package="openssh-server",
            )],
            leaf=["httpd"], baseline=["bash", "openssh-server"],
        )
        cf = self._render_cf(snap)
        assert "RUN systemctl enable sshd.service" in cf

    def test_auto_dep_of_leaf_included(self):
        """Service from an auto package that is a dep of a leaf must be included."""
        from yoinkc.schema import ServiceStateChange
        snap = self._make_snap(
            enabled=["mod_ssl.service"],
            state_changes=[ServiceStateChange(
                unit="mod_ssl.service", current_state="enabled",
                default_state="disabled", action="enable",
                owning_package="mod_ssl",
            )],
            leaf=["httpd"], auto=["mod_ssl"],
            baseline=["bash"],
            dep_tree={"httpd": ["mod_ssl"]},
        )
        cf = self._render_cf(snap)
        assert "RUN systemctl enable mod_ssl.service" in cf

    def test_orphan_auto_package_skipped(self):
        """Service from an auto package not depended on by any leaf must be skipped."""
        from yoinkc.schema import ServiceStateChange
        snap = self._make_snap(
            enabled=["insights-client-boot.service", "httpd.service"],
            state_changes=[
                ServiceStateChange(
                    unit="insights-client-boot.service", current_state="enabled",
                    default_state="disabled", action="enable",
                    owning_package="insights-client",
                ),
                ServiceStateChange(
                    unit="httpd.service", current_state="enabled",
                    default_state="disabled", action="enable",
                    owning_package="httpd",
                ),
            ],
            leaf=["httpd"], auto=["insights-client"],
            baseline=["bash"],
            dep_tree={"httpd": []},
        )
        cf = self._render_cf(snap)
        enable_lines = [l for l in cf.splitlines() if l.startswith("RUN systemctl enable")]
        assert enable_lines, "expected at least one RUN systemctl enable line"
        for line in enable_lines:
            assert "insights-client-boot.service" not in line
            assert "httpd.service" in line
        assert "insights-client-boot.service" in cf, "should appear as a skip comment"
        assert "skipped (package insights-client not in dnf install line)" in cf

    def test_unknown_owner_included(self):
        """Service with unknown owning package must be included (safe default)."""
        from yoinkc.schema import ServiceStateChange
        snap = self._make_snap(
            enabled=["custom.service"],
            state_changes=[ServiceStateChange(
                unit="custom.service", current_state="enabled",
                default_state="disabled", action="enable",
                owning_package=None,
            )],
            leaf=["httpd"], baseline=["bash"],
        )
        cf = self._render_cf(snap)
        assert "RUN systemctl enable custom.service" in cf

    def test_orphan_disable_skipped(self):
        """systemctl disable must also skip units whose package won't be installed."""
        from yoinkc.schema import ServiceStateChange
        snap = self._make_snap(
            disabled=["insights-client.service"],
            state_changes=[ServiceStateChange(
                unit="insights-client.service", current_state="disabled",
                default_state="enabled", action="disable",
                owning_package="insights-client",
            )],
            leaf=["httpd"], auto=["insights-client"],
            baseline=["bash"],
            dep_tree={"httpd": []},
        )
        cf = self._render_cf(snap)
        assert "RUN systemctl disable" not in cf
        assert "insights-client.service" in cf
        assert "skipped (package insights-client not in dnf install line)" in cf

    def test_no_rpm_data_includes_all(self):
        """When RPM section has no package lists, all units should be included."""
        from yoinkc.schema import (
            InspectionSnapshot, ServiceSection, ServiceStateChange,
        )
        snap = InspectionSnapshot(
            services=ServiceSection(
                enabled_units=["httpd.service"],
                state_changes=[ServiceStateChange(
                    unit="httpd.service", current_state="enabled",
                    default_state="disabled", action="enable",
                    owning_package="httpd",
                )],
            ),
        )
        cf = self._render_cf(snap)
        assert "RUN systemctl enable httpd.service" in cf


def test_bootc_container_lint_is_last_run():
    """RUN bootc container lint must appear at the end of every generated Containerfile."""
    from yoinkc.renderers.containerfile import render as render_containerfile
    from yoinkc.schema import InspectionSnapshot
    from jinja2 import Environment
    import tempfile

    snap = InspectionSnapshot()

    with tempfile.TemporaryDirectory() as td:
        out = Path(td)
        render_containerfile(snap, Environment(), out)
        cf = (out / "Containerfile").read_text()

    assert "RUN bootc container lint" in cf
    # It must be the last non-empty line
    last_run = next(
        (line.strip() for line in reversed(cf.splitlines()) if line.strip()),
        "",
    )
    assert last_run == "RUN bootc container lint", (
        f"Expected 'RUN bootc container lint' as last line, got: {last_run!r}"
    )


def test_nonrpm_emits_nodejs_prereq_when_missing_from_packages():
    """A dnf install for nodejs must appear before npm ci when nodejs is not in packages_added."""
    from yoinkc.renderers.containerfile import render as render_containerfile
    from yoinkc.schema import InspectionSnapshot, NonRpmSoftwareSection, NonRpmItem
    from jinja2 import Environment
    import tempfile

    snap = InspectionSnapshot()
    snap.non_rpm_software = NonRpmSoftwareSection()
    snap.non_rpm_software.items = [
        NonRpmItem(path="opt/webapp", method="npm package-lock.json", include=True),
    ]
    # No packages_added — nodejs/npm not in the dnf install block

    with tempfile.TemporaryDirectory() as td:
        out = Path(td)
        render_containerfile(snap, Environment(), out)
        cf = (out / "Containerfile").read_text()

    assert "nodejs" in cf, "Expected a nodejs install directive"
    nodejs_idx = cf.find("nodejs")
    npm_ci_idx = cf.find("npm ci")
    assert npm_ci_idx != -1, "Expected RUN npm ci"
    assert nodejs_idx < npm_ci_idx, (
        f"dnf install nodejs (pos {nodejs_idx}) must come before npm ci (pos {npm_ci_idx})"
    )


def test_nonrpm_no_nodejs_prereq_when_already_in_packages():
    """No extra nodejs install when nodejs is already in the leaf packages."""
    from yoinkc.renderers.containerfile import render as render_containerfile
    from yoinkc.schema import (
        InspectionSnapshot, NonRpmSoftwareSection, NonRpmItem,
        RpmSection, PackageEntry, PackageState,
    )
    from jinja2 import Environment
    import tempfile

    snap = InspectionSnapshot()
    snap.non_rpm_software = NonRpmSoftwareSection()
    snap.non_rpm_software.items = [
        NonRpmItem(path="opt/webapp", method="npm package-lock.json", include=True),
    ]
    snap.rpm = RpmSection()
    snap.rpm.packages_added = [
        PackageEntry(name="nodejs", epoch="0", version="20.0", release="1.el10",
                     arch="x86_64", state=PackageState.ADDED, include=True),
    ]
    snap.rpm.leaf_packages = ["nodejs"]

    with tempfile.TemporaryDirectory() as td:
        out = Path(td)
        render_containerfile(snap, Environment(), out)
        cf = (out / "Containerfile").read_text()

    # The prerequisite block must not appear (nodejs is already in the dnf install)
    assert "Tool prerequisites not in the dnf install block" not in cf


# ===========================================================================
# Fleet prevalence UI tests (Chunk 1)
# ===========================================================================

class TestFleetColor:
    """Tests for the _fleet_color Jinja2 filter."""

    def test_full_prevalence_returns_blue(self):
        from yoinkc.renderers.html_report import _fleet_color
        fleet = FleetPrevalence(count=3, total=3)
        assert _fleet_color(fleet) == "pf-m-blue"

    def test_majority_prevalence_returns_gold(self):
        from yoinkc.renderers.html_report import _fleet_color
        fleet = FleetPrevalence(count=2, total=3)
        assert _fleet_color(fleet) == "pf-m-gold"

    def test_fifty_percent_returns_gold(self):
        from yoinkc.renderers.html_report import _fleet_color
        fleet = FleetPrevalence(count=50, total=100)
        assert _fleet_color(fleet) == "pf-m-gold"

    def test_minority_prevalence_returns_red(self):
        from yoinkc.renderers.html_report import _fleet_color
        fleet = FleetPrevalence(count=1, total=3)
        assert _fleet_color(fleet) == "pf-m-red"

    def test_none_returns_blue(self):
        from yoinkc.renderers.html_report import _fleet_color
        assert _fleet_color(None) == "pf-m-blue"

    def test_zero_total_returns_blue(self):
        from yoinkc.renderers.html_report import _fleet_color
        fleet = FleetPrevalence(count=0, total=0)
        assert _fleet_color(fleet) == "pf-m-blue"


class TestFleetBanner:
    """Tests for fleet banner rendering."""

    def _render_with_fleet_meta(self, tmp_path):
        """Render a snapshot with fleet metadata and return HTML."""
        snap = InspectionSnapshot(
            schema_version=1,
            os_release=OsRelease(
                name="Red Hat Enterprise Linux", version_id="9.4",
                id="rhel", platform_id="platform:el9",
            ),
            meta={
                "fleet": {
                    "source_hosts": ["web-01", "web-02", "web-03"],
                    "total_hosts": 3,
                    "min_prevalence": 67,
                }
            },
            rpm=RpmSection(
                packages_added=[
                    PackageEntry(name="httpd", version="2.4.57", release="1.el9", arch="x86_64",
                                 fleet=FleetPrevalence(count=3, total=3, hosts=["web-01", "web-02", "web-03"])),
                ],
            ),
        )
        env = Environment(autoescape=True)
        html_report.render(snap, env, tmp_path)
        return (tmp_path / "report.html").read_text()

    def test_fleet_banner_present(self, tmp_path):
        html = self._render_with_fleet_meta(tmp_path)
        assert "Fleet Analysis" in html
        assert "3 hosts merged" in html
        assert "67%" in html
        assert "web-01" in html
        assert "web-02" in html
        assert "web-03" in html
        assert "included" in html

    def test_fleet_banner_absent(self, tmp_path):
        snap = InspectionSnapshot(
            schema_version=1,
            os_release=OsRelease(
                name="Red Hat Enterprise Linux", version_id="9.4",
                id="rhel", platform_id="platform:el9",
            ),
            rpm=RpmSection(
                packages_added=[
                    PackageEntry(name="httpd", version="2.4.57", release="1.el9", arch="x86_64"),
                ],
            ),
        )
        env = Environment(autoescape=True)
        html_report.render(snap, env, tmp_path)
        html = (tmp_path / "report.html").read_text()
        assert "Fleet Analysis" not in html
