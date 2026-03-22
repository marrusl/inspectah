"""
CLI entry point. Parses args and delegates to pipeline.
"""

import os
import sys
import traceback
from functools import partial
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


def _run_renderers(
    snapshot: InspectionSnapshot,
    output_dir: Path,
    refine_mode: bool = False,
    original_snapshot_path: Optional[Path] = None,
) -> None:
    """Run all renderers."""
    from .renderers import run_all

    run_all(
        snapshot, output_dir,
        refine_mode=refine_mode,
        original_snapshot_path=original_snapshot_path,
    )


def _run_inspect(args) -> int:
    """Run the inspect pipeline (default subcommand)."""
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

    def run_inspectors(host_root: Path):
        return _run_inspectors(host_root, args)

    renderers = partial(
        _run_renderers,
        refine_mode=args.refine_mode,
        original_snapshot_path=args.original_snapshot,
    )

    snapshot = run_pipeline(
        host_root=args.host_root,
        run_inspectors=run_inspectors,
        run_renderers=renderers,
        from_snapshot_path=args.from_snapshot,
        inspect_only=args.inspect_only,
        output_file=args.output_file,
        output_dir=args.output_dir,
        no_subscription=args.no_subscription,
    )
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


def _run_fleet(args, cwd: Optional[Path] = None) -> int:
    """Run the fleet aggregation pipeline."""
    import tempfile

    from .fleet.loader import discover_snapshots, validate_snapshots
    from .fleet.merge import merge_snapshots
    from .pipeline import run_pipeline

    input_dir: Path = args.input_dir
    if not input_dir.is_dir():
        print(f"Error: {input_dir} is not a directory.", file=sys.stderr)
        return 1

    snapshots = discover_snapshots(input_dir)
    if len(snapshots) < 2:
        print(
            f"Error: Need at least 2 snapshots, found {len(snapshots)} in {input_dir}.",
            file=sys.stderr,
        )
        return 1

    validate_snapshots(snapshots)

    fleet_name = input_dir.resolve().name
    merged = merge_snapshots(
        snapshots,
        min_prevalence=args.min_prevalence,
        fleet_name=fleet_name,
        include_hosts=not args.no_hosts,
    )

    if args.output_file and args.output_dir:
        print("Error: -o/--output-file and --output-dir are mutually exclusive.", file=sys.stderr)
        return 1
    if args.json_only and args.output_dir:
        print("Error: --json-only and --output-dir are mutually exclusive.", file=sys.stderr)
        return 1

    if args.json_only:
        output_path = args.output_file or (input_dir / "fleet-snapshot.json")
        output_path.write_text(merged.model_dump_json(indent=2))
        print(f"Fleet snapshot written to {output_path}")
        print(f"  {len(snapshots)} hosts merged, threshold {args.min_prevalence}%")
        return 0

    def _fleet_renderers(snapshot, output_dir):
        from .renderers import run_all
        run_all(snapshot, output_dir)

    print(f"Merged {len(snapshots)} hosts (threshold {args.min_prevalence}%)")

    with tempfile.NamedTemporaryFile(suffix=".json", delete=False) as tmp:
        tmp.write(merged.model_dump_json(indent=2).encode())
        tmp_path = Path(tmp.name)
    try:
        run_pipeline(
            host_root=Path("/"),
            run_inspectors=None,
            run_renderers=_fleet_renderers,
            from_snapshot_path=tmp_path,
            inspect_only=False,
            output_file=args.output_file,
            output_dir=args.output_dir,
            no_subscription=True,
            cwd=cwd or input_dir.resolve(),
        )
    finally:
        tmp_path.unlink(missing_ok=True)

    return 0


def main(argv: Optional[list] = None, cwd: Optional[Path] = None) -> int:
    args = parse_args(argv)

    try:
        match args.command:
            case None | "inspect":
                return _run_inspect(args)
            case "fleet":
                return _run_fleet(args, cwd=cwd)
            case "refine":
                from .refine import run_refine
                return run_refine(args)
            case other:
                print(f"Unknown command: {other}", file=sys.stderr)
                return 1
    except NotImplementedError as e:
        print(f"Error: {e}", file=sys.stderr)
        return 1
    except Exception as e:
        print(f"Error: {e}", file=sys.stderr)
        if os.environ.get("YOINKC_DEBUG"):
            traceback.print_exc()
        else:
            print("Set YOINKC_DEBUG=1 for the full traceback.", file=sys.stderr)
        return 1
