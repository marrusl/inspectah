"""secrets-review.md renderer: list of redacted items and remediation."""

from pathlib import Path

from jinja2 import Environment

from ..schema import InspectionSnapshot, RedactionFinding


_REMEDIATION_LABELS = {
    "regenerate": "Regenerate on target",
    "provision": "Provision from secret store",
    "value-removed": "Supply value at deploy time",
}


def _detection_label(finding: RedactionFinding) -> str:
    """Human-readable detection method label for table cells."""
    method = finding.detection_method or "pattern"
    if method == "pattern":
        return "pattern"
    if method == "heuristic":
        conf = finding.confidence or "unknown"
        return f"heuristic ({conf})"
    return method


def render(
    snapshot: InspectionSnapshot,
    env: Environment,
    output_dir: Path,
    no_redaction: bool = False,
) -> None:
    output_dir = Path(output_dir)
    path = output_dir / "secrets-review.md"

    # Fallback: check snapshot meta for no_redaction flag
    if not no_redaction:
        no_redaction = bool(snapshot.meta.get("_no_redaction", False))

    if not snapshot.redactions:
        path.write_text("# Secrets Review\n\nNo redactions recorded.\n")
        return

    lines = [
        "# Secrets Review",
        "",
    ]

    # Separate typed findings from legacy dicts
    typed = [r for r in snapshot.redactions if isinstance(r, RedactionFinding)]
    legacy = [r for r in snapshot.redactions if not isinstance(r, RedactionFinding)]

    excluded = [r for r in typed if r.kind == "excluded"]
    inline = [r for r in typed if r.kind == "inline"]
    flagged = [r for r in typed if r.kind == "flagged"]

    # Summary line
    inline_pattern = [f for f in inline if (f.detection_method or "pattern") == "pattern"]
    inline_heuristic = [f for f in inline if f.detection_method == "heuristic"]
    n_redacted = len(inline)
    parts = []
    if inline_pattern:
        parts.append(f"{len(inline_pattern)} pattern")
    if inline_heuristic:
        parts.append(f"{len(inline_heuristic)} heuristic")
    breakdown = f" ({', '.join(parts)})" if parts else ""
    flagged_part = f", {len(flagged)} flagged for review" if flagged else ""
    lines.append(f"> Detected secrets: {n_redacted} redacted{breakdown}{flagged_part}")
    lines.append("")

    # No-redaction warning
    if no_redaction:
        lines.append("> WARNING: Redaction was disabled for this run. Output may contain live secrets.")
        lines.append("")

    lines.append("The following items were redacted or excluded. Handle them according to")
    lines.append("the action specified for each item.")
    lines.append("")

    if excluded:
        lines.append("## Excluded Files")
        lines.append("")
        lines.append("| Path | Action | Reason |")
        lines.append("|------|--------|--------|")
        for f in excluded:
            action = _REMEDIATION_LABELS.get(f.remediation, f.remediation)
            lines.append(f"| {f.path} | {action} | {f.pattern} |")
        lines.append("")

    if inline:
        lines.append("## Inline Redactions")
        lines.append("")
        lines.append("| Path | Line | Type | Detection | Placeholder | Action |")
        lines.append("|------|------|------|-----------|-------------|--------|")
        for f in inline:
            line_str = str(f.line) if f.line is not None else "\u2014"
            replacement = f.replacement or "\u2014"
            action = _REMEDIATION_LABELS.get(f.remediation, f.remediation)
            detection = _detection_label(f)
            lines.append(f"| {f.path} | {line_str} | {f.pattern} | {detection} | {replacement} | {action} |")
        lines.append("")

    if flagged:
        lines.append("## Flagged for Review")
        lines.append("")
        lines.append("| Path | Line | Confidence | Why Flagged |")
        lines.append("|------|------|------------|-------------|")
        for f in flagged:
            line_str = str(f.line) if f.line is not None else "\u2014"
            confidence = f.confidence or "\u2014"
            why = f.pattern or "\u2014"
            lines.append(f"| {f.path} | {line_str} | {confidence} | {why} |")
        lines.append("")

    # Legacy dict entries (from older snapshots or fleet merge of old data)
    if legacy:
        lines.append("## Other Redactions")
        lines.append("")
        lines.append("| Path | Pattern | Line | Remediation |")
        lines.append("|------|---------|------|-------------|")
        for r in legacy:
            rpath = str(r.get("path") or "").replace("|", "\\|")
            pattern = str(r.get("pattern") or "").replace("|", "\\|")
            line = str(r.get("line") or "").replace("|", "\\|")
            rem = str(r.get("remediation") or "").replace("|", "\\|")
            lines.append(f"| {rpath} | {pattern} | {line} | {rem} |")
        lines.append("")

    path.write_text("\n".join(lines))
