"""Tests for subscription cert detection and bundling."""

import tempfile
from pathlib import Path

import pytest

from inspectah.subscription import bundle_subscription_certs


def _make_host_root_with_certs(root: Path) -> None:
    """Create a fake host root with subscription certs and rhsm config."""
    ent_dir = root / "etc" / "pki" / "entitlement"
    ent_dir.mkdir(parents=True)
    (ent_dir / "123456.pem").write_text("cert-data")
    (ent_dir / "123456-key.pem").write_text("key-data")
    rhsm_dir = root / "etc" / "rhsm"
    rhsm_dir.mkdir(parents=True)
    (rhsm_dir / "rhsm.conf").write_text("[rhsm]\nbaseurl=https://cdn.redhat.com")


def test_bundles_certs_when_present():
    """Subscription certs and rhsm dir are copied to output."""
    with tempfile.TemporaryDirectory() as tmp:
        host_root = Path(tmp) / "host"
        output_dir = Path(tmp) / "output"
        output_dir.mkdir()
        _make_host_root_with_certs(host_root)

        bundle_subscription_certs(host_root, output_dir)

        assert (output_dir / "entitlement" / "123456.pem").read_text() == "cert-data"
        assert (output_dir / "entitlement" / "123456-key.pem").read_text() == "key-data"
        assert (output_dir / "rhsm" / "rhsm.conf").exists()


def test_skips_silently_when_no_certs():
    """No error or output when subscription certs do not exist."""
    with tempfile.TemporaryDirectory() as tmp:
        host_root = Path(tmp) / "host"
        host_root.mkdir()
        output_dir = Path(tmp) / "output"
        output_dir.mkdir()

        bundle_subscription_certs(host_root, output_dir)

        assert not (output_dir / "entitlement").exists()
        assert not (output_dir / "rhsm").exists()


def test_skips_silently_when_host_root_missing():
    """No error when HOST_ROOT does not exist at all."""
    with tempfile.TemporaryDirectory() as tmp:
        host_root = Path(tmp) / "nonexistent"
        output_dir = Path(tmp) / "output"
        output_dir.mkdir()

        bundle_subscription_certs(host_root, output_dir)

        assert not (output_dir / "entitlement").exists()


def test_copies_only_pem_files():
    """Only .pem files from the subscription dir are copied, not other files."""
    with tempfile.TemporaryDirectory() as tmp:
        host_root = Path(tmp) / "host"
        ent_dir = host_root / "etc" / "pki" / "entitlement"
        ent_dir.mkdir(parents=True)
        (ent_dir / "cert.pem").write_text("cert")
        (ent_dir / "README").write_text("ignore me")
        output_dir = Path(tmp) / "output"
        output_dir.mkdir()

        bundle_subscription_certs(host_root, output_dir)

        assert (output_dir / "entitlement" / "cert.pem").exists()
        assert not (output_dir / "entitlement" / "README").exists()


def test_bundles_rhsm_without_subscription_certs():
    """rhsm dir is bundled even if subscription certs are absent."""
    with tempfile.TemporaryDirectory() as tmp:
        host_root = Path(tmp) / "host"
        rhsm_dir = host_root / "etc" / "rhsm"
        rhsm_dir.mkdir(parents=True)
        (rhsm_dir / "rhsm.conf").write_text("[rhsm]")
        output_dir = Path(tmp) / "output"
        output_dir.mkdir()

        bundle_subscription_certs(host_root, output_dir)

        assert not (output_dir / "entitlement").exists()
        assert (output_dir / "rhsm" / "rhsm.conf").exists()
