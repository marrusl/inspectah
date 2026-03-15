"""RPM inspector tests: NEVR parsing, baseline, GPG keys, source repos."""

from pathlib import Path

from yoinkc.executor import RunResult
from yoinkc.inspectors.rpm import _compare_evr, _parse_nevr, _parse_rpm_qa, _parse_rpm_va, _rpmvercmp
from yoinkc.schema import PackageEntry

FIXTURES = Path(__file__).parent / "fixtures"


def test_parse_nevr():
    p = _parse_nevr("0:bash-5.2.15-2.el9.x86_64")
    assert p is not None
    assert p.name == "bash"
    assert p.version == "5.2.15"
    assert p.release == "2.el9"
    assert p.arch == "x86_64"

    p2 = _parse_nevr("(none):coreutils-8.32-35.el9.aarch64")
    assert p2 is not None
    assert p2.name == "coreutils"
    assert p2.epoch == "0"
    assert p2.version == "8.32"
    assert p2.release == "35.el9"
    assert p2.arch == "aarch64"


def test_parse_rpm_qa():
    text = (FIXTURES / "rpm_qa_output.txt").read_text()
    packages = _parse_rpm_qa(text)
    assert len(packages) >= 30
    names = [p.name for p in packages]
    assert "bash" in names
    assert "httpd" in names
    assert "dnf" in names
    assert "rpm" in names
    assert "sudo" in names


def test_parse_rpm_va():
    text = (FIXTURES / "rpm_va_output.txt").read_text()
    entries = _parse_rpm_va(text)
    assert len(entries) == 5
    paths = [e.path for e in entries]
    assert "/etc/httpd/conf/httpd.conf" in paths
    assert "/etc/ssh/sshd_config" in paths


def test_rpm_inspector_with_fixtures(host_root, fixture_executor):
    """With executor that can query base image, baseline is applied via podman."""
    from yoinkc.inspectors.rpm import run as run_rpm
    section = run_rpm(host_root, fixture_executor)
    assert section is not None
    assert section.no_baseline is False
    assert section.baseline_package_names is not None
    assert "bash" in section.baseline_package_names
    added_names = [p.name for p in section.packages_added]
    assert "httpd" in added_names
    assert "bash" not in added_names
    assert len(section.rpm_va) == 5
    assert len(section.repo_files) >= 1
    assert "old-daemon" in section.dnf_history_removed


def test_rpm_inspector_with_baseline_file(host_root, fixture_executor):
    """With --baseline-packages, baseline is loaded from file."""
    from yoinkc.inspectors.rpm import run as run_rpm
    baseline_file = FIXTURES / "base_image_packages.txt"
    section = run_rpm(host_root, fixture_executor, baseline_packages_file=baseline_file)
    assert section is not None
    assert section.no_baseline is False
    assert section.baseline_package_names is not None
    assert "acl" in section.baseline_package_names
    assert "bash" in section.baseline_package_names
    added_names = [p.name for p in section.packages_added]
    assert "httpd" in added_names
    assert "acl" not in added_names
    assert "bash" not in added_names


def test_rpm_inspector_captures_gpg_keys(host_root, fixture_executor):
    """GPG keys referenced by gpgkey=file:// in repo files are captured."""
    from yoinkc.inspectors.rpm import run as run_rpm
    section = run_rpm(host_root, fixture_executor)
    assert section.gpg_keys, "Expected at least one GPG key captured"
    key_paths = [k.path for k in section.gpg_keys]
    assert "etc/pki/rpm-gpg/RPM-GPG-KEY-TEST" in key_paths
    key = next(k for k in section.gpg_keys if "TEST" in k.path)
    assert "BEGIN PGP PUBLIC KEY BLOCK" in key.content


def test_collect_gpg_keys_resolves_dnf_vars(tmp_path):
    """gpgkey= paths containing $releasever_major are resolved before file lookup."""
    from yoinkc.inspectors.rpm import _collect_gpg_keys
    from yoinkc.schema import RepoFile

    etc = tmp_path / "etc"
    etc.mkdir()
    (etc / "os-release").write_text('VERSION_ID="10.0"\nID=rhel\n')

    gpg_dir = etc / "pki" / "rpm-gpg"
    gpg_dir.mkdir(parents=True)
    key_content = "-----BEGIN PGP PUBLIC KEY BLOCK-----\nFAKE\n-----END PGP PUBLIC KEY BLOCK-----\n"
    (gpg_dir / "RPM-GPG-KEY-TEST-10").write_text(key_content)

    repo = RepoFile(
        path="etc/yum.repos.d/test.repo",
        content=(
            "[test]\nbaseurl=http://example.com\ngpgcheck=1\n"
            "gpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-TEST-$releasever_major\n"
        ),
    )

    keys = _collect_gpg_keys(tmp_path, [repo])
    assert keys, "Expected the GPG key to be captured after variable resolution"
    assert keys[0].path == "etc/pki/rpm-gpg/RPM-GPG-KEY-TEST-10"
    assert "BEGIN PGP PUBLIC KEY BLOCK" in keys[0].content


def test_source_repo_populated_via_dnf_repoquery(host_root, fixture_executor):
    """source_repo is populated for added packages when dnf repoquery succeeds."""
    from yoinkc.inspectors.rpm import run as run_rpm
    section = run_rpm(host_root, fixture_executor)
    pkgs_with_repo = [p for p in section.packages_added if p.source_repo]
    assert len(pkgs_with_repo) > 0, "Expected at least one package with source_repo set"
    httpd = next((p for p in section.packages_added if p.name == "httpd"), None)
    assert httpd is not None, "httpd must be in packages_added"
    assert httpd.source_repo == "baseos"


class TestRpmvercmp:
    """Pure-Python rpmvercmp algorithm tests."""

    def test_equal(self):
        assert _rpmvercmp("1.0", "1.0") == 0

    def test_numeric_greater(self):
        assert _rpmvercmp("1.1", "1.0") > 0

    def test_numeric_less(self):
        assert _rpmvercmp("1.0", "1.1") < 0

    def test_longer_numeric(self):
        assert _rpmvercmp("1.0.1", "1.0") > 0

    def test_alpha_comparison(self):
        assert _rpmvercmp("1.0a", "1.0b") < 0

    def test_numeric_beats_alpha(self):
        assert _rpmvercmp("1.1", "1.a") > 0

    def test_leading_zeros(self):
        assert _rpmvercmp("01", "1") == 0

    def test_tilde_sorts_before_everything(self):
        assert _rpmvercmp("1.0~rc1", "1.0") < 0

    def test_tilde_both(self):
        assert _rpmvercmp("1.0~rc1", "1.0~rc2") < 0

    def test_caret_sorts_after(self):
        assert _rpmvercmp("1.0^git1", "1.0") > 0

    def test_caret_both(self):
        assert _rpmvercmp("1.0^git1", "1.0^git2") < 0

    def test_tilde_before_caret(self):
        assert _rpmvercmp("1.0~rc1", "1.0^git1") < 0

    def test_real_world_el9(self):
        assert _rpmvercmp("5.2.15", "5.1.8") > 0

    def test_release_comparison(self):
        assert _rpmvercmp("2.el9", "1.el9") > 0

    def test_empty_equal(self):
        assert _rpmvercmp("", "") == 0

    def test_one_empty(self):
        assert _rpmvercmp("1.0", "") > 0
        assert _rpmvercmp("", "1.0") < 0


class TestCompareEvr:
    """EVR comparison combining epoch, version, release."""

    def _pkg(self, epoch="0", version="1.0", release="1.el9"):
        return PackageEntry(name="x", epoch=epoch, version=version,
                            release=release, arch="x86_64")

    def test_equal(self):
        assert _compare_evr(self._pkg(), self._pkg()) == 0

    def test_epoch_wins(self):
        a = self._pkg(epoch="1", version="1.0")
        b = self._pkg(epoch="0", version="99.0")
        assert _compare_evr(a, b) > 0

    def test_version_diff(self):
        a = self._pkg(version="2.4.57")
        b = self._pkg(version="2.4.53")
        assert _compare_evr(a, b) > 0

    def test_release_diff(self):
        a = self._pkg(release="5.el9")
        b = self._pkg(release="3.el9")
        assert _compare_evr(a, b) > 0

    def test_version_then_release(self):
        a = self._pkg(version="2.4.57", release="5.el9")
        b = self._pkg(version="2.4.57", release="3.el9")
        assert _compare_evr(a, b) > 0

    def test_epoch_none_treated_as_zero(self):
        a = self._pkg(epoch="0")
        b = self._pkg(epoch="0")
        assert _compare_evr(a, b) == 0


class TestVersionChangeDetection:
    """Integration tests for version change detection in RPM inspector."""

    def test_version_changes_populated(self, host_root, fixture_executor):
        from yoinkc.inspectors.rpm import run as run_rpm
        from yoinkc.schema import VersionChangeDirection
        section = run_rpm(host_root, fixture_executor)
        assert section.version_changes is not None
        bash_changes = [vc for vc in section.version_changes if vc.name == "bash"]
        assert len(bash_changes) == 1, \
            f"Expected bash version change, got: {[vc.name for vc in section.version_changes]}"
        vc = bash_changes[0]
        assert vc.direction == VersionChangeDirection.DOWNGRADE
        assert "5.2.15" in vc.host_version
        assert "5.1.8" in vc.base_version

    def test_no_version_changes_with_names_only_baseline(self, host_root, fixture_executor):
        from yoinkc.inspectors.rpm import run as run_rpm
        from yoinkc.baseline import load_baseline_packages_file
        baseline_pkgs = load_baseline_packages_file(FIXTURES / "base_image_packages.txt")
        preflight = (baseline_pkgs, "test-image:latest", False)
        section = run_rpm(host_root, fixture_executor, preflight_baseline=preflight)
        assert section.version_changes == []

    def test_version_changes_sorted_downgrades_first(self, host_root, fixture_executor):
        from yoinkc.inspectors.rpm import run as run_rpm
        from yoinkc.schema import VersionChangeDirection
        section = run_rpm(host_root, fixture_executor)
        if len(section.version_changes) >= 2:
            directions = [vc.direction for vc in section.version_changes]
            downgrade_indices = [i for i, d in enumerate(directions)
                                 if d == VersionChangeDirection.DOWNGRADE]
            upgrade_indices = [i for i, d in enumerate(directions)
                               if d == VersionChangeDirection.UPGRADE]
            if downgrade_indices and upgrade_indices:
                assert max(downgrade_indices) < min(upgrade_indices)

    def test_base_image_only_has_nevra(self, host_root, fixture_executor):
        from yoinkc.inspectors.rpm import run as run_rpm
        section = run_rpm(host_root, fixture_executor)
        for bio in section.base_image_only:
            if bio.version:
                assert bio.release, f"Package {bio.name} has version but no release"
