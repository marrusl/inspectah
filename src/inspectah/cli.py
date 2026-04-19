"""
CLI argument parsing with subcommand-based structure.

Subcommands: scan (default), fleet, refine.
Backwards-compatible: bare flags without a subcommand are treated as scan.
"""

import argparse
from pathlib import Path
from typing import Optional

from .fleet.cli import add_fleet_args

SUBCOMMANDS = ("scan", "fleet", "refine", "architect")


def _preprocess_argv(argv: list[str]) -> list[str]:
    """Prepend 'scan' if the first arg looks like a flag, not a subcommand.

    This preserves backwards compatibility so that `inspectah --from-snapshot f`
    behaves the same as `inspectah scan --from-snapshot f`.
    """
    if not argv or (argv[0].startswith("-") and argv[0] not in ("-h", "--help")):
        return ["scan"] + argv
    return argv


def _add_scan_args(parser: argparse.ArgumentParser) -> None:
    """Register all scan-specific flags on the given parser."""
    parser.add_argument(
        "--host-root",
        type=Path,
        default=Path("/host"),
        help="Root path for host inspection (default: /host)",
    )
    output_group = parser.add_mutually_exclusive_group()
    output_group.add_argument(
        "-o",
        dest="output_file",
        type=Path,
        metavar="FILE",
        help="Write tarball to FILE (default: HOSTNAME-TIMESTAMP.tar.gz in cwd)",
    )
    output_group.add_argument(
        "--output-dir",
        dest="output_dir",
        type=Path,
        metavar="DIR",
        help="Write files to a directory instead of producing a tarball",
    )
    parser.add_argument(
        "--no-subscription",
        action="store_true",
        help="Skip bundling RHEL subscription certs into the output",
    )
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
    parser.add_argument(
        "--target-version",
        type=str,
        metavar="VERSION",
        help="Target bootc image version (e.g. 9.6, 10.2). "
             "Default: source host version, clamped to minimum bootc-supported release.",
    )
    parser.add_argument(
        "--target-image",
        type=str,
        metavar="IMAGE",
        help="Full target bootc base image reference "
             "(e.g. registry.redhat.io/rhel10/rhel-bootc:10.2). "
             "Overrides --target-version and all automatic mapping.",
    )
    parser.add_argument(
        "--baseline-packages",
        type=Path,
        metavar="FILE",
        help="Path to a newline-separated list of package names for air-gapped "
             "environments where the base image cannot be queried via podman.",
    )
    parser.add_argument(
        "--no-baseline",
        action="store_true",
        help="Run without base image comparison — all installed packages will be "
             "included in the Containerfile. Use when the base image cannot be "
             "queried and --baseline-packages is unavailable.",
    )
    _VALID_STRATEGIES = ("sysusers", "blueprint", "useradd", "kickstart")
    parser.add_argument(
        "--user-strategy",
        type=str,
        metavar="STRATEGY",
        choices=_VALID_STRATEGIES,
        help="Override user creation strategy for all users. "
             f"Valid: {', '.join(_VALID_STRATEGIES)}. "
             "Default: auto-assigned per classification (service→sysusers, human→kickstart, ambiguous→useradd).",
    )
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
    parser.add_argument(
        "--refine-mode",
        action="store_true",
        help="Enable editor UI in rendered report (set by inspectah refine)",
    )
    parser.add_argument(
        "--original-snapshot",
        type=Path,
        metavar="PATH",
        help="Path to unmodified original snapshot for editor diff/reset support "
             "(set by inspectah refine during re-render)",
    )
    parser.add_argument(
        "--sensitivity",
        type=str,
        choices=("strict", "moderate"),
        default="strict",
        help="Heuristic detection sensitivity: strict (default) redacts high-confidence "
             "heuristic findings, moderate flags all heuristic findings without redacting.",
    )
    parser.add_argument(
        "--no-redaction",
        action="store_true",
        help="Disable all redaction — detection still runs but no content is modified. "
             "WARNING: output may contain secrets.",
    )
    parser.add_argument(
        "--skip-unavailable",
        action="store_true",
        help="Skip the package availability preflight check. All packages "
             "included in the Containerfile without validation.",
    )


def _add_refine_args(parser: argparse.ArgumentParser) -> None:
    """Register refine-specific flags on the given parser."""
    parser.add_argument(
        "tarball",
        type=Path,
        help="Path to a inspectah output tarball (.tar.gz)",
    )
    parser.add_argument(
        "--no-browser",
        action="store_true",
        help="Do not auto-open the browser on startup",
    )
    parser.add_argument(
        "--port",
        type=int,
        default=8642,
        help="HTTP server port (default: 8642)",
    )
    parser.add_argument(
        "--bind",
        default="127.0.0.1",
        help=argparse.SUPPRESS,
    )


def build_parser() -> argparse.ArgumentParser:
    """Build and return the top-level inspectah argument parser."""
    parser = argparse.ArgumentParser(
        prog="inspectah",
        description="Inspect RHEL/CentOS hosts and produce bootc image artifacts.",
    )
    subparsers = parser.add_subparsers(dest="command")

    scan_parser = subparsers.add_parser(
        "scan",
        help="Scan a host and generate migration artifacts (default)",
    )
    _add_scan_args(scan_parser)

    fleet_parser = subparsers.add_parser(
        "fleet",
        help="Aggregate multiple inspection snapshots into a fleet report",
    )
    add_fleet_args(fleet_parser)

    refine_parser = subparsers.add_parser(
        "refine",
        help="Interactively edit and re-render inspection output",
    )
    _add_refine_args(refine_parser)

    from .architect.cli import add_architect_args
    architect_parser = subparsers.add_parser(
        "architect",
        help="Plan layer decomposition from refined fleets",
    )
    add_architect_args(architect_parser)

    return parser


def parse_args(argv: Optional[list[str]] = None) -> argparse.Namespace:
    if argv is None:
        import sys
        argv = sys.argv[1:]

    original_argv = list(argv)

    host_root_explicit = any(
        arg == "--host-root" or arg.startswith("--host-root=")
        for arg in argv
    )
    argv = _preprocess_argv(argv)

    parser = build_parser()

    args = parser.parse_args(argv)
    args.host_root_explicit = host_root_explicit

    if args.command == "fleet":
        if not (1 <= args.min_prevalence <= 100):
            parser.error("--min-prevalence must be between 1 and 100")

    if args.command == "scan":
        if args.from_snapshot and args.inspect_only:
            parser.error("--from-snapshot and --inspect-only cannot be used together")

        if args.no_baseline and args.baseline_packages:
            parser.error("--no-baseline and --baseline-packages cannot be used together")

        if (args.validate or args.push_to_github) and args.output_dir is None:
            parser.error(
                "--validate and --push-to-github require --output-dir "
                "(directory output mode)"
            )

        if getattr(args, "no_redaction", False):
            sensitivity_explicit = any(
                arg == "--sensitivity" or arg.startswith("--sensitivity=")
                for arg in original_argv
            )
            if sensitivity_explicit:
                parser.error("--sensitivity has no effect when --no-redaction is set")

    return args
