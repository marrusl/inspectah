"""
Secret redaction pass. Runs over all captured file contents before any output is written.
Replaces matched values with REDACTED_<TYPE>_<hash> and populates snapshot.redactions.
"""

import hashlib
import re
from pathlib import Path
from typing import List, Optional, Tuple

from .schema import (
    ConfigFileEntry, InspectionSnapshot,
    FirewallZone, QuadletUnit, RunningContainer,
    GeneratedTimerUnit, SystemdTimer,
)


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
    (r"(?i)(?<![a-z])(password|passwd|pass|passphrase)\s*[:=]\s*['\"]?([^\s'\"]+)['\"]?", "PASSWORD"),
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
    """Return a new snapshot with all captured text content redacted.

    Scans both config.files and the content fields of other sections that
    can carry raw text with credentials (NM connection profiles, firewall
    zone XML, quadlet units, running container env vars, timer service
    units, GRUB defaults, kernel module configs, sudoers rules).

    Does not mutate the input.  Returns a new snapshot with redacted
    content and snapshot.redactions populated.
    """
    redactions: List[dict] = list(snapshot.redactions)
    updates: dict = {}

    _EXCLUDED_PLACEHOLDER = "# Content excluded (sensitive path). Handle manually.\n"

    # -----------------------------------------------------------------------
    # 1. config.files — existing behaviour
    # -----------------------------------------------------------------------
    if snapshot.config and snapshot.config.files:
        new_files: List[ConfigFileEntry] = []
        for entry in snapshot.config.files:
            if _is_excluded_path(entry.path):
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
        updates["config"] = snapshot.config.model_copy(update={"files": new_files})

    # -----------------------------------------------------------------------
    # 2. NetworkSection — firewall zone XML (can contain VPN/wifi secrets)
    # -----------------------------------------------------------------------
    if snapshot.network and snapshot.network.firewall_zones:
        new_zones: List[FirewallZone] = []
        changed = False
        for z in snapshot.network.firewall_zones:
            new_content = _redact_text(z.content, f"network:firewall_zone/{z.name}", redactions)
            if new_content != z.content:
                new_zones.append(z.model_copy(update={"content": new_content}))
                changed = True
            else:
                new_zones.append(z)
        if changed:
            updates["network"] = snapshot.network.model_copy(
                update={"firewall_zones": new_zones}
            )

    # -----------------------------------------------------------------------
    # 3. ContainerSection — quadlet unit content and running container env
    # -----------------------------------------------------------------------
    if snapshot.containers:
        ct_updates: dict = {}

        if snapshot.containers.quadlet_units:
            new_units: List[QuadletUnit] = []
            changed = False
            for u in snapshot.containers.quadlet_units:
                new_content = _redact_text(u.content, f"containers:quadlet/{u.name}", redactions)
                if new_content != u.content:
                    new_units.append(u.model_copy(update={"content": new_content}))
                    changed = True
                else:
                    new_units.append(u)
            if changed:
                ct_updates["quadlet_units"] = new_units

        if snapshot.containers.running_containers:
            new_containers: List[RunningContainer] = []
            changed = False
            for c in snapshot.containers.running_containers:
                name = c.name or c.id[:12]
                new_env: List[str] = []
                env_changed = False
                for e in c.env:
                    redacted_e = _redact_text(e, f"containers:running/{name}:env", redactions)
                    new_env.append(redacted_e)
                    if redacted_e != e:
                        env_changed = True
                if env_changed:
                    new_containers.append(c.model_copy(update={"env": new_env}))
                    changed = True
                else:
                    new_containers.append(c)
            if changed:
                ct_updates["running_containers"] = new_containers

        if ct_updates:
            updates["containers"] = snapshot.containers.model_copy(update=ct_updates)

    # -----------------------------------------------------------------------
    # 4. ScheduledTaskSection — generated timer service content and commands
    # -----------------------------------------------------------------------
    if snapshot.scheduled_tasks:
        st_updates: dict = {}

        if snapshot.scheduled_tasks.generated_timer_units:
            new_gen: List[GeneratedTimerUnit] = []
            changed = False
            for u in snapshot.scheduled_tasks.generated_timer_units:
                item_updates: dict = {}
                new_svc = _redact_text(
                    u.service_content, f"scheduled:timer/{u.name}:service_content", redactions
                )
                if new_svc != u.service_content:
                    item_updates["service_content"] = new_svc
                new_cmd = _redact_text(
                    u.command, f"scheduled:timer/{u.name}:command", redactions
                )
                if new_cmd != u.command:
                    item_updates["command"] = new_cmd
                if item_updates:
                    new_gen.append(u.model_copy(update=item_updates))
                    changed = True
                else:
                    new_gen.append(u)
            if changed:
                st_updates["generated_timer_units"] = new_gen

        if snapshot.scheduled_tasks.systemd_timers:
            new_timers: List[SystemdTimer] = []
            changed = False
            for t in snapshot.scheduled_tasks.systemd_timers:
                if t.source != "local":
                    new_timers.append(t)
                    continue
                new_svc = _redact_text(
                    t.service_content, f"scheduled:systemd_timer/{t.name}:service_content", redactions
                )
                if new_svc != t.service_content:
                    new_timers.append(t.model_copy(update={"service_content": new_svc}))
                    changed = True
                else:
                    new_timers.append(t)
            if changed:
                st_updates["systemd_timers"] = new_timers

        if st_updates:
            updates["scheduled_tasks"] = snapshot.scheduled_tasks.model_copy(update=st_updates)

    # -----------------------------------------------------------------------
    # 5. KernelBootSection — GRUB defaults and module configs
    # -----------------------------------------------------------------------
    if snapshot.kernel_boot:
        kb_updates: dict = {}

        new_grub = _redact_text(
            snapshot.kernel_boot.grub_defaults,
            "kernel:grub_defaults",
            redactions,
        )
        if new_grub != snapshot.kernel_boot.grub_defaults:
            kb_updates["grub_defaults"] = new_grub

        for attr, label in (
            ("modules_load_d", "modules_load_d"),
            ("modprobe_d", "modprobe_d"),
            ("dracut_conf", "dracut_conf"),
        ):
            entries = getattr(snapshot.kernel_boot, attr)
            if not entries:
                continue
            new_entries = []
            changed = False
            for entry in entries:
                path = entry.get("path", "")
                content = entry.get("content", "")
                new_content = _redact_text(content, f"kernel:{label}/{path}", redactions)
                if new_content != content:
                    new_entries.append({"path": path, "content": new_content})
                    changed = True
                else:
                    new_entries.append(entry)
            if changed:
                kb_updates[attr] = new_entries

        if kb_updates:
            updates["kernel_boot"] = snapshot.kernel_boot.model_copy(update=kb_updates)

    # -----------------------------------------------------------------------
    # 6. UserGroupSection — sudoers rules
    # -----------------------------------------------------------------------
    if snapshot.users_groups and snapshot.users_groups.sudoers_rules:
        new_rules: List[str] = []
        changed = False
        for rule in snapshot.users_groups.sudoers_rules:
            new_rule = _redact_text(rule, "users:sudoers", redactions)
            new_rules.append(new_rule)
            if new_rule != rule:
                changed = True
        if changed:
            updates["users_groups"] = snapshot.users_groups.model_copy(
                update={"sudoers_rules": new_rules}
            )

    updates["redactions"] = redactions
    return snapshot.model_copy(update=updates)
