"""Entry point for yoinkc-fleet CLI."""

import sys
from pathlib import Path
from typing import Optional

from .cli import parse_args
from .loader import discover_snapshots, validate_snapshots
from .merge import merge_snapshots


def main(argv: Optional[list[str]] = None) -> int:
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

    output_path = args.output or (input_dir / "fleet-snapshot.json")
    output_path.write_text(merged.model_dump_json(indent=2))
    print(f"Fleet snapshot written to {output_path}")
    print(f"  {len(snapshots)} hosts merged, threshold {args.min_prevalence}%")

    return 0


if __name__ == "__main__":
    sys.exit(main())
