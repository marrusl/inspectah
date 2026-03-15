"""Detect and bundle RHEL subscription certs into the output directory."""

import shutil
from pathlib import Path


def bundle_subscription_certs(host_root: Path, output_dir: Path) -> None:
    """Copy subscription certs and rhsm config from host_root into output_dir.

    Silently skips if host_root does not exist or certs are not found.
    Red Hat stores subscription certs at /etc/pki/entitlement/ — that path
    is unchanged; only the yoinkc user-facing terminology is "subscriptions".
    """
    if not host_root.is_dir():
        return

    # Copy .pem files from /etc/pki/entitlement/ (Red Hat's path, unchanged)
    ent_src = host_root / "etc" / "pki" / "entitlement"
    if ent_src.is_dir():
        pems = list(ent_src.glob("*.pem"))
        if pems:
            ent_dst = output_dir / "entitlement"
            ent_dst.mkdir(exist_ok=True)
            for pem in pems:
                shutil.copy2(pem, ent_dst / pem.name)

    # Copy /etc/rhsm/ tree
    rhsm_src = host_root / "etc" / "rhsm"
    if rhsm_src.is_dir():
        shutil.copytree(rhsm_src, output_dir / "rhsm", dirs_exist_ok=True)
