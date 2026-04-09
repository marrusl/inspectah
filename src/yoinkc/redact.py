"""
Secret redaction pass. Runs over all captured file contents before any output is written.
Replaces matched values with REDACTED_<TYPE>_<N> sequential counter tokens and populates
snapshot.redactions. Identical secret values share the same counter across files.
"""

import hashlib
import re
from pathlib import Path
from typing import List, Optional, Tuple

from .schema import (
    ConfigFileEntry, InspectionSnapshot, RedactionFinding,
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
    # v2 additions
    r".*\.p12$",
    r".*\.pfx$",
    r".*\.jks$",
    r"/etc/cockpit/ws-certs\.d/.*",
    r"/etc/containers/auth\.json",
)

# (pattern, type_label). Order matters: more specific first.
REDACT_PATTERNS: List[Tuple[str, str]] = [
    # --- PEM blocks (most specific structural match) ---
    (r"-----BEGIN\s+(?:\w+\s+)*PRIVATE KEY-----[\s\S]+?-----END\s+(?:\w+\s+)*PRIVATE KEY-----", "PRIVATE_KEY"),

    # --- Vendor-specific prefix patterns (standalone, no key= prefix needed) ---
    # Stripe
    (r"(?:sk|rk)_(?:test|live)_[a-zA-Z0-9]{10,99}", "STRIPE_KEY"),
    # Anthropic
    (r"sk-ant-(?:api03|admin01)-[a-zA-Z0-9_\-]{80,}", "ANTHROPIC_KEY"),
    # OpenAI
    (r"sk-(?:proj|svcacct|admin)-[A-Za-z0-9_-]{20,}", "OPENAI_KEY"),

    # --- Cloud provider prefix patterns ---
    # AWS long-term keys
    (r"AKIA[0-9A-Z]{16}", "AWS_KEY"),
    # AWS temp session keys
    (r"(?:A3T[A-Z0-9]|ASIA|ABIA|ACCA)[A-Z2-7]{16}", "AWS_TEMP_KEY"),

    # --- Git forge tokens ---
    (r"ghp_[a-zA-Z0-9]{36}", "GITHUB_TOKEN"),
    (r"ghu_[a-zA-Z0-9]{36}", "GITHUB_TOKEN"),
    # GitHub fine-grained PAT
    (r"github_pat_[a-zA-Z0-9_]{36,255}", "GITHUB_TOKEN"),
    # GitHub app installation
    (r"ghs_[0-9a-zA-Z]{36}", "GITHUB_TOKEN"),
    # GitHub OAuth
    (r"gho_[a-zA-Z0-9]{36}", "GITHUB_TOKEN"),
    # GitLab
    (r"glpat-[a-zA-Z0-9_-]{20,}", "GITLAB_TOKEN"),
    (r"glrt-[0-9a-zA-Z_\-]{20}", "GITLAB_TOKEN"),
    (r"gldt-[0-9a-zA-Z_\-]{20}", "GITLAB_TOKEN"),
    (r"glptt-[0-9a-f]{40}", "GITLAB_TOKEN"),

    # --- Infrastructure tokens ---
    # OpenShift
    (r"sha256~[\w-]{43}", "OPENSHIFT_TOKEN"),
    # Vault service
    (r"hvs\.[a-zA-Z0-9_-]{24,}", "VAULT_TOKEN"),
    # Vault batch
    (r"hvb\.[\w-]{138,300}", "VAULT_TOKEN"),

    # --- SaaS / CI tokens ---
    # Slack
    (r"xox[bp]-[a-zA-Z0-9-]{24,}", "SLACK_TOKEN"),
    # SendGrid
    (r"SG\.[a-zA-Z0-9_-]{22,}", "SENDGRID_KEY"),
    # Databricks
    (r"dapi[a-f0-9]{32}(?:-\d)?", "DATABRICKS_TOKEN"),
    # Atlassian
    (r"ATATT3[A-Za-z0-9_\-=]{186}", "ATLASSIAN_TOKEN"),
    # Artifactory
    (r"AKCp[A-Za-z0-9]{69}", "ARTIFACTORY_KEY"),

    # --- Cloud / registry tokens ---
    # Alibaba
    (r"LTAI[a-zA-Z0-9]{20}", "ALIBABA_KEY"),
    # npm
    (r"npm_[a-zA-Z0-9]{36}", "NPM_TOKEN"),
    # PyPI
    (r"pypi-AgEIcHlwaS5vcmc[\w-]{50,1000}", "PYPI_TOKEN"),
    # RubyGems
    (r"rubygems_[a-f0-9]{48}", "RUBYGEMS_TOKEN"),

    # --- Encryption keys ---
    # age
    (r"AGE-SECRET-KEY-1[qpzry9x8gf2tvdw0s3jn54khce6mua7l]{58}", "AGE_KEY"),

    # --- Generic assignment-based patterns (less specific, last) ---
    (r"(?i)(api[_-]?key|apikey)\s*[:=]\s*['\"]?([a-zA-Z0-9_\-]{20,})['\"]?", "API_KEY"),
    (r"(?i)(token)\s*[:=]\s*['\"]?([a-zA-Z0-9_\-]{20,})['\"]?", "TOKEN"),
    (r"(?i)(?<![a-z])(password|passwd|pass|passphrase)\s*[:=]\s*['\"]?([^\s'\"]+)['\"]?", "PASSWORD"),
    (r"(?i)secret\s*[:=]\s*['\"]?([^\s'\"]+)['\"]?", "SECRET"),
    (r"(?i)bearer\s+([a-zA-Z0-9_\-\.]{20,})", "BEARER_TOKEN"),

    # --- Cloud provider assignment-based patterns ---
    (r"(?i)(?:gcp|google)[_-]?(?:api[_-]?key|credentials?)\s*[:=]\s*['\"]?([^\s'\"]{10,})['\"]?", "GCP_CREDENTIAL"),
    (r"(?i)(?:azure|az)[_-]?(?:storage[_-]?key|account[_-]?key|secret)\s*[:=]\s*['\"]?([^\s'\"]{10,})['\"]?", "AZURE_CREDENTIAL"),

    # --- Database connection strings ---
    (r"(?i)jdbc:[^:]+://[^:]+:([^@\s]+)@", "JDBC_PASSWORD"),
    (r"(?i)postgres(ql)?://[^:]+:([^@\s]+)@", "POSTGRES_PASSWORD"),
    (r"(?i)mongodb(\+srv)?://[^:]+:([^@\s]+)@", "MONGODB_PASSWORD"),
    (r"(?i)redis://[^:]*:([^@\s]+)@", "REDIS_PASSWORD"),

    # --- Protocol-specific keys ---
    # WireGuard private key (bare base64, not PEM-wrapped)
    # group(1)=assignment prefix, group(2)=key value
    (r"(PrivateKey\s*=\s*)([A-Za-z0-9+/]{43}=)", "WIREGUARD_KEY"),
    # WiFi PSK in NetworkManager connections
    # group(1)=assignment prefix, group(2)=psk value
    (r"(psk\s*=\s*)(\S+)", "WIFI_PSK"),
]


def _is_excluded_path(path: str) -> bool:
    # Normalise to a leading-slash form so that anchored patterns like
    # /etc/shadow match regardless of how the caller stored the path.
    normalised = "/" + path.lstrip("/")
    for pat in EXCLUDED_PATHS:
        regex = pat.replace("*", ".*")
        if re.search(regex, normalised):
            return True
    return False


def _truncated_sha256(value: str, length: int = 8) -> str:
    """Legacy fallback for callers that don't pass a _CounterRegistry.

    The primary code path (redact_snapshot) uses sequential counters via
    _CounterRegistry. This function is retained for direct _redact_text()
    and _redact_shadow_entry() callers without a registry.
    """
    return hashlib.sha256(value.encode()).hexdigest()[:length]


class _CounterRegistry:
    """Maps (type_label, secret_value) -> deterministic sequential counter token.

    One instance shared across ALL finding types within a single
    redact_snapshot() call. Ordering: file-backed findings first (sorted
    by path), then non-file-backed (shadow, container-env, etc.).
    """

    def __init__(self):
        self._counters: dict[str, int] = {}  # type_label -> next counter
        self._seen: dict[tuple[str, str], str] = {}  # (type_label, value) -> token

    def get_token(self, type_label: str, value: str) -> str:
        key = (type_label, value)
        if key in self._seen:
            return self._seen[key]
        n = self._counters.get(type_label, 0) + 1
        self._counters[type_label] = n
        token = f"REDACTED_{type_label}_{n}"
        self._seen[key] = token
        return token


# Pattern → remediation state for excluded paths
_EXCLUDED_REMEDIATION: list[tuple[str, str]] = [
    (r"/etc/cockpit/ws-certs\.d/.*", "regenerate"),
    (r"/etc/ssh/ssh_host_.*", "regenerate"),
    # All others default to "provision"
]


def _remediation_for_excluded(path: str) -> str:
    """Return remediation state for an excluded path."""
    normalised = "/" + path.lstrip("/")
    for pattern, remediation in _EXCLUDED_REMEDIATION:
        if re.search(pattern, normalised):
            return remediation
    return "provision"


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


_SHADOW_NOOP_HASHES = frozenset({"*", "!", "!!", ""})


def _redact_shadow_entry(
    line: str, redactions: list,
    registry: Optional[_CounterRegistry] = None,
) -> str:
    """Redact the password hash (field index 1) of a shadow-format line.

    Locked/disabled markers (``*``, ``!``, ``!!``, empty) are left intact.
    """
    fields = line.split(":")
    if len(fields) < 2:
        return line
    raw_hash = fields[1]
    if raw_hash.startswith("REDACTED_"):
        return line
    # Locked prefixes: "!" prepended to a real hash (e.g. "!$y$j9T$…")
    stripped = raw_hash.lstrip("!")
    if stripped in _SHADOW_NOOP_HASHES or not stripped:
        return line
    if registry is not None:
        replacement = registry.get_token("SHADOW_HASH", raw_hash)
    else:
        replacement = f"REDACTED_SHADOW_HASH_{_truncated_sha256(raw_hash)}"
    redactions.append(RedactionFinding(
        path=f"users:shadow/{fields[0]}",
        source="shadow",
        kind="inline",
        pattern="SHADOW_HASH",
        remediation="value-removed",
        replacement=replacement,
        detection_method="pattern",
    ))
    fields[1] = replacement
    return ":".join(fields)


def _redact_text(
    text: str, path: str, redactions: list,
    registry: Optional[_CounterRegistry] = None,
    source: str = "file",
) -> str:
    out = text
    for pattern, type_label in REDACT_PATTERNS:
        matches = list(re.finditer(pattern, out, re.IGNORECASE | re.DOTALL))
        spans: List[Tuple[int, int, str]] = []
        for m in matches:
            if _is_comment_line(out, m.start()):
                continue
            if type_label == "PRIVATE_KEY":
                if registry is not None:
                    token = registry.get_token(type_label, m.group(0))
                else:
                    token = f"REDACTED_{type_label}_<removed>"
                replacement = token
            else:
                sub = m.group(2) if m.lastindex and m.lastindex >= 2 else m.group(0)
                if type_label == "PASSWORD" and sub.strip().lower() in _FALSE_POSITIVE_VALUES:
                    continue
                if registry is not None:
                    token = registry.get_token(type_label, sub)
                else:
                    token = f"REDACTED_{type_label}_{_truncated_sha256(sub)}"
                replacement = token
                # Preserve the prefix (group 1) when it captures the
                # assignment syntax (contains = or :).  Patterns like
                # WIREGUARD_KEY and WIFI_PSK capture "PrivateKey = " or
                # "psk=" as group(1) so the output keeps the key name.
                prefix = m.group(1) if m.lastindex and m.lastindex >= 2 else None
                if prefix and ("=" in prefix or ":" in prefix):
                    replacement = prefix + token
            spans.append((m.start(), m.end(), replacement))
            # Calculate line number for file-backed sources
            line_num = None
            if source in ("file", "diff"):
                line_num = out[:m.start()].count('\n') + 1
            redactions.append(RedactionFinding(
                path=path,
                source=source,
                kind="inline",
                pattern=type_label,
                remediation="value-removed",
                replacement=replacement,
                line=line_num,
                detection_method="pattern",
            ))
        # Apply in reverse so earlier positions stay valid.
        for start, end, replacement in reversed(spans):
            out = out[:start] + replacement + out[end:]
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
            for m in re.finditer(pattern, text, re.IGNORECASE | re.DOTALL):
                # Extract the captured secret value (group 2 if present, else full match)
                captured = m.group(m.lastindex) if m.lastindex else m.group(0)
                if captured.startswith("REDACTED_"):
                    continue
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
    redactions: list = list(snapshot.redactions)
    updates: dict = {}
    registry = _CounterRegistry()

    _EXCLUDED_PLACEHOLDER = "# Content excluded (sensitive path). Handle manually.\n"

    # -----------------------------------------------------------------------
    # 1. config.files — sorted by path for deterministic counter assignment
    # -----------------------------------------------------------------------
    if snapshot.config and snapshot.config.files:
        new_files: List[ConfigFileEntry] = []
        sorted_files = sorted(snapshot.config.files, key=lambda f: f.path)
        for entry in sorted_files:
            if _is_excluded_path(entry.path):
                if entry.content != _EXCLUDED_PLACEHOLDER:
                    redactions.append(RedactionFinding(
                        path=entry.path,
                        source="file",
                        kind="excluded",
                        pattern="EXCLUDED_PATH",
                        remediation=_remediation_for_excluded(entry.path),
                        detection_method="excluded_path",
                    ))
                new_files.append(entry.model_copy(update={"content": _EXCLUDED_PLACEHOLDER, "include": False}))
                continue
            new_content = _redact_text(entry.content or "", entry.path, redactions, registry=registry)
            new_diff = _redact_text(
                entry.diff_against_rpm or "", f"{entry.path}:diff", redactions,
                registry=registry, source="diff",
            ) if entry.diff_against_rpm else None
            file_updates: dict = {}
            if new_content != (entry.content or ""):
                file_updates["content"] = new_content
            if new_diff is not None and new_diff != entry.diff_against_rpm:
                file_updates["diff_against_rpm"] = new_diff
            if file_updates:
                new_files.append(entry.model_copy(update=file_updates))
            else:
                new_files.append(entry)
        updates["config"] = snapshot.config.model_copy(update={"files": new_files})

    # -----------------------------------------------------------------------
    # 2. NetworkSection — firewall zone XML (sorted by name)
    # -----------------------------------------------------------------------
    if snapshot.network and snapshot.network.firewall_zones:
        new_zones: List[FirewallZone] = []
        changed = False
        sorted_zones = sorted(snapshot.network.firewall_zones, key=lambda z: z.name)
        for z in sorted_zones:
            new_content = _redact_text(z.content, f"network:firewall_zone/{z.name}", redactions, registry=registry)
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
    # 3. ContainerSection — quadlet units (sorted by name) and running
    #    container env (sorted by name/id, env vars sorted within each)
    # -----------------------------------------------------------------------
    if snapshot.containers:
        ct_updates: dict = {}

        if snapshot.containers.quadlet_units:
            new_units: List[QuadletUnit] = []
            changed = False
            sorted_quadlets = sorted(snapshot.containers.quadlet_units, key=lambda u: u.name)
            for u in sorted_quadlets:
                new_content = _redact_text(u.content, f"containers:quadlet/{u.name}", redactions, registry=registry)
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
            sorted_containers = sorted(snapshot.containers.running_containers, key=lambda c: c.name or c.id[:12])
            for c in sorted_containers:
                name = c.name or c.id[:12]
                new_env: List[str] = []
                env_changed = False
                sorted_env = sorted(c.env)
                for e in sorted_env:
                    redacted_e = _redact_text(e, f"containers:running/{name}:env", redactions,
                                              registry=registry, source="container-env")
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
    # 4. ScheduledTaskSection — generated timer units (sorted by name),
    #    systemd timers (sorted by name)
    # -----------------------------------------------------------------------
    if snapshot.scheduled_tasks:
        st_updates: dict = {}

        if snapshot.scheduled_tasks.generated_timer_units:
            new_gen: List[GeneratedTimerUnit] = []
            changed = False
            sorted_gen = sorted(snapshot.scheduled_tasks.generated_timer_units, key=lambda u: u.name)
            for u in sorted_gen:
                item_updates: dict = {}
                new_svc = _redact_text(
                    u.service_content, f"scheduled:timer/{u.name}:service_content", redactions,
                    registry=registry, source="timer-cmd",
                )
                if new_svc != u.service_content:
                    item_updates["service_content"] = new_svc
                new_cmd = _redact_text(
                    u.command, f"scheduled:timer/{u.name}:command", redactions,
                    registry=registry, source="timer-cmd",
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
            sorted_timers = sorted(snapshot.scheduled_tasks.systemd_timers, key=lambda t: t.name)
            for t in sorted_timers:
                if t.source != "local":
                    new_timers.append(t)
                    continue
                new_svc = _redact_text(
                    t.service_content, f"scheduled:systemd_timer/{t.name}:service_content", redactions,
                    registry=registry, source="timer-cmd",
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
    # 5. KernelBootSection — GRUB defaults and module configs (sorted by path)
    # -----------------------------------------------------------------------
    if snapshot.kernel_boot:
        kb_updates: dict = {}

        new_grub = _redact_text(
            snapshot.kernel_boot.grub_defaults,
            "kernel:grub_defaults",
            redactions,
            registry=registry,
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
            sorted_entries = sorted(entries, key=lambda e: e.path)
            new_entries = []
            changed = False
            for entry in sorted_entries:
                new_content = _redact_text(entry.content, f"kernel:{label}/{entry.path}", redactions, registry=registry)
                if new_content != entry.content:
                    new_entries.append(entry.model_copy(update={"content": new_content}))
                    changed = True
                else:
                    new_entries.append(entry)
            if changed:
                kb_updates[attr] = new_entries

        if kb_updates:
            updates["kernel_boot"] = snapshot.kernel_boot.model_copy(update=kb_updates)

    # -----------------------------------------------------------------------
    # 6. NonRpmSoftwareSection — dotenv / secret files under /opt (sorted by path)
    # -----------------------------------------------------------------------
    if snapshot.non_rpm_software and snapshot.non_rpm_software.env_files:
        new_env_files: List[ConfigFileEntry] = []
        changed = False
        sorted_env_files = sorted(snapshot.non_rpm_software.env_files, key=lambda f: f.path)
        for entry in sorted_env_files:
            if _is_excluded_path(entry.path):
                if entry.content != _EXCLUDED_PLACEHOLDER:
                    redactions.append(RedactionFinding(
                        path=entry.path,
                        source="file",
                        kind="excluded",
                        pattern="EXCLUDED_PATH",
                        remediation=_remediation_for_excluded(entry.path),
                        detection_method="excluded_path",
                    ))
                new_env_files.append(entry.model_copy(update={"content": _EXCLUDED_PLACEHOLDER, "include": False}))
                continue
            new_content = _redact_text(entry.content or "", entry.path, redactions, registry=registry)
            if new_content != (entry.content or ""):
                new_env_files.append(entry.model_copy(update={"content": new_content}))
                changed = True
            else:
                new_env_files.append(entry)
        if changed or len(new_env_files) != len(snapshot.non_rpm_software.env_files):
            updates["non_rpm_software"] = snapshot.non_rpm_software.model_copy(
                update={"env_files": new_env_files}
            )

    # -----------------------------------------------------------------------
    # 7. UserGroupSection — sudoers (sorted), shadow (sorted by username),
    #    passwd (sorted by username)
    # -----------------------------------------------------------------------
    if snapshot.users_groups:
        ug = snapshot.users_groups
        ug_updates: dict = {}

        if ug.sudoers_rules:
            new_rules: List[str] = []
            changed = False
            sorted_rules = sorted(ug.sudoers_rules)
            for rule in sorted_rules:
                new_rule = _redact_text(rule, "users:sudoers", redactions, registry=registry)
                new_rules.append(new_rule)
                if new_rule != rule:
                    changed = True
            if changed:
                ug_updates["sudoers_rules"] = new_rules

        if ug.shadow_entries:
            new_shadow: List[str] = []
            changed = False
            sorted_shadow = sorted(ug.shadow_entries, key=lambda e: e.split(":")[0] if ":" in e else e)
            for entry in sorted_shadow:
                new_entry = _redact_shadow_entry(entry, redactions, registry=registry)
                new_shadow.append(new_entry)
                if new_entry != entry:
                    changed = True
            if changed:
                ug_updates["shadow_entries"] = new_shadow

        if ug.passwd_entries:
            new_passwd: List[str] = []
            changed = False
            sorted_passwd = sorted(ug.passwd_entries, key=lambda e: e.split(":")[0] if ":" in e else e)
            for entry in sorted_passwd:
                fields = entry.split(":")
                if len(fields) >= 5:
                    new_gecos = _redact_text(fields[4], f"users:passwd/{fields[0]}:gecos", redactions, registry=registry)
                    if new_gecos != fields[4]:
                        fields[4] = new_gecos
                        new_passwd.append(":".join(fields))
                        changed = True
                        continue
                new_passwd.append(entry)
            if changed:
                ug_updates["passwd_entries"] = new_passwd

        if ug_updates:
            updates["users_groups"] = ug.model_copy(update=ug_updates)

    # -----------------------------------------------------------------------
    # Output-order sort (tokens are already assigned — this only reorders
    # the findings list for consistent rendering)
    # -----------------------------------------------------------------------
    def _redaction_sort_key(r) -> tuple:
        if isinstance(r, dict):
            path = r.get("path", "")
            return (True, "", path, 0)  # legacy dicts sort after file-backed
        is_non_file = r.source != "file"
        if is_non_file:
            return (True, r.source, r.path, 0)
        else:
            return (False, "", r.path, r.line or 0)

    redactions.sort(key=_redaction_sort_key)

    updates["redactions"] = redactions
    return snapshot.model_copy(update=updates)
