"""Fleet prevalence UI tests: color filter, banner, badges, config passthrough, variant grouping."""

from jinja2 import Environment

from yoinkc.renderers import html_report
from yoinkc.schema import (
    ConfigFileEntry,
    ConfigFileKind,
    ConfigSection,
    FleetPrevalence,
    InspectionSnapshot,
    OsRelease,
    PackageEntry,
    RpmSection,
)


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


class TestFleetPrevalenceBadge:
    """Tests for fleet prevalence badges on item rows."""

    def _render_fleet_snapshot(self, tmp_path):
        """Render a fleet snapshot with prevalence data on items."""
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
                    PackageEntry(name="debug-tools", version="1.0", release="1.el9", arch="x86_64",
                                 include=False,
                                 fleet=FleetPrevalence(count=1, total=3, hosts=["web-03"])),
                ],
            ),
        )
        env = Environment(autoescape=True)
        html_report.render(snap, env, tmp_path)
        return (tmp_path / "report.html").read_text()

    def test_prevalence_bar_present(self, tmp_path):
        html = self._render_fleet_snapshot(tmp_path)
        assert "fleet-bar" in html
        assert 'data-count="3"' in html
        assert 'data-total="3"' in html

    def test_prevalence_bar_absent_without_fleet(self, tmp_path):
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
        assert "fleet-bar" not in html
        assert "fleet-prevalence" not in html

    def test_color_class_applied(self, tmp_path):
        html = self._render_fleet_snapshot(tmp_path)
        assert "pf-m-blue" in html
        assert "pf-m-red" in html

    def test_hosts_in_data_attribute(self, tmp_path):
        html = self._render_fleet_snapshot(tmp_path)
        assert "web-01, web-02, web-03" in html

    def test_empty_hosts_renders_empty_data_attr(self, tmp_path):
        snap = InspectionSnapshot(
            schema_version=1,
            os_release=OsRelease(
                name="Red Hat Enterprise Linux", version_id="9.4",
                id="rhel", platform_id="platform:el9",
            ),
            meta={
                "fleet": {
                    "source_hosts": ["web-01"],
                    "total_hosts": 1,
                    "min_prevalence": 100,
                }
            },
            rpm=RpmSection(
                packages_added=[
                    PackageEntry(name="httpd", version="2.4.57", release="1.el9", arch="x86_64",
                                 fleet=FleetPrevalence(count=1, total=1, hosts=[])),
                ],
            ),
        )
        env = Environment(autoescape=True)
        html_report.render(snap, env, tmp_path)
        html = (tmp_path / "report.html").read_text()
        assert 'data-hosts=""' in html


class TestFleetConfigPassthrough:
    """Test that _prepare_config_files preserves fleet data."""

    def test_fleet_field_preserved(self):
        from yoinkc.renderers.html_report import _prepare_config_files
        fleet = FleetPrevalence(count=2, total=3, hosts=["web-01", "web-02"])
        snap = InspectionSnapshot(
            schema_version=1,
            os_release=OsRelease(
                name="Red Hat Enterprise Linux", version_id="9.4",
                id="rhel", platform_id="platform:el9",
            ),
            config=ConfigSection(
                files=[
                    ConfigFileEntry(
                        path="/etc/httpd/conf/httpd.conf",
                        kind=ConfigFileKind.UNOWNED,
                        content="ServerName localhost",
                        include=True,
                        fleet=fleet,
                    ),
                ],
            ),
        )
        result = _prepare_config_files(snap)
        assert len(result) == 1
        assert result[0]["fleet"] is not None
        assert result[0]["fleet"].count == 2
        assert result[0]["fleet"].total == 3

    def test_fleet_field_none_when_absent(self):
        from yoinkc.renderers.html_report import _prepare_config_files
        snap = InspectionSnapshot(
            schema_version=1,
            os_release=OsRelease(
                name="Red Hat Enterprise Linux", version_id="9.4",
                id="rhel", platform_id="platform:el9",
            ),
            config=ConfigSection(
                files=[
                    ConfigFileEntry(
                        path="/etc/httpd/conf/httpd.conf",
                        kind=ConfigFileKind.UNOWNED,
                        content="ServerName localhost",
                        include=True,
                    ),
                ],
            ),
        )
        result = _prepare_config_files(snap)
        assert len(result) == 1
        assert result[0]["fleet"] is None


class TestFleetVariantGrouping:
    """Tests for content variant grouping in fleet mode."""

    def test_variant_grouping_renders_expand_toggle(self, tmp_path):
        """Config items sharing a path render as a grouped row with expand toggle."""
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
            config=ConfigSection(
                files=[
                    ConfigFileEntry(
                        path="/etc/httpd/conf/httpd.conf",
                        kind=ConfigFileKind.UNOWNED,
                        content="ServerName web-01",
                        include=True,
                        fleet=FleetPrevalence(count=2, total=3, hosts=["web-01", "web-02"]),
                    ),
                    ConfigFileEntry(
                        path="/etc/httpd/conf/httpd.conf",
                        kind=ConfigFileKind.UNOWNED,
                        content="ServerName web-03",
                        include=False,
                        fleet=FleetPrevalence(count=1, total=3, hosts=["web-03"]),
                    ),
                ],
            ),
        )
        env = Environment(autoescape=True)
        html_report.render(snap, env, tmp_path)
        html = (tmp_path / "report.html").read_text()
        assert "2 variants" in html
        assert "fleet-variant-group" in html

    def test_no_grouping_without_fleet(self, tmp_path):
        """Without fleet data, config files render as individual rows."""
        snap = InspectionSnapshot(
            schema_version=1,
            os_release=OsRelease(
                name="Red Hat Enterprise Linux", version_id="9.4",
                id="rhel", platform_id="platform:el9",
            ),
            config=ConfigSection(
                files=[
                    ConfigFileEntry(
                        path="/etc/httpd/conf/httpd.conf",
                        kind=ConfigFileKind.UNOWNED,
                        content="ServerName localhost",
                        include=True,
                    ),
                ],
            ),
        )
        env = Environment(autoescape=True)
        html_report.render(snap, env, tmp_path)
        html = (tmp_path / "report.html").read_text()
        assert 'class="fleet-variant-group' not in html

    def test_single_item_path_not_grouped(self, tmp_path):
        """A config path with only one item renders as a normal row."""
        snap = InspectionSnapshot(
            schema_version=1,
            os_release=OsRelease(
                name="Red Hat Enterprise Linux", version_id="9.4",
                id="rhel", platform_id="platform:el9",
            ),
            meta={
                "fleet": {
                    "source_hosts": ["web-01"],
                    "total_hosts": 1,
                    "min_prevalence": 100,
                }
            },
            config=ConfigSection(
                files=[
                    ConfigFileEntry(
                        path="/etc/httpd/conf/httpd.conf",
                        kind=ConfigFileKind.UNOWNED,
                        content="ServerName localhost",
                        include=True,
                        fleet=FleetPrevalence(count=1, total=1),
                    ),
                ],
            ),
        )
        env = Environment(autoescape=True)
        html_report.render(snap, env, tmp_path)
        html = (tmp_path / "report.html").read_text()
        assert 'class="fleet-variant-group' not in html
