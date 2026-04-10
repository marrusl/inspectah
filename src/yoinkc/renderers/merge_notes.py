"""Render merge-notes.md — fleet merge ambiguity drill-down."""

import hashlib
from pathlib import Path
from typing import NamedTuple

from ..schema import InspectionSnapshot


class _VariantInfo(NamedTuple):
    path: str
    item_type: str
    fleet_count: int
    fleet_total: int
    content_hash: str
    is_winner: bool
    is_tie: bool


def _collect_variant_items(snapshot: InspectionSnapshot) -> list[_VariantInfo]:
    """Collect all non-unanimous variant items across all five item types."""
    items: list[_VariantInfo] = []

    def _add_items(item_list, item_type: str, content_fn):
        if not item_list:
            return
        groups: dict[str, list] = {}
        for item in item_list:
            groups.setdefault(item.path, []).append(item)
        for path, variants in groups.items():
            if not any(v.fleet for v in variants):
                continue
            total = variants[0].fleet.total if variants[0].fleet else 0
            if total == 0:
                continue
            if len(variants) == 1 and variants[0].fleet.count == total:
                continue
            for v in variants:
                c_hash = hashlib.sha256(
                    content_fn(v).encode()
                ).hexdigest()  # Full 64-char SHA-256
                items.append(_VariantInfo(
                    path=v.path,
                    item_type=item_type,
                    fleet_count=v.fleet.count if v.fleet else 0,
                    fleet_total=total,
                    content_hash=c_hash,
                    is_winner=v.include,
                    is_tie=getattr(v, "tie", False),
                ))

    if snapshot.config and snapshot.config.files:
        _add_items(snapshot.config.files, "config", lambda v: v.content)
    if snapshot.services and snapshot.services.drop_ins:
        _add_items(snapshot.services.drop_ins, "drop-in", lambda v: v.content)
    if snapshot.containers:
        if snapshot.containers.quadlet_units:
            _add_items(snapshot.containers.quadlet_units, "quadlet", lambda v: v.content)
        if snapshot.containers.compose_files:
            _add_items(
                snapshot.containers.compose_files, "compose",
                lambda v: str(sorted((img.service, img.image) for img in v.images)),
            )
    if snapshot.non_rpm_software and snapshot.non_rpm_software.env_files:
        _add_items(snapshot.non_rpm_software.env_files, "env", lambda v: v.content)

    return items


def render_merge_notes(snapshot: InspectionSnapshot, output_dir: Path) -> None:
    """Write merge-notes.md if there are non-unanimous or tied items."""
    items = _collect_variant_items(snapshot)
    if not items:
        return

    lines = [
        "# Fleet Merge Notes",
        "",
        "This file documents fleet merge decisions where hosts disagreed on file content.",
        "Review these items to verify the auto-selected variant is correct for your target image.",
        "",
    ]

    tied: dict[str, list[_VariantInfo]] = {}
    non_unanimous: dict[str, list[_VariantInfo]] = {}

    for item in items:
        if item.is_tie:
            tied.setdefault(item.path, []).append(item)
        else:
            non_unanimous.setdefault(item.path, []).append(item)

    if tied:
        lines.append("## Tied Items (auto-resolved by content hash)")
        lines.append("")
        for path, variants in sorted(tied.items()):
            item_type = variants[0].item_type
            total = variants[0].fleet_total
            winner = next((v for v in variants if v.is_winner), None)
            lines.append(f"### `{path}` ({item_type})")
            lines.append("")
            lines.append(f"- **Variants:** {len(variants)}")
            lines.append(f"- **Fleet total:** {total} hosts")
            if winner:
                lines.append(f"- **Auto-selected:** hash `{winner.content_hash}` ({winner.fleet_count}/{total} hosts)")
            lines.append("")
            lines.append("| Variant hash | Hosts | Selected |")
            lines.append("|---|---|---|")
            for v in sorted(variants, key=lambda x: x.content_hash):
                selected = "**winner**" if v.is_winner else "—"
                lines.append(f"| `{v.content_hash}` | {v.fleet_count}/{v.fleet_total} | {selected} |")
            lines.append("")

    if non_unanimous:
        lines.append("## Non-Unanimous Items")
        lines.append("")
        lines.append("These items have a clear winner but were not present on all hosts.")
        lines.append("")
        for path, variants in sorted(non_unanimous.items()):
            item_type = variants[0].item_type
            total = variants[0].fleet_total
            winner = next((v for v in variants if v.is_winner), None)
            if winner:
                lines.append(f"- `{path}` ({item_type}): winner at {winner.fleet_count}/{total} hosts, "
                             f"{len(variants)} variant(s)")
            else:
                lines.append(f"- `{path}` ({item_type}): {len(variants)} variant(s), no winner selected")

    output_dir.mkdir(parents=True, exist_ok=True)
    (output_dir / "merge-notes.md").write_text("\n".join(lines) + "\n")
