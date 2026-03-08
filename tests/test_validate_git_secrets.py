"""Tests for validate.py, git_github.py, and redact.scan_directory_for_secrets."""

import tempfile
from pathlib import Path

from yoinkc.validate import _append_build_failure_to_reports, run_validate
from yoinkc.redact import scan_directory_for_secrets
from yoinkc.git_github import output_stats


# ---------------------------------------------------------------------------
# validate.py
# ---------------------------------------------------------------------------

def test_run_validate_no_containerfile():
    """When no Containerfile exists, validate returns True (nothing to do)."""
    with tempfile.TemporaryDirectory() as tmp:
        assert run_validate(Path(tmp)) is True


def test_run_validate_podman_not_found(capsys):
    """FileNotFoundError (podman missing) must return False with a warning, not True."""
    import unittest.mock as mock
    with tempfile.TemporaryDirectory() as tmp:
        d = Path(tmp)
        (d / "Containerfile").write_text("FROM scratch\n")
        with mock.patch("subprocess.run", side_effect=FileNotFoundError):
            result = run_validate(d)
    assert result is False
    assert "podman not found" in capsys.readouterr().err


def test_run_validate_build_failure_creates_log(capsys):
    """A non-zero podman exit code must write build-errors.log and return False."""
    import unittest.mock as mock
    with tempfile.TemporaryDirectory() as tmp:
        d = Path(tmp)
        (d / "Containerfile").write_text("FROM scratch\n")
        failed = mock.MagicMock()
        failed.returncode = 1
        failed.stdout = "step 1/1 failed"
        failed.stderr = "error: no such image"
        with mock.patch("subprocess.run", return_value=failed):
            result = run_validate(d)
        assert result is False
        log = d / "build-errors.log"
        assert log.exists(), "build-errors.log must be created on build failure"
        assert "failed" in log.read_text().lower()


def test_append_build_failure_to_audit_report():
    with tempfile.TemporaryDirectory() as tmp:
        d = Path(tmp)
        (d / "audit-report.md").write_text("# Report\n\nSome content.\n")
        _append_build_failure_to_reports(d, "Error: package xyz not found")
        text = (d / "audit-report.md").read_text()
        assert "Build validation failed" in text
        assert "package xyz not found" in text


def test_append_build_failure_to_html_report():
    with tempfile.TemporaryDirectory() as tmp:
        d = Path(tmp)
        (d / "report.html").write_text("<html><body></body></html>")
        _append_build_failure_to_reports(d, "Error: missing dep")
        html = (d / "report.html").read_text()
        assert "Build validation failed" in html
        assert "missing dep" in html


def test_append_build_failure_escapes_html():
    with tempfile.TemporaryDirectory() as tmp:
        d = Path(tmp)
        (d / "report.html").write_text("<html><body></body></html>")
        _append_build_failure_to_reports(d, '<script>alert("xss")</script>')
        html = (d / "report.html").read_text()
        assert "<script>" not in html
        assert "&lt;script&gt;" in html


def test_append_build_failure_no_reports():
    """Gracefully handles missing report files."""
    with tempfile.TemporaryDirectory() as tmp:
        _append_build_failure_to_reports(Path(tmp), "some error")


# ---------------------------------------------------------------------------
# redact.scan_directory_for_secrets
# ---------------------------------------------------------------------------

def test_scan_clean_directory():
    with tempfile.TemporaryDirectory() as tmp:
        d = Path(tmp)
        (d / "config.txt").write_text("some_setting=value\nmode=production\n")
        (d / "notes.md").write_text("# Notes\n\nNothing secret here.\n")
        assert scan_directory_for_secrets(d) is None


def test_scan_detects_api_key():
    with tempfile.TemporaryDirectory() as tmp:
        d = Path(tmp)
        (d / "clean.txt").write_text("hello world\n")
        (d / "config.env").write_text("API_KEY=TESTKEY_not_real_xxxxxxxxxxxxxxxxxxxx\n")
        result = scan_directory_for_secrets(d)
        assert result is not None
        assert "config.env" in result


def test_scan_detects_private_key():
    with tempfile.TemporaryDirectory() as tmp:
        d = Path(tmp)
        (d / "server.key").write_text(
            "-----BEGIN RSA PRIVATE KEY-----\n"
            "MIIEowIBAAKCAQEA...\n"
            "-----END RSA PRIVATE KEY-----\n"
        )
        result = scan_directory_for_secrets(d)
        assert result is not None


def test_scan_detects_password():
    with tempfile.TemporaryDirectory() as tmp:
        d = Path(tmp)
        (d / "db.conf").write_text("password=supersecret123\n")
        result = scan_directory_for_secrets(d)
        assert result is not None


def test_scan_skips_git_directory():
    with tempfile.TemporaryDirectory() as tmp:
        d = Path(tmp)
        git_dir = d / ".git" / "objects"
        git_dir.mkdir(parents=True)
        (git_dir / "secret.txt").write_text("password=leaked\n")
        (d / "clean.txt").write_text("safe content\n")
        assert scan_directory_for_secrets(d) is None


def test_scan_handles_binary_files():
    with tempfile.TemporaryDirectory() as tmp:
        d = Path(tmp)
        (d / "binary.bin").write_bytes(b"\x00\x01\x02\xff\xfe")
        assert scan_directory_for_secrets(d) is None


# ---------------------------------------------------------------------------
# git_github.output_stats
# ---------------------------------------------------------------------------

def test_output_stats_counts():
    with tempfile.TemporaryDirectory() as tmp:
        d = Path(tmp)
        (d / "Containerfile").write_text("FROM base\n# FIXME: check this\nRUN echo\n")
        (d / "audit-report.md").write_text("# Report\n\n2 FIXME items\n\nAnother FIXME.\n")
        sub = d / "config" / "etc"
        sub.mkdir(parents=True)
        (sub / "foo.conf").write_text("key=value\n")
        total, count, fixmes = output_stats(d)
        assert count == 3
        assert fixmes == 3
        assert total > 0


def test_output_stats_empty_dir():
    with tempfile.TemporaryDirectory() as tmp:
        total, count, fixmes = output_stats(Path(tmp))
        assert count == 0
        assert total == 0
        assert fixmes == 0


# ---------------------------------------------------------------------------
# git_github.push_to_github — org vs user repo creation and URL match
# ---------------------------------------------------------------------------

import unittest.mock as _mock


def _make_remote(name, url):
    """Build a MagicMock that looks like a gitpython Remote."""
    r = _mock.MagicMock()
    r.name = name
    r.url = url
    r.push = _mock.MagicMock(return_value=None)
    return r


def _make_remotes_mock(remotes_list):
    """Return a MagicMock whose .origin attribute works and iterates correctly."""
    m = _mock.MagicMock()
    m.__iter__ = _mock.MagicMock(return_value=iter(remotes_list))
    m.__len__ = _mock.MagicMock(return_value=len(remotes_list))
    m.__contains__ = _mock.MagicMock(
        side_effect=lambda name: any(r.name == name for r in remotes_list)
    )
    if remotes_list:
        m.origin = remotes_list[0]
    return m


def _push_test_context(tmp, mock_git, mock_Github, mock_repo):
    """Return a context-manager stack for push_to_github tests."""
    import contextlib, sys
    github_mod = _mock.MagicMock()
    github_mod.Github = mock_Github
    return (
        _mock.patch("yoinkc.redact.scan_directory_for_secrets", return_value=None),
        _mock.patch.dict(sys.modules, {"git": mock_git, "github": github_mod}),
    )


def _run_push(tmp_path, repo_spec, mock_git, mock_Github, mock_repo):
    from yoinkc import git_github
    (tmp_path / ".git").mkdir(exist_ok=True)
    mock_git.Repo.return_value = mock_repo
    patches = _push_test_context(tmp_path, mock_git, mock_Github, mock_repo)
    with patches[0], patches[1]:
        return git_github.push_to_github(
            tmp_path, repo_spec,
            skip_confirmation=True,
            github_token="tok",
        )


def _make_github_mocks(user_login):
    mock_gh_repo = _mock.MagicMock()
    mock_gh_repo.clone_url = "https://github.com/placeholder/repo.git"

    mock_user = _mock.MagicMock()
    mock_user.login = user_login
    mock_user.create_repo = _mock.MagicMock(return_value=mock_gh_repo)

    mock_org = _mock.MagicMock()
    mock_org.create_repo = _mock.MagicMock(return_value=mock_gh_repo)

    mock_g = _mock.MagicMock()
    mock_g.get_user.return_value = mock_user
    mock_g.get_organization.return_value = mock_org

    mock_Github = _mock.MagicMock(return_value=mock_g)

    mock_git = _mock.MagicMock()
    return mock_git, mock_Github, mock_user, mock_org, mock_gh_repo


def test_push_creates_repo_under_org_not_user():
    """When owner != authenticated user, repo is created under the org."""
    mock_git, mock_Github, mock_user, mock_org, mock_gh_repo = _make_github_mocks("alice")
    mock_gh_repo.clone_url = "https://github.com/my-org/myrepo.git"
    mock_repo = _mock.MagicMock()
    mock_repo.remotes = _make_remotes_mock([])  # no origin → will create

    with tempfile.TemporaryDirectory() as tmp:
        _run_push(Path(tmp), "my-org/myrepo", mock_git, mock_Github, mock_repo)

    mock_org.create_repo.assert_called_once()
    mock_user.create_repo.assert_not_called()


def test_push_creates_repo_under_user_when_owner_matches():
    """When owner == authenticated user, repo is created under the user."""
    mock_git, mock_Github, mock_user, mock_org, mock_gh_repo = _make_github_mocks("alice")
    mock_gh_repo.clone_url = "https://github.com/alice/myrepo.git"
    mock_repo = _mock.MagicMock()
    mock_repo.remotes = _make_remotes_mock([])

    with tempfile.TemporaryDirectory() as tmp:
        _run_push(Path(tmp), "alice/myrepo", mock_git, mock_Github, mock_repo)

    mock_user.create_repo.assert_called_once()
    mock_org.create_repo.assert_not_called()


def test_push_url_exact_match_no_spurious_reset():
    """'foo/bar' in origin URL must not match 'foo/bar-legacy.git'."""
    mock_git, mock_Github, mock_user, mock_org, _ = _make_github_mocks("foo")
    origin = _make_remote("origin", "https://github.com/foo/bar-legacy.git")
    mock_repo = _mock.MagicMock()
    mock_repo.remotes = _make_remotes_mock([origin])

    with tempfile.TemporaryDirectory() as tmp:
        _run_push(Path(tmp), "foo/bar", mock_git, mock_Github, mock_repo)

    origin.set_url.assert_called_once_with("https://github.com/foo/bar.git")


def test_push_url_no_reset_when_already_correct():
    """When remote URL already points to the right repo, set_url must not be called."""
    mock_git, mock_Github, mock_user, mock_org, _ = _make_github_mocks("foo")
    origin = _make_remote("origin", "https://github.com/foo/bar.git")
    mock_repo = _mock.MagicMock()
    mock_repo.remotes = _make_remotes_mock([origin])

    with tempfile.TemporaryDirectory() as tmp:
        _run_push(Path(tmp), "foo/bar", mock_git, mock_Github, mock_repo)

    origin.set_url.assert_not_called()
