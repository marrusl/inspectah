"""Tests for resolve_install_set — the shared package list that preflight and renderer use."""

from inspectah.install_set import resolve_install_set
from inspectah.schema import InspectionSnapshot, PackageEntry, PackageState, RpmSection


def _make_snapshot(
    packages=None,
    leaf_packages=None,
    auto_packages=None,
    no_baseline=False,
):
    """Build a minimal snapshot with RPM data for testing install set resolution."""
    entries = []
    for name, include in (packages or []):
        entries.append(PackageEntry(
            name=name, epoch="0", version="1.0", release="1.el9",
            arch="x86_64", state=PackageState.ADDED, include=include,
        ))
    section = RpmSection(
        packages_added=entries, leaf_packages=leaf_packages,
        auto_packages=auto_packages, no_baseline=no_baseline,
    )
    return InspectionSnapshot(rpm=section)


def test_basic_all_included():
    snapshot = _make_snapshot(packages=[("httpd", True), ("nginx", True), ("rsync", True)], no_baseline=True)
    result = resolve_install_set(snapshot)
    assert sorted(result) == ["httpd", "nginx", "rsync"]

def test_exclude_filter():
    snapshot = _make_snapshot(packages=[("httpd", True), ("nginx", False), ("rsync", True)], no_baseline=True)
    result = resolve_install_set(snapshot)
    assert "nginx" not in result
    assert sorted(result) == ["httpd", "rsync"]

def test_leaf_filter_with_baseline():
    snapshot = _make_snapshot(
        packages=[("httpd", True), ("nginx", True), ("mod_ssl", True)],
        leaf_packages=["httpd", "nginx"], auto_packages=["mod_ssl"],
    )
    result = resolve_install_set(snapshot)
    assert sorted(result) == ["httpd", "nginx"]
    assert "mod_ssl" not in result

def test_no_baseline_includes_all():
    snapshot = _make_snapshot(
        packages=[("httpd", True), ("nginx", True), ("mod_ssl", True)], no_baseline=True,
    )
    result = resolve_install_set(snapshot)
    assert sorted(result) == ["httpd", "mod_ssl", "nginx"]

def test_shell_unsafe_names_excluded():
    snapshot = _make_snapshot(packages=[("httpd", True), ("bad;pkg", True), ("rsync", True)], no_baseline=True)
    result = resolve_install_set(snapshot)
    assert "bad;pkg" not in result
    assert sorted(result) == ["httpd", "rsync"]

def test_empty_rpm_section():
    snapshot = InspectionSnapshot()
    result = resolve_install_set(snapshot)
    assert result == []

def test_no_packages_added():
    snapshot = InspectionSnapshot(rpm=RpmSection())
    result = resolve_install_set(snapshot)
    assert result == []

def test_deduplication():
    snapshot = _make_snapshot(packages=[("httpd", True), ("httpd", True), ("nginx", True)], no_baseline=True)
    result = resolve_install_set(snapshot)
    assert result.count("httpd") == 1

def test_result_is_sorted():
    snapshot = _make_snapshot(packages=[("zsh", True), ("apache", True), ("mysql", True)], no_baseline=True)
    result = resolve_install_set(snapshot)
    assert result == ["apache", "mysql", "zsh"]

def test_tuned_injected_when_active():
    from inspectah.schema import KernelBootSection
    snapshot = _make_snapshot(packages=[("httpd", True)], no_baseline=True)
    snapshot.kernel_boot = KernelBootSection(tuned_active="throughput-performance")
    result = resolve_install_set(snapshot)
    assert "tuned" in result

def test_tuned_not_duplicated():
    from inspectah.schema import KernelBootSection
    snapshot = _make_snapshot(packages=[("httpd", True), ("tuned", True)], no_baseline=True)
    snapshot.kernel_boot = KernelBootSection(tuned_active="throughput-performance")
    result = resolve_install_set(snapshot)
    assert result.count("tuned") == 1
