"""Service inspector tests: fixture-based, fs fallback, preset globs, owning packages, drop-ins."""

from pathlib import Path


def test_service_inspector_with_fixtures(host_root, fixture_executor):
    from yoinkc.inspectors.service import run as run_service
    section = run_service(host_root, fixture_executor)
    assert section is not None
    assert any(s.unit == "httpd.service" and s.action == "enable" for s in section.state_changes)
    assert "httpd.service" in section.enabled_units


def test_scan_unit_files_from_fs(host_root):
    """Test the filesystem fallback for unit file state detection."""
    from yoinkc.inspectors.service import _scan_unit_files_from_fs
    units = _scan_unit_files_from_fs(host_root)

    assert units.get("test-installable.service") == "enabled", (
        "Unit in .wants/ should be enabled"
    )
    assert units.get("test-masked.service") == "masked", (
        "Symlink to /dev/null should be masked"
    )
    assert units.get("test-static.service") == "static", (
        "Vendor unit without [Install] should be static"
    )
    if "fstrim.service" in units:
        assert units["fstrim.service"] in ("disabled", "static")


def test_preset_glob_rules_applied(host_root, fixture_executor):
    """Glob preset rules like 'enable cloud-*' must set default_state correctly."""
    from yoinkc.inspectors.service import run as run_service

    preset_text = "enable cloud-*\ndisable *\n"
    section = run_service(
        host_root, fixture_executor, base_image_preset_text=preset_text,
    )
    changes = {s.unit: s for s in section.state_changes}

    cloud_init = changes.get("cloud-init.service")
    assert cloud_init is not None, (
        f"cloud-init.service not in state_changes; units: {list(changes)}"
    )
    assert cloud_init.default_state == "enabled", (
        f"expected default_state='enabled' via glob, got '{cloud_init.default_state}'"
    )


def test_preset_glob_first_match_wins(host_root, fixture_executor):
    """Glob rules use first-match-wins: earlier rules take precedence."""
    from yoinkc.inspectors.service import run as run_service

    preset_text = "disable cloud-*\nenable cloud-*\ndisable *\n"
    section = run_service(
        host_root, fixture_executor, base_image_preset_text=preset_text,
    )
    changes = {s.unit: s for s in section.state_changes}

    cloud_init = changes.get("cloud-init.service")
    assert cloud_init is not None, (
        f"cloud-init.service not in state_changes; units: {list(changes)}"
    )
    assert cloud_init.default_state == "disabled", (
        f"first-match-wins: 'disable cloud-*' should beat 'enable cloud-*', "
        f"got '{cloud_init.default_state}'"
    )


def test_service_inspector_resolves_owning_packages(host_root, fixture_executor):
    """Changed units should have owning_package populated via rpm -qf."""
    from yoinkc.inspectors.service import run as run_service
    section = run_service(host_root, fixture_executor)
    httpd = next((s for s in section.state_changes if s.unit == "httpd.service"), None)
    assert httpd is not None, "httpd.service must be in state_changes"
    assert httpd.owning_package == "httpd", (
        f"expected owning_package='httpd', got {httpd.owning_package!r}"
    )
    unchanged = [s for s in section.state_changes if s.action == "unchanged"]
    for s in unchanged:
        assert s.owning_package is None, (
            f"unchanged unit {s.unit} should not have owning_package set"
        )


def test_service_inspector_detects_drop_ins(host_root, fixture_executor):
    """Drop-in overrides under /etc/systemd/system/*.service.d/ are detected."""
    from yoinkc.inspectors.service import run as run_service
    section = run_service(host_root, fixture_executor)
    assert len(section.drop_ins) >= 1
    httpd_dropin = next(
        (d for d in section.drop_ins if d.unit == "httpd.service"), None,
    )
    assert httpd_dropin is not None, (
        f"expected httpd.service drop-in, got units: {[d.unit for d in section.drop_ins]}"
    )
    assert httpd_dropin.path.endswith("override.conf")
    assert "TimeoutStartSec=600" in httpd_dropin.content
