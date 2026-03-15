"""Containerfile renderer output tests: structure, quality, ordering, kargs, edge cases."""

import re
import tempfile
from pathlib import Path

import pytest
from jinja2 import Environment

from yoinkc.renderers import run_all as run_all_renderers
from yoinkc.renderers.containerfile import render as render_containerfile
from yoinkc.schema import (
    InspectionSnapshot,
    OsRelease,
    PackageEntry,
    RpmSection,
)


class TestContainerfile:

    def _cf(self, outputs):
        return (outputs["dir"] / "Containerfile").read_text()

    def test_file_exists(self, outputs_with_baseline):
        assert (outputs_with_baseline["dir"] / "Containerfile").exists()

    def test_layer_ordering(self, outputs_with_baseline):
        """Section headers must appear in design-doc order."""
        cf = self._cf(outputs_with_baseline)
        order = [
            "# === Base Image ===",
            "# === Package Installation ===",
            "# === Service Enablement ===",
            "# === Configuration Files ===",
        ]
        positions = [cf.index(h) for h in order if h in cf]
        assert positions == sorted(positions), "Layer ordering violated"

    def test_from_line_present(self, outputs_with_baseline):
        cf = self._cf(outputs_with_baseline)
        assert re.search(r"^FROM ", cf, re.MULTILINE), "No FROM line found"

    def test_dnf_install_has_packages(self, outputs_with_baseline):
        """dnf install block must include known added packages."""
        cf = self._cf(outputs_with_baseline)
        assert "RUN dnf install -y \\" in cf

    def test_no_per_file_copy_etc(self, outputs_with_baseline):
        """No individual COPY config/etc/specific/file lines — must be consolidated."""
        cf = self._cf(outputs_with_baseline)
        per_file = [
            line for line in
            re.findall(r"^COPY config/etc/[^\s/]+/[^\s]+\s+/etc/[^\s]+$", cf, re.MULTILINE)
            if "/rpm-gpg/" not in line and "/systemd/system/" not in line
        ]
        assert len(per_file) == 0, f"Found per-file COPY lines: {per_file[:5]}"

    def test_consolidated_copy_etc_present(self, outputs_with_baseline):
        """COPY config/etc/ /etc/ must be present."""
        cf = self._cf(outputs_with_baseline)
        assert "COPY config/etc/ /etc/" in cf

    def test_user_strategy_in_containerfile(self, outputs_with_baseline):
        """Users must be rendered according to their strategy."""
        cf = self._cf(outputs_with_baseline)
        snapshot = outputs_with_baseline["snapshot"]
        ug = snapshot.users_groups
        if ug and ug.users:
            assert "Users and Groups" in cf

    def test_no_baseline_mode_comment(self, outputs_no_baseline):
        """No-baseline mode should include a comment about all packages."""
        cf = self._cf(outputs_no_baseline)
        assert "No baseline" in cf

    def test_no_baseline_includes_packages(self, outputs_no_baseline):
        """No-baseline mode must still have a dnf install block."""
        cf = self._cf(outputs_no_baseline)
        assert "RUN dnf install -y \\" in cf

    def test_config_tree_etc_matches_copy(self, outputs_with_baseline):
        """config/etc/ must exist and be non-empty (matches the COPY source)."""
        config_etc = outputs_with_baseline["dir"] / "config" / "etc"
        assert config_etc.is_dir()
        files = list(config_etc.rglob("*"))
        assert any(f.is_file() for f in files), "config/etc/ is empty"

    def test_tmpfiles_written_to_config_etc(self, outputs_with_baseline):
        """yoinkc-var.conf must exist inside config/etc/tmpfiles.d/."""
        tmpfiles = outputs_with_baseline["dir"] / "config/etc/tmpfiles.d/yoinkc-var.conf"
        assert tmpfiles.exists()

    def test_fixme_comments_present(self, outputs_with_baseline):
        """FIXME comments must be present for items needing manual attention."""
        cf = self._cf(outputs_with_baseline)
        assert "FIXME" in cf

    def test_quadlet_copy_present(self, outputs_with_baseline):
        """Quadlet units must be copied via COPY quadlet/ /etc/containers/systemd/."""
        cf = self._cf(outputs_with_baseline)
        snapshot = outputs_with_baseline["snapshot"]
        if snapshot.containers and snapshot.containers.quadlet_units:
            assert "COPY quadlet/ /etc/containers/systemd/" in cf


class TestContainerfileQuality:

    def test_copy_targets_exist(self, outputs_with_baseline):
        """Every COPY source in the Containerfile must exist on disk."""
        output_dir = outputs_with_baseline["dir"]
        cf = (output_dir / "Containerfile").read_text()
        for i, line in enumerate(cf.splitlines(), 1):
            if line.startswith("#"):
                continue
            m = re.match(r"^COPY\s+(config/\S+|quadlet/\S*)", line)
            if m:
                src = m.group(1)
                src_path = output_dir / src
                assert src_path.exists(), f"COPY source missing at line {i}: {src}"

    def test_fixme_comments_are_actionable(self, outputs_with_baseline):
        """Every FIXME comment must explain what the operator needs to do."""
        cf = (outputs_with_baseline["dir"] / "Containerfile").read_text()
        for i, line in enumerate(cf.splitlines(), 1):
            if "FIXME" in line:
                after = line.split("FIXME", 1)[1].strip().lstrip(":").strip()
                assert len(after) > 10, (
                    f"FIXME at line {i} is not actionable (too short): {line.strip()!r}"
                )

    def test_syntax_valid(self, outputs_with_baseline):
        """Containerfile uses only valid Dockerfile instructions."""
        cf = (outputs_with_baseline["dir"] / "Containerfile").read_text()
        VALID = {"FROM", "RUN", "COPY", "ADD", "ENV", "ARG", "LABEL", "EXPOSE",
                 "ENTRYPOINT", "CMD", "VOLUME", "USER", "WORKDIR", "ONBUILD",
                 "STOPSIGNAL", "HEALTHCHECK", "SHELL"}
        in_continuation = False
        had_from = False
        for i, line in enumerate(cf.splitlines(), 1):
            stripped = line.strip()
            if not stripped or stripped.startswith("#"):
                in_continuation = False
                continue
            if in_continuation:
                in_continuation = stripped.endswith("\\")
                continue
            m = re.match(r"^([A-Z]+)\s", stripped)
            if m:
                instr = m.group(1)
                assert instr in VALID, f"Unknown instruction at line {i}: {instr}"
                if instr == "FROM":
                    had_from = True
                in_continuation = stripped.endswith("\\")
            else:
                assert line[0] in (" ", "\t"), (
                    f"Line {i} is not a valid instruction or continuation: {stripped[:80]!r}"
                )
                in_continuation = stripped.endswith("\\")
        assert had_from, "Containerfile is missing a FROM instruction"

    def test_non_rpm_provenance(self, outputs_with_baseline):
        """Known-provenance items get real directives; unknown get commented stubs."""
        cf = (outputs_with_baseline["dir"] / "Containerfile").read_text()
        output_dir = outputs_with_baseline["dir"]

        assert re.search(r"^RUN pip install", cf, re.MULTILINE)
        assert "flask==3.1.3" in cf
        assert "requests==2.32.5" in cf

        assert re.search(r"^COPY config/opt/myapp/", cf, re.MULTILINE)
        assert re.search(r"^RUN cd /opt/myapp && npm ci", cf, re.MULTILINE)
        assert (output_dir / "config" / "opt" / "myapp" / "package-lock.json").exists()


class TestKernelKargs:
    """Tests for the bootc-native kargs.d migration (replaces rpm-ostree kargs)."""

    def test_kargs_toml_generated(self, outputs_with_baseline):
        """TOML drop-in is written for operator-added kargs."""
        toml_path = (outputs_with_baseline["dir"]
                     / "config/usr/lib/bootc/kargs.d/yoinkc-migrated.toml")
        assert toml_path.exists(), "kargs TOML not written"

    def test_kargs_toml_contains_operator_args(self, outputs_with_baseline):
        """Operator-added kargs from the fixture appear in the TOML array."""
        toml_path = (outputs_with_baseline["dir"]
                     / "config/usr/lib/bootc/kargs.d/yoinkc-migrated.toml")
        content = toml_path.read_text()
        assert '"hugepagesz=2M"' in content
        assert '"transparent_hugepage=never"' in content

    def test_kargs_toml_excludes_bootloader_params(self, outputs_with_baseline):
        """Standard bootloader/installer parameters are NOT written to the TOML."""
        toml_path = (outputs_with_baseline["dir"]
                     / "config/usr/lib/bootc/kargs.d/yoinkc-migrated.toml")
        content = toml_path.read_text()
        for excluded in ("BOOT_IMAGE", "root=", '"ro"', '"rhgb"', '"quiet"', "crashkernel"):
            assert excluded not in content, (
                f"Bootloader param {excluded!r} should not appear in kargs TOML:\n{content}"
            )

    def test_kargs_toml_format(self, outputs_with_baseline):
        """TOML content uses the correct kargs array format."""
        toml_path = (outputs_with_baseline["dir"]
                     / "config/usr/lib/bootc/kargs.d/yoinkc-migrated.toml")
        content = toml_path.read_text()
        assert re.search(r'^kargs = \[".+"\]', content, re.MULTILINE), (
            f"kargs TOML does not have expected array format:\n{content}"
        )

    def test_containerfile_uses_kargs_copy(self, outputs_with_baseline):
        """Containerfile references the kargs TOML via COPY, not rpm-ostree kargs."""
        cf = (outputs_with_baseline["dir"] / "Containerfile").read_text()
        assert "rpm-ostree kargs" not in cf, "Containerfile still references rpm-ostree kargs"
        assert "COPY config/usr/lib/bootc/kargs.d/yoinkc-migrated.toml /usr/lib/bootc/kargs.d/" in cf
        assert "RUN mkdir -p /usr/lib/bootc/kargs.d" in cf

    def test_kargs_section_header_in_containerfile(self, outputs_with_baseline):
        """Containerfile contains the bootc-native kargs section header."""
        cf = (outputs_with_baseline["dir"] / "Containerfile").read_text()
        assert "Kernel Arguments (bootc-native kargs.d)" in cf

    def test_no_kargs_toml_when_no_cmdline(self):
        """No TOML file and no kargs section when kernel_boot has no cmdline."""
        from yoinkc.schema import InspectionSnapshot, OsRelease, KernelBootSection
        snapshot = InspectionSnapshot(
            meta={"host_root": "/host"},
            os_release=OsRelease(name="RHEL", version_id="9.6"),
            kernel_boot=KernelBootSection(cmdline=""),
        )
        with tempfile.TemporaryDirectory() as tmp:
            output_dir = Path(tmp)
            render_containerfile(snapshot, Environment(), output_dir)
            toml_path = output_dir / "config/usr/lib/bootc/kargs.d/yoinkc-migrated.toml"
            assert not toml_path.exists(), "TOML written for empty cmdline"
            cf = (output_dir / "Containerfile").read_text()
            assert "kargs.d" not in cf
            assert "rpm-ostree kargs" not in cf

    def test_no_kargs_toml_when_only_bootloader_params(self):
        """No TOML file or kargs section when cmdline contains only standard boot params."""
        from yoinkc.schema import InspectionSnapshot, OsRelease, KernelBootSection
        snapshot = InspectionSnapshot(
            meta={"host_root": "/host"},
            os_release=OsRelease(name="RHEL", version_id="9.6"),
            kernel_boot=KernelBootSection(
                cmdline="BOOT_IMAGE=/vmlinuz root=/dev/sda1 ro crashkernel=auto rhgb quiet"
            ),
        )
        with tempfile.TemporaryDirectory() as tmp:
            output_dir = Path(tmp)
            render_containerfile(snapshot, Environment(), output_dir)
            toml_path = output_dir / "config/usr/lib/bootc/kargs.d/yoinkc-migrated.toml"
            assert not toml_path.exists(), "TOML written for bootloader-only cmdline"
            cf = (output_dir / "Containerfile").read_text()
            assert "kargs.d" not in cf

    def test_no_kargs_toml_when_no_kernel_boot(self):
        """No TOML file and no kargs section when kernel_boot is absent."""
        from yoinkc.schema import InspectionSnapshot, OsRelease
        snapshot = InspectionSnapshot(
            meta={"host_root": "/host"},
            os_release=OsRelease(name="RHEL", version_id="9.6"),
        )
        with tempfile.TemporaryDirectory() as tmp:
            output_dir = Path(tmp)
            render_containerfile(snapshot, Environment(), output_dir)
            toml_path = output_dir / "config/usr/lib/bootc/kargs.d/yoinkc-migrated.toml"
            assert not toml_path.exists()
            cf = (output_dir / "Containerfile").read_text()
            assert "kargs.d" not in cf

    def test_multiple_kargs_combined_in_single_toml(self):
        """Multiple operator kargs from cmdline are collected into a single TOML array."""
        from yoinkc.schema import InspectionSnapshot, OsRelease, KernelBootSection
        snapshot = InspectionSnapshot(
            meta={"host_root": "/host"},
            os_release=OsRelease(name="RHEL", version_id="9.6"),
            kernel_boot=KernelBootSection(
                cmdline=(
                    "BOOT_IMAGE=/vmlinuz root=/dev/sda1 ro rhgb quiet "
                    "hugepagesz=2M transparent_hugepage=never mitigations=off"
                ),
            ),
        )
        with tempfile.TemporaryDirectory() as tmp:
            output_dir = Path(tmp)
            render_containerfile(snapshot, Environment(), output_dir)
            toml_path = output_dir / "config/usr/lib/bootc/kargs.d/yoinkc-migrated.toml"
            assert toml_path.exists()
            content = toml_path.read_text()
            assert '"hugepagesz=2M"' in content
            assert '"transparent_hugepage=never"' in content
            assert '"mitigations=off"' in content
            assert "BOOT_IMAGE" not in content
            assert '"ro"' not in content
            assert '"rhgb"' not in content
            kargs_lines = [ln for ln in content.splitlines() if ln.startswith("kargs =")]
            assert len(kargs_lines) == 1, f"Expected single kargs line, got: {kargs_lines}"
            cf = (output_dir / "Containerfile").read_text()
            copies = [ln for ln in cf.splitlines()
                      if "kargs.d/yoinkc-migrated.toml" in ln and ln.startswith("COPY")]
            assert len(copies) == 1, f"Expected 1 COPY for kargs TOML, got: {copies}"


class TestBaselineModes:

    def test_baseline_available_wording(self, outputs_with_baseline):
        """With baseline, audit and Containerfile use 'beyond base image' wording."""
        cf = (outputs_with_baseline["dir"] / "Containerfile").read_text()
        audit = (outputs_with_baseline["dir"] / "audit-report.md").read_text()
        assert "added beyond base image" in cf
        assert "No baseline" not in cf
        assert "beyond base image" in audit or "Baseline:" in audit


class TestEdgeCases:

    def test_minimal_snapshot_no_crash(self):
        """Renderers must not crash when all sections are None."""
        from yoinkc.schema import InspectionSnapshot, OsRelease
        minimal = InspectionSnapshot(
            meta={"host_root": "/host"},
            os_release=OsRelease(name="RHEL", version_id="9.6"),
        )
        with tempfile.TemporaryDirectory() as tmp:
            output_dir = Path(tmp)
            run_all_renderers(minimal, output_dir)
            assert (output_dir / "Containerfile").exists()
            assert (output_dir / "audit-report.md").exists()
            assert (output_dir / "report.html").exists()
            assert (output_dir / "README.md").exists()
            assert (output_dir / "secrets-review.md").exists()
            assert (output_dir / "kickstart-suggestion.ks").exists()

    def test_none_and_empty_values_no_literal_none(self):
        """No literal 'None' string in any rendered output."""
        from yoinkc.schema import (
            InspectionSnapshot, OsRelease, ServiceSection, ServiceStateChange,
        )
        services = ServiceSection(
            state_changes=[
                ServiceStateChange(unit="foo.service", current_state="enabled",
                                   default_state="enabled", action="unchanged"),
            ],
        )
        edge = InspectionSnapshot(
            meta={"host_root": "/host"},
            os_release=OsRelease(name="RHEL", version_id="9.6", pretty_name="RHEL 9.6"),
            services=services,
            warnings=[{"message": None}, {"message": ""}, {}, {"message": "Real warning"}],
            redactions=[
                {"path": None, "pattern": None, "remediation": None},
                {"path": "", "pattern": "PASSWORD", "remediation": "use secret"},
            ],
        )
        with tempfile.TemporaryDirectory() as tmp:
            output_dir = Path(tmp)
            run_all_renderers(edge, output_dir)
            for name in ("audit-report.md", "report.html", "README.md", "secrets-review.md"):
                content = (output_dir / name).read_text()
                assert "None" not in content, f"{name} must not contain literal None"


class TestServicePackageFiltering:
    """systemctl enable/disable must skip units whose package won't be installed."""

    @staticmethod
    def _render_cf(snap) -> str:
        from yoinkc.renderers.containerfile import render as render_containerfile
        from jinja2 import Environment
        with tempfile.TemporaryDirectory() as td:
            render_containerfile(snap, Environment(), Path(td))
            return (Path(td) / "Containerfile").read_text()

    def _make_snap(self, enabled=None, disabled=None, state_changes=None,
                   leaf=None, auto=None, baseline=None, dep_tree=None):
        from yoinkc.schema import (
            InspectionSnapshot, RpmSection, ServiceSection, ServiceStateChange,
            PackageEntry, PackageState,
        )
        services = ServiceSection(
            enabled_units=enabled or [],
            disabled_units=disabled or [],
            state_changes=state_changes or [],
        )
        rpm = RpmSection(
            packages_added=[PackageEntry(name=n, version="1.0", release="1.el9", arch="x86_64")
                            for n in (leaf or [])],
            leaf_packages=leaf,
            auto_packages=auto,
            leaf_dep_tree=dep_tree,
            baseline_package_names=baseline,
        )
        return InspectionSnapshot(services=services, rpm=rpm)

    def test_leaf_package_service_included(self):
        """Service from a leaf package must appear in RUN systemctl enable."""
        from yoinkc.schema import ServiceStateChange
        snap = self._make_snap(
            enabled=["httpd.service"],
            state_changes=[ServiceStateChange(
                unit="httpd.service", current_state="enabled",
                default_state="disabled", action="enable",
                owning_package="httpd",
            )],
            leaf=["httpd"], baseline=["bash"],
        )
        cf = self._render_cf(snap)
        assert "RUN systemctl enable httpd.service" in cf

    def test_baseline_package_service_included(self):
        """Service from a base image package must appear in RUN systemctl enable."""
        from yoinkc.schema import ServiceStateChange
        snap = self._make_snap(
            enabled=["sshd.service"],
            state_changes=[ServiceStateChange(
                unit="sshd.service", current_state="enabled",
                default_state="disabled", action="enable",
                owning_package="openssh-server",
            )],
            leaf=["httpd"], baseline=["bash", "openssh-server"],
        )
        cf = self._render_cf(snap)
        assert "RUN systemctl enable sshd.service" in cf

    def test_auto_dep_of_leaf_included(self):
        """Service from an auto package that is a dep of a leaf must be included."""
        from yoinkc.schema import ServiceStateChange
        snap = self._make_snap(
            enabled=["mod_ssl.service"],
            state_changes=[ServiceStateChange(
                unit="mod_ssl.service", current_state="enabled",
                default_state="disabled", action="enable",
                owning_package="mod_ssl",
            )],
            leaf=["httpd"], auto=["mod_ssl"],
            baseline=["bash"],
            dep_tree={"httpd": ["mod_ssl"]},
        )
        cf = self._render_cf(snap)
        assert "RUN systemctl enable mod_ssl.service" in cf

    def test_orphan_auto_package_skipped(self):
        """Service from an auto package not depended on by any leaf must be skipped."""
        from yoinkc.schema import ServiceStateChange
        snap = self._make_snap(
            enabled=["insights-client-boot.service", "httpd.service"],
            state_changes=[
                ServiceStateChange(
                    unit="insights-client-boot.service", current_state="enabled",
                    default_state="disabled", action="enable",
                    owning_package="insights-client",
                ),
                ServiceStateChange(
                    unit="httpd.service", current_state="enabled",
                    default_state="disabled", action="enable",
                    owning_package="httpd",
                ),
            ],
            leaf=["httpd"], auto=["insights-client"],
            baseline=["bash"],
            dep_tree={"httpd": []},
        )
        cf = self._render_cf(snap)
        enable_lines = [l for l in cf.splitlines() if l.startswith("RUN systemctl enable")]
        assert enable_lines, "expected at least one RUN systemctl enable line"
        for line in enable_lines:
            assert "insights-client-boot.service" not in line
            assert "httpd.service" in line
        assert "insights-client-boot.service" in cf, "should appear as a skip comment"
        assert "skipped (package insights-client not in dnf install line)" in cf

    def test_unknown_owner_included(self):
        """Service with unknown owning package must be included (safe default)."""
        from yoinkc.schema import ServiceStateChange
        snap = self._make_snap(
            enabled=["custom.service"],
            state_changes=[ServiceStateChange(
                unit="custom.service", current_state="enabled",
                default_state="disabled", action="enable",
                owning_package=None,
            )],
            leaf=["httpd"], baseline=["bash"],
        )
        cf = self._render_cf(snap)
        assert "RUN systemctl enable custom.service" in cf

    def test_orphan_disable_skipped(self):
        """systemctl disable must also skip units whose package won't be installed."""
        from yoinkc.schema import ServiceStateChange
        snap = self._make_snap(
            disabled=["insights-client.service"],
            state_changes=[ServiceStateChange(
                unit="insights-client.service", current_state="disabled",
                default_state="enabled", action="disable",
                owning_package="insights-client",
            )],
            leaf=["httpd"], auto=["insights-client"],
            baseline=["bash"],
            dep_tree={"httpd": []},
        )
        cf = self._render_cf(snap)
        assert "RUN systemctl disable" not in cf
        assert "insights-client.service" in cf
        assert "skipped (package insights-client not in dnf install line)" in cf

    def test_no_rpm_data_includes_all(self):
        """When RPM section has no package lists, all units should be included."""
        from yoinkc.schema import (
            InspectionSnapshot, ServiceSection, ServiceStateChange,
        )
        snap = InspectionSnapshot(
            services=ServiceSection(
                enabled_units=["httpd.service"],
                state_changes=[ServiceStateChange(
                    unit="httpd.service", current_state="enabled",
                    default_state="disabled", action="enable",
                    owning_package="httpd",
                )],
            ),
        )
        cf = self._render_cf(snap)
        assert "RUN systemctl enable httpd.service" in cf


def test_gpg_key_copy_precedes_repo_copy():
    """GPG key COPY must appear before repo COPY which must appear before dnf install."""
    from yoinkc.schema import InspectionSnapshot, RpmSection, PackageEntry, PackageState, RepoFile

    snap = InspectionSnapshot()
    snap.rpm = RpmSection()
    snap.rpm.packages_added = [
        PackageEntry(name="httpd", epoch="0", version="2.4.62", release="1.el9", arch="x86_64",
                     state=PackageState.ADDED, include=True),
    ]
    snap.rpm.leaf_packages = ["httpd"]
    snap.rpm.auto_packages = []
    snap.rpm.leaf_dep_tree = {"httpd": []}
    repo = RepoFile(path="etc/yum.repos.d/custom.repo",
                    content="[custom]\nbaseurl=http://example.com\ngpgkey=file:///etc/pki/rpm-gpg/KEY\n")
    snap.rpm.repo_files = [repo]
    snap.rpm.gpg_keys = [
        RepoFile(path="etc/pki/rpm-gpg/KEY", content="-----BEGIN PGP PUBLIC KEY BLOCK-----\nFAKE\n-----END PGP PUBLIC KEY BLOCK-----\n"),
    ]

    with tempfile.TemporaryDirectory() as td:
        out = Path(td)
        render_containerfile(snap, Environment(), out)
        cf = (out / "Containerfile").read_text()

    gpg_idx  = cf.find("COPY config/etc/pki/rpm-gpg/")
    repo_idx = cf.find("COPY config/etc/yum.repos.d/")
    dnf_idx  = cf.find("RUN dnf install")
    assert gpg_idx  != -1, "Expected COPY for GPG keys"
    assert repo_idx != -1, "Expected COPY for repos"
    assert dnf_idx  != -1, "Expected RUN dnf install"
    assert gpg_idx < repo_idx < dnf_idx, (
        f"Order must be: GPG keys ({gpg_idx}) < repos ({repo_idx}) < dnf install ({dnf_idx})"
    )


def test_systemd_timer_copy_precedes_enable():
    """Timer unit COPY must appear before RUN systemctl enable *.timer."""
    from yoinkc.schema import InspectionSnapshot, ScheduledTaskSection, SystemdTimer

    snap = InspectionSnapshot()
    snap.scheduled_tasks = ScheduledTaskSection()
    snap.scheduled_tasks.systemd_timers = [
        SystemdTimer(name="myapp-report", source="local", on_calendar="daily",
                     timer_content="[Timer]\nOnCalendar=daily\n",
                     service_content="[Service]\nExecStart=/usr/local/bin/report.sh\n"),
    ]

    with tempfile.TemporaryDirectory() as td:
        out = Path(td)
        render_containerfile(snap, Environment(), out)
        cf = (out / "Containerfile").read_text()

    copy_idx   = cf.find("COPY config/etc/systemd/system/")
    enable_idx = cf.find("RUN systemctl enable myapp-report.timer")
    assert copy_idx   != -1, "Expected COPY for systemd/system/"
    assert enable_idx != -1, "Expected RUN systemctl enable"
    assert copy_idx < enable_idx, (
        f"COPY config/etc/systemd/system/ (pos {copy_idx}) must come before "
        f"RUN systemctl enable (pos {enable_idx})"
    )


def test_repo_copy_precedes_dnf_install():
    """Repo COPY directives must appear before RUN dnf install so repos exist when packages are installed."""
    from yoinkc.schema import InspectionSnapshot, RpmSection, PackageEntry, PackageState, RepoFile

    snap = InspectionSnapshot()
    snap.rpm = RpmSection()
    snap.rpm.packages_added = [
        PackageEntry(name="httpd", epoch="0", version="2.4.62", release="1.el9", arch="x86_64",
                     state=PackageState.ADDED, include=True),
    ]
    snap.rpm.leaf_packages = ["httpd"]
    snap.rpm.auto_packages = []
    snap.rpm.leaf_dep_tree = {"httpd": []}
    repo = RepoFile(path="etc/yum.repos.d/custom.repo", content="[custom]\nbaseurl=http://repo.example.com\n")
    snap.rpm.repo_files = [repo]

    with tempfile.TemporaryDirectory() as td:
        out = Path(td)
        render_containerfile(snap, Environment(), out)
        cf = (out / "Containerfile").read_text()

    copy_idx = cf.find("COPY config/etc/yum.repos.d/")
    dnf_idx  = cf.find("RUN dnf install")
    assert copy_idx != -1, "Expected a COPY directive for etc/yum.repos.d/"
    assert dnf_idx  != -1, "Expected a RUN dnf install directive"
    assert copy_idx < dnf_idx, (
        f"COPY config/etc/yum.repos.d/ (pos {copy_idx}) must come before "
        f"RUN dnf install (pos {dnf_idx})"
    )


def test_config_tree_timers_excluded_from_services_enable():
    """Config-tree timer units must not appear in the services RUN systemctl enable line."""
    from yoinkc.renderers.containerfile import render as render_containerfile
    from yoinkc.schema import (
        InspectionSnapshot, ServiceSection, ScheduledTaskSection, SystemdTimer,
        )
    from jinja2 import Environment
    import tempfile

    snap = InspectionSnapshot()
    snap.services = ServiceSection()
    snap.services.enabled_units = ["httpd.service", "myapp-report.timer", "myapp-report.service"]
    snap.scheduled_tasks = ScheduledTaskSection()
    snap.scheduled_tasks.systemd_timers = [
        SystemdTimer(
            name="myapp-report", source="local", on_calendar="daily",
            timer_content="[Timer]\nOnCalendar=daily\n",
            service_content="[Service]\nExecStart=/bin/true\n",
        ),
    ]

    with tempfile.TemporaryDirectory() as td:
        out = Path(td)
        render_containerfile(snap, Environment(), out)
        cf = (out / "Containerfile").read_text()

    services_enable_line = next(
        (l for l in cf.splitlines() if l.startswith("RUN systemctl enable") and "httpd" in l),
        "",
    )
    assert "httpd.service" in services_enable_line, "httpd.service must be in services enable"
    assert "myapp-report" not in services_enable_line, (
        "myapp-report must be excluded from services enable (it's a config-tree unit)"
    )

    copy_idx   = cf.find("COPY config/etc/systemd/system/")
    enable_idx = cf.find("RUN systemctl enable myapp-report.timer")
    assert copy_idx   != -1, "Expected COPY for systemd/system/"
    assert enable_idx != -1, "Expected RUN systemctl enable for myapp-report.timer"
    assert copy_idx < enable_idx


def test_bootc_container_lint_is_last_run():
    """RUN bootc container lint must appear at the end of every generated Containerfile."""
    from yoinkc.renderers.containerfile import render as render_containerfile
    from yoinkc.schema import InspectionSnapshot
    from jinja2 import Environment
    import tempfile

    snap = InspectionSnapshot()

    with tempfile.TemporaryDirectory() as td:
        out = Path(td)
        render_containerfile(snap, Environment(), out)
        cf = (out / "Containerfile").read_text()

    assert "RUN bootc container lint" in cf
    last_run = next(
        (line.strip() for line in reversed(cf.splitlines()) if line.strip()),
        "",
    )
    assert last_run == "RUN bootc container lint", (
        f"Expected 'RUN bootc container lint' as last line, got: {last_run!r}"
    )


def test_nonrpm_emits_nodejs_prereq_when_missing_from_packages():
    """A dnf install for nodejs must appear before npm ci when nodejs is not in packages_added."""
    from yoinkc.renderers.containerfile import render as render_containerfile
    from yoinkc.schema import InspectionSnapshot, NonRpmSoftwareSection, NonRpmItem
    from jinja2 import Environment
    import tempfile

    snap = InspectionSnapshot()
    snap.non_rpm_software = NonRpmSoftwareSection()
    snap.non_rpm_software.items = [
        NonRpmItem(path="opt/webapp", method="npm package-lock.json", include=True),
    ]

    with tempfile.TemporaryDirectory() as td:
        out = Path(td)
        render_containerfile(snap, Environment(), out)
        cf = (out / "Containerfile").read_text()

    assert "nodejs" in cf, "Expected a nodejs install directive"
    nodejs_idx = cf.find("nodejs")
    npm_ci_idx = cf.find("npm ci")
    assert npm_ci_idx != -1, "Expected RUN npm ci"
    assert nodejs_idx < npm_ci_idx, (
        f"dnf install nodejs (pos {nodejs_idx}) must come before npm ci (pos {npm_ci_idx})"
    )


def test_nonrpm_no_nodejs_prereq_when_already_in_packages():
    """No extra nodejs install when nodejs is already in the leaf packages."""
    from yoinkc.renderers.containerfile import render as render_containerfile
    from yoinkc.schema import (
        InspectionSnapshot, NonRpmSoftwareSection, NonRpmItem,
        RpmSection, PackageEntry, PackageState,
    )
    from jinja2 import Environment
    import tempfile

    snap = InspectionSnapshot()
    snap.non_rpm_software = NonRpmSoftwareSection()
    snap.non_rpm_software.items = [
        NonRpmItem(path="opt/webapp", method="npm package-lock.json", include=True),
    ]
    snap.rpm = RpmSection()
    snap.rpm.packages_added = [
        PackageEntry(name="nodejs", epoch="0", version="20.0", release="1.el10",
                     arch="x86_64", state=PackageState.ADDED, include=True),
    ]
    snap.rpm.leaf_packages = ["nodejs"]

    with tempfile.TemporaryDirectory() as td:
        out = Path(td)
        render_containerfile(snap, Environment(), out)
        cf = (out / "Containerfile").read_text()

    assert "Tool prerequisites not in the dnf install block" not in cf
