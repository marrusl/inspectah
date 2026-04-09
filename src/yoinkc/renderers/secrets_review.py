"""secrets-review.md renderer: list of redacted items and remediation."""

from pathlib import Path

from jinja2 import Environment

from ..schema import InspectionSnapshot, RedactionFinding


_REMEDIATION_LABELS = {
    "regenerate": "Regenerate on target",
    "provision": "Provision from secret store",
    "value-removed": "Supply value at deploy time",
}


def render(
    snapshot: InspectionSnapshot,
    env: Environment,
    output_dir: Path,
) -> None:
    output_dir = Path(output_dir)
    path = output_dir / "secrets-review.md"

    if not snapshot.redactions:
        path.write_text("# Secrets Review\n\nNo redactions recorded.\n")
        return

    lines = [
        "# Secrets Review",
        "",
        "The following items were redacted or excluded. Handle them according to",
        "the action specified for each item.",
        "",
    ]

    # Separate typed findings from legacy dicts
    typed = [r for r in snapshot.redactions if isinstance(r, RedactionFinding)]
    legacy = [r for r in snapshot.redactions if not isinstance(r, RedactionFinding)]

    excluded = [r for r in typed if r.kind == "excluded"]
    inline = [r for r in typed if r.kind == "inline"]

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
        lines.append("| Path | Line | Type | Placeholder | Action |")
        lines.append("|------|------|------|-------------|--------|")
        for f in inline:
            line_str = str(f.line) if f.line is not None else "\u2014"
            replacement = f.replacement or "\u2014"
            action = _REMEDIATION_LABELS.get(f.remediation, f.remediation)
            lines.append(f"| {f.path} | {line_str} | {f.pattern} | {replacement} | {action} |")
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
