"""Flatpak detection tests -- runs on all system types."""
from pathlib import Path
from yoinkc.executor import RunResult

_FLATPAK_LIST_OUTPUT = (
    "org.mozilla.firefox\tflathub\tstable\n"
    "org.gnome.Calculator\tflathub\tstable\n"
    "org.fedoraproject.MediaWriter\tfedora\tstable\n"
)

def _flatpak_executor(cmd, *, cwd=None):
    if cmd == ["which", "flatpak"]:
        return RunResult(stdout="/usr/bin/flatpak\n", stderr="", returncode=0)
    if cmd[:2] == ["flatpak", "list"]:
        return RunResult(stdout=_FLATPAK_LIST_OUTPUT, stderr="", returncode=0)
    return RunResult(stdout="", stderr="", returncode=1)

def _no_flatpak_executor(cmd, *, cwd=None):
    if cmd == ["which", "flatpak"]:
        return RunResult(stdout="", stderr="not found", returncode=1)
    return RunResult(stdout="", stderr="", returncode=1)

def test_flatpak_apps_detected(tmp_path):
    from yoinkc.inspectors.container import run as run_container
    section = run_container(tmp_path, _flatpak_executor)
    assert len(section.flatpak_apps) == 3
    ids = {a.app_id for a in section.flatpak_apps}
    assert ids == {"org.mozilla.firefox", "org.gnome.Calculator", "org.fedoraproject.MediaWriter"}

def test_flatpak_origin_captured(tmp_path):
    from yoinkc.inspectors.container import run as run_container
    section = run_container(tmp_path, _flatpak_executor)
    firefox = next(a for a in section.flatpak_apps if a.app_id == "org.mozilla.firefox")
    assert firefox.origin == "flathub"

def test_flatpak_not_present_silently_skipped(tmp_path):
    from yoinkc.inspectors.container import run as run_container
    section = run_container(tmp_path, _no_flatpak_executor)
    assert section.flatpak_apps == []

def test_flatpak_list_nonzero_exit_handled(tmp_path):
    from yoinkc.inspectors.container import run as run_container
    def failing_exec(cmd, *, cwd=None):
        if cmd == ["which", "flatpak"]:
            return RunResult(stdout="/usr/bin/flatpak", stderr="", returncode=0)
        if cmd[:2] == ["flatpak", "list"]:
            return RunResult(stdout="", stderr="error", returncode=1)
        return RunResult(stdout="", stderr="", returncode=1)
    section = run_container(tmp_path, failing_exec)
    assert section.flatpak_apps == []

def test_flatpak_malformed_output_no_crash(tmp_path):
    from yoinkc.inspectors.container import run as run_container
    def bad_exec(cmd, *, cwd=None):
        if cmd == ["which", "flatpak"]:
            return RunResult(stdout="/usr/bin/flatpak", stderr="", returncode=0)
        if cmd[:2] == ["flatpak", "list"]:
            return RunResult(
                stdout="org.valid.App\tflathub\tstable\nsingle-column-only\n\n",
                stderr="", returncode=0,
            )
        return RunResult(stdout="", stderr="", returncode=1)
    section = run_container(tmp_path, bad_exec)
    assert len(section.flatpak_apps) == 1
    assert section.flatpak_apps[0].app_id == "org.valid.App"
