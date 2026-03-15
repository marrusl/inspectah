"""Entry point for yoinkc-fleet CLI."""

import sys
import tempfile
from pathlib import Path
from typing import Optional

from .cli import parse_args
from .loader import discover_snapshots, validate_snapshots
from .merge import merge_snapshots


def _run_renderers(snapshot, output_dir):
    from yoinkc.renderers import run_all
    run_all(snapshot, output_dir)


def main(argv: Optional[list[str]] = None, cwd: Optional[Path] = None) -> int:
    args = parse_args(argv)

    input_dir: Path = args.input_dir
    if not input_dir.is_dir():
        print(f"Error: {input_dir} is not a directory.", file=sys.stderr)
        sys.exit(1)

    snapshots = discover_snapshots(input_dir)
    if len(snapshots) < 2:
        print(
            f"Error: Need at least 2 snapshots, found {len(snapshots)} in {input_dir}.",
            file=sys.stderr,
        )
        sys.exit(1)

    validate_snapshots(snapshots)

    fleet_name = input_dir.resolve().name
    merged = merge_snapshots(
        snapshots,
        min_prevalence=args.min_prevalence,
        fleet_name=fleet_name,
        include_hosts=not args.no_hosts,
    )

    if args.output and args.output_dir:
        print("Error: --output and --output-dir are mutually exclusive.", file=sys.stderr)
        sys.exit(1)
    if args.json_only and args.output_dir:
        print("Error: --json-only and --output-dir are mutually exclusive.", file=sys.stderr)
        sys.exit(1)

    if args.json_only:
        output_path = args.output or (input_dir / "fleet-snapshot.json")
        output_path.write_text(merged.model_dump_json(indent=2))
        print(f"Fleet snapshot written to {output_path}")
        print(f"  {len(snapshots)} hosts merged, threshold {args.min_prevalence}%")
        return 0

    from yoinkc.pipeline import run_pipeline

    print(f"Merged {len(snapshots)} hosts (threshold {args.min_prevalence}%)")

    with tempfile.NamedTemporaryFile(suffix=".json", delete=False) as tmp:
        tmp.write(merged.model_dump_json(indent=2).encode())
        tmp_path = Path(tmp.name)
    try:
        run_pipeline(
            host_root=Path("/"),
            run_inspectors=None,
            run_renderers=_run_renderers,
            from_snapshot_path=tmp_path,
            inspect_only=False,
            output_file=args.output,
            output_dir=args.output_dir,
            no_entitlement=True,
            cwd=cwd,
        )
    finally:
        tmp_path.unlink(missing_ok=True)

    return 0


if __name__ == "__main__":
    sys.exit(main())
