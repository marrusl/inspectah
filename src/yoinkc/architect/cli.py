"""CLI registration for yoinkc architect subcommand."""

from __future__ import annotations

import argparse
import logging
import sys
from pathlib import Path

logger = logging.getLogger(__name__)


def add_architect_args(parser: argparse.ArgumentParser) -> None:
    """Register architect-specific CLI arguments."""
    parser.add_argument(
        "input_dir",
        type=Path,
        metavar="INPUT_DIR",
        help="Directory containing refined fleet tarballs (.tar.gz)",
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
    from yoinkc.architect.loader import load_refined_fleets
    from yoinkc.architect.analyzer import analyze_fleets
    from yoinkc.architect.server import start_server

    input_dir = args.input_dir
    if not input_dir.exists():
        print(f"Error: directory {input_dir} does not exist", file=sys.stderr)
        return 1

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
    base_image = "registry.redhat.io/rhel9/rhel-bootc:9.4"

    port, httpd = start_server(
        topology,
        base_image=base_image,
        template_dir=template_dir,
        patternfly_css=patternfly_css,
        bind=args.bind,
        port=args.port,
        open_browser=not args.no_browser,
    )

    print(f"Serving architect UI at http://{args.bind}:{port}")
    print("Press Ctrl+C to stop")

    try:
        httpd.serve_forever()
    except KeyboardInterrupt:
        print("\nStopping architect server")
        httpd.shutdown()

    return 0
