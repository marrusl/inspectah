"""
Command execution abstraction.

Inspectors never call subprocess directly. They use the provided executor
so that tests can inject fixture file reads instead of running real commands.
"""

from dataclasses import dataclass
from typing import List, Optional, Protocol


@dataclass
class RunResult:
    """Result of running a command (or reading a fixture)."""

    stdout: str
    stderr: str
    returncode: int


class Executor(Protocol):
    """Protocol for command execution. Implementations may run commands or read fixtures."""

    def __call__(self, cmd: List[str], *, cwd: Optional[str] = None) -> RunResult:
        """Execute command (or resolve to fixture). Returns stdout, stderr, returncode."""
        ...


def subprocess_executor(cmd: List[str], *, cwd: Optional[str] = None) -> RunResult:
    """Default implementation: run the command via subprocess."""
    import subprocess
    try:
        result = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            cwd=cwd,
            timeout=300,
        )
        return RunResult(
            stdout=result.stdout or "",
            stderr=result.stderr or "",
            returncode=result.returncode,
        )
    except subprocess.TimeoutExpired as e:
        return RunResult(
            stdout=e.stdout.decode() if e.stdout else "",
            stderr=f"Command timed out after {e.timeout}s",
            returncode=-1,
        )
    except FileNotFoundError:
        return RunResult(stdout="", stderr="Command not found", returncode=127)


def make_executor(host_root: str) -> Executor:
    """Create the default executor that runs commands with host_root as context."""
    def run(cmd: List[str], *, cwd: Optional[str] = None) -> RunResult:
        return subprocess_executor(cmd, cwd=cwd or host_root)
    return run
