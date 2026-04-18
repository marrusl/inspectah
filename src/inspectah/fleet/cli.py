"""Fleet-specific CLI flag definitions."""

import argparse
from pathlib import Path


def add_fleet_args(parser: argparse.ArgumentParser) -> None:
    """Register fleet-specific flags on the given subparser."""
    parser.add_argument(
        "input_dir",
        type=Path,
        metavar="INPUT_DIR",
        help="Directory containing yoinkc tarballs (.tar.gz) and/or JSON snapshot files",
    )
    parser.add_argument(
        "-p", "--min-prevalence",
        type=int,
        default=100,
        metavar="PCT",
        help="Include items present on >= PCT%% of hosts (default: 100)",
    )
    parser.add_argument(
        "-o", "--output-file",
        dest="output_file",
        type=Path,
        default=None,
        metavar="FILE",
        help="Output path for tarball (or JSON with --json-only; default: auto-named in CWD)",
    )
    parser.add_argument(
        "--output-dir",
        type=Path,
        default=None,
        metavar="DIR",
        help="Write rendered files to a directory instead of tarball",
    )
    parser.add_argument(
        "--json-only",
        action="store_true",
        help="Write merged JSON only, skip rendering",
    )
    parser.add_argument(
        "--no-hosts",
        action="store_true",
        help="Omit per-item host lists from fleet metadata",
    )
