"""CLI registration for yoinkc architect subcommand."""

from __future__ import annotations

import argparse
import logging
import os
import shutil
import sys
import tarfile
import tempfile
from pathlib import Path

logger = logging.getLogger(__name__)


def add_architect_args(parser: argparse.ArgumentParser) -> None:
    """Register architect-specific CLI arguments."""
    parser.add_argument(
        "input_dir",
        type=Path,
        metavar="INPUT",
        help="Directory or tarball containing refined fleet tarballs (.tar.gz)",
    )
    parser.add_argument(
        "--port",
        type=int,
        default=8643,
        help="Port for the architect web UI (default: 8643)",
    )
    parser.add_argument(
        "--no-browser",
        action="store_true",
        help="Don't open browser automatically",
    )
    parser.add_argument(
        "--bind",
        default="127.0.0.1",
        help="Address to bind (default: 127.0.0.1)",
    )


def run_architect(args: argparse.Namespace) -> int:
    """Entry point for the architect subcommand."""
    input_path = args.input_dir
    if not input_path.exists():
        print(f"Error: {input_path} does not exist", file=sys.stderr)
        return 1

    tmp_dir = None
    if input_path.is_file() and input_path.name.endswith(".tar.gz"):
        tmp_dir = Path(tempfile.mkdtemp(prefix="architect-bundle-"))
        try:
            with tarfile.open(input_path, "r:gz") as tar:
                if sys.version_info >= (3, 12):
                    tar.extractall(tmp_dir, filter="data")
                else:
                    # Validate members against path traversal before extracting
                    safe_members = []
                    resolved_tmp = tmp_dir.resolve()
                    for member in tar.getmembers():
                        # Reject absolute paths and parent-directory references
                        member_path = (tmp_dir / member.name).resolve()
                        if not member_path.is_relative_to(resolved_tmp):
                            raise tarfile.TarError(
                                f"Path traversal detected in tarball member: {member.name}"
                            )
                        # Reject symlinks/hardlinks pointing outside extraction dir
                        if member.issym() or member.islnk():
                            link_target = Path(
                                os.path.normpath(
                                    os.path.join(
                                        str(tmp_dir / os.path.dirname(member.name)),
                                        member.linkname,
                                    )
                                )
                            ).resolve()
                            if not link_target.is_relative_to(resolved_tmp):
                                raise tarfile.TarError(
                                    f"Symlink/hardlink escape detected: {member.name} -> {member.linkname}"
                                )
                        safe_members.append(member)
                    tar.extractall(tmp_dir, members=safe_members)
        except tarfile.TarError as e:
            print(f"Error: failed to extract bundle {input_path}: {e}", file=sys.stderr)
            shutil.rmtree(tmp_dir, ignore_errors=True)
            return 1
        input_dir = tmp_dir
        logger.info("Extracted bundle to %s", tmp_dir)
    else:
        input_dir = input_path

    try:
        return _run_architect_inner(args, input_dir)
    finally:
        if tmp_dir is not None:
            shutil.rmtree(tmp_dir, ignore_errors=True)


def _run_architect_inner(args: argparse.Namespace, input_dir: Path) -> int:
    """Core architect logic after input resolution."""
    from yoinkc.architect.loader import load_refined_fleets
    from yoinkc.architect.analyzer import analyze_fleets
    from yoinkc.architect.server import start_server

    fleets = load_refined_fleets(input_dir)
    if not fleets:
        print(f"Error: no refined fleet tarballs found in {input_dir}", file=sys.stderr)
        return 1

    if len(fleets) < 2:
        print(
            f"Error: architect requires at least 2 fleets, found {len(fleets)}. "
            "Load multiple refined fleet tarballs to decompose into layers.",
            file=sys.stderr,
        )
        return 1

    print(f"Loaded {len(fleets)} fleets: {', '.join(f.name for f in fleets)}")

    topology = analyze_fleets(fleets)
    base = topology.get_layer("base")
    print(f"Proposed topology: {len(base.packages)} base packages, "
          f"{len(topology.layers) - 1} derived layers")

    # Load PatternFly CSS
    template_dir = Path(__file__).resolve().parent.parent / "templates"
    pf_path = template_dir / "patternfly.css"
    patternfly_css = pf_path.read_text() if pf_path.exists() else ""

    # Determine base image from first fleet's snapshot (if available)
    base_image = fleets[0].base_image or "registry.redhat.io/rhel9/rhel-bootc:9.4"

    port, httpd = start_server(
        topology,
        base_image=base_image,
        template_dir=template_dir,
        patternfly_css=patternfly_css,
        bind=args.bind,
        port=args.port,
        open_browser=not args.no_browser,
    )

    print(f"Serving architect UI at http://{args.bind}:{port}", flush=True)
    print("Press Ctrl+C to stop", flush=True)

    try:
        httpd.serve_forever()
    except KeyboardInterrupt:
        print("\nStopping architect server")
        httpd.shutdown()

    return 0
