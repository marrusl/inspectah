"""
CLI argument parsing. All flags from the design doc plus --from-snapshot and --inspect-only.
"""

import argparse
from pathlib import Path
from typing import Optional


def parse_args(argv: Optional[list[str]] = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        prog="yoinkc",
        description="Inspect RHEL/CentOS hosts and produce bootc image artifacts.",
    )
    parser.add_argument(
        "--host-root",
        type=Path,
        default=Path("/host"),
        help="Root path for host inspection (default: /host)",
    )
    parser.add_argument(
        "-o",
        "--output-dir",
        dest="output_dir",
        type=Path,
        default=Path("./output"),
        help="Output directory for all artifacts (default: ./output)",
    )

    # Snapshot load/save
    parser.add_argument(
        "--from-snapshot",
        type=Path,
        metavar="PATH",
        help="Skip inspection; load snapshot from PATH and run renderers only",
    )
    parser.add_argument(
        "--inspect-only",
        action="store_true",
        help="Run inspectors and save snapshot to output; do not run renderers",
    )

    # Baseline
    parser.add_argument(
        "--baseline-packages",
        type=Path,
        metavar="FILE",
        help="Path to a newline-separated list of package names for air-gapped "
             "environments where the base image cannot be queried via podman.",
    )
    # Opt-in deeper inspection (design doc)
    parser.add_argument(
        "--config-diffs",
        action="store_true",
        help="Generate line-by-line diffs for modified configs (rpm2cpio from cache/repos)",
    )
    parser.add_argument(
        "--deep-binary-scan",
        action="store_true",
        help="Full strings scan on unknown binaries for version detection (slow)",
    )
    parser.add_argument(
        "--query-podman",
        action="store_true",
        help="Connect to podman socket to enumerate running containers",
    )

    # Build validation
    parser.add_argument(
        "--skip-preflight",
        action="store_true",
        help="Skip container privilege checks (rootful, --pid=host, --privileged, SELinux)",
    )

    parser.add_argument(
        "--validate",
        action="store_true",
        help="After generating output, run podman build to verify Containerfile",
    )

    # GitHub push
    parser.add_argument(
        "--push-to-github",
        type=str,
        metavar="REPO",
        help="Push output to GitHub repository (e.g. owner/repo)",
    )
    parser.add_argument(
        "--github-token",
        type=str,
        metavar="TOKEN",
        default=None,
        help="GitHub personal access token for repo creation (falls back to GITHUB_TOKEN env var)",
    )
    parser.add_argument(
        "--public",
        action="store_true",
        help="When creating a new repo, make it public (default: private)",
    )
    parser.add_argument(
        "--yes",
        action="store_true",
        help="Skip interactive confirmation prompts",
    )

    return parser.parse_args(argv)
