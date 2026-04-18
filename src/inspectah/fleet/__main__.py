"""Entry point for `python -m yoinkc.fleet` — thin wrapper around the top-level CLI."""

import sys
from pathlib import Path
from typing import Optional

from yoinkc.__main__ import main as _top_level_main


def main(argv: Optional[list[str]] = None, cwd: Optional[Path] = None) -> int:
    """Run `yoinkc fleet` with the given argv (sans the 'fleet' token)."""
    fleet_argv = ["fleet"] + (argv if argv is not None else sys.argv[1:])
    return _top_level_main(fleet_argv, cwd=cwd)


if __name__ == "__main__":
    sys.exit(main())
