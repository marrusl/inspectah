"""Non-RPM Software inspector.

Scans /opt, /srv, /usr/local for:
  - readelf-based binary classification (Go, Rust, dynamic/static C/C++)
  - pip dist-info packages & venv detection (system-site-packages flag)
  - pip list --path for live venvs
  - npm/yarn/gem lockfiles
  - git-managed directories (remote URL + commit hash)
  - generic directory scan with optional deep strings scan

User home directories (/home) are intentionally excluded — artifacts found
there are overwhelmingly development checkouts, not deployed services.
"""

import configparser
import os
import re
import sys
from pathlib import Path
from typing import Dict, List, Optional, Tuple

from ..executor import Executor
from ..schema import NonRpmSoftwareSection
from . import is_dev_artifact, filtered_rglob

_DEBUG = bool(os.environ.get("YOINKC_DEBUG", ""))


def _debug(msg: str) -> None:
    if _DEBUG:
        print(f"[yoinkc] non-rpm: {msg}", file=sys.stderr)


# Quick patterns for the default (4KB head) scan
VERSION_PATTERNS = [
    re.compile(rb"version\s*[=:]\s*[\"']?([0-9]+\.[0-9]+(?:\.[0-9]+)?)", re.I),
    re.compile(rb"v([0-9]+\.[0-9]+(?:\.[0-9]+)?)[\s\-]"),
    re.compile(rb"([0-9]+\.[0-9]+\.[0-9]+)(?:\s|$|\))"),
]

# Extended patterns for --deep-binary-scan (full strings output)
DEEP_VERSION_PATTERNS = VERSION_PATTERNS + [
    # Go version string embedded by linker: "go1.21.5"
    re.compile(rb"go([0-9]+\.[0-9]+(?:\.[0-9]+)?)\b"),
    # Rust version: "rustc 1.75.0"
    re.compile(rb"rustc\s+([0-9]+\.[0-9]+\.[0-9]+)"),
    # Common build metadata: "Built with X 1.2.3", "Compiled against 1.2.3"
    re.compile(rb"(?:built|compiled|linked)\s+(?:with|against)\s+\S+\s+([0-9]+\.[0-9]+\.[0-9]+)", re.I),
    # Release/tag pattern: "release-1.2.3", "tag/v1.2.3"
    re.compile(rb"(?:release|tag)[/\-]v?([0-9]+\.[0-9]+\.[0-9]+)"),
    # Semantic version with pre-release: "1.2.3-beta.1"
    re.compile(rb"([0-9]+\.[0-9]+\.[0-9]+[\-][a-zA-Z0-9.]+)"),
    # Git describe: "v1.2.3-45-gabcdef"
    re.compile(rb"v([0-9]+\.[0-9]+\.[0-9]+)-[0-9]+-g[0-9a-f]+"),
    # OpenSSL-style: "OpenSSL 3.0.12"
    re.compile(rb"(?:OpenSSL|LibreSSL|BoringSSL)\s+([0-9]+\.[0-9]+\.[0-9]+[a-z]?)", re.I),
    # Java: "java version \"17.0.5\""
    re.compile(rb"java\s+version\s+[\"']([0-9]+\.[0-9]+\.[0-9]+)", re.I),
    # Node: "node v20.10.0"
    re.compile(rb"node\s+v([0-9]+\.[0-9]+\.[0-9]+)", re.I),
    # Python embedded version
    re.compile(rb"Python\s+([0-9]+\.[0-9]+\.[0-9]+)"),
]


def _safe_iterdir(d: Path) -> List[Path]:
    try:
        return list(d.iterdir())
    except (PermissionError, OSError):
        return []


def _safe_read(p: Path) -> str:
    try:
        return p.read_text()
    except (PermissionError, OSError):
        return ""


_FHS_DIRS = frozenset({
    "bin", "etc", "games", "include", "lib", "lib64", "libexec",
    "sbin", "share", "src", "man",
})


def _dir_has_content(d: Path) -> bool:
    try:
        for p in d.rglob("*"):
            if p.is_file():
                return True
    except (PermissionError, OSError):
        pass
    return False


# ---------------------------------------------------------------------------
# readelf-based binary classification
# ---------------------------------------------------------------------------

def _classify_binary(executor: Optional[Executor], path: Path) -> Optional[dict]:
    """Use readelf to classify a binary. Returns classification dict or None."""
    if not executor:
        return None

    _debug(f"readelf -S {path}")
    r = executor(["readelf", "-S", str(path)])
    if r.returncode != 0:
        _debug(f"readelf -S failed (rc={r.returncode}): {r.stderr.strip()[:200]}")
        return None

    sections_output = r.stdout

    is_go = ".note.go.buildid" in sections_output or ".gopclntab" in sections_output
    is_rust = ".rustc" in sections_output

    rd = executor(["readelf", "-d", str(path)])
    dynamic_output = rd.stdout if rd.returncode == 0 else ""
    is_static = "no dynamic section" in dynamic_output.lower() or not dynamic_output.strip()
    shared_libs: List[str] = []
    for line in dynamic_output.splitlines():
        if "(NEEDED)" in line:
            m = re.search(r"\[(.+?)\]", line)
            if m:
                shared_libs.append(m.group(1))

    lang = "go" if is_go else ("rust" if is_rust else "c/c++")
    _debug(f"classified {path.name}: lang={lang} static={is_static} libs={len(shared_libs)}")
    return {
        "lang": lang,
        "static": is_static,
        "shared_libs": shared_libs,
    }


def _is_binary(executor: Optional[Executor], host_root: Path, path: Path) -> bool:
    if not executor:
        return False
    r = executor(["file", "-b", str(path)])
    if r.returncode != 0:
        _debug(f"file -b failed for {path} (rc={r.returncode}): {r.stderr.strip()[:200]}")
        return False
    out = r.stdout.lower()
    result = "elf" in out or "executable" in out or "script" in out
    _debug(f"file -b {path.name}: {r.stdout.strip()[:80]} -> binary={result}")
    return result


def _strings_version(executor: Optional[Executor], path: Path, limit_kb: Optional[int] = None, deep: bool = False) -> Optional[str]:
    if not executor:
        return None
    if limit_kb:
        cmd = ["sh", "-c", f"head -c {limit_kb * 1024} {path!s} | strings"]
    else:
        cmd = ["strings", str(path)]
    r = executor(cmd)
    if r.returncode != 0:
        return None
    data = r.stdout.encode() if isinstance(r.stdout, str) else r.stdout
    patterns = DEEP_VERSION_PATTERNS if deep else VERSION_PATTERNS
    for pat in patterns:
        m = pat.search(data)
        if m:
            return m.group(1).decode("utf-8", errors="replace").strip()
    return None


# ---------------------------------------------------------------------------
# Git repository detection
# ---------------------------------------------------------------------------

def _scan_git_repo(host_root: Path, d: Path) -> Optional[dict]:
    """Check if a directory has .git and extract remote URL + commit hash."""
    git_dir = d / ".git"
    if not git_dir.is_dir():
        return None

    remote_url = ""
    config_file = git_dir / "config"
    if config_file.exists():
        text = _safe_read(config_file)
        for line in text.splitlines():
            stripped = line.strip()
            if stripped.startswith("url ="):
                remote_url = stripped.split("=", 1)[1].strip()
                break

    commit_hash = ""
    head = git_dir / "HEAD"
    if head.exists():
        head_content = _safe_read(head).strip()
        if head_content.startswith("ref:"):
            ref_path = git_dir / head_content.split(":", 1)[1].strip()
            if ref_path.exists():
                commit_hash = _safe_read(ref_path).strip()
        else:
            commit_hash = head_content

    branch = ""
    head_content = _safe_read(head).strip() if head.exists() else ""
    if head_content.startswith("ref:"):
        ref = head_content.split(":", 1)[1].strip()
        if ref.startswith("refs/heads/"):
            branch = ref[len("refs/heads/"):]

    return {
        "path": str(d.relative_to(host_root)),
        "name": d.name,
        "method": "git repository",
        "confidence": "high",
        "git_remote": remote_url,
        "git_commit": commit_hash,
        "git_branch": branch,
    }


# ---------------------------------------------------------------------------
# Venv detection and pip list --path
# ---------------------------------------------------------------------------

def _find_venvs(host_root: Path) -> List[Tuple[Path, bool]]:
    """Find Python venvs under /opt, /srv. Returns (venv_path, system_site_packages)."""
    results: List[Tuple[Path, bool]] = []
    for search_root in ("opt", "srv"):
        d = host_root / search_root
        if not d.exists():
            continue
        try:
            for cfg in filtered_rglob(d, "pyvenv.cfg"):
                if not cfg.is_file():
                    continue
                venv_dir = cfg.parent
                text = _safe_read(cfg)
                system_sp = False
                for line in text.splitlines():
                    stripped = line.strip().lower()
                    if stripped.startswith("include-system-site-packages"):
                        system_sp = "true" in stripped
                        break
                results.append((venv_dir, system_sp))
        except (PermissionError, OSError):
            continue
    return results


def _scan_venv_packages(
    section: NonRpmSoftwareSection,
    host_root: Path,
    executor: Optional[Executor],
    warnings: Optional[List] = None,
) -> None:
    """Discover venvs, scan dist-info inside them, and run pip list --path if possible."""
    venvs = _find_venvs(host_root)
    pip_fail_count = 0

    for venv_path, system_sp in venvs:
        rel = str(venv_path.relative_to(host_root))
        packages: List[dict] = []

        # Scan dist-info inside the venv
        try:
            for sp_dir in venv_path.rglob("site-packages"):
                if not sp_dir.is_dir():
                    continue
                for dist_info in sp_dir.glob("*.dist-info"):
                    stem = dist_info.name.replace(".dist-info", "")
                    parts = stem.split("-")
                    name, version = stem, ""
                    for idx, part in enumerate(parts):
                        if part and part[0].isdigit():
                            name = "-".join(parts[:idx])
                            version = "-".join(parts[idx:])
                            break
                    packages.append({"name": name, "version": version})
        except (PermissionError, OSError):
            pass

        # Try pip list --path for a richer package list
        if executor:
            try:
                sp_paths = list(venv_path.rglob("site-packages"))
                pip_ok = False
                for sp_path in sp_paths:
                    if sp_path.is_dir():
                        r = executor(["pip", "list", "--path", str(sp_path), "--format", "columns"])
                        if r.returncode == 0 and r.stdout.strip():
                            pip_packages = _parse_pip_list(r.stdout)
                            if pip_packages:
                                packages = pip_packages
                                pip_ok = True
                        elif r.returncode != 0:
                            pip_fail_count += 1
                        break
            except Exception:
                pass

        section.items.append({
            "path": rel,
            "name": venv_path.name,
            "method": "python venv",
            "confidence": "high",
            "system_site_packages": system_sp,
            "packages": packages,
        })

    if pip_fail_count > 0 and warnings is not None:
        warnings.append({
            "source": "non_rpm_software",
            "message": f"pip list --path failed for {pip_fail_count} venv(s) — package inventory may be incomplete (dist-info scan used as fallback).",
            "severity": "warning",
        })


def _parse_pip_list(output: str) -> List[dict]:
    """Parse `pip list` columnar output into [{name, version}]."""
    results: List[dict] = []
    lines = output.strip().splitlines()
    for line in lines:
        if line.startswith("---") or line.startswith("Package"):
            continue
        parts = line.split()
        if len(parts) >= 2:
            results.append({"name": parts[0], "version": parts[1]})
    return results


# ---------------------------------------------------------------------------
# Existing scanners (pip dist-info, npm, gem, directory scan)
# ---------------------------------------------------------------------------

_FHS_BIN_DIRS = frozenset({"bin", "sbin", "libexec"})
_FHS_LIB_DIRS = frozenset({"lib", "lib64"})
_FHS_ENUMERATE_DIRS = _FHS_BIN_DIRS | _FHS_LIB_DIRS


def _classify_file(
    host_root: Path,
    f: Path,
    executor: Optional[Executor],
    deep: bool,
) -> dict:
    """Classify a single file and return an item dict."""
    item: dict = {
        "path": str(f.relative_to(host_root)),
        "name": f.name,
        "confidence": "low",
        "method": "file scan",
    }
    if not executor:
        _debug(f"classify {f.name}: no executor, returning low confidence")
        return item

    binary_info = _classify_binary(executor, f)
    if binary_info:
        item["lang"] = binary_info["lang"]
        item["static"] = binary_info["static"]
        item["shared_libs"] = binary_info["shared_libs"]
        item["confidence"] = "high"
        item["method"] = f"readelf ({binary_info['lang']})"
        return item

    if _is_binary(executor, host_root, f):
        limit = None if deep else 4
        ver = _strings_version(executor, f, limit_kb=limit, deep=deep)
        if ver:
            item["version"] = ver
            item["method"] = "strings" if deep else "strings (first 4KB)"
            item["confidence"] = "medium"

    _debug(f"classify {f.name}: confidence={item['confidence']} method={item['method']}")
    return item


def _scan_fhs_dir_files(
    section: NonRpmSoftwareSection,
    host_root: Path,
    fhs_dir: Path,
    executor: Optional[Executor],
    deep: bool,
) -> None:
    """Enumerate individual files inside an FHS directory (bin, lib, etc.)."""
    try:
        entries = sorted(fhs_dir.iterdir())
    except (PermissionError, OSError):
        return

    for f in entries:
        if f.name.startswith("."):
            continue
        if f.is_file() or f.is_symlink():
            if f.is_symlink() and not f.exists():
                continue
            item = _classify_file(host_root, f, executor, deep)
            section.items.append(item)
        elif f.is_dir():
            # Recurse one level for lib subdirs (e.g. lib/python3.x/)
            if fhs_dir.name in _FHS_LIB_DIRS:
                _scan_fhs_dir_files(section, host_root, f, executor, deep)


def _scan_dirs(
    section: NonRpmSoftwareSection,
    host_root: Path,
    executor: Optional[Executor],
    deep: bool,
) -> None:
    """Scan /opt and /usr/local for non-RPM software directories."""
    for base in ("opt", "usr/local"):
        d = host_root / base
        if not d.exists():
            continue
        for entry in _safe_iterdir(d):
            if not entry.is_dir() or entry.name.startswith("."):
                continue
            if is_dev_artifact(entry, host_root):
                continue
            if base == "usr/local" and entry.name in _FHS_DIRS and not _dir_has_content(entry):
                continue

            # FHS bin/lib dirs under /usr/local: enumerate individual files
            if base == "usr/local" and entry.name in _FHS_ENUMERATE_DIRS:
                _scan_fhs_dir_files(section, host_root, entry, executor, deep)
                continue

            # Check for git repo first
            git_info = _scan_git_repo(host_root, entry)
            if git_info:
                section.items.append(git_info)
                continue

            # Check for venv (handled separately)
            if (entry / "pyvenv.cfg").exists():
                continue

            item: dict = {
                "path": str(entry.relative_to(host_root)),
                "name": entry.name,
                "confidence": "low",
                "method": "directory scan",
            }

            if executor:
                try:
                    for f in filtered_rglob(entry, "*"):
                        if not f.is_file():
                            continue
                        binary_info = _classify_binary(executor, f)
                        if binary_info:
                            item["lang"] = binary_info["lang"]
                            item["static"] = binary_info["static"]
                            item["shared_libs"] = binary_info["shared_libs"]
                            item["confidence"] = "high"
                            item["method"] = f"readelf ({binary_info['lang']})"
                            break
                        if _is_binary(executor, host_root, f):
                            limit = None if deep else 4
                            ver = _strings_version(executor, f, limit_kb=limit, deep=deep)
                            if ver:
                                item["version"] = ver
                                item["method"] = "strings" if deep else "strings (first 4KB)"
                                item["confidence"] = "medium"
                                break
                except Exception:
                    pass

            section.items.append(item)


def _scan_pip(section: NonRpmSoftwareSection, host_root: Path, executor: Optional[Executor]) -> None:
    """Detect pip-installed packages by scanning system dist-info directories."""
    for search_root in ("usr/lib/python3", "usr/lib64/python3", "usr/local/lib/python3"):
        base = host_root / search_root
        if not base.exists():
            continue
        try:
            for parent in base.iterdir():
                if not parent.is_dir():
                    continue
                site_packages = parent / "site-packages"
                if not site_packages.exists():
                    site_packages = parent
                for dist_info in site_packages.glob("*.dist-info"):
                    stem = dist_info.name.replace(".dist-info", "")
                    parts = stem.split("-")
                    name, version = stem, ""
                    for idx, part in enumerate(parts):
                        if part and part[0].isdigit():
                            name = "-".join(parts[:idx])
                            version = "-".join(parts[idx:])
                            break
                    has_c_ext = False
                    record_file = dist_info / "RECORD"
                    try:
                        if record_file.is_file():
                            for rec_line in record_file.read_text().splitlines():
                                if rec_line.strip().endswith(".so") or ".so," in rec_line:
                                    has_c_ext = True
                                    break
                    except (PermissionError, OSError):
                        pass
                    item_dict: dict = {
                        "path": str(dist_info.relative_to(host_root)),
                        "name": name,
                        "version": version,
                        "confidence": "high",
                        "method": "pip dist-info",
                    }
                    if has_c_ext:
                        item_dict["has_c_extensions"] = True
                    section.items.append(item_dict)
        except Exception:
            continue

    for venv_root in ("opt", "srv"):
        d = host_root / venv_root
        if not d.exists():
            continue
        try:
            for req in filtered_rglob(d, "requirements.txt"):
                if not req.is_file():
                    continue
                try:
                    content = req.read_text()
                except (PermissionError, OSError):
                    content = ""
                section.items.append({
                        "path": str(req.relative_to(host_root)),
                        "name": "requirements.txt",
                        "confidence": "high",
                        "method": "pip requirements.txt",
                        "content": content,
                    })
        except Exception:
            continue


_LOCKFILE_NAMES = frozenset({
    "package.json", "package-lock.json", "yarn.lock",
    "Gemfile", "Gemfile.lock",
})


def _read_lockfile_dir(d: Path) -> dict:
    result: dict = {}
    for name in _LOCKFILE_NAMES:
        f = d / name
        try:
            if f.is_file():
                result[name] = f.read_text()
        except (PermissionError, OSError):
            pass
    return result


def _scan_npm(section: NonRpmSoftwareSection, host_root: Path) -> None:
    for search_root in ("opt", "srv", "usr/local"):
        d = host_root / search_root
        if not d.exists():
            continue
        try:
            for lock in filtered_rglob(d, "package-lock.json"):
                if not lock.is_file():
                    continue
                files = _read_lockfile_dir(lock.parent)
                section.items.append({
                    "path": str(lock.parent.relative_to(host_root)),
                    "name": lock.parent.name,
                    "confidence": "high",
                    "method": "npm package-lock.json",
                    "files": files,
                })
            for lock in filtered_rglob(d, "yarn.lock"):
                if not lock.is_file():
                    continue
                files = _read_lockfile_dir(lock.parent)
                section.items.append({
                    "path": str(lock.parent.relative_to(host_root)),
                    "name": lock.parent.name,
                    "confidence": "high",
                    "method": "yarn.lock",
                    "files": files,
                })
        except Exception:
            continue


def _scan_gem(section: NonRpmSoftwareSection, host_root: Path) -> None:
    for search_root in ("opt", "srv", "usr/local"):
        d = host_root / search_root
        if not d.exists():
            continue
        try:
            for lock in filtered_rglob(d, "Gemfile.lock"):
                if not lock.is_file():
                    continue
                files = _read_lockfile_dir(lock.parent)
                section.items.append({
                    "path": str(lock.parent.relative_to(host_root)),
                    "name": lock.parent.name,
                    "confidence": "high",
                    "method": "gem Gemfile.lock",
                    "files": files,
                })
        except Exception:
            continue


# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------

def run(
    host_root: Path,
    executor: Optional[Executor],
    deep_binary_scan: bool = False,
    warnings: Optional[list] = None,
) -> NonRpmSoftwareSection:
    section = NonRpmSoftwareSection()
    host_root = Path(host_root)

    # Probe for binary analysis tool availability (warn once, not per file)
    if executor:
        probe = executor(["readelf", "--version"])
        if probe.returncode == 127:
            if warnings is not None:
                warnings.append({
                    "source": "non_rpm_software",
                    "message": "readelf not available (rc=127) — ELF binary classification skipped. Install binutils in the yoinkc container image.",
                    "severity": "warning",
                })
        else:
            probe = executor(["file", "--version"])
            if probe.returncode == 127 and warnings is not None:
                warnings.append({
                    "source": "non_rpm_software",
                    "message": "file not available (rc=127) — binary type detection skipped. Install file in the yoinkc container image.",
                    "severity": "warning",
                })

    _scan_dirs(section, host_root, executor, deep_binary_scan)
    _scan_venv_packages(section, host_root, executor, warnings=warnings)
    _scan_pip(section, host_root, executor)
    _scan_npm(section, host_root)
    _scan_gem(section, host_root)

    _CONFIDENCE_RANK = {"high": 2, "medium": 1, "low": 0}
    seen: dict = {}
    for item in section.items:
        path = item.get("path", "")
        rank = _CONFIDENCE_RANK.get(item.get("confidence", "low"), 0)
        if path not in seen or rank > seen[path][1]:
            seen[path] = (item, rank)
    section.items = [v[0] for v in seen.values()]

    return section
