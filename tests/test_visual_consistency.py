"""Tests for the visual consistency pass."""

import tempfile
from pathlib import Path

from yoinkc.renderers import run_all as run_all_renderers
from yoinkc.schema import (
    AtJob,
    ConfigCategory,
    ConfigFileEntry,
    ConfigFileKind,
    ConfigSection,
    ContainerSection,
    InspectionSnapshot,
    NetworkSection,
    NMConnection,
    OsRelease,
    ScheduledTaskSection,
    ServiceSection,
    SystemdDropIn,
    SystemdTimer,
    QuadletUnit,
)


def _render(refine_mode: bool = False, **snapshot_kwargs) -> str:
    """Render a report and return the HTML string."""
    defaults = {
        "meta": {"host_root": "/host"},
        "os_release": OsRelease(name="RHEL", version_id="9", pretty_name="RHEL 9"),
    }
    defaults.update(snapshot_kwargs)
    snapshot = InspectionSnapshot(**defaults)
    with tempfile.TemporaryDirectory() as tmp:
        run_all_renderers(snapshot, Path(tmp), refine_mode=refine_mode)
        return (Path(tmp) / "report.html").read_text()


def _content_template_paths() -> list[Path]:
    report_dir = Path(__file__).resolve().parents[1] / "src" / "yoinkc" / "templates" / "report"
    return sorted(
        path
        for path in report_dir.glob("_*.html.j2")
        if path.name not in {"_css.html.j2", "_js.html.j2"}
    )


def _render_with_scheduled_and_network() -> str:
    return _render(
        scheduled_tasks=ScheduledTaskSection(
            systemd_timers=[
                SystemdTimer(
                    name="myapp.timer",
                    on_calendar="daily",
                    exec_start="/usr/bin/true",
                    source="local",
                )
            ]
        ),
        network=NetworkSection(
            connections=[
                NMConnection(
                    path="/etc/NetworkManager/system-connections/ens3.nmconnection",
                    name="ens3",
                    method="dhcp",
                    type="ethernet",
                )
            ]
        ),
    )


def _render_with_at_jobs() -> str:
    return _render(
        scheduled_tasks=ScheduledTaskSection(
            at_jobs=[
                AtJob(
                    file="/var/spool/at/a0000101",
                    user="root",
                    command="echo hello",
                )
            ]
        )
    )


def _render_with_config(
    flags: str = "S.5.....",
    diff: str = "--- a\n+++ b\n@@ -1 +1 @@\n-old\n+new",
    refine_mode: bool = False,
) -> str:
    return _render(
        refine_mode=refine_mode,
        config=ConfigSection(
            files=[
                ConfigFileEntry(
                    path="/etc/test.conf",
                    rpm_va_flags=flags,
                    diff_against_rpm=diff,
                    kind=ConfigFileKind.RPM_OWNED_MODIFIED,
                    category=ConfigCategory.OTHER,
                    include=True,
                )
            ]
        ),
    )


class TestGlobalSpacing:
    """Part A: global table spacing via CSS variables."""

    def test_relaxed_padding_block_start(self):
        html = _render()
        assert "--pf-v6-c-table--cell--PaddingBlockStart" in html

    def test_relaxed_padding_block_end(self):
        html = _render()
        assert "--pf-v6-c-table--cell--PaddingBlockEnd" in html

    def test_relaxed_padding_inline_start(self):
        html = _render()
        assert "--pf-v6-c-table--cell--PaddingInlineStart" in html

    def test_relaxed_padding_inline_end(self):
        html = _render()
        assert "--pf-v6-c-table--cell--PaddingInlineEnd" in html


class TestInlineStyleCleanup:
    """Part E: inline styles migrated to CSS classes."""

    def test_css_classes_defined(self):
        html = _render()
        assert ".fleet-variant-toggle" in html
        assert ".fleet-variant-table" in html
        assert ".variant-index" in html
        assert ".fleet-banner-hosts" in html

    def test_no_inline_margin_left_cursor(self):
        for path in _content_template_paths():
            text = path.read_text()
            assert 'style="margin-left: 8px; cursor: pointer;"' not in text, (
                f"inline margin-left/cursor still in {path}"
            )

    def test_no_inline_margin_zero(self):
        for path in _content_template_paths():
            text = path.read_text()
            assert 'style="margin: 0;"' not in text, (
                f"inline margin:0 still in {path}"
            )

    def test_no_inline_variant_color(self):
        for path in _content_template_paths():
            text = path.read_text()
            assert 'style="color: var(--pf-v6-global--Color--200);"' not in text, (
                f"inline variant color still in {path}"
            )

    def test_banner_uses_dedicated_hosts_class(self):
        banner_template = (
            Path(__file__).resolve().parents[1]
            / "src"
            / "yoinkc"
            / "templates"
            / "report"
            / "_banner.html.j2"
        )
        text = banner_template.read_text()
        assert 'class="fleet-banner-hosts"' in text
        assert 'class="variant-index"' not in text


class TestColumnConsistency:
    """Part D: fit-content on narrow columns."""

    def test_timers_schedule_has_fit_content(self):
        html = _render_with_scheduled_and_network()
        sched_start = html.find('id="section-scheduled_tasks"')
        assert sched_start != -1, "scheduled tasks section missing from test fixture"
        sched_html = html[sched_start:sched_start + 5000]
        first_table = sched_html[sched_html.find("<table"):sched_html.find("</table>") + 8]
        assert 'scope="col">Timer</th><th class="pf-m-fit-content" scope="col">Schedule</th>' in first_table

    def test_connections_columns_have_fit_content(self):
        html = _render_with_scheduled_and_network()
        net_start = html.find('id="section-network"')
        assert net_start != -1, "network section missing from test fixture"
        net_html = html[net_start:net_start + 5000]
        first_table = net_html[net_html.find("<table"):net_html.find("</table>") + 8]
        assert '<th class="pf-m-fit-content" scope="col">Method</th>' in first_table
        assert '<th class="pf-m-fit-content" scope="col">Type</th>' in first_table
        assert '<th class="pf-m-fit-content" scope="col">Deployment</th>' in first_table
        assert "pf-v6-c-table__check" not in first_table

    def test_at_jobs_table_is_unchanged(self):
        html = _render_with_at_jobs()
        at_jobs_start = html.find('id="card-sched-at-jobs"')
        assert at_jobs_start != -1, "at jobs card missing from test fixture"
        at_jobs_html = html[at_jobs_start:at_jobs_start + 3000]
        first_table = at_jobs_html[at_jobs_html.find("<table"):at_jobs_html.find("</table>") + 8]
        assert (
            '<th scope="col">File</th><th class="pf-m-fit-content" scope="col">User</th>'
            '<th scope="col">Command</th>'
        ) in first_table
        assert "pf-v6-c-table__check" not in first_table


class TestConfigColumnCleanup:
    """Part B1-B2: rpm-Va and diff columns removed."""

    def test_rpm_va_column_removed(self):
        html = _render_with_config()
        config_start = html.find('id="section-config"')
        assert config_start != -1, "config section missing from test fixture"
        config_html = html[config_start:config_start + 5000]
        assert "rpm -Va" not in config_html
        assert "S.5....." not in config_html

    def test_diff_column_removed(self):
        html = _render_with_config()
        config_start = html.find('id="section-config"')
        assert config_start != -1, "config section missing from test fixture"
        config_html = html[config_start:config_start + 5000]
        assert "diff-view" not in config_html
        assert "diff-add" not in config_html

    def test_diff_css_classes_removed(self):
        html = _render_with_config()
        assert ".diff-view" not in html
        assert ".diff-hdr" not in html
        assert ".diff-hunk" not in html
        assert ".diff-add" not in html
        assert ".diff-del" not in html

    def test_render_diff_html_function_removed(self):
        from yoinkc.renderers import html_report

        assert not hasattr(html_report, "_render_diff_html")


class TestPermissionsBadge:
    """Part B3: permissions badge when rpm-Va flags contain M, U, or G."""

    def test_badge_shown_for_mode_change(self):
        html = _render_with_config(flags="SM5.....", diff="")
        config_start = html.find('id="section-config"')
        config_html = html[config_start:config_start + 5000]
        assert "permissions" in config_html.lower()

    def test_badge_shown_for_user_change(self):
        html = _render_with_config(flags="..5..U..", diff="")
        config_start = html.find('id="section-config"')
        config_html = html[config_start:config_start + 5000]
        assert "permissions" in config_html.lower()

    def test_badge_shown_for_group_change(self):
        html = _render_with_config(flags="..5...G.", diff="")
        config_start = html.find('id="section-config"')
        config_html = html[config_start:config_start + 5000]
        assert "permissions" in config_html.lower()

    def test_badge_not_shown_for_content_only(self):
        html = _render_with_config(flags="S.5.....", diff="")
        config_start = html.find('id="section-config"')
        config_html = html[config_start:config_start + 5000]
        assert "permissions" not in config_html.lower()

    def test_badge_not_shown_for_empty_flags(self):
        html = _render_with_config(flags="", diff="")
        config_start = html.find('id="section-config"')
        config_html = html[config_start:config_start + 5000]
        assert "permissions" not in config_html.lower()


class TestPencilReorder:
    """Part B4-B5: pencil icon between checkbox and path-like columns."""

    def test_config_pencil_before_path(self):
        html = _render_with_config(refine_mode=True)
        config_start = html.find('id="section-config"')
        config_html = html[config_start:config_start + 5000]
        pencil_pos = config_html.find("editor-icon")
        path_pos = config_html.find("/etc/test.conf")
        assert pencil_pos != -1, "pencil icon not found in config section"
        assert path_pos != -1, "path not found in config section"
        assert pencil_pos < path_pos, "pencil should appear before path in config DOM order"

    def test_services_pencil_before_dropin_path(self):
        html = _render(
            refine_mode=True,
            services=ServiceSection(
                drop_ins=[
                    SystemdDropIn(
                        unit="postgresql.service",
                        path="etc/systemd/system/postgresql.service.d/override.conf",
                        content="[Service]\nLimitNOFILE=65536",
                    )
                ]
            ),
        )
        services_start = html.find('id="section-services"')
        services_html = html[services_start:services_start + 6000]
        pencil_pos = services_html.find("editor-icon")
        path_pos = services_html.find("override.conf")
        assert pencil_pos != -1, "pencil icon not found in services section"
        assert path_pos != -1, "drop-in path not found in services section"
        assert pencil_pos < path_pos, "pencil should appear before drop-in path in services DOM order"

    def test_containers_pencil_before_path(self):
        html = _render(
            refine_mode=True,
            containers=ContainerSection(
                quadlet_units=[
                    QuadletUnit(
                        path="/etc/containers/systemd/myapp.container",
                        name="myapp.container",
                        image="ghcr.io/myapp:latest",
                        content="[Container]\nImage=ghcr.io/myapp:latest",
                    )
                ]
            ),
        )
        containers_start = html.find('id="section-containers"')
        containers_html = html[containers_start:containers_start + 6000]
        pencil_pos = containers_html.find("editor-icon")
        path_pos = containers_html.find("/etc/containers/systemd/myapp.container")
        assert pencil_pos != -1, "pencil icon not found in containers section"
        assert path_pos != -1, "quadlet path not found in containers section"
        assert pencil_pos < path_pos, "pencil should appear before path in containers DOM order"

    def test_pencil_not_in_readonly_mode(self):
        html = _render(
            refine_mode=False,
            config=ConfigSection(
                files=[
                    ConfigFileEntry(
                        path="/etc/test.conf",
                        rpm_va_flags="S.5.....",
                        kind=ConfigFileKind.RPM_OWNED_MODIFIED,
                        category=ConfigCategory.OTHER,
                        include=True,
                    )
                ]
            ),
            services=ServiceSection(
                drop_ins=[
                    SystemdDropIn(
                        unit="postgresql.service",
                        path="etc/systemd/system/postgresql.service.d/override.conf",
                        content="[Service]\nLimitNOFILE=65536",
                    )
                ]
            ),
            containers=ContainerSection(
                quadlet_units=[
                    QuadletUnit(
                        path="/etc/containers/systemd/myapp.container",
                        name="myapp.container",
                        image="ghcr.io/myapp:latest",
                        content="[Container]\nImage=ghcr.io/myapp:latest",
                    )
                ]
            ),
        )
        config_start = html.find('id="section-config"')
        config_html = html[config_start:config_start + 5000]
        services_start = html.find('id="section-services"')
        services_html = html[services_start:services_start + 6000]
        containers_start = html.find('id="section-containers"')
        containers_html = html[containers_start:containers_start + 6000]
        assert "editor-icon" not in config_html
        assert "editor-icon" not in services_html
        assert "editor-icon" not in containers_html
