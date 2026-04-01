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
    PackageEntry,
    ScheduledTaskSection,
    RepoFile,
    RpmSection,
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


def _extract_js_block(html: str, start_marker: str, end_marker: str) -> str:
    start = html.find(start_marker)
    assert start >= 0, f"could not find start marker {start_marker!r}"
    end = html.find(end_marker, start)
    assert end >= 0, f"could not find end marker {end_marker!r}"
    return html[start:end]


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


class TestPackagesRestructure:
    """Part C: merge the repo card into the dependency tree."""

    def _render_packages(self, refine_mode: bool = False) -> str:
        return _render(
            refine_mode=refine_mode,
            rpm=RpmSection(
                packages_added=[
                    PackageEntry(
                        name="httpd",
                        version="2.4.57",
                        release="8.el9",
                        arch="x86_64",
                        source_repo="appstream",
                        include=True,
                    ),
                    PackageEntry(
                        name="htop",
                        version="3.2.2",
                        release="1.el9",
                        arch="x86_64",
                        source_repo="epel",
                        include=True,
                    ),
                ],
                repo_files=[
                    RepoFile(
                        path="etc/yum.repos.d/redhat.repo",
                        content="[appstream]\nname=AppStream\n",
                        include=True,
                        is_default_repo=True,
                    ),
                    RepoFile(
                        path="etc/yum.repos.d/epel.repo",
                        content="[epel]\nname=Extra Packages for Enterprise Linux\n",
                        include=True,
                        is_default_repo=False,
                    ),
                ],
                leaf_packages=["httpd", "htop"],
                auto_packages=[],
                leaf_dep_tree={"httpd": [], "htop": []},
            ),
        )

    def _render_packages_with_shared_repo_file(self, refine_mode: bool = False) -> str:
        return _render(
            refine_mode=refine_mode,
            rpm=RpmSection(
                packages_added=[
                    PackageEntry(
                        name="bash",
                        version="5.1",
                        release="1.el9",
                        arch="x86_64",
                        source_repo="baseos",
                        include=True,
                    ),
                    PackageEntry(
                        name="httpd",
                        version="2.4.57",
                        release="8.el9",
                        arch="x86_64",
                        source_repo="appstream",
                        include=True,
                    ),
                ],
                repo_files=[
                    RepoFile(
                        path="etc/yum.repos.d/redhat.repo",
                        content="[baseos]\nname=BaseOS\n[appstream]\nname=AppStream\n",
                        include=True,
                        is_default_repo=True,
                    ),
                ],
                leaf_packages=["bash", "httpd"],
                auto_packages=[],
                leaf_dep_tree={"bash": [], "httpd": []},
            ),
        )

    def test_separate_repo_card_removed(self):
        html = self._render_packages()
        assert 'id="card-pkg-repos"' not in html
        assert 'id="pkg-repo-table"' not in html

    def test_repo_headers_render_inside_dep_tree(self):
        html = self._render_packages(refine_mode=True)
        pkg_start = html.find('id="section-packages"')
        assert pkg_start != -1, "packages section missing from test fixture"
        pkg_html = html[pkg_start:pkg_start + 12000]
        dep_tree_start = pkg_html.find('id="card-pkg-dep-tree"')
        assert dep_tree_start != -1, "dependency tree card missing from packages section"
        dep_tree_html = pkg_html[dep_tree_start:]
        assert 'data-repo-group="appstream"' in dep_tree_html
        assert 'data-repo-group="epel"' in dep_tree_html
        assert 'class="pf-v6-c-button pf-m-plain repo-collapse-btn"' in dep_tree_html
        assert 'aria-expanded="true"' in dep_tree_html

    def test_repo_headers_render_repo_file_toggles(self):
        html = self._render_packages(refine_mode=True)
        pkg_start = html.find('id="section-packages"')
        pkg_html = html[pkg_start:pkg_start + 12000]
        dep_tree_start = pkg_html.find('id="card-pkg-dep-tree"')
        assert dep_tree_start != -1, "dependency tree card missing from packages section"
        dep_tree_html = pkg_html[dep_tree_start:]
        assert 'class="pf-v6-c-switch__input include-toggle repo-cb"' in dep_tree_html
        assert 'data-snap-section="rpm"' in dep_tree_html
        assert 'data-snap-list="repo_files"' in dep_tree_html
        assert 'data-snap-index="0"' in dep_tree_html
        assert 'data-snap-index="1"' in dep_tree_html

    def test_default_repo_toggle_disabled(self):
        html = self._render_packages(refine_mode=True)
        pkg_start = html.find('id="section-packages"')
        pkg_html = html[pkg_start:pkg_start + 12000]
        title_pos = pkg_html.find('Default distribution repository — cannot be excluded')
        assert title_pos != -1
        toggle_html = pkg_html[max(0, title_pos - 200):title_pos + 200]
        assert 'repo-cb' in toggle_html
        assert 'disabled' in toggle_html

    def test_empty_repos_hidden_from_report(self):
        """Repos with 0 packages are completely hidden from the rendered output."""
        html = _render(
            refine_mode=True,
            rpm=RpmSection(
                packages_added=[
                    PackageEntry(
                        name="httpd",
                        version="2.4.57",
                        release="8.el9",
                        arch="x86_64",
                        source_repo="appstream",
                        include=True,
                    ),
                ],
                repo_files=[
                    RepoFile(
                        path="etc/yum.repos.d/redhat.repo",
                        content="[appstream]\nname=AppStream\n",
                        include=True,
                        is_default_repo=True,
                    ),
                    RepoFile(
                        path="etc/yum.repos.d/epel.repo",
                        content=(
                            "[epel]\nname=EPEL\n"
                            "[epel-testing]\nname=EPEL Testing\n"
                        ),
                        include=True,
                        is_default_repo=False,
                    ),
                ],
                leaf_packages=["httpd"],
                auto_packages=[],
                leaf_dep_tree={"httpd": []},
            ),
        )
        # Non-empty repo should render
        assert 'data-repo-group="appstream"' in html
        # Empty repos should be completely absent
        assert 'data-repo-group="epel"' not in html
        assert 'data-repo-group="epel-testing"' not in html

    def test_initial_excluded_repo_renders_excluded_header_and_disabled_leafs(self):
        html = _render(
            refine_mode=True,
            rpm=RpmSection(
                packages_added=[
                    PackageEntry(
                        name="httpd",
                        version="2.4.57",
                        release="8.el9",
                        arch="x86_64",
                        source_repo="appstream",
                        include=True,
                    ),
                ],
                repo_files=[
                    RepoFile(
                        path="etc/yum.repos.d/redhat.repo",
                        content="[appstream]\nname=AppStream\n",
                        include=False,
                        is_default_repo=False,
                    ),
                ],
                leaf_packages=["httpd"],
                auto_packages=[],
                leaf_dep_tree={"httpd": []},
            ),
        )
        header_pos = html.find('data-repo-group="appstream"')
        leaf_pos = html.find('data-leaf="httpd"')
        header_html = html[max(0, header_pos - 80):header_pos + 400]
        leaf_html = html[max(0, leaf_pos - 40):leaf_pos + 400]
        assert 'repo-group-row excluded' in header_html
        assert 'repo-cb"' in header_html and 'checked' not in header_html
        assert 'class="excluded"' in leaf_html
        assert 'leaf-cb"' in leaf_html and 'disabled' in leaf_html

    def test_repo_without_matching_leaf_rows_is_hidden(self):
        """Repos with 0 packages are hidden — they no longer render at all."""
        html = _render(
            refine_mode=True,
            rpm=RpmSection(
                packages_added=[
                    PackageEntry(
                        name="httpd",
                        version="2.4.57",
                        release="8.el9",
                        arch="x86_64",
                        source_repo="appstream",
                        include=True,
                    ),
                ],
                repo_files=[
                    RepoFile(
                        path="etc/yum.repos.d/epel.repo",
                        content="[epel]\nname=Extra Packages for Enterprise Linux\n",
                        include=True,
                        is_default_repo=False,
                    ),
                ],
                leaf_packages=["httpd"],
                auto_packages=[],
                leaf_dep_tree={"httpd": []},
            ),
        )
        pkg_start = html.find('id="section-packages"')
        pkg_html = html[pkg_start:pkg_start + 12000]
        assert 'data-repo-group="epel"' not in pkg_html
        assert 'epel (0 packages)' not in pkg_html

    def test_overlapping_repo_names_get_distinct_repo_file_indices(self):
        html = _render(
            refine_mode=True,
            rpm=RpmSection(
                packages_added=[
                    PackageEntry(
                        name="epel-pkg",
                        version="1.0",
                        release="1.el9",
                        arch="x86_64",
                        source_repo="epel",
                        include=True,
                    ),
                    PackageEntry(
                        name="epel-testing-pkg",
                        version="1.0",
                        release="1.el9",
                        arch="x86_64",
                        source_repo="epel-testing",
                        include=True,
                    ),
                ],
                repo_files=[
                    RepoFile(
                        path="etc/yum.repos.d/epel-testing.repo",
                        content="[epel-testing]\nname=EPEL Testing\n",
                        include=True,
                        is_default_repo=False,
                    ),
                    RepoFile(
                        path="etc/yum.repos.d/epel.repo",
                        content="[epel]\nname=EPEL\n",
                        include=True,
                        is_default_repo=False,
                    ),
                ],
                leaf_packages=["epel-pkg", "epel-testing-pkg"],
                auto_packages=[],
                leaf_dep_tree={"epel-pkg": [], "epel-testing-pkg": []},
            ),
        )
        epel_header = html[html.find('data-repo-group="epel"'):html.find('data-repo-group="epel"') + 300]
        testing_header = html[html.find('data-repo-group="epel-testing"'):html.find('data-repo-group="epel-testing"') + 300]
        assert 'data-snap-index="1"' in epel_header
        assert 'data-snap-index="0"' in testing_header

    def test_shared_repo_file_headers_share_same_repo_file_index(self):
        html = self._render_packages_with_shared_repo_file(refine_mode=True)
        baseos_header = html[html.find('data-repo-group="baseos"'):html.find('data-repo-group="baseos"') + 300]
        appstream_header = html[html.find('data-repo-group="appstream"'):html.find('data-repo-group="appstream"') + 300]
        assert 'data-snap-index="0"' in baseos_header
        assert 'data-snap-index="0"' in appstream_header

    def test_duplicate_repo_ids_across_files_suppress_ambiguous_toggle(self):
        html = _render(
            refine_mode=True,
            rpm=RpmSection(
                packages_added=[
                    PackageEntry(
                        name="epel-pkg",
                        version="1.0",
                        release="1.el9",
                        arch="x86_64",
                        source_repo="epel",
                        include=True,
                    ),
                ],
                repo_files=[
                    RepoFile(
                        path="etc/yum.repos.d/epel-primary.repo",
                        content="[epel]\nname=EPEL Primary\n",
                        include=True,
                        is_default_repo=False,
                    ),
                    RepoFile(
                        path="etc/yum.repos.d/epel-secondary.repo",
                        content="[epel]\nname=EPEL Secondary\n",
                        include=True,
                        is_default_repo=False,
                    ),
                ],
                leaf_packages=["epel-pkg"],
                auto_packages=[],
                leaf_dep_tree={"epel-pkg": []},
            ),
        )
        epel_header = html[html.find('data-repo-group="epel"'):html.find('data-repo-group="epel"') + 300]
        assert 'data-snap-index=' not in epel_header
        assert 'repo-cb' not in epel_header

    def test_apply_repo_cascade_scopes_to_header_repo_group(self):
        html = self._render_packages(refine_mode=True)
        repo_js = _extract_js_block(
            html,
            "function applyRepoCascade",
            "  function resetToOriginal",
        )
        assert "repoGroupNamesForCheckbox(repoCb)" in repo_js
        assert "!repoNames.has(pkg.source_repo)" in repo_js
        assert "syncRepoHeaderCheckboxes(repoCb);" in repo_js

    def test_shared_repo_file_toggle_targets_all_linked_repo_groups(self):
        html = self._render_packages_with_shared_repo_file(refine_mode=True)
        helper_js = _extract_js_block(
            html,
            "function repoGroupNamesForCheckbox",
            "  function syncRepoLeafInteractivity",
        )
        assert 'data-snap-list="repo_files"' in html
        assert 'data-repo-group="baseos"' in html
        assert 'data-repo-group="appstream"' in html
        assert '[data-snap-index="' in helper_js
        assert "repoNames.add(peerName)" in helper_js

    def test_shared_repo_file_header_sync_updates_peer_row_state(self):
        html = self._render_packages_with_shared_repo_file(refine_mode=True)
        sync_js = _extract_js_block(
            html,
            "function syncRepoHeaderCheckboxes",
            "  function repoGroupNamesForCheckbox",
        )
        assert "peerRow.querySelector('.repo-cb')" in sync_js
        assert "peerRow.classList.toggle('excluded', !repoCb.checked);" in sync_js

    def test_reset_to_original_syncs_repo_ui_without_reapplying_cascade(self):
        html = self._render_packages_with_shared_repo_file(refine_mode=True)
        reset_js = _extract_js_block(
            html,
            "  function resetToOriginal()",
            "  if (resetBtn)",
        )
        assert "syncRepoHeaderCheckboxes(cb);" in reset_js
        assert "syncRepoLeafInteractivity(cb);" in reset_js
        assert "applyRepoCascade(cb);" not in reset_js

    def test_repo_toggle_handler_recalculates_counts(self):
        html = self._render_packages(refine_mode=True)
        handler_js = _extract_js_block(
            html,
            "document.querySelectorAll('.repo-cb').forEach(function(cb)",
            "  if (resetBtn)",
        )
        assert "applyRepoCascade(this);" in handler_js
        assert "recalcTriageCounts();" in handler_js

    def test_toggle_change_counting_deduplicates_shared_repo_file_headers(self):
        html = self._render_packages_with_shared_repo_file(refine_mode=True)
        count_js = _extract_js_block(
            html,
            "  function countToggleChanges()",
            "  function updateToolbar()",
        )
        assert "var seenIncludeKeys = new Set();" in count_js
        assert "if (seenIncludeKeys.has(key)) return;" in count_js


class TestRepoSectionIds:
    """Disabled repo sections should be filtered out."""

    def test_enabled_sections_returned(self):
        from yoinkc.renderers.html_report import _repo_section_ids
        content = "[baseos]\nenabled=1\n[appstream]\nenabled=1\n"
        assert _repo_section_ids(content) == ["baseos", "appstream"]

    def test_disabled_sections_excluded(self):
        from yoinkc.renderers.html_report import _repo_section_ids
        content = "[baseos]\nenabled=1\n[supplementary]\nenabled=0\n"
        assert _repo_section_ids(content) == ["baseos"]

    def test_enabled_default_when_absent(self):
        from yoinkc.renderers.html_report import _repo_section_ids
        content = "[epel]\ngpgcheck=1\nbaseurl=https://example.com\n"
        assert _repo_section_ids(content) == ["epel"]

    def test_mixed_enabled_disabled(self):
        from yoinkc.renderers.html_report import _repo_section_ids
        content = (
            "[baseos]\nenabled=1\n"
            "[debug-rpms]\nenabled=0\n"
            "[appstream]\n"
            "[source-rpms]\nenabled=0\n"
            "[codeready]\nenabled=0\n"
        )
        assert _repo_section_ids(content) == ["baseos", "appstream"]

    def test_enabled_metadata_key_ignored(self):
        from yoinkc.renderers.html_report import _repo_section_ids
        content = "[myrepo]\nenabled_metadata=0\ngpgcheck=1\n"
        assert _repo_section_ids(content) == ["myrepo"]

    def test_inline_comment_stripped(self):
        from yoinkc.renderers.html_report import _repo_section_ids
        content = "[disabled-repo]\nenabled=0 # turned off for now\n"
        assert _repo_section_ids(content) == []

    def test_inline_comment_on_enabled(self):
        from yoinkc.renderers.html_report import _repo_section_ids
        content = "[active-repo]\nenabled=1 # keep this on\n"
        assert _repo_section_ids(content) == ["active-repo"]

    def test_empty_content(self):
        from yoinkc.renderers.html_report import _repo_section_ids
        assert _repo_section_ids("") == []
        assert _repo_section_ids(None) == []

    def test_common_false_values_disable_section(self):
        from yoinkc.renderers.html_report import _repo_section_ids
        for value in ("0", "no", "false", "off", "NO", "False", " Off "):
            content = f"[disabled-repo]\nenabled={value}\n"
            assert _repo_section_ids(content) == [], value


class TestRepoFileCandidateNames:
    def test_disabled_only_repo_file_keeps_section_ids(self):
        from yoinkc.renderers.html_report import _repo_file_candidate_names

        repo_file = RepoFile(
            path="etc/yum.repos.d/redhat.repo",
            content="[baseos]\nenabled=0\n[appstream]\nenabled=0\n",
            include=True,
            is_default_repo=True,
        )

        assert _repo_file_candidate_names(repo_file) == ["baseos", "appstream"]
