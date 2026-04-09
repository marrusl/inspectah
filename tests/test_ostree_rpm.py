"""Tests for RPM inspector ostree/bootc adaptations.

Verifies that:
- rpm -Va is skipped on ostree/bootc systems
- rpm-ostree status --json is parsed for layered, removed, and overridden packages
- Failures and invalid JSON are handled gracefully
"""

import json
from pathlib import Path
from typing import List

from yoinkc.executor import RunResult
from yoinkc.inspectors.rpm import run as run_rpm
from yoinkc.schema import SystemType, OstreePackageOverride, PackageEntry


FIXTURES = Path(__file__).parent / "fixtures"

# Realistic rpm-ostree status --json output
_RPMOSTREE_STATUS_JSON = json.dumps({
    "deployments": [
        {
            "booted": True,
            "requested-packages": ["httpd", "vim-enhanced", "htop"],
            "requested-local-packages": [],
            "base-removals": [
                {"name": "nano", "nevra": "nano-7.2-3.fc41.x86_64"}
            ],
            "base-local-replacements": [{
                "name": "kernel",
                "nevra": "kernel-6.8.1-100.fc41.x86_64",
                "base-nevra": "kernel-6.7.9-200.fc41.x86_64",
            }],
        },
        {
            "booted": False,
            "requested-packages": ["old-pkg"],
            "requested-local-packages": [],
            "base-removals": [],
            "base-local-replacements": [],
        },
    ]
})


class OstreeExecutorSpy:
    """Executor that records all commands and returns canned responses.

    Allows test assertions that certain commands were (or were not) invoked.
    """

    def __init__(self, rpmostree_stdout: str = _RPMOSTREE_STATUS_JSON,
                 rpmostree_rc: int = 0):
        self.commands_called: List[str] = []
        self._rpmostree_stdout = rpmostree_stdout
        self._rpmostree_rc = rpmostree_rc

    def __call__(self, cmd, cwd=None):
        cmd_str = " ".join(cmd)
        self.commands_called.append(cmd_str)

        # nsenter probe
        if cmd[-1] == "true" and "nsenter" in cmd:
            return RunResult(stdout="", stderr="", returncode=0)

        # rpm -qa -- return minimal package list
        if "rpm" in cmd_str and "-qa" in cmd_str:
            return RunResult(
                stdout="0:bash-5.2.15-2.el9.x86_64\n0:coreutils-8.32-35.el9.x86_64\n",
                stderr="",
                returncode=0,
            )

        # rpm -Va -- should NOT be called on ostree but still provide a response
        if "rpm" in cmd_str and "-Va" in cmd_str:
            return RunResult(
                stdout="S.5....T.  c /etc/should-not-appear.conf\n",
                stderr="",
                returncode=0,
            )

        # rpm-ostree status --json
        if "rpm-ostree" in cmd_str and "status" in cmd_str:
            return RunResult(
                stdout=self._rpmostree_stdout,
                stderr="" if self._rpmostree_rc == 0 else "error: rpm-ostree unavailable",
                returncode=self._rpmostree_rc,
            )

        # dnf commands -- fail gracefully
        if "dnf" in cmd_str:
            return RunResult(stdout="", stderr="", returncode=1)

        # podman login check
        if "podman" in cmd_str and "login" in cmd_str:
            return RunResult(stdout="testuser\n", stderr="", returncode=0)

        # podman image exists
        if "podman" in cmd_str and "image" in cmd_str and "exists" in cmd_str:
            return RunResult(stdout="", stderr="", returncode=0)

        # podman rpm query for baseline
        if "podman" in cmd_str and "rpm" in cmd_str:
            return RunResult(stdout="", stderr="", returncode=1)

        return RunResult(stdout="", stderr="unknown command", returncode=1)

    def was_called(self, substring: str) -> bool:
        """Check if any command contained the given substring."""
        return any(substring in c for c in self.commands_called)


class TestRpmVaSkippedOnOstree:
    """rpm -Va must NEVER be called on ostree/bootc systems."""

    def test_rpm_va_not_called_on_ostree(self):
        spy = OstreeExecutorSpy()
        section = run_rpm(
            Path("/fake-root"),
            spy,
            system_type=SystemType.RPM_OSTREE,
        )
        assert not spy.was_called("-Va"), (
            f"rpm -Va was called on ostree system! Commands: {spy.commands_called}"
        )
        # rpm_va should be empty
        assert section.rpm_va == []

    def test_rpm_va_not_called_on_bootc(self):
        spy = OstreeExecutorSpy()
        section = run_rpm(
            Path("/fake-root"),
            spy,
            system_type=SystemType.BOOTC,
        )
        assert not spy.was_called("-Va"), (
            f"rpm -Va was called on bootc system! Commands: {spy.commands_called}"
        )
        assert section.rpm_va == []

    def test_rpm_va_still_called_on_package_mode(self):
        spy = OstreeExecutorSpy()
        section = run_rpm(
            Path("/fake-root"),
            spy,
            system_type=SystemType.PACKAGE_MODE,
        )
        assert spy.was_called("-Va"), (
            "rpm -Va was NOT called on package-mode system (should have been)"
        )


class TestLayeredPackages:
    """Layered packages from rpm-ostree status appear in packages_added."""

    def test_layered_packages_from_rpmostree_status(self):
        spy = OstreeExecutorSpy()
        section = run_rpm(
            Path("/fake-root"),
            spy,
            system_type=SystemType.RPM_OSTREE,
        )
        added_names = [p.name for p in section.packages_added]
        # httpd is in both rpm -qa (no, it isn't in our minimal list) and
        # rpm-ostree status requested-packages. It should appear.
        assert "httpd" in added_names, f"httpd missing from packages_added: {added_names}"
        assert "vim-enhanced" in added_names, f"vim-enhanced missing: {added_names}"
        assert "htop" in added_names, f"htop missing: {added_names}"

    def test_layered_packages_not_duplicated(self):
        """If a package is already in packages_added from rpm -qa parsing,
        it should not be added again from rpm-ostree status."""
        # Make rpm -qa return httpd so it's already in packages_added
        spy = OstreeExecutorSpy()

        # Override to include httpd in rpm -qa output
        original_call = spy.__call__

        def patched_call(cmd, cwd=None):
            cmd_str = " ".join(cmd)
            if "rpm" in cmd_str and "-qa" in cmd_str:
                return RunResult(
                    stdout="0:httpd-2.4.57-5.el9.x86_64\n0:bash-5.2.15-2.el9.x86_64\n",
                    stderr="",
                    returncode=0,
                )
            return original_call(cmd, cwd)

        spy.__call__ = patched_call

        section = run_rpm(
            Path("/fake-root"),
            spy,
            system_type=SystemType.RPM_OSTREE,
        )
        httpd_entries = [p for p in section.packages_added if p.name == "httpd"]
        assert len(httpd_entries) == 1, (
            f"httpd should appear exactly once, got {len(httpd_entries)}"
        )


class TestRemovedPackages:
    """base-removals from rpm-ostree status appear in ostree_removals."""

    def test_removed_packages_captured(self):
        spy = OstreeExecutorSpy()
        section = run_rpm(
            Path("/fake-root"),
            spy,
            system_type=SystemType.RPM_OSTREE,
        )
        assert "nano" in section.ostree_removals, (
            f"nano should be in ostree_removals, got: {section.ostree_removals}"
        )


class TestOverriddenPackages:
    """base-local-replacements from rpm-ostree status appear in ostree_overrides."""

    def test_overridden_packages_captured(self):
        spy = OstreeExecutorSpy()
        section = run_rpm(
            Path("/fake-root"),
            spy,
            system_type=SystemType.RPM_OSTREE,
        )
        assert len(section.ostree_overrides) == 1
        override = section.ostree_overrides[0]
        assert override.name == "kernel"
        assert override.to_nevra == "kernel-6.8.1-100.fc41.x86_64"
        assert override.from_nevra == "kernel-6.7.9-200.fc41.x86_64"


class TestRpmOstreeStatusFailure:
    """Graceful handling when rpm-ostree status fails."""

    def test_rpmostree_status_failure_handled(self):
        spy = OstreeExecutorSpy(rpmostree_rc=1)
        # Should not crash
        section = run_rpm(
            Path("/fake-root"),
            spy,
            system_type=SystemType.RPM_OSTREE,
        )
        assert section.ostree_overrides == []
        assert section.ostree_removals == []

    def test_rpmostree_status_invalid_json_handled(self):
        spy = OstreeExecutorSpy(rpmostree_stdout="this is not json {{{")
        # Should not crash
        section = run_rpm(
            Path("/fake-root"),
            spy,
            system_type=SystemType.RPM_OSTREE,
        )
        assert section.ostree_overrides == []
        assert section.ostree_removals == []
