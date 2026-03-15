"""HTML report renderer output tests: structure, sections, snap-index, triage, XSS."""

import re
import tempfile
from pathlib import Path

from yoinkc.renderers import run_all as run_all_renderers
from yoinkc.schema import (
    ConfigFileEntry,
    ConfigFileKind,
    ConfigSection,
    InspectionSnapshot,
    OsRelease,
    ServiceSection,
    SystemdDropIn,
)


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
        """originalSnapshot should be embedded separately, not deep-copied."""
        html = self._html(outputs_with_baseline)
        assert "var snapshot" in html
        assert "var originalSnapshot" in html
        assert "JSON.parse(JSON.stringify(snapshot))" not in html

    def test_original_snapshot_from_file(self):
        """When --original-snapshot is provided, it should be embedded instead of a copy."""
        snapshot = InspectionSnapshot(
            meta={"host_root": "/host", "hostname": "edited-host"},
            os_release=OsRelease(name="RHEL", version_id="9.6", pretty_name="RHEL 9.6"),
        )
        original = InspectionSnapshot(
            meta={"host_root": "/host", "hostname": "original-host"},
            os_release=OsRelease(name="RHEL", version_id="9.6", pretty_name="RHEL 9.6"),
        )
        with tempfile.TemporaryDirectory() as tmp:
            orig_path = Path(tmp) / "original-snapshot.json"
            orig_path.write_text(original.model_dump_json())
            run_all_renderers(
                snapshot, Path(tmp),
                original_snapshot_path=orig_path,
            )
            html = (Path(tmp) / "report.html").read_text()

        assert "original-host" in html
        assert "edited-host" in html

    def test_refine_mode_defaults_to_false(self, outputs_with_baseline):
        """Static report has refine_mode=False embedded as JS variable."""
        html = self._html(outputs_with_baseline)
        assert "var refineMode = false" in html

    def test_refine_mode_true_renders_correctly(self):
        """When refine_mode=True, the JS variable should be true."""
        snapshot = InspectionSnapshot(
            meta={"host_root": "/host"},
            os_release=OsRelease(name="RHEL", version_id="9.6", pretty_name="RHEL 9.6"),
        )
        with tempfile.TemporaryDirectory() as tmp:
            run_all_renderers(snapshot, Path(tmp), refine_mode=True)
            html = (Path(tmp) / "report.html").read_text()

        assert "var refineMode = true" in html

    def test_output_tree_includes_dropins(self):
        """File browser tree includes drop-ins folder when drop-ins exist."""
        snapshot = InspectionSnapshot(
            meta={"host_root": "/host"},
            os_release=OsRelease(name="RHEL", version_id="9.6", pretty_name="RHEL 9.6"),
            services=ServiceSection(drop_ins=[
                SystemdDropIn(
                    unit="postgresql.service",
                    path="etc/systemd/system/postgresql.service.d/override.conf",
                    content="[Service]\nLimitNOFILE=65536\n",
                ),
            ]),
        )
        with tempfile.TemporaryDirectory() as tmp:
            run_all_renderers(snapshot, Path(tmp))
            html = (Path(tmp) / "report.html").read_text()

        assert "drop-ins" in html
        assert "override.conf" in html


class TestHtmlStructure:

    def test_section_ids_match_template(self, outputs_with_baseline):
        """Every card data-section and tab data-tab has a matching section element."""
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
