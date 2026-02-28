"""
Secret redaction pass. Runs over all captured file contents before any output is written.
Replaces matched values with REDACTED_<TYPE>_<hash> and populates snapshot.redactions.
"""

import hashlib
import re
from pathlib import Path
from typing import List, Optional, Tuple

from .schema import ConfigFileEntry, ConfigFileKind, InspectionSnapshot


# Paths that are never included in content; only referenced with a note
EXCLUDED_PATHS = (
    r"/etc/shadow",
    r"/etc/gshadow",
    r"/etc/ssh/ssh_host_.*",
    r"/etc/pki/.*\.key",
    r".*\.key$",
    r".*keytab$",
)

# (pattern, type_label). Order matters: more specific first.
REDACT_PATTERNS: List[Tuple[str, str]] = [
    (r"-----BEGIN\s+.+PRIVATE KEY-----[\s\S]+?-----END\s+.+-----", "PRIVATE_KEY"),
    (r"(?i)(api[_-]?key|apikey)\s*[:=]\s*['\"]?([a-zA-Z0-9_\-]{20,})['\"]?", "API_KEY"),
    (r"(?i)(token)\s*[:=]\s*['\"]?([a-zA-Z0-9_\-]{20,})['\"]?", "TOKEN"),
    (r"(?i)(password|passwd|pass)\s*[:=]\s*['\"]?([^\s'\"]+)['\"]?", "PASSWORD"),
    (r"(?i)secret\s*[:=]\s*['\"]?([^\s'\"]+)['\"]?", "SECRET"),
    (r"(?i)bearer\s+([a-zA-Z0-9_\-\.]{20,})", "BEARER_TOKEN"),
    (r"AKIA[0-9A-Z]{16}", "AWS_KEY"),
    (r"ghp_[a-zA-Z0-9]{36}", "GITHUB_TOKEN"),
    (r"ghu_[a-zA-Z0-9]{36}", "GITHUB_TOKEN"),
    (r"(?i)(?:gcp|google)[_-]?(?:api[_-]?key|credentials?)\s*[:=]\s*['\"]?([^\s'\"]{10,})['\"]?", "GCP_CREDENTIAL"),
    (r"(?i)(?:azure|az)[_-]?(?:storage[_-]?key|account[_-]?key|secret)\s*[:=]\s*['\"]?([^\s'\"]{10,})['\"]?", "AZURE_CREDENTIAL"),
    (r"(?i)jdbc:[^:]+://[^:]+:([^@\s]+)@", "JDBC_PASSWORD"),
    (r"(?i)postgres(ql)?://[^:]+:([^@\s]+)@", "POSTGRES_PASSWORD"),
    (r"(?i)mongodb(\+srv)?://[^:]+:([^@\s]+)@", "MONGODB_PASSWORD"),
    (r"(?i)redis://[^:]*:([^@\s]+)@", "REDIS_PASSWORD"),
]


def _is_excluded_path(path: str) -> bool:
    # Normalise to a leading-slash form so that anchored patterns like
    # /etc/shadow match regardless of how the caller stored the path.
    normalised = "/" + path.lstrip("/")
    for pat in EXCLUDED_PATHS:
        regex = pat.replace("*", ".*")
        if re.fullmatch(regex, normalised) or re.search(regex, normalised):
            return True
    return False


def _truncated_sha256(value: str, length: int = 8) -> str:
    return hashlib.sha256(value.encode()).hexdigest()[:length]


# Values that commonly appear after "password:" or "passwd:" in config files
# but are not actual secrets (e.g. nsswitch.conf, PAM configs, sudoers).
_FALSE_POSITIVE_VALUES = frozenset({
    "files", "sss", "compat", "nis", "ldap", "systemd", "winbind", "dns",
    "required", "requisite", "sufficient", "optional", "include", "substack",
    "prompt", "true", "false", "yes", "no", "none", "null", "disabled",
    "all",
    "sha512", "sha256", "md5", "blowfish", "yescrypt", "des",
    "pam_unix.so", "pam_deny.so", "pam_permit.so", "pam_pwquality.so",
    "pam_sss.so", "pam_faildelay.so", "pam_env.so", "pam_localuser.so",
    "pam_systemd.so", "pam_faillock.so", "pam_succeed_if.so",
})


def _is_comment_line(text: str, match_start: int) -> bool:
    """Check whether the match occurs on a comment line (starts with # or ;)."""
    line_start = text.rfind("\n", 0, match_start) + 1
    prefix = text[line_start:match_start].lstrip()
    return prefix.startswith("#") or prefix.startswith(";") or prefix.startswith("!")


def _redact_text(text: str, path: str, redactions: List[dict]) -> str:
    out = text
    for pattern, type_label in REDACT_PATTERNS:
        for m in list(re.finditer(pattern, out, re.IGNORECASE | re.DOTALL)):
            if _is_comment_line(out, m.start()):
                continue
            original = m.group(0)
            if type_label == "PRIVATE_KEY":
                replacement = f"REDACTED_{type_label}_<removed>"
            else:
                sub = m.group(2) if m.lastindex and m.lastindex >= 2 else m.group(0)
                if type_label == "PASSWORD" and sub.strip().lower() in _FALSE_POSITIVE_VALUES:
                    continue
                replacement = f"REDACTED_{type_label}_{_truncated_sha256(sub)}"
            out = out.replace(original, replacement, 1)
            redactions.append({
                "path": path,
                "pattern": type_label,
                "line": "content",
                "remediation": "Use a secret store or inject at deploy time.",
            })
    return out


def scan_directory_for_secrets(root: Path) -> Optional[str]:
    """
    Scan all text files under root for secret patterns. Returns first path where
    a pattern was found, or None if clean. Used to verify output before GitHub push.
    """
    root = Path(root)
    for f in root.rglob("*"):
        if not f.is_file() or ".git" in str(f):
            continue
        try:
            text = f.read_text()
        except Exception:
            continue
        for pattern, _ in REDACT_PATTERNS:
            if re.search(pattern, text, re.IGNORECASE | re.DOTALL):
                return str(f.relative_to(root))
    return None


def redact_snapshot(snapshot: InspectionSnapshot) -> InspectionSnapshot:
    """
    Return a new snapshot with config file contents redacted and redactions list populated.
    Does not mutate the input.
    """
    redactions: List[dict] = list(snapshot.redactions)
    if not snapshot.config or not snapshot.config.files:
        return snapshot.model_copy(update={"redactions": redactions})

    _EXCLUDED_PLACEHOLDER = "# Content excluded (sensitive path). Handle manually.\n"

    new_files: List[ConfigFileEntry] = []
    for entry in snapshot.config.files:
        if _is_excluded_path(entry.path):
            # Already excluded on a previous redact pass — don't double-count.
            if entry.content != _EXCLUDED_PLACEHOLDER:
                redactions.append({
                    "path": entry.path,
                    "pattern": "EXCLUDED_PATH",
                    "line": "entire file",
                    "remediation": "File not included; handle credentials manually (e.g. systemd credential, secret store).",
                })
            new_files.append(entry.model_copy(update={"content": _EXCLUDED_PLACEHOLDER}))
            continue
        new_content = _redact_text(entry.content or "", entry.path, redactions)
        if new_content != (entry.content or ""):
            new_files.append(entry.model_copy(update={"content": new_content}))
        else:
            new_files.append(entry)

    new_config = snapshot.config.model_copy(update={"files": new_files})
    return snapshot.model_copy(update={"config": new_config, "redactions": redactions})
