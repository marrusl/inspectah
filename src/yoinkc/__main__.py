"""
CLI entry point. Parses args and delegates to pipeline.
"""

import os
import sys
import traceback
from pathlib import Path
from typing import Optional

from .cli import parse_args
from .pipeline import run_pipeline
from .schema import InspectionSnapshot


def _run_inspectors(host_root: Path, args) -> InspectionSnapshot:
    """Run all inspectors and merge into one snapshot."""
    from .inspectors import run_all

    return run_all(
        host_root,
        config_diffs=args.config_diffs,
        deep_binary_scan=args.deep_binary_scan,
        query_podman=args.query_podman,
        baseline_packages_file=args.baseline_packages,
        target_version=args.target_version,
        target_image=args.target_image,
        user_strategy=args.user_strategy,
        no_baseline_opt_in=args.no_baseline,
    )


def _run_renderers(snapshot: InspectionSnapshot, output_dir: Path) -> None:
    """Run all renderers."""
    from .renderers import run_all

    run_all(snapshot, output_dir)


def main(argv: Optional[list] = None) -> int:
    args = parse_args(argv)

    # Preflight: bail out early if container privileges are missing.
    if (
        args.from_snapshot is None
        and str(args.host_root) != "/"
        and not args.skip_preflight
    ):
        from .preflight import check_container_privileges
        errors = check_container_privileges()
        if errors:
            print("ERROR: container privilege checks failed:\n", file=sys.stderr)
            for err in errors:
                print(f"  \u2022 {err}", file=sys.stderr)
            print(
                "\nRun with the required flags, e.g.:\n"
                "  sudo podman run --rm --pid=host --privileged "
                "--security-opt label=disable \\\n"
                "    -v /:/host:ro yoinkc\n"
                "\nOr use --skip-preflight to bypass these checks.",
                file=sys.stderr,
            )
            return 1

    try:
        def run_inspectors(host_root: Path):
            return _run_inspectors(host_root, args)

        snapshot = run_pipeline(
            host_root=args.host_root,
            run_inspectors=run_inspectors,
            run_renderers=_run_renderers,
            from_snapshot_path=args.from_snapshot,
            inspect_only=args.inspect_only,
            output_file=args.output_file,
            output_dir=args.output_dir,
            no_entitlement=args.no_entitlement,
        )
        # --validate and --push-to-github require --output-dir (enforced by CLI)
        if args.output_dir and not args.inspect_only:
            if args.validate:
                from .validate import run_validate
                run_validate(args.output_dir)
            if args.push_to_github:
                from .git_github import init_git_repo, add_and_commit, push_to_github, output_stats
                if not init_git_repo(args.output_dir):
                    print(
                        "Error: failed to initialise git repository in output directory. "
                        "GitPython may not be installed \u2014 try: pip install 'yoinkc[github]'",
                        file=sys.stderr,
                    )
                    return 1
                if not add_and_commit(args.output_dir):
                    print(
                        "Error: failed to commit output files to git repository.",
                        file=sys.stderr,
                    )
                    return 1
                size, file_count, fixme_count = output_stats(args.output_dir)
                err = push_to_github(
                    args.output_dir,
                    args.push_to_github,
                    create_private=not args.public,
                    skip_confirmation=args.yes,
                    total_size_bytes=size,
                    file_count=file_count,
                    fixme_count=fixme_count,
                    redaction_count=len(snapshot.redactions),
                    github_token=args.github_token,
                )
                if err:
                    print(f"GitHub push failed: {err}", file=sys.stderr)
                    return 1
        return 0
    except Exception as e:
        print(f"Error: {e}", file=sys.stderr)
        if os.environ.get("YOINKC_DEBUG"):
            traceback.print_exc()
        else:
            print("Set YOINKC_DEBUG=1 for the full traceback.", file=sys.stderr)
        return 1
