"""Tests verifying shell completion scripts stay in sync with the CLI."""

import argparse
from pathlib import Path

import pytest

from inspectah.cli import SUBCOMMANDS, build_parser

COMPLETIONS_DIR = Path(__file__).parent.parent / "completions"
COMPLETION_FILES = {
    "bash": COMPLETIONS_DIR / "inspectah.bash",
    "zsh": COMPLETIONS_DIR / "inspectah.zsh",
    "fish": COMPLETIONS_DIR / "inspectah.fish",
}

# Flags that are suppressed or strictly internal (set programmatically, never
# typed by a user) and should NOT appear in completions.
_INTERNAL_FLAGS = frozenset({"--bind", "--refine-mode", "--original-snapshot"})


def _long_flags_for(parser: argparse.ArgumentParser, subcommand: str) -> set[str]:
    """Extract all user-facing --long-flags for a subcommand."""
    for action in parser._subparsers._actions:
        if isinstance(action, argparse._SubParsersAction):
            sub = action.choices.get(subcommand)
            if sub is None:
                continue
            flags = set()
            for a in sub._actions:
                if isinstance(a, argparse._HelpAction):
                    continue
                for opt in a.option_strings:
                    if opt.startswith("--") and opt not in _INTERNAL_FLAGS:
                        flags.add(opt)
            return flags
    return set()


# -----------------------------------------------------------------------
# Tests
# -----------------------------------------------------------------------


@pytest.mark.parametrize("shell", ["bash", "zsh", "fish"])
def test_completion_files_exist(shell):
    path = COMPLETION_FILES[shell]
    assert path.is_file(), f"Missing completion script: {path}"


@pytest.mark.parametrize("shell", ["bash", "zsh", "fish"])
def test_completions_cover_all_subcommands(shell):
    content = COMPLETION_FILES[shell].read_text()
    for sub in SUBCOMMANDS:
        assert sub in content, f"{shell} completion missing subcommand: {sub}"


@pytest.mark.parametrize("shell", ["bash", "zsh", "fish"])
def test_completions_cover_all_flags(shell):
    content = COMPLETION_FILES[shell].read_text()
    parser = build_parser()

    missing = []
    for sub in SUBCOMMANDS:
        for flag in _long_flags_for(parser, sub):
            # fish uses `-l flag-name` rather than `--flag-name`
            needle = flag.lstrip("-") if shell == "fish" else flag
            if needle not in content:
                missing.append(f"{sub} {flag}")

    assert not missing, (
        f"{shell} completion missing flags:\n  " + "\n  ".join(sorted(missing))
    )


def test_build_parser_exposes_real_subcommands_and_flags():
    """The completion drift test must introspect the real parser definition."""
    parser = build_parser()

    refine = None
    for action in parser._subparsers._actions:
        if isinstance(action, argparse._SubParsersAction):
            refine = action.choices.get("refine")
            if refine is not None:
                break

    assert refine is not None
    refine_flags = {
        opt
        for a in refine._actions
        for opt in a.option_strings
        if opt.startswith("--")
    }
    assert "--no-browser" in refine_flags
    assert "--port" in refine_flags


def test_zsh_completion_supports_top_level_scan_flags():
    """zsh should complete scan flags for `inspectah --flag` as well as `inspectah scan --flag`."""
    content = COMPLETION_FILES["zsh"].read_text()
    assert 'CURRENT == 2' in content
    assert '$PREFIX' in content or '${words[CURRENT]}' in content
    assert '_inspectah_scan' in content


def test_fish_completion_supports_top_level_scan_flags():
    """fish should complete scan flags before an explicit subcommand."""
    content = COMPLETION_FILES["fish"].read_text()
    assert 'not __fish_seen_subcommand_from $subcmds' in content
    assert '-l from-snapshot' in content
