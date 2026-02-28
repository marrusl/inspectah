"""
Git output and GitHub push. Optional: requires GitPython and PyGithub when using --push-to-github.
"""

import os
from pathlib import Path
from typing import Optional


def init_git_repo(output_dir: Path) -> bool:
    """Initialize a git repo in output_dir if not already. Return True if repo is ready."""
    try:
        import git
    except ImportError:
        return False
    output_dir = Path(output_dir)
    git_dir = output_dir / ".git"
    if git_dir.exists():
        return True
    try:
        repo = git.Repo.init(output_dir)
        return True
    except Exception:
        return False


def add_and_commit(output_dir: Path, message: str = "yoinkc output") -> bool:
    """Add all files and commit. Return True on success."""
    try:
        import git
    except ImportError:
        return False
    try:
        repo = git.Repo(output_dir)
        repo.index.add("*")
        try:
            repo.index.commit(message)
        except Exception:
            if not repo.index.diff("HEAD"):
                return True  # nothing to commit
            raise
        return True
    except Exception:
        return False


def push_to_github(
    output_dir: Path,
    repo_spec: str,
    create_private: bool = True,
    skip_confirmation: bool = False,
    total_size_bytes: int = 0,
    file_count: int = 0,
    fixme_count: int = 0,
    redaction_count: int = 0,
    github_token: Optional[str] = None,
) -> Optional[str]:
    """
    Push output_dir to GitHub. repo_spec is 'owner/repo'.
    If repo does not exist and PyGithub is available, create it (private by default).
    Re-scans output for secret patterns and aborts if any found.
    Returns error message on failure, None on success.
    """
    from .redact import scan_directory_for_secrets
    secret_path = scan_directory_for_secrets(output_dir)
    if secret_path is not None:
        return f"Redaction verification failed: secret pattern found in output at {secret_path}. Aborting push."
    if not skip_confirmation:
        print(f"About to push to GitHub: {repo_spec}")
        print(f"  Files: {file_count}, Size: {total_size_bytes} bytes, Redactions: {redaction_count}, FIXMEs: {fixme_count}")
        try:
            r = input("Proceed? [y/N]: ").strip().lower()
            if r != "y" and r != "yes":
                return "Aborted by user"
        except EOFError:
            return "Aborted (no TTY)"
    try:
        import git
    except ImportError:
        return "GitPython not installed. Install with: pip install GitPython"
    output_dir = Path(output_dir)
    if not (output_dir / ".git").exists():
        if not init_git_repo(output_dir):
            return "Failed to init git repo"
        if not add_and_commit(output_dir):
            pass  # may have nothing to commit
    try:
        repo = git.Repo(output_dir)
        remotes = [r.name for r in repo.remotes]
        if "origin" not in remotes:
            # Create GitHub repo if possible — requires an authenticated token
            token = github_token or os.environ.get("GITHUB_TOKEN", "")
            try:
                from github import Github
                if not token:
                    return (
                        "Cannot create GitHub repo: no token provided. "
                        "Set GITHUB_TOKEN env var or pass --github-token."
                    )
                g = Github(token)
                user = g.get_user()
                name = repo_spec.split("/")[-1] if "/" in repo_spec else "yoinkc"
                gh_repo = user.create_repo(name, private=create_private, auto_init=False)
                origin_url = gh_repo.clone_url
            except ImportError:
                origin_url = f"https://github.com/{repo_spec}.git"
            except Exception as e:
                return f"Failed to create GitHub repo: {e}"
            repo.create_remote("origin", origin_url)
        else:
            origin = repo.remotes.origin
            if repo_spec not in str(origin.url):
                origin.set_url(f"https://github.com/{repo_spec}.git")
        origin = repo.remotes.origin
        try:
            origin.push("HEAD:main")
        except Exception:
            try:
                origin.push("HEAD:master")
            except Exception as e:
                return f"Push failed: {e}"
        return None
    except Exception as e:
        return str(e)


def output_stats(output_dir: Path) -> tuple:
    """Return (total_size_bytes, file_count, fixme_count) for output_dir."""
    output_dir = Path(output_dir)
    total = 0
    count = 0
    fixmes = 0
    for f in output_dir.rglob("*"):
        if f.is_file() and ".git" not in str(f):
            total += f.stat().st_size
            count += 1
            try:
                fixmes += f.read_text().count("FIXME")
            except Exception:
                pass
    return total, count, fixmes
