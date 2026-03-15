"""
Pipeline orchestrator: run inspectors (or load snapshot), redact, optionally
bundle entitlement certs, then produce a tarball or write to a directory.
"""

import json
import os
import shutil
import sys
import tempfile
from pathlib import Path
from typing import Callable, Optional

from .entitlement import bundle_entitlement_certs
from .packaging import create_tarball, get_output_stamp
from .redact import redact_snapshot
from .schema import InspectionSnapshot, SCHEMA_VERSION


def load_snapshot(path: Path) -> InspectionSnapshot:
    """Load and deserialize an inspection snapshot from JSON."""
    data = json.loads(path.read_text())
    file_version = data.get("schema_version", 1)
    if file_version != SCHEMA_VERSION:
        raise ValueError(
            f"Snapshot was created by a different version of yoinkc "
            f"(schema v{file_version}, expected v{SCHEMA_VERSION}). "
            f"Re-run the inspection to generate a new snapshot."
        )
    return InspectionSnapshot.model_validate(data)


def save_snapshot(snapshot: InspectionSnapshot, path: Path) -> None:
    """Serialize snapshot to JSON."""
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(snapshot.model_dump_json(indent=2))


def run_pipeline(
    *,
    host_root: Path,
    run_inspectors: Optional[Callable[[Path], InspectionSnapshot]],
    run_renderers: Callable[[InspectionSnapshot, Path], None],
    from_snapshot_path: Optional[Path] = None,
    inspect_only: bool = False,
    output_file: Optional[Path] = None,
    output_dir: Optional[Path] = None,
    no_entitlement: bool = False,
    cwd: Optional[Path] = None,
) -> InspectionSnapshot:
    """Run the yoinkc pipeline.

    Output modes (mutually exclusive):
    - output_file: write tarball to this path
    - output_dir: write files to this directory
    - neither: write tarball to CWD with auto-generated name

    inspect_only: save snapshot to CWD and exit early.
    cwd: override working directory for default output paths (testing).
    """
    working_dir = cwd or Path.cwd()

    # Load or build the snapshot
    if from_snapshot_path is not None:
        snapshot = load_snapshot(from_snapshot_path)
        snapshot = redact_snapshot(snapshot)
    else:
        assert run_inspectors is not None, "run_inspectors required when not loading from snapshot"
        snapshot = run_inspectors(host_root)
        snapshot = redact_snapshot(snapshot)

    # --inspect-only: save snapshot and return
    if inspect_only:
        save_snapshot(snapshot, working_dir / "inspection-snapshot.json")
        return snapshot

    # Render into a temp directory
    tmp_dir = Path(tempfile.mkdtemp(prefix="yoinkc-"))
    try:
        save_snapshot(snapshot, tmp_dir / "inspection-snapshot.json")
        run_renderers(snapshot, tmp_dir)

        # Bundle entitlement certs (skip in --from-snapshot mode where
        # host filesystem may not be mounted)
        if not no_entitlement and from_snapshot_path is None:
            bundle_entitlement_certs(host_root, tmp_dir)

        # Output: tarball or directory
        if output_dir is not None:
            output_dir.mkdir(parents=True, exist_ok=True)
            for item in tmp_dir.iterdir():
                dest = output_dir / item.name
                if item.is_dir():
                    shutil.copytree(item, dest, dirs_exist_ok=True)
                else:
                    shutil.copy2(item, dest)
        else:
            # Prefer the hostname captured by the inspectors over re-reading
            # /etc/hostname, which is empty on RHEL hosts using hostnamectl.
            meta_hostname = snapshot.meta.get("hostname") or None
            stamp = get_output_stamp(hostname=meta_hostname, host_root=host_root)
            if output_file is None:
                output_file = working_dir / f"{stamp}.tar.gz"
            create_tarball(tmp_dir, output_file, prefix=stamp)
            name = output_file.name
            scp_host = stamp.rsplit("-", 2)[0]
            host_cwd = os.environ.get("YOINKC_HOST_CWD")
            scp_path = f"{host_cwd}/{name}" if host_cwd else name
            print(f"\nOutput: {name}\n")
            print("Next steps:")
            print(f"  Copy to workstation:    scp {scp_host}:{scp_path} .")
            print(f"  Interactive refinement: ./yoinkc-refine {name}")
            print(f"  Build the image:        ./yoinkc-build {name} my-image:latest")
    except Exception:
        print(
            f"Error during output. Rendered files preserved at: {tmp_dir}",
            file=sys.stderr,
        )
        raise
    else:
        shutil.rmtree(tmp_dir)

    return snapshot
