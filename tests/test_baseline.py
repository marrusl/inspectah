"""Tests for baseline generation (base image query)."""

import subprocess
from pathlib import Path
from unittest.mock import patch

import yoinkc.baseline as baseline_mod
from yoinkc.baseline import (
    BaselineResolver,
    select_base_image,
    load_baseline_packages_file,
)
from yoinkc.executor import RunResult
from yoinkc.schema import PackageEntry


FIXTURES = Path(__file__).parent / "fixtures"


def _make_executor(podman_result=None, probe_ok=True):
    """Build a mock executor that handles the nsenter probe and podman commands."""
    def executor(cmd, cwd=None):
        if cmd[-1] == "true" and "nsenter" in cmd:
            if probe_ok:
                return RunResult(stdout="", stderr="", returncode=0)
            return RunResult(stdout="", stderr="Operation not permitted", returncode=1)
        # Always report the image as cached so pull_image() is a no-op in tests.
        if "podman" in cmd and "image" in cmd and "exists" in cmd:
            return RunResult(stdout="", stderr="", returncode=0)
        if podman_result is not None and "podman" in cmd:
            return podman_result(cmd) if callable(podman_result) else podman_result
        return RunResult(stdout="", stderr="", returncode=1)
    return executor


# ---------------------------------------------------------------------------
# select_base_image / load_baseline_packages_file (pure functions)
# ---------------------------------------------------------------------------

def test_select_base_image_rhel9_clamped():
    image, ver = select_base_image("rhel", "9.4")
    assert image == "registry.redhat.io/rhel9/rhel-bootc:9.6"
    assert ver == "9.6"


def test_select_base_image_rhel9_at_minimum():
    image, ver = select_base_image("rhel", "9.6")
    assert image == "registry.redhat.io/rhel9/rhel-bootc:9.6"
    assert ver == "9.6"


def test_select_base_image_rhel9_above_minimum():
    image, ver = select_base_image("rhel", "9.8")
    assert image == "registry.redhat.io/rhel9/rhel-bootc:9.8"
    assert ver == "9.8"


def test_select_base_image_rhel9_target_override():
    image, ver = select_base_image("rhel", "9.4", target_version="9.8")
    assert image == "registry.redhat.io/rhel9/rhel-bootc:9.8"
    assert ver == "9.8"


def test_select_base_image_rhel9_target_below_minimum():
    image, ver = select_base_image("rhel", "9.4", target_version="9.2")
    assert image == "registry.redhat.io/rhel9/rhel-bootc:9.6"
    assert ver == "9.6"


def test_select_base_image_rhel10():
    image, ver = select_base_image("rhel", "10.0")
    assert image == "registry.redhat.io/rhel10/rhel-bootc:10.0"
    assert ver == "10.0"


def test_select_base_image_rhel10_target_override():
    image, ver = select_base_image("rhel", "10.0", target_version="10.2")
    assert image == "registry.redhat.io/rhel10/rhel-bootc:10.2"
    assert ver == "10.2"


def test_select_base_image_centos_stream9():
    image, ver = select_base_image("centos", "9")
    assert image == "quay.io/centos-bootc/centos-bootc:stream9"


def test_select_base_image_centos_stream10():
    image, ver = select_base_image("centos", "10")
    assert image == "quay.io/centos-bootc/centos-bootc:stream10"


def test_select_base_image_fedora():
    image, ver = select_base_image("fedora", "41")
    assert image == "quay.io/fedora/fedora-bootc:41"
    assert ver == "41"


def test_select_base_image_fedora_clamped():
    image, ver = select_base_image("fedora", "40")
    assert image == "quay.io/fedora/fedora-bootc:41"
    assert ver == "41"


def test_select_base_image_fedora_above_minimum():
    image, ver = select_base_image("fedora", "42")
    assert image == "quay.io/fedora/fedora-bootc:42"
    assert ver == "42"


def test_select_base_image_unknown():
    image, ver = select_base_image("ubuntu", "24.04")
    assert image is None
    assert ver is None


def test_load_baseline_packages_file():
    path = FIXTURES / "base_image_packages.txt"
    names = load_baseline_packages_file(path)
    assert names is not None
    assert "bash" in names
    assert "glibc" in names
    assert len(names) > 10


def test_load_baseline_packages_file_missing(tmp_path):
    assert load_baseline_packages_file(tmp_path / "nope.txt") is None


class TestBaselineNevraFormat:
    """Auto-detection of NEVRA vs names-only baseline files."""

    def test_load_nevra_format(self):
        result = load_baseline_packages_file(FIXTURES / "base_image_packages_nevra.txt")
        assert result is not None
        assert isinstance(result, dict)
        assert "bash.x86_64" in result
        pkg = result["bash.x86_64"]
        assert isinstance(pkg, PackageEntry)
        assert pkg.name == "bash"
        assert pkg.version == "5.1.8"
        assert pkg.release == "9.el9"
        assert pkg.arch == "x86_64"

    def test_load_names_only_format(self):
        result = load_baseline_packages_file(FIXTURES / "base_image_packages.txt")
        assert result is not None
        assert isinstance(result, dict)
        assert "bash" in result
        pkg = result["bash"]
        assert isinstance(pkg, PackageEntry)
        assert pkg.name == "bash"
        assert pkg.version == ""
        assert pkg.arch == ""

    def test_load_names_only_name_set(self):
        result = load_baseline_packages_file(FIXTURES / "base_image_packages.txt")
        assert result is not None
        name_set = {p.name for p in result.values()}
        assert "bash" in name_set
        assert "glibc" in name_set


@patch.object(baseline_mod, "in_user_namespace", return_value=False)
def test_query_packages_returns_nevra_dict(_mock_userns):
    """query_packages() returns Dict[str, PackageEntry] with full NEVRA."""
    nevra_output = (
        "0:bash-5.1.8-9.el9.x86_64\n"
        "0:glibc-2.34-100.el9.x86_64\n"
        "(none):setup-2.13.7-10.el9.noarch\n"
    )

    def podman_handler(cmd):
        if "rpm" in cmd:
            return RunResult(stdout=nevra_output, stderr="", returncode=0)
        return RunResult(stdout="", stderr="", returncode=1)

    resolver = BaselineResolver(_make_executor(podman_result=podman_handler))
    result = resolver.query_packages("test-image:latest")
    assert result is not None
    assert isinstance(result, dict)
    assert "bash.x86_64" in result
    assert result["bash.x86_64"].version == "5.1.8"
    assert "setup.noarch" in result
    assert result["setup.noarch"].epoch == "0"


# ---------------------------------------------------------------------------
# BaselineResolver.get_baseline_packages — file and no-executor paths
# ---------------------------------------------------------------------------

def test_get_baseline_with_file():
    """--baseline-packages FILE loads the file directly, no podman needed."""
    host_root = FIXTURES / "host_etc"
    resolver = BaselineResolver(None)
    names, base_image, no_baseline = resolver.get_baseline_packages(
        host_root, "centos", "9",
        baseline_packages_file=FIXTURES / "base_image_packages.txt",
    )
    assert no_baseline is False
    assert names is not None
    assert any(p.name == "bash" for p in names.values())
    assert base_image == "quay.io/centos-bootc/centos-bootc:stream9"


def test_get_baseline_no_executor_no_file():
    """Without executor or file, falls back to no-baseline mode."""
    host_root = FIXTURES / "host_etc"
    resolver = BaselineResolver(None)
    names, base_image, no_baseline = resolver.get_baseline_packages(
        host_root, "centos", "9",
    )
    assert no_baseline is True


# ---------------------------------------------------------------------------
# BaselineResolver — no global state, each test is independent
# ---------------------------------------------------------------------------

@patch.object(baseline_mod, "in_user_namespace", return_value=False)
def test_resolver_with_podman(_mock_userns):
    """Resolver queries podman when probe succeeds."""
    host_root = FIXTURES / "host_etc"
    pkg_list = (FIXTURES / "base_image_packages_nevra.txt").read_text()

    def podman_handler(cmd):
        if "rpm" in cmd:
            return RunResult(stdout=pkg_list, stderr="", returncode=0)
        return RunResult(stdout="", stderr="", returncode=1)

    resolver = BaselineResolver(_make_executor(podman_result=podman_handler))
    names, base_image, no_baseline = resolver.get_baseline_packages(
        host_root, "centos", "9",
    )
    assert no_baseline is False
    assert names is not None
    assert any(p.name == "bash" for p in names.values())
    assert any(p.name == "glibc" for p in names.values())


@patch.object(baseline_mod, "in_user_namespace", return_value=False)
def test_resolver_podman_fails(_mock_userns):
    """When podman fails, resolver falls back to no-baseline mode."""
    host_root = FIXTURES / "host_etc"
    podman_err = RunResult(stdout="", stderr="Error: ...", returncode=125)
    resolver = BaselineResolver(_make_executor(podman_result=podman_err))
    names, base_image, no_baseline = resolver.get_baseline_packages(
        host_root, "centos", "9",
    )
    assert no_baseline is True
    assert base_image == "quay.io/centos-bootc/centos-bootc:stream9"


@patch.object(baseline_mod, "in_user_namespace", return_value=False)
def test_resolver_nsenter_eperm_falls_back(_mock_userns):
    """nsenter EPERM → probe fails → no-baseline mode."""
    host_root = FIXTURES / "host_etc"
    resolver = BaselineResolver(_make_executor(probe_ok=False))
    names, base_image, no_baseline = resolver.get_baseline_packages(
        host_root, "centos", "9",
    )
    assert no_baseline is True
    assert base_image == "quay.io/centos-bootc/centos-bootc:stream9"


@patch.object(baseline_mod, "in_user_namespace", return_value=True)
def test_resolver_skipped_in_user_namespace(_mock_userns):
    """User namespace detected → nsenter never attempted, no executor calls."""
    host_root = FIXTURES / "host_etc"
    calls = []

    def tracking_executor(cmd, cwd=None):
        calls.append(cmd)
        return RunResult(stdout="", stderr="", returncode=0)

    resolver = BaselineResolver(tracking_executor)
    names, base_image, no_baseline = resolver.get_baseline_packages(
        host_root, "centos", "9",
    )
    assert no_baseline is True
    assert len(calls) == 0, "No commands should be executed when in user namespace"


@patch.object(baseline_mod, "in_user_namespace", return_value=False)
def test_resolver_probe_cached(_mock_userns):
    """nsenter probe runs exactly once even when called multiple times."""
    probe_calls = []

    def executor(cmd, cwd=None):
        if cmd[-1] == "true" and "nsenter" in cmd:
            probe_calls.append(cmd)
            return RunResult(stdout="", stderr="", returncode=0)
        return RunResult(stdout="", stderr="", returncode=1)

    resolver = BaselineResolver(executor)
    resolver._probe_nsenter()
    resolver._probe_nsenter()
    resolver._probe_nsenter()
    assert len(probe_calls) == 1, "Probe should be cached after first call"


@patch.object(baseline_mod, "in_user_namespace", return_value=False)
def test_resolver_instances_independent(_mock_userns):
    """Two resolver instances have independent probe caches."""
    r1 = BaselineResolver(_make_executor(probe_ok=True))
    r2 = BaselineResolver(_make_executor(probe_ok=False))
    assert r1._probe_nsenter() is True
    assert r2._probe_nsenter() is False
    # r1's state is unchanged
    assert r1._nsenter_available is True
    assert r2._nsenter_available is False


# ---------------------------------------------------------------------------
# BaselineResolver.resolve — unified entry point
# ---------------------------------------------------------------------------

def test_resolve_target_image_with_file():
    """resolve() with --target-image and --baseline-packages loads from file."""
    resolver = BaselineResolver(None)
    names, image, no_baseline = resolver.resolve(
        FIXTURES / "host_etc", "rhel", "9.4",
        baseline_packages_file=FIXTURES / "base_image_packages.txt",
        target_image="my-registry.example.com/custom:latest",
    )
    assert no_baseline is False
    assert image == "my-registry.example.com/custom:latest"
    assert names is not None
    assert any(p.name == "bash" for p in names.values())


def test_resolve_target_image_no_executor():
    """resolve() with --target-image but no executor returns no_baseline=True."""
    resolver = BaselineResolver(None)
    names, image, no_baseline = resolver.resolve(
        FIXTURES / "host_etc", "rhel", "9.4",
        target_image="registry.redhat.io/rhel9/rhel-bootc:9.6",
    )
    assert no_baseline is True
    assert image == "registry.redhat.io/rhel9/rhel-bootc:9.6"
    assert names is None


@patch.object(baseline_mod, "in_user_namespace", return_value=False)
def test_resolve_target_image_with_executor(_mock_userns):
    """resolve() with --target-image and an executor queries podman."""
    pkg_list = (FIXTURES / "base_image_packages_nevra.txt").read_text()

    def podman_handler(cmd):
        if "rpm" in cmd:
            return RunResult(stdout=pkg_list, stderr="", returncode=0)
        return RunResult(stdout="", stderr="", returncode=1)

    resolver = BaselineResolver(_make_executor(podman_result=podman_handler))
    names, image, no_baseline = resolver.resolve(
        FIXTURES / "host_etc", "centos", "9",
        target_image="quay.io/centos-bootc/centos-bootc:stream9",
    )
    assert no_baseline is False
    assert image == "quay.io/centos-bootc/centos-bootc:stream9"
    assert names is not None
    assert any(p.name == "bash" for p in names.values())


def test_resolve_delegates_to_get_baseline_packages():
    """resolve() without --target-image delegates to get_baseline_packages."""
    resolver = BaselineResolver(None)
    names, base_image, no_baseline = resolver.resolve(
        FIXTURES / "host_etc", "centos", "9",
        baseline_packages_file=FIXTURES / "base_image_packages.txt",
    )
    assert no_baseline is False
    assert base_image == "quay.io/centos-bootc/centos-bootc:stream9"
    assert any(p.name == "bash" for p in names.values())


# ---------------------------------------------------------------------------
# pull_image / _image_is_cached
# ---------------------------------------------------------------------------

# ---------------------------------------------------------------------------
# BaselineResolver.query_module_streams
# ---------------------------------------------------------------------------

@patch.object(baseline_mod, "in_user_namespace", return_value=False)
def test_query_module_streams_returns_dict(_mock_userns):
    """query_module_streams() parses module INI output into {name: stream}."""
    module_output = (
        "[postgresql]\n"
        "name=postgresql\n"
        "stream=15\n"
        "profiles=server\n"
        "state=enabled\n"
        "\n"
        "[nodejs]\n"
        "name=nodejs\n"
        "stream=18\n"
        "profiles=common\n"
        "state=installed\n"
        "\n"
        "[nginx]\n"
        "name=nginx\n"
        "stream=mainline\n"
        "state=disabled\n"
    )

    def podman_handler(cmd):
        if "cat" in " ".join(cmd):
            return RunResult(stdout=module_output, stderr="", returncode=0)
        return RunResult(stdout="", stderr="", returncode=1)

    resolver = BaselineResolver(_make_executor(podman_result=podman_handler))
    result = resolver.query_module_streams("test-image:latest")
    assert result == {"postgresql": "15", "nodejs": "18"}


@patch.object(baseline_mod, "in_user_namespace", return_value=False)
def test_query_module_streams_empty_output(_mock_userns):
    """Empty podman output returns an empty dict (no module streams in image)."""
    def podman_handler(cmd):
        return RunResult(stdout="", stderr="", returncode=0)

    resolver = BaselineResolver(_make_executor(podman_result=podman_handler))
    result = resolver.query_module_streams("test-image:latest")
    assert result == {}


@patch.object(baseline_mod, "in_user_namespace", return_value=False)
def test_query_module_streams_podman_failure(_mock_userns):
    """Podman command failure returns an empty dict, not an exception."""
    def podman_handler(cmd):
        if "cat" in " ".join(cmd):
            return RunResult(stdout="", stderr="Error: no such container", returncode=1)
        return RunResult(stdout="", stderr="", returncode=0)

    resolver = BaselineResolver(_make_executor(podman_result=podman_handler))
    result = resolver.query_module_streams("test-image:latest")
    assert result == {}


@patch.object(baseline_mod, "in_user_namespace", return_value=False)
def test_pull_image_skipped_when_cached(_mock_userns):
    """pull_image() returns True immediately when the image is already cached."""
    cmds = []

    def executor(cmd, cwd=None):
        cmds.append(cmd)
        if cmd[-1] == "true" and "nsenter" in cmd:
            return RunResult(stdout="", stderr="", returncode=0)
        if "podman" in cmd and "image" in cmd and "exists" in cmd:
            return RunResult(stdout="", stderr="", returncode=0)  # cached
        return RunResult(stdout="", stderr="", returncode=1)

    resolver = BaselineResolver(executor)
    result = resolver.pull_image("quay.io/centos-bootc/centos-bootc:stream9")
    assert result is True
    pull_cmds = [c for c in cmds if "podman" in c and "pull" in c]
    assert len(pull_cmds) == 0, "pull should be skipped when image is already cached"


def _not_cached_executor(cmd, cwd=None):
    """Executor that reports nsenter available but the image not cached."""
    if cmd[-1] == "true" and "nsenter" in cmd:
        return RunResult(stdout="", stderr="", returncode=0)
    if "podman" in cmd and "image" in cmd and "exists" in cmd:
        return RunResult(stdout="", stderr="", returncode=1)  # not cached
    return RunResult(stdout="", stderr="", returncode=1)


@patch.object(baseline_mod, "in_user_namespace", return_value=False)
def test_pull_image_triggers_subprocess_when_not_cached(_mock_userns):
    """pull_image() calls subprocess.run when the image is not cached."""
    subprocess_calls = []

    def fake_subprocess_run(cmd, **kwargs):
        subprocess_calls.append(cmd)
        return subprocess.CompletedProcess(cmd, returncode=0)

    resolver = BaselineResolver(_not_cached_executor)
    with patch("yoinkc.baseline.subprocess.run", fake_subprocess_run):
        result = resolver.pull_image("quay.io/centos-bootc/centos-bootc:stream9")

    assert result is True
    assert len(subprocess_calls) == 1
    pull_cmd = subprocess_calls[0]
    assert "podman" in pull_cmd
    assert "pull" in pull_cmd
    assert "quay.io/centos-bootc/centos-bootc:stream9" in pull_cmd
    assert "nsenter" in pull_cmd


@patch.object(baseline_mod, "in_user_namespace", return_value=False)
def test_pull_image_returns_false_on_subprocess_failure(_mock_userns):
    """pull_image() returns False when podman pull exits non-zero."""
    def failing_subprocess_run(cmd, **kwargs):
        return subprocess.CompletedProcess(cmd, returncode=125)

    resolver = BaselineResolver(_not_cached_executor)
    with patch("yoinkc.baseline.subprocess.run", failing_subprocess_run):
        result = resolver.pull_image("quay.io/centos-bootc/centos-bootc:stream9")

    assert result is False


@patch.object(baseline_mod, "in_user_namespace", return_value=False)
def test_pull_image_returns_false_on_timeout(_mock_userns, capsys):
    """pull_image() returns False and prints an error when podman pull times out."""
    def timeout_subprocess_run(cmd, **kwargs):
        raise subprocess.TimeoutExpired(cmd, timeout=baseline_mod._PULL_TIMEOUT_S)

    resolver = BaselineResolver(_not_cached_executor)
    with patch("yoinkc.baseline.subprocess.run", timeout_subprocess_run):
        result = resolver.pull_image("quay.io/centos-bootc/centos-bootc:stream9")

    assert result is False
    assert "timed out" in capsys.readouterr().err


@patch.object(baseline_mod, "in_user_namespace", return_value=False)
def test_pull_image_returns_false_on_file_not_found(_mock_userns, capsys):
    """pull_image() returns False and prints an error when nsenter/podman is not found."""
    def fnfe_subprocess_run(cmd, **kwargs):
        raise FileNotFoundError("nsenter not found")

    resolver = BaselineResolver(_not_cached_executor)
    with patch("yoinkc.baseline.subprocess.run", fnfe_subprocess_run):
        result = resolver.pull_image("quay.io/centos-bootc/centos-bootc:stream9")

    assert result is False
    assert "not found" in capsys.readouterr().err


@patch.object(baseline_mod, "in_user_namespace", return_value=False)
def test_pull_image_skipped_when_nsenter_unavailable(_mock_userns):
    """pull_image() returns False without calling subprocess when nsenter fails."""
    def eperm_executor(cmd, cwd=None):
        if cmd[-1] == "true" and "nsenter" in cmd:
            return RunResult(stdout="", stderr="Operation not permitted", returncode=1)
        if "podman" in cmd and "image" in cmd and "exists" in cmd:
            return RunResult(stdout="", stderr="", returncode=1)
        return RunResult(stdout="", stderr="", returncode=1)

    subprocess_calls = []

    def should_not_be_called(cmd, **kwargs):
        subprocess_calls.append(cmd)
        return subprocess.CompletedProcess(cmd, returncode=0)

    resolver = BaselineResolver(eperm_executor)
    with patch("yoinkc.baseline.subprocess.run", should_not_be_called):
        result = resolver.pull_image("quay.io/centos-bootc/centos-bootc:stream9")

    assert result is False
    assert len(subprocess_calls) == 0, "subprocess.run must not be called when nsenter unavailable"
