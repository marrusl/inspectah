"""CLI for yoinkc-fleet."""

import argparse
from pathlib import Path
from typing import Optional


def parse_args(argv: Optional[list[str]] = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        prog="yoinkc-fleet",
        description="Fleet-level analysis of yoinkc inspection snapshots.",
    )
    sub = parser.add_subparsers(dest="command", required=True)

    agg = sub.add_parser("aggregate", help="Merge N snapshots into a fleet snapshot")
    agg.add_argument(
        "input_dir",
        type=Path,
        help="Directory containing yoinkc tarballs (.tar.gz) and/or JSON snapshot files",
    )
    agg.add_argument(
        "-p", "--min-prevalence",
        type=int,
        default=100,
        metavar="PCT",
        help="Include items present on >= PCT%% of hosts (default: 100)",
    )
    agg.add_argument(
        "-o", "--output",
        type=Path,
        default=None,
        metavar="FILE",
        help="Output path for tarball (or JSON with --json-only; default: auto-named in CWD)",
    )
    agg.add_argument(
        "--output-dir",
        type=Path,
        default=None,
        metavar="DIR",
        help="Write rendered files to a directory instead of tarball",
    )
    agg.add_argument(
        "--json-only",
        action="store_true",
        help="Write merged JSON only, skip rendering",
    )
    agg.add_argument(
        "--no-hosts",
        action="store_true",
        help="Omit per-item host lists from fleet metadata",
    )

    args = parser.parse_args(argv)

    if hasattr(args, "min_prevalence"):
        if not (1 <= args.min_prevalence <= 100):
            parser.error("--min-prevalence must be between 1 and 100")

    return args
