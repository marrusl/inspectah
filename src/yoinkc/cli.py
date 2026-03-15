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
    # Output mode: tarball (default) or directory
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

    # Target image
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

    # Baseline
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

    parser.add_argument(
        "--refine-mode",
        action="store_true",
        help="Enable editor UI in rendered report (set by yoinkc-refine)",
    )
    parser.add_argument(
        "--original-snapshot",
        type=Path,
        metavar="PATH",
        help="Path to unmodified original snapshot for editor diff/reset support "
             "(set by yoinkc-refine during re-render)",
    )

    args = parser.parse_args(argv)

    if args.from_snapshot and args.inspect_only:
        parser.error("--from-snapshot and --inspect-only cannot be used together")

    if args.no_baseline and args.baseline_packages:
        parser.error("--no-baseline and --baseline-packages cannot be used together")

    if (args.validate or args.push_to_github) and args.output_dir is None:
        parser.error(
            "--validate and --push-to-github require --output-dir "
            "(directory output mode)"
        )

    return args
