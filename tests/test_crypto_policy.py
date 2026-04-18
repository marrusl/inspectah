"""Tests for system crypto policy detection and Containerfile rendering."""

import tempfile
from pathlib import Path

from jinja2 import Environment

from inspectah.inspectors.config import _detect_crypto_policy, run as run_config
from inspectah.renderers.containerfile import render as render_containerfile
from inspectah.renderers.containerfile.config import _crypto_policy_lines
from inspectah.schema import (
    ConfigCategory,
    ConfigFileEntry,
    ConfigFileKind,
    ConfigSection,
    InspectionSnapshot,
    OsRelease,
)


# ---------------------------------------------------------------------------
# Inspector: _detect_crypto_policy
# ---------------------------------------------------------------------------


class TestDetectCryptoPolicy:

    def test_legacy_policy_warns(self, tmp_path):
        host = tmp_path / "host"
        (host / "etc/crypto-policies").mkdir(parents=True)
        (host / "etc/crypto-policies/config").write_text("LEGACY\n")
        warnings: list = []
        _detect_crypto_policy(host, warnings)
        assert len(warnings) == 1
        assert "LEGACY" in warnings[0]["message"]
        assert "base image may use DEFAULT" in warnings[0]["message"]

    def test_fips_policy_warns(self, tmp_path):
        host = tmp_path / "host"
        (host / "etc/crypto-policies").mkdir(parents=True)
        (host / "etc/crypto-policies/config").write_text("FIPS\n")
        warnings: list = []
        _detect_crypto_policy(host, warnings)
        assert len(warnings) == 1
        assert "FIPS" in warnings[0]["message"]

    def test_default_policy_no_warning(self, tmp_path):
        host = tmp_path / "host"
        (host / "etc/crypto-policies").mkdir(parents=True)
        (host / "etc/crypto-policies/config").write_text("DEFAULT\n")
        warnings: list = []
        _detect_crypto_policy(host, warnings)
        assert warnings == []

    def test_missing_file_no_warning(self, tmp_path):
        host = tmp_path / "host"
        host.mkdir()
        warnings: list = []
        _detect_crypto_policy(host, warnings)
        assert warnings == []

    def test_empty_file_no_warning(self, tmp_path):
        host = tmp_path / "host"
        (host / "etc/crypto-policies").mkdir(parents=True)
        (host / "etc/crypto-policies/config").write_text("")
        warnings: list = []
        _detect_crypto_policy(host, warnings)
        assert warnings == []

    def test_inline_comment_stripped(self, tmp_path):
        host = tmp_path / "host"
        (host / "etc/crypto-policies").mkdir(parents=True)
        (host / "etc/crypto-policies/config").write_text("FUTURE  # set by admin\n")
        warnings: list = []
        _detect_crypto_policy(host, warnings)
        assert len(warnings) == 1
        assert "FUTURE" in warnings[0]["message"]

    def test_invalid_policy_warns_and_skips_rendering(self, tmp_path):
        host = tmp_path / "host"
        (host / "etc/crypto-policies").mkdir(parents=True)
        (host / "etc/crypto-policies/config").write_text("LEGACY;rm -rf /\n")
        warnings: list = []
        _detect_crypto_policy(host, warnings)
        assert len(warnings) == 1
        assert "unexpected characters" in warnings[0]["message"]

    def test_warnings_none_does_not_crash(self, tmp_path):
        """Passing warnings=None must not raise."""
        host = tmp_path / "host"
        (host / "etc/crypto-policies").mkdir(parents=True)
        (host / "etc/crypto-policies/config").write_text("LEGACY\n")
        _detect_crypto_policy(host, None)


class TestCryptoPolicyInRunFunction:
    """Verify _detect_crypto_policy is called during config.run()."""

    def test_run_emits_crypto_warning(self, tmp_path):
        host = tmp_path / "host"
        (host / "etc/crypto-policies").mkdir(parents=True)
        (host / "etc/crypto-policies/config").write_text("LEGACY\n")
        warnings: list = []
        run_config(host, executor=None, warnings=warnings)
        crypto_warnings = [w for w in warnings if "crypto policy" in w["message"].lower()]
        assert len(crypto_warnings) == 1

    def test_run_excludes_generated_crypto_policy_backends(self, tmp_path):
        host = tmp_path / "host"
        (host / "etc/crypto-policies/back-ends").mkdir(parents=True)
        (host / "etc/crypto-policies/back-ends/nss.config").write_text("generated\n")

        section = run_config(
            host,
            executor=None,
            rpm_owned_paths_override=set(),
            warnings=[],
        )

        assert all(
            entry.path != "/etc/crypto-policies/back-ends/nss.config"
            for entry in section.files
        )


# ---------------------------------------------------------------------------
# Containerfile renderer: _crypto_policy_lines
# ---------------------------------------------------------------------------


class TestCryptoPolicyRendering:

    @staticmethod
    def _snap_with_crypto(policy: str, custom_pols: list[str] | None = None) -> InspectionSnapshot:
        files = [
            ConfigFileEntry(
                path="/etc/crypto-policies/config",
                kind=ConfigFileKind.RPM_OWNED_MODIFIED,
                category=ConfigCategory.CRYPTO_POLICY,
                content=f"{policy}\n",
                include=True,
            ),
        ]
        for pol_path in (custom_pols or []):
            files.append(ConfigFileEntry(
                path=pol_path,
                kind=ConfigFileKind.UNOWNED,
                category=ConfigCategory.CRYPTO_POLICY,
                content="# custom policy\n",
                include=True,
            ))
        return InspectionSnapshot(
            config=ConfigSection(files=files),
        )

    def test_legacy_emits_update_command(self):
        snap = self._snap_with_crypto("LEGACY")
        lines = _crypto_policy_lines(snap)
        assert any("update-crypto-policies --set LEGACY" in l for l in lines)
        assert any("System crypto policy: LEGACY" in l for l in lines)

    def test_fips_emits_update_command(self):
        snap = self._snap_with_crypto("FIPS")
        lines = _crypto_policy_lines(snap)
        assert any("update-crypto-policies --set FIPS" in l for l in lines)

    def test_default_emits_nothing(self):
        snap = self._snap_with_crypto("DEFAULT")
        lines = _crypto_policy_lines(snap)
        assert lines == []

    def test_no_config_section_emits_nothing(self):
        snap = InspectionSnapshot()
        lines = _crypto_policy_lines(snap)
        assert lines == []

    def test_custom_pol_files_emit_copy(self):
        snap = self._snap_with_crypto(
            "LEGACY",
            custom_pols=["/etc/crypto-policies/policies/LEGACY-TLS1.pol"],
        )
        lines = _crypto_policy_lines(snap)
        assert any("RUN update-crypto-policies --set LEGACY" in l for l in lines)

    def test_excluded_file_not_rendered(self):
        """If the crypto-policies/config file has include=False, skip rendering."""
        snap = InspectionSnapshot(
            config=ConfigSection(files=[
                ConfigFileEntry(
                    path="/etc/crypto-policies/config",
                    kind=ConfigFileKind.RPM_OWNED_MODIFIED,
                    category=ConfigCategory.CRYPTO_POLICY,
                    content="LEGACY\n",
                    include=False,
                ),
            ]),
        )
        lines = _crypto_policy_lines(snap)
        assert lines == []

    def test_full_containerfile_includes_crypto_policy(self):
        """End-to-end: crypto policy appears in the rendered Containerfile."""
        snap = self._snap_with_crypto("LEGACY")
        snap.os_release = OsRelease(name="RHEL", version_id="9.6")
        with tempfile.TemporaryDirectory() as td:
            render_containerfile(snap, Environment(), Path(td))
            cf = (Path(td) / "Containerfile").read_text()
        assert "RUN update-crypto-policies --set LEGACY" in cf
        assert "COPY config/etc/ /etc/" in cf
        assert cf.index("COPY config/etc/ /etc/") < cf.index("RUN update-crypto-policies --set LEGACY")

    def test_full_containerfile_default_no_crypto_line(self):
        """Default policy must not produce any crypto-policies RUN."""
        snap = self._snap_with_crypto("DEFAULT")
        snap.os_release = OsRelease(name="RHEL", version_id="9.6")
        with tempfile.TemporaryDirectory() as td:
            render_containerfile(snap, Environment(), Path(td))
            cf = (Path(td) / "Containerfile").read_text()
        assert "update-crypto-policies" not in cf

    def test_invalid_policy_is_skipped_with_warning_comment(self):
        snap = self._snap_with_crypto("LEGACY;rm -rf /")
        lines = _crypto_policy_lines(snap)
        assert not any("RUN update-crypto-policies --set" in l for l in lines)
        assert any("WARNING" in l and "skipped" in l for l in lines)

    def test_subpolicy_name_is_allowed(self):
        snap = self._snap_with_crypto("DEFAULT:SHA1")
        lines = _crypto_policy_lines(snap)
        assert any("RUN update-crypto-policies --set DEFAULT:SHA1" in l for l in lines)
