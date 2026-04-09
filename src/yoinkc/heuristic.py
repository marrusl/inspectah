"""
Heuristic secret detection engine.

Runs AFTER pattern-based redaction (redact.py). Evaluates content that
survived the pattern pass using Shannon entropy analysis, keyword proximity,
vendor prefix residual matching, and false positive filters.

Produces high/low confidence candidates for review.
"""

import math
import re
from dataclasses import dataclass, field
from typing import Optional

# ---------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------

_MIN_VALUE_LENGTH = 16
_MAX_VALUE_LENGTH = 512

# Entropy thresholds per charset class (bits per character)
_ENTROPY_THRESHOLD_MIXED = 4.5
_ENTROPY_THRESHOLD_HEX = 3.8
_ENTROPY_THRESHOLD_BASE64 = 4.2

MAX_FINDINGS_PER_FILE = 10
MAX_FINDINGS_PER_RUN = 100

# ---------------------------------------------------------------------------
# Keyword set
# ---------------------------------------------------------------------------

_SECRET_KEYWORDS: frozenset[str] = frozenset({
    "password", "passwd", "pass", "passphrase",
    "secret", "token",
    "api_key", "apikey", "api-key",
    "credential", "credentials",
    "auth", "authorization",
    "private_key", "private-key", "privatekey",
    "access_key", "access-key", "accesskey",
    "secret_key", "secret-key", "secretkey",
    "auth_token", "auth-token", "authtoken",
    "client_secret", "client-secret",
    "signing_key", "signing-key",
    "encryption_key", "encryption-key",
    "master_key", "master-key",
    "db_password", "db-password",
    "database_password",
    "connection_string", "conn_string",
})

# ---------------------------------------------------------------------------
# False positive values
# ---------------------------------------------------------------------------

_HEURISTIC_FALSE_POSITIVE_VALUES: frozenset[str] = frozenset({
    "true", "false", "yes", "no", "none", "null",
    "disabled", "enabled", "on", "off",
    "default", "required", "optional",
})

# ---------------------------------------------------------------------------
# Regex constants
# ---------------------------------------------------------------------------

_UUID_RE = re.compile(
    r"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$"
)

# Hex checksums: exactly 32, 40, or 64 hex chars, uniform case (no mixed)
_HEX_CHECKSUM_RE = re.compile(
    r"^(?:[0-9a-f]{32}|[0-9a-f]{40}|[0-9a-f]{64}|[0-9A-F]{32}|[0-9A-F]{40}|[0-9A-F]{64})$"
)

# Key-value assignment: key = value  OR  key: value
_KV_RE = re.compile(
    r"""
    (?:^|[\s;])                          # start of line or separator
    ([\w.\-]+)                           # key (captured)
    \s*[=:]\s*                           # assignment operator
    ["\']?                               # optional opening quote
    ([^\s"\'#;]+(?:\s+[^\s"\'#;]+)*)     # value (captured)
    ["\']?                               # optional closing quote
    """,
    re.VERBOSE,
)

# Vendor prefix residual: short alpha prefix + underscore + long alphanumeric
_VENDOR_PREFIX_RESIDUAL_RE = re.compile(
    r"^[a-zA-Z]{2,8}_[a-zA-Z0-9]{20,}$"
)

# Already-redacted placeholders
_REDACTED_RE = re.compile(
    r"REDACTED_\w+_\d+|<REDACTED>|\*{3,}|x{8,}"
)


# ---------------------------------------------------------------------------
# Data structures
# ---------------------------------------------------------------------------

@dataclass
class HeuristicCandidate:
    """A candidate secret found by heuristic analysis."""
    path: str
    source: str
    line_number: Optional[int]
    value: str
    confidence: str          # "high" or "low"
    why_flagged: str         # human-readable reason
    key_name: Optional[str] = None
    signals: list[str] = field(default_factory=list)


@dataclass
class NoiseControlResult:
    """Result of applying noise control to a list of candidates.

    ``reported`` is the capped/deduped subset for advisory output.
    ``all_candidates`` is the full deduped list — use this for
    redaction and push-block evaluation (caps limit reporting only).
    """
    reported: list[HeuristicCandidate]
    all_candidates: list[HeuristicCandidate]
    suppressed_per_file: dict[str, int]
    suppressed_total: int
    dedup_counts: dict[str, int]
    graduation_candidates: dict[str, int]


# ---------------------------------------------------------------------------
# Shannon entropy
# ---------------------------------------------------------------------------

def shannon_entropy(s: str) -> float:
    """Compute Shannon entropy in bits per character."""
    if not s:
        return 0.0
    length = len(s)
    freq: dict[str, int] = {}
    for ch in s:
        freq[ch] = freq.get(ch, 0) + 1
    entropy = 0.0
    for count in freq.values():
        p = count / length
        entropy -= p * math.log2(p)
    return entropy


# ---------------------------------------------------------------------------
# Keyword detection
# ---------------------------------------------------------------------------

def is_secret_keyword(key: str) -> bool:
    """Check if a key name matches a known secret keyword (case-insensitive)."""
    normalised = key.lower().strip()
    return normalised in _SECRET_KEYWORDS


# ---------------------------------------------------------------------------
# Internal helpers
# ---------------------------------------------------------------------------

def _classify_charset(value: str) -> str:
    """Classify a value's character set as 'hex', 'base64', or 'mixed'."""
    if re.fullmatch(r"[0-9a-fA-F]+", value):
        return "hex"
    if re.fullmatch(r"[A-Za-z0-9+/=]+", value):
        return "base64"
    return "mixed"


def _entropy_threshold(charset: str) -> float:
    """Return the entropy threshold for a given charset class."""
    if charset == "hex":
        return _ENTROPY_THRESHOLD_HEX
    if charset == "base64":
        return _ENTROPY_THRESHOLD_BASE64
    return _ENTROPY_THRESHOLD_MIXED


def _is_false_positive_value(value: str) -> bool:
    """Check if a value is a likely false positive."""
    # Too short
    if len(value) < _MIN_VALUE_LENGTH:
        return True
    # Too long
    if len(value) > _MAX_VALUE_LENGTH:
        return True
    # Boolean/keyword constants
    if value.lower() in _HEURISTIC_FALSE_POSITIVE_VALUES:
        return True
    # Pure numeric
    stripped = value.lstrip("-+")
    if stripped.replace(".", "", 1).isdigit():
        return True
    # UUID
    if _UUID_RE.match(value):
        return True
    # Hex checksum (uniform case)
    if _HEX_CHECKSUM_RE.match(value):
        return True
    # Already redacted
    if _REDACTED_RE.search(value):
        return True
    return False


def _is_comment_line(line: str) -> bool:
    """Check if a line is a comment."""
    stripped = line.lstrip()
    return stripped.startswith("#") or stripped.startswith("//") or stripped.startswith(";") or stripped.startswith("!")


def _score_candidate(
    key: Optional[str], value: str
) -> Optional[tuple[str, str, list[str]]]:
    """
    Score a candidate value and return (confidence, why_flagged, signals).

    Returns None if no signals warrant flagging.

    Signal strength:
    - Strong: keyword proximity, high entropy, vendor prefix residual
    - Weak: value length 20-128, assignment context

    Confidence rules:
    - high: 2+ strong signals, OR 1 strong + any weak corroboration
    - low: 1 strong signal alone
    - None: weak-only or no signals
    """
    strong_signals: list[str] = []
    weak_signals: list[str] = []

    # --- Strong signals ---

    # Keyword proximity
    if key and is_secret_keyword(key):
        strong_signals.append(f"keyword:{key}")

    # Shannon entropy
    charset = _classify_charset(value)
    threshold = _entropy_threshold(charset)
    ent = shannon_entropy(value)
    if ent >= threshold:
        strong_signals.append(f"entropy:{ent:.2f}({charset}>={threshold})")

    # Vendor prefix residual
    if _VENDOR_PREFIX_RESIDUAL_RE.match(value):
        strong_signals.append("vendor_prefix_residual")

    # --- Weak signals ---

    # Value length
    vlen = len(value)
    if 20 <= vlen <= 128:
        weak_signals.append(f"length:{vlen}")

    # Assignment context (always true when we get here via KV regex)
    if key is not None:
        weak_signals.append("assignment_context")

    # --- Confidence decision ---
    all_signals = strong_signals + weak_signals

    if not strong_signals:
        return None  # weak-only = no finding

    if len(strong_signals) >= 2:
        confidence = "high"
    elif weak_signals:
        confidence = "high"
    else:
        confidence = "low"

    # Build why_flagged
    why_parts = []
    if any(s.startswith("keyword:") for s in strong_signals):
        why_parts.append("secret keyword in key name")
    if any(s.startswith("entropy:") for s in strong_signals):
        why_parts.append(f"high entropy ({charset})")
    if "vendor_prefix_residual" in strong_signals:
        why_parts.append("vendor prefix residual pattern")
    why_flagged = "; ".join(why_parts)

    return confidence, why_flagged, all_signals


# ---------------------------------------------------------------------------
# Main entry point
# ---------------------------------------------------------------------------

def find_heuristic_candidates(
    lines: list[str],
    path: str,
    source: str = "file",
) -> list[HeuristicCandidate]:
    """
    Scan lines for heuristic secret candidates.

    Returns a list of HeuristicCandidate objects.  Per-file and per-run
    caps are applied externally by ``apply_noise_control()``.
    """
    candidates: list[HeuristicCandidate] = []

    for line_idx, line in enumerate(lines):
        if _is_comment_line(line):
            continue

        # Find key-value pairs
        for match in _KV_RE.finditer(line):
            key = match.group(1)
            value = match.group(2).strip().strip("'\"")

            # Filter false positives
            if _is_false_positive_value(value):
                continue

            # Score
            result = _score_candidate(key, value)
            if result is None:
                continue

            confidence, why_flagged, signals = result

            candidates.append(HeuristicCandidate(
                path=path,
                source=source,
                line_number=line_idx + 1 if source == "file" else None,
                value=value,
                confidence=confidence,
                why_flagged=why_flagged,
                key_name=key,
                signals=signals,
            ))

    return candidates


# ---------------------------------------------------------------------------
# Noise control — dedup, caps, graduation
# ---------------------------------------------------------------------------

def apply_noise_control(
    candidates: list[HeuristicCandidate],
) -> NoiseControlResult:
    """Apply dedup, per-file caps, per-run caps, and residual graduation.

    Order: (1) dedup identical values, (2) per-file cap, (3) per-run cap.
    Sort order: file-backed by path/line first, then non-file-backed by
    source/path.

    IMPORTANT: Caps limit *reporting* only — not redaction or push-block
    evaluation.  The ``all_candidates`` field on the result contains every
    candidate (post-dedup) so callers can still redact and evaluate
    push-block decisions on the full set.
    """
    # Sort by standard finding order
    def _sort_key(c: HeuristicCandidate) -> tuple:
        is_non_file = c.source != "file"
        return (is_non_file, c.source if is_non_file else "", c.path, c.line_number or 0)

    sorted_candidates = sorted(candidates, key=_sort_key)

    # (1) Dedup: collapse identical values, keep primary (first by sort)
    dedup_counts: dict[str, int] = {}
    seen_values: dict[str, HeuristicCandidate] = {}
    deduped: list[HeuristicCandidate] = []
    for c in sorted_candidates:
        if c.value in seen_values:
            dedup_counts[c.value] = dedup_counts.get(c.value, 1) + 1
        else:
            seen_values[c.value] = c
            deduped.append(c)
            dedup_counts[c.value] = 1

    # (2) Per-file cap
    suppressed_per_file: dict[str, int] = {}
    file_counts: dict[str, int] = {}
    file_capped: list[HeuristicCandidate] = []
    for c in deduped:
        count = file_counts.get(c.path, 0)
        if count < MAX_FINDINGS_PER_FILE:
            file_capped.append(c)
            file_counts[c.path] = count + 1
        else:
            suppressed_per_file[c.path] = suppressed_per_file.get(c.path, 0) + 1

    # (3) Per-run cap
    suppressed_total = max(0, len(file_capped) - MAX_FINDINGS_PER_RUN)
    reported = file_capped[:MAX_FINDINGS_PER_RUN]

    # (4) Residual prefix graduation — uses ALL candidates (pre-dedup)
    graduation_candidates: dict[str, int] = {}
    for c in candidates:
        if any("vendor_prefix_residual" in s for s in c.signals):
            idx = c.value.find("_")
            if idx > 0:
                prefix = c.value[: idx + 1]
                graduation_candidates[prefix] = graduation_candidates.get(prefix, 0) + 1
    graduation_candidates = {k: v for k, v in graduation_candidates.items() if v >= 3}

    return NoiseControlResult(
        reported=reported,
        all_candidates=deduped,
        suppressed_per_file=suppressed_per_file,
        suppressed_total=suppressed_total,
        dedup_counts=dedup_counts,
        graduation_candidates=graduation_candidates,
    )
