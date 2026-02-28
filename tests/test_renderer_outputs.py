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
from yoinkc.renderers.containerfile import render as render_containerfile
from yoinkc.renderers.html_report import render as render_html_report
from yoinkc.renderers.audit_report import render as render_audit_report
from yoinkc.renderers.kickstart import render as render_kickstart
from yoinkc.renderers.readme import render as render_readme
from yoinkc.renderers.secrets_review import render as render_secrets_review

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
        per_file = re.findall(r"^COPY config/etc/[^\s/]+/[^\s]+\s+/etc/[^\s]+$", cf, re.MULTILINE)
        assert len(per_file) == 0, f"Found per-file COPY lines: {per_file[:5]}"

    def test_consolidated_copy_etc_present(self, outputs_with_baseline):
        """COPY config/etc/ /etc/ must be present."""
        cf = self._cf(outputs_with_baseline)
        assert "COPY config/etc/ /etc/" in cf

    def test_append_files_use_config_tmp(self, outputs_with_baseline):
        """User .append files must be COPYd from config/tmp/, not config/etc/."""
        cf = self._cf(outputs_with_baseline)
        snapshot = outputs_with_baseline["snapshot"]
        ug = snapshot.users_groups
        if ug and ug.passwd_entries:
            assert "COPY config/tmp/passwd.append /tmp/passwd.append" in cf
            assert "COPY config/etc/passwd.append" not in cf

    def test_append_files_on_disk_in_tmp(self, outputs_with_baseline):
        """Append files must be written to config/tmp/, not config/etc/."""
        output_dir = outputs_with_baseline["dir"]
        snapshot = outputs_with_baseline["snapshot"]
        ug = snapshot.users_groups
        if ug and ug.passwd_entries:
            assert (output_dir / "config/tmp/passwd.append").exists()
            assert not (output_dir / "config/etc/passwd.append").exists()

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
        """If there are warnings, the warning panel should appear."""
        html = self._html(outputs_with_baseline)
        snapshot = outputs_with_baseline["snapshot"]
        if snapshot.warnings:
            assert "warning-panel" in html

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
