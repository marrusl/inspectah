"""HTML report renderer.

Builds a context dict from the snapshot and delegates to
templates/report.html.j2 via Jinja2.  A few helpers produce pre-rendered
Markup for the file-browser tree and audit report.
"""

import logging
import re
from collections import OrderedDict
from pathlib import Path
from typing import List, Optional

from jinja2 import Environment
from markupsafe import Markup

from ..schema import ConfigFileKind, FleetMeta, InspectionSnapshot
from .._util import make_warning
from ._triage import _QUADLET_PREFIX, _config_file_count

# Max size per file to embed in the report (bytes); larger files show a truncation note
_MAX_FILE_CONTENT = 100 * 1024


def _fleet_color(fleet) -> str:
    """Jinja2 filter: return PF6 color class based on fleet prevalence."""
    if not fleet or fleet.total == 0:
        return "pf-m-blue"
    pct = fleet.count * 100 // fleet.total
    if pct >= 100:
        return "pf-m-blue"
    elif pct >= 50:
        return "pf-m-gold"
    else:
        return "pf-m-red"


# ---------------------------------------------------------------------------
# File browser helpers  (produce pre-rendered Markup)
# ---------------------------------------------------------------------------

def _build_output_tree(output_dir: Path) -> List[dict]:
    """Build a tree of config/ and quadlet/ for the file browser."""
    roots: List[dict] = []
    for folder_name in ("config", "quadlet"):
        folder = output_dir / folder_name
        if not folder.is_dir():
            continue
        try:
            children = _walk_dir(folder, folder_name, folder_name)
            if children:
                roots.append({"type": "dir", "name": folder_name, "children": children})
        except Exception:
            continue
    return roots


def _walk_dir(path: Path, prefix: str, rel_path: str) -> List[dict]:
    """Recursively walk a directory; return list of tree nodes."""
    out: List[dict] = []
    try:
        for p in sorted(path.iterdir(), key=lambda x: (not x.is_dir(), x.name.lower())):
            child_rel = f"{rel_path}/{p.name}"
            if p.is_file():
                content = ""
                content_id = "file-" + child_rel.replace("/", "-").replace(".", "-").replace(" ", "-")
                try:
                    raw = p.read_bytes()
                    if b"\x00" in raw[:8192]:
                        content = "(binary file)"
                    else:
                        text = raw.decode("utf-8", errors="replace")
                        if len(text) > _MAX_FILE_CONTENT:
                            text = text[:_MAX_FILE_CONTENT] + "\n\n... (truncated)"
                        content = text.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")
                except Exception:
                    content = "(could not read)"
                out.append({"type": "file", "name": p.name, "rel_path": child_rel,
                             "content": content, "content_id": content_id})
            else:
                sub = _walk_dir(p, prefix, child_rel)
                out.append({"type": "dir", "name": p.name, "children": sub})
    except Exception:
        pass
    return out


_TOGGLE_ICON_SVG = (
    '<span class="pf-v6-c-tree-view__node-toggle-icon">'
    '<svg aria-hidden="true" fill="currentColor" height="1em" width="1em" viewBox="0 0 256 512">'
    '<path d="M224.3 273l-136 136c-9.4 9.4-24.6 9.4-33.9 0l-22.6-22.6c-9.4-9.4-9.4-24.6 0-33.9l96.4-96.4-96.4-96.4c-9.4-9.4-9.4-24.6 0-33.9L54.3 103c9.4-9.4 24.6-9.4 33.9 0l136 136c9.5 9.4 9.5 24.6.1 34z"/>'
    '</svg></span>'
)

_FOLDER_ICON_SVG = (
    '<svg aria-hidden="true" viewBox="0 0 16 16" width="1em" height="1em" fill="currentColor">'
    '<path d="M1 3.5A1.5 1.5 0 0 1 2.5 2h3.879a1.5 1.5 0 0 1 1.06.44l1.122 1.12A1.5 1.5 0 0 0 9.62 4H13.5A1.5 1.5 0 0 1 15 5.5v7a1.5 1.5 0 0 1-1.5 1.5h-11A1.5 1.5 0 0 1 1 12.5v-9z"/>'
    '</svg>'
)

_FILE_ICON_SVG = (
    '<svg aria-hidden="true" viewBox="0 0 16 16" width="1em" height="1em" fill="currentColor">'
    '<path d="M4 0a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h8a2 2 0 0 0 2-2V4.707A1 1 0 0 0 13.707 4L10 .293A1 1 0 0 0 9.293 0H4zm5.5 1.5v2a1 1 0 0 0 1 1h2z"/>'
    '</svg>'
)


def _render_tree_items(nodes: List[dict], content_snippets: List[str]) -> str:
    """Render tree nodes as PF6 tree-view list items."""
    parts: List[str] = []
    for n in nodes:
        name_esc = (n.get("name", "")).replace("&", "&amp;").replace("<", "&lt;")
        if n.get("type") == "dir":
            ch_html = _render_tree_items(n.get("children", []), content_snippets)
            parts.append(
                '<li class="pf-v6-c-tree-view__list-item pf-m-expanded" role="treeitem" aria-expanded="true">'
                '<div class="pf-v6-c-tree-view__content">'
                '<button class="pf-v6-c-tree-view__node" type="button">'
                '<div class="pf-v6-c-tree-view__node-container">'
                f'<span class="pf-v6-c-tree-view__node-toggle">{_TOGGLE_ICON_SVG}</span>'
                f'<span class="pf-v6-c-tree-view__node-icon">{_FOLDER_ICON_SVG}</span>'
                f'<span class="pf-v6-c-tree-view__node-text">{name_esc}</span>'
                '</div></button></div>'
                f'<ul class="pf-v6-c-tree-view__list" role="group">{ch_html}</ul>'
                '</li>'
            )
        else:
            cid = n.get("content_id", "")
            content = n.get("content", "")
            path_attr = (n.get("rel_path", "")).replace('"', "&quot;")
            content_snippets.append(
                f'<div id="{cid}" class="file-content-hidden">{content}</div>'
            )
            parts.append(
                '<li class="pf-v6-c-tree-view__list-item" role="treeitem">'
                '<div class="pf-v6-c-tree-view__content">'
                f'<button class="pf-v6-c-tree-view__node file-entry" type="button"'
                f' data-content-id="{cid}" data-path="{path_attr}">'
                '<div class="pf-v6-c-tree-view__node-container">'
                f'<span class="pf-v6-c-tree-view__node-icon">{_FILE_ICON_SVG}</span>'
                f'<span class="pf-v6-c-tree-view__node-text">{name_esc}</span>'
                '</div></button></div></li>'
            )
    return "\n".join(parts)


def _render_tree_html(nodes: List[dict], content_snippets: List[str]) -> str:
    """Render tree nodes as a complete PF6 tree-view component."""
    items = _render_tree_items(nodes, content_snippets)
    return (
        '<div class="pf-v6-c-tree-view pf-m-guides">'
        f'<ul class="pf-v6-c-tree-view__list" role="tree" aria-label="File browser">{items}</ul>'
        '</div>'
    )


# ---------------------------------------------------------------------------
# Markdown → HTML  (for the audit report section)
# ---------------------------------------------------------------------------

def _markdown_to_html(md: str) -> str:
    """Convert basic markdown to HTML for display in the report."""
    if not md or not md.strip():
        return "<p>(empty)</p>"
    lines = md.replace("\r\n", "\n").split("\n")
    out: List[str] = []
    in_pre = False
    pre_content: List[str] = []
    in_list = False
    in_table = False
    table_rows: List[str] = []

    def flush_pre():
        nonlocal in_pre, pre_content
        if in_pre and pre_content:
            content = "\n".join(pre_content).replace("<", "&lt;").replace(">", "&gt;")
            out.append(f"<pre class=\"audit-pre\">{content}</pre>")
        in_pre = False
        pre_content = []

    def flush_list():
        nonlocal in_list
        if in_list:
            out.append("</ul>")
        in_list = False

    def flush_table():
        nonlocal in_table, table_rows
        if in_table and table_rows:
            out.append("<table class=\"audit-table\">")
            for i, row in enumerate(table_rows):
                raw = [c.strip() for c in row.split("|")]
                # "| a | b |" splits to ['', 'a', 'b', ''] — drop boundary empties
                if raw and raw[0] == "":
                    raw = raw[1:]
                if raw and raw[-1] == "":
                    raw = raw[:-1]
                cells = [c for c in raw if c]
                if not cells:
                    continue
                if i == 1 and all(re.match(r"^[-:\s]+$", c) for c in cells):
                    continue
                tag = "th" if i == 0 else "td"
                out.append("<tr>" + "".join(f"<{tag}>{_escape_md_cell(c)}</{tag}>" for c in cells) + "</tr>")
            out.append("</table>")
        in_table = False
        table_rows = []

    def _escape_md_cell(s: str) -> str:
        s = s.replace("<", "&lt;").replace(">", "&gt;")
        while "**" in s:
            s = s.replace("**", "<strong>", 1).replace("**", "</strong>", 1)
        while "`" in s:
            i = s.index("`")
            j = s.index("`", i + 1) if "`" in s[i + 1:] else -1
            if j > i:
                s = s[:i] + "<code>" + s[i + 1:j] + "</code>" + s[j + 1:]
            else:
                break
        return s

    i = 0
    while i < len(lines):
        line = lines[i]
        if line.strip().startswith("```"):
            flush_list()
            flush_table()
            if in_pre:
                flush_pre()
            else:
                in_pre = True
                pre_content = []
            i += 1
            continue
        if in_pre:
            if line.strip().startswith("```"):
                flush_pre()
            else:
                pre_content.append(line)
            i += 1
            continue
        if line.strip().startswith("|") and "|" in line[1:]:
            flush_list()
            if not in_table:
                in_table = True
                table_rows = []
            table_rows.append(line)
            i += 1
            continue
        else:
            flush_table()
        if line.startswith("#### "):
            flush_list()
            out.append("<h4>" + _escape_md_cell(line[5:]) + "</h4>")
        elif line.startswith("### "):
            flush_list()
            out.append("<h3>" + _escape_md_cell(line[4:]) + "</h3>")
        elif line.startswith("## "):
            flush_list()
            out.append("<h2>" + _escape_md_cell(line[3:]) + "</h2>")
        elif line.startswith("# "):
            flush_list()
            out.append("<h1>" + _escape_md_cell(line[2:]) + "</h1>")
        elif line.strip().startswith("- "):
            if not in_list:
                out.append("<ul>")
                in_list = True
            content = line.strip()[2:]
            out.append("<li>" + _escape_md_cell(content) + "</li>")
        elif line.strip() == "":
            flush_list()
            out.append("<p></p>")
        else:
            flush_list()
            out.append("<p>" + _escape_md_cell(line) + "</p>")
        i += 1
    flush_pre()
    flush_list()
    flush_table()
    return "\n".join(out)


# ---------------------------------------------------------------------------
# Summary counts
# ---------------------------------------------------------------------------

def _summary_counts(snapshot: InspectionSnapshot) -> dict:
    n_network = 0
    if snapshot.network:
        n_network = (len(snapshot.network.connections or [])
                     + len(snapshot.network.firewall_zones or [])
                     + len(snapshot.network.firewall_direct_rules or [])
                     + len(snapshot.network.ip_rules or []))
    n_storage = len(snapshot.storage.fstab_entries or []) if snapshot.storage else 0
    n_scheduled = (len(snapshot.scheduled_tasks.cron_jobs or [])
                   + len(snapshot.scheduled_tasks.systemd_timers or [])
                   + len(snapshot.scheduled_tasks.at_jobs or [])
                   + len(snapshot.scheduled_tasks.generated_timer_units or [])
                   ) if snapshot.scheduled_tasks else 0
    n_containers = 0
    if snapshot.containers:
        n_containers = (len(snapshot.containers.quadlet_units or [])
                        + len(snapshot.containers.compose_files or [])
                        + len(snapshot.containers.running_containers or []))
    n_non_rpm = len(snapshot.non_rpm_software.items or []) if snapshot.non_rpm_software else 0
    n_users = (len(snapshot.users_groups.users or []) + len(snapshot.users_groups.groups or [])
               ) if snapshot.users_groups else 0
    n_kernel = 0
    if snapshot.kernel_boot:
        n_kernel = ((1 if snapshot.kernel_boot.cmdline else 0)
                    + len(snapshot.kernel_boot.sysctl_overrides or [])
                    + len(snapshot.kernel_boot.non_default_modules or [])
                    + len(snapshot.kernel_boot.modules_load_d or [])
                    + len(snapshot.kernel_boot.modprobe_d or [])
                    + len(snapshot.kernel_boot.dracut_conf or []))
    n_selinux = 0
    if snapshot.selinux:
        n_selinux = (len(snapshot.selinux.custom_modules or [])
                     + len(snapshot.selinux.boolean_overrides or [])
                     + len(snapshot.selinux.audit_rules or [])
                     + len(snapshot.selinux.port_labels or [])
                     + (1 if snapshot.selinux.fips_mode else 0))
    return {
        "packages_added": len(snapshot.rpm.packages_added or []) if snapshot.rpm else 0,
        "base_image_only": len(snapshot.rpm.base_image_only or []) if snapshot.rpm else 0,
        "rpm_va": len(snapshot.rpm.rpm_va or []) if snapshot.rpm else 0,
        "config_files": _config_file_count(snapshot),
        "services_enabled": len(snapshot.services.enabled_units or []) if snapshot.services else 0,
        "services_disabled": len(snapshot.services.disabled_units or []) if snapshot.services else 0,
        "redactions": len(snapshot.redactions or []),
        "warnings": len(snapshot.warnings or []),
        "network": n_network,
        "storage": n_storage,
        "scheduled_tasks": n_scheduled,
        "containers": n_containers,
        "non_rpm": n_non_rpm,
        "users_groups": n_users,
        "kernel_boot": n_kernel,
        "selinux": n_selinux,
    }


# ---------------------------------------------------------------------------
# Pre-computed diff HTML for config files
# ---------------------------------------------------------------------------

def _render_diff_html(diff_text: str) -> str:
    """Produce colored diff HTML from a unified diff string."""
    if not diff_text:
        return ""
    diff_lines = diff_text.splitlines()[:80]
    colored = []
    for dl in diff_lines:
        escaped_line = dl.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")
        if dl.startswith("+++") or dl.startswith("---"):
            colored.append(f'<span class="diff-hdr">{escaped_line}</span>')
        elif dl.startswith("@@"):
            colored.append(f'<span class="diff-hunk">{escaped_line}</span>')
        elif dl.startswith("+"):
            colored.append(f'<span class="diff-add">{escaped_line}</span>')
        elif dl.startswith("-"):
            colored.append(f'<span class="diff-del">{escaped_line}</span>')
        else:
            colored.append(escaped_line)
    total_lines = len(diff_text.splitlines())
    if total_lines > 80:
        colored.append(f'<span class="diff-hdr">... {total_lines - 80} more lines</span>')
    return '<pre class="diff-view">' + "\n".join(colored) + "</pre>"


def _prepare_config_files(snapshot: InspectionSnapshot) -> List[dict]:
    """Pre-process config file entries with pre-rendered diff HTML."""
    if not snapshot.config or not snapshot.config.files:
        return []
    result = []
    for idx, f in enumerate(snapshot.config.files):
        if f.path.lstrip("/").startswith(_QUADLET_PREFIX):
            continue
        result.append({
            "path": f.path,
            "kind": f.kind.value,
            "flags": f.rpm_va_flags or "",
            "diff_html": _render_diff_html(f.diff_against_rpm or ""),
            "snap_index": idx,
            "include": f.include,
            "fleet": f.fleet,
        })
    return result


# ---------------------------------------------------------------------------
# Pre-computed container data (mount/network summaries use HTML entities)
# ---------------------------------------------------------------------------

def _prepare_containers(snapshot: InspectionSnapshot) -> dict:
    """Pre-process running container data with pre-rendered summaries."""
    if not snapshot.containers or not snapshot.containers.running_containers:
        return {"running": []}
    running = []
    for r in snapshot.containers.running_containers:
        name = r.name or r.id[:12]
        mount_summary = ", ".join(
            f'{m.source}&rarr;{m.destination}' for m in r.mounts[:3]
        )
        if len(r.mounts) > 3:
            mount_summary += f" +{len(r.mounts) - 3} more"
        net_summary = ", ".join(
            f'{n}: {info.get("ip", "")}'
            for n, info in r.networks.items()
        )
        running.append({
            "name": name,
            "image": r.image,
            "status": r.status,
            "mount_summary": mount_summary or "<em>none</em>",
            "net_summary": net_summary,
        })
    return {"running": running}


# ---------------------------------------------------------------------------
# Pre-categorized non-RPM items
# ---------------------------------------------------------------------------

def _prepare_non_rpm(snapshot: InspectionSnapshot) -> dict:
    if not snapshot.non_rpm_software or not snapshot.non_rpm_software.items:
        return {"elf": [], "venv": [], "git": [], "pip": [], "other": []}
    items = snapshot.non_rpm_software.items
    return {
        "elf": [i for i in items if i.lang],
        "venv": [i for i in items if i.method == "python venv"],
        "git": [i for i in items if i.method == "git repository"],
        "pip": [i for i in items if i.method == "pip dist-info"],
        "other": [i for i in items
                  if not i.lang and i.method not in
                  ("python venv", "git repository", "pip dist-info")],
    }


# ---------------------------------------------------------------------------
# Fleet variant grouping
# ---------------------------------------------------------------------------

def _variant_prevalence(item):
    """Extract prevalence count for sorting. Returns 0 if no fleet data."""
    fleet = item.get("fleet") if isinstance(item, dict) else getattr(item, "fleet", None)
    return fleet.count if fleet else 0


def _group_variants(items, path_key="path"):
    """Group items by path for variant display.

    Returns OrderedDict[path, list[dict]] where each entry has
    {"item": item_or_dict, "snap_index": int}. Variants within each
    group are sorted by prevalence (highest first).
    """
    groups: OrderedDict = OrderedDict()
    for idx, item in enumerate(items):
        path = item[path_key] if isinstance(item, dict) else getattr(item, path_key)
        if path not in groups:
            groups[path] = []
        groups[path].append({"item": item, "snap_index": idx})

    for path, variants in groups.items():
        variants.sort(key=lambda v: _variant_prevalence(v["item"]), reverse=True)
    return groups


# ---------------------------------------------------------------------------
# Context builder
# ---------------------------------------------------------------------------

def _build_context(
    snapshot: InspectionSnapshot,
    output_dir: Path,
    env: Environment,
    original_snapshot_path: Optional[Path] = None,
) -> dict:
    from ._triage import compute_triage, compute_triage_detail

    counts = _summary_counts(snapshot)

    fleet_raw = (snapshot.meta or {}).get("fleet")
    fleet_meta = FleetMeta(**fleet_raw) if fleet_raw else None

    # Aggregate included/excluded counts across key sections for the fleet banner.
    _inc_items = []
    if snapshot.rpm:
        _inc_items.extend(snapshot.rpm.packages_added or [])
        _inc_items.extend(snapshot.rpm.base_image_only or [])
    if snapshot.config:
        _inc_items.extend(snapshot.config.files or [])
    counts["n_included"] = sum(1 for i in _inc_items if getattr(i, "include", True))
    counts["n_excluded"] = sum(1 for i in _inc_items if not getattr(i, "include", True))

    triage = compute_triage(snapshot, output_dir)
    triage_detail = compute_triage_detail(snapshot, output_dir)
    os_desc = (snapshot.os_release.pretty_name or snapshot.os_release.name
               if snapshot.os_release else "Unknown")

    containerfile_content = ""
    cf_path = output_dir / "Containerfile"
    if cf_path.exists():
        try:
            containerfile_content = cf_path.read_text()
        except Exception:
            pass

    audit_report_content = ""
    ar_path = output_dir / "audit-report.md"
    if ar_path.exists():
        try:
            audit_report_content = ar_path.read_text()
        except Exception:
            pass

    warnings: List[dict] = list(snapshot.warnings) if snapshot.warnings else []
    if snapshot.redactions:
        for r in snapshot.redactions:
            w = make_warning("redaction", f"Redacted: {r.get('path') or ''}")
            w["detail"] = r.get("remediation") or ""
            warnings.append(w)

    output_tree = _build_output_tree(output_dir)
    file_content_snippets: List[str] = []
    tree_html = _render_tree_html(output_tree, file_content_snippets)

    audit_html = _markdown_to_html(audit_report_content) if audit_report_content else "<p>(Audit report not generated.)</p>"

    # Escape containerfile for embedding in <pre>
    containerfile_html = (containerfile_content or "(Containerfile not generated)").replace(
        "&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")

    # Sort leaf packages by dependency count descending for the HTML tree view
    leaf_sorted: List[str] = []
    if snapshot.rpm and snapshot.rpm.leaf_packages and snapshot.rpm.leaf_dep_tree:
        dep_tree = snapshot.rpm.leaf_dep_tree
        leaf_sorted = sorted(snapshot.rpm.leaf_packages, key=lambda k: -len(dep_tree.get(k, [])))

    # Pre-compute repo groups for the dependency tree display
    repo_groups: dict = {}
    if snapshot.rpm and snapshot.rpm.leaf_packages:
        dep_tree = snapshot.rpm.leaf_dep_tree or {}
        pkg_by_name = {p.name: p for p in snapshot.rpm.packages_added}
        for lf in (leaf_sorted or snapshot.rpm.leaf_packages):
            pkg = pkg_by_name.get(lf)
            repo = pkg.source_repo if pkg and pkg.source_repo else "(unknown)"
            snap_idx = -1
            for idx, p in enumerate(snapshot.rpm.packages_added):
                if p.name == lf:
                    snap_idx = idx
                    break
            repo_groups.setdefault(repo, []).append({
                "name": lf,
                "version": f"{pkg.version}-{pkg.release}" if pkg and pkg.version else "",
                "deps": dep_tree.get(lf, []),
                "snap_index": snap_idx,
                "include": pkg.include if pkg else True,
                "fleet": pkg.fleet if pkg else None,
            })
        # Sort groups: known repos alphabetically, "(unknown)" last
        sorted_groups = []
        for k in sorted(repo_groups.keys()):
            if k != "(unknown)":
                sorted_groups.append((k, repo_groups[k]))
        if "(unknown)" in repo_groups:
            sorted_groups.append(("(unknown)", repo_groups["(unknown)"]))
        repo_groups = dict(sorted_groups)

    # Pre-compute repo display metadata for the template
    repo_display: List[dict] = []
    if snapshot.rpm and snapshot.rpm.repo_files:
        for rf in snapshot.rpm.repo_files:
            section_ids = []
            for line in (rf.content or "").splitlines():
                stripped = line.strip()
                if stripped.startswith("[") and stripped.endswith("]"):
                    section_ids.append(stripped[1:-1])
            content_lower = (rf.content or "").lower()
            path_lower = rf.path.lower()
            if rf.is_default_repo:
                badge = "BASE"
                badge_color = ""
            elif "epel" in path_lower or any(sid.startswith("epel") for sid in section_ids):
                badge = "EPEL"
                badge_color = "pf-m-blue"
            elif "copr" in content_lower or "copr" in path_lower:
                badge = "COPR"
                badge_color = "pf-m-red"
            elif "rpmfusion" in content_lower or "rpmfusion" in path_lower:
                badge = "RPM FUSION"
                badge_color = "pf-m-orange"
            else:
                badge = ""
                badge_color = ""
            repo_display.append({
                "path": rf.path,
                "badge": badge,
                "badge_color": badge_color,
                "section_ids": section_ids,
                "is_default": rf.is_default_repo,
            })

    # Secrets data for dedicated tab
    redactions = snapshot.redactions or []
    secrets_files = len(set(r.get("path", "") for r in redactions))

    # Embed snapshot as JSON for interactive UI.
    # Escape "</" so a value containing "</script>" cannot terminate the
    # enclosing <script> block (standard JSON-in-HTML XSS prevention).
    snapshot_json = snapshot.model_dump_json().replace("</", "<\\/")
    # On re-render the server passes the true original via --original-snapshot;
    # at initial render both are identical.
    if original_snapshot_path and original_snapshot_path.exists():
        try:
            original_snapshot_json = original_snapshot_path.read_text().replace("</", "<\\/")
        except Exception:
            logging.getLogger(__name__).warning(
                "Could not read original snapshot from %s; "
                "editor reset-to-original will use the current snapshot instead",
                original_snapshot_path,
            )
            original_snapshot_json = snapshot_json
    else:
        original_snapshot_json = snapshot_json

    # Load PatternFly 6 CSS for inline embedding (self-contained report)
    pf_css_path = Path(__file__).resolve().parent.parent / "templates" / "patternfly.css"
    patternfly_css = ""
    if pf_css_path.exists():
        patternfly_css = pf_css_path.read_text()

    config_files = _prepare_config_files(snapshot)

    if fleet_meta:
        config_variant_groups = _group_variants(config_files, path_key="path")
        quadlet_variant_groups = _group_variants(
            snapshot.containers.quadlet_units, path_key="path"
        ) if snapshot.containers and snapshot.containers.quadlet_units else OrderedDict()
        dropin_variant_groups = _group_variants(
            snapshot.services.drop_ins, path_key="path"
        ) if snapshot.services and snapshot.services.drop_ins else OrderedDict()
    else:
        config_variant_groups = None
        quadlet_variant_groups = None
        dropin_variant_groups = None

    return {
        "snapshot": snapshot,
        "snapshot_json": snapshot_json,
        "original_snapshot_json": original_snapshot_json,
        "patternfly_css": Markup(patternfly_css),
        "counts": counts,
        "fleet_meta": fleet_meta,
        "triage": triage,
        "os_desc": os_desc,
        "os_id": snapshot.os_release.id if snapshot.os_release else "",
        "hostname": (snapshot.meta or {}).get("hostname", ""),
        "meta": snapshot.meta or {},
        "warnings": warnings,
        "warnings_panel": warnings[:50],
        "warnings_overflow": max(0, len(warnings) - 50),
        "containerfile_html": Markup(containerfile_html),
        "containerfile_lines": containerfile_content.count("\n") + 1 if containerfile_content else 0,
        "tree_html": Markup(tree_html),
        "file_content_snippets_html": Markup("".join(file_content_snippets)),
        "audit_html": Markup(audit_html),
        "config_files_rendered": config_files,
        "config_variant_groups": config_variant_groups,
        "quadlet_variant_groups": quadlet_variant_groups,
        "dropin_variant_groups": dropin_variant_groups,
        "containers_data": _prepare_containers(snapshot),
        "non_rpm_data": _prepare_non_rpm(snapshot),
        "triage_detail": triage_detail,
        "leaf_packages_sorted": leaf_sorted,
        "repo_groups": repo_groups,
        "repo_display": repo_display,
        "secrets_data": redactions,
        "secrets_file_count": secrets_files,
    }


# ---------------------------------------------------------------------------
# Public render entry point
# ---------------------------------------------------------------------------

def render(
    snapshot: InspectionSnapshot,
    env: Environment,
    output_dir: Path,
    original_snapshot_path: Optional[Path] = None,
) -> None:
    """Render report.html by building a context dict and invoking the Jinja2 template."""
    from jinja2 import FileSystemLoader

    output_dir = Path(output_dir)

    # Ensure the environment has a loader that can find the templates.
    # When called from run_all() the loader is already set; when called
    # directly (e.g. tests), we set it up from the package templates dir.
    if env.loader is None:
        templates_dir = Path(__file__).resolve().parent.parent / "templates"
        env = env.overlay(loader=FileSystemLoader(str(templates_dir)))

    env.filters["fleet_color"] = _fleet_color

    ctx = _build_context(snapshot, output_dir, env, original_snapshot_path=original_snapshot_path)
    template = env.get_template("report.html.j2")
    html = template.render(ctx)
    (output_dir / "report.html").write_text(html)
