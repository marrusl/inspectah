"""Tests for the visual consistency pass."""

import tempfile
from pathlib import Path

from yoinkc.renderers import run_all as run_all_renderers
from yoinkc.schema import (
    AtJob,
    InspectionSnapshot,
    NetworkSection,
    NMConnection,
    OsRelease,
    ScheduledTaskSection,
    SystemdTimer,
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
