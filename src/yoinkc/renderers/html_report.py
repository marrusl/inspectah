"""HTML report renderer.

Builds a context dict from the snapshot and delegates all HTML generation
to templates/report.html.j2 via Jinja2.  No HTML strings live in this file.
"""

import re
from pathlib import Path
from typing import List

from jinja2 import Environment
from markupsafe import Markup

from ..schema import ConfigFileKind, InspectionSnapshot

# Max size per file to embed in the report (bytes); larger files show a truncation note
_MAX_FILE_CONTENT = 100 * 1024


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
                        content = text.replace("<", "&lt;").replace(">", "&gt;")
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


def _render_tree_html(nodes: List[dict], content_snippets: List[str]) -> str:
    """Render tree nodes to HTML; append file content divs to content_snippets."""
    parts: List[str] = []
    for n in nodes:
        if n.get("type") == "dir":
            ch_html = _render_tree_html(n.get("children", []), content_snippets)
            parts.append(
                '<details class="tree-dir" open><summary class="tree-dir-summary">'
                + (n.get("name", "")).replace("<", "&lt;")
                + "</summary><ul class=\"tree-children\">"
                + ch_html
                + "</ul></details>"
            )
        else:
            cid = n.get("content_id", "")
            content = n.get("content", "")
            path_attr = (n.get("rel_path", "")).replace('"', "&quot;")
            content_snippets.append(
                f'<div id="{cid}" class="file-content-hidden">'
                f'<pre class="file-content-pre">{content}</pre></div>'
            )
            parts.append(
                f'<li><button type="button" class="file-entry"'
                f' data-content-id="{cid}" data-path="{path_attr}">'
                f'{(n.get("name", "")).replace("<", "&lt;")}</button></li>'
            )
    return "\n".join(parts)


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
                cells = [c.strip() for c in row.split("|") if c.strip() or row.strip().startswith("|")]
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
        if line.startswith("### "):
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
                     + (1 if snapshot.selinux.fips_mode else 0))
    return {
        "packages_added": len(snapshot.rpm.packages_added or []) if snapshot.rpm else 0,
        "packages_removed": len(snapshot.rpm.packages_removed or []) if snapshot.rpm else 0,
        "rpm_va": len(snapshot.rpm.rpm_va or []) if snapshot.rpm else 0,
        "config_files": len(snapshot.config.files or []) if snapshot.config else 0,
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
    for f in snapshot.config.files:
        result.append({
            "path": f.path,
            "kind": f.kind.value,
            "flags": f.rpm_va_flags or "",
            "diff_html": _render_diff_html(f.diff_against_rpm or ""),
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
# Context builder
# ---------------------------------------------------------------------------

def _build_context(
    snapshot: InspectionSnapshot,
    output_dir: Path,
    env: Environment,
) -> dict:
    from ._triage import compute_triage

    counts = _summary_counts(snapshot)
    triage = compute_triage(snapshot, output_dir)
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
            warnings.append({
                "severity": "warning",
                "source": "redaction",
                "message": f"Redacted: {r.get('path') or ''}",
                "detail": r.get("remediation") or "",
            })

    output_tree = _build_output_tree(output_dir)
    file_content_snippets: List[str] = []
    tree_html = _render_tree_html(output_tree, file_content_snippets)

    audit_html = _markdown_to_html(audit_report_content) if audit_report_content else "<p>(Audit report not generated.)</p>"

    # Escape containerfile for embedding in <pre>
    containerfile_html = (containerfile_content or "(Containerfile not generated)").replace(
        "&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")

    summary_glance = [
        (counts["packages_added"], "Packages added"),
        (counts["packages_removed"], "Packages removed"),
        (counts["config_files"], "Config files"),
        (counts["services_enabled"], "Services enabled"),
        (counts["redactions"], "Secrets redacted"),
        (len(warnings), "Warnings"),
        (counts["containers"], "Containers/quadlet"),
        (counts["users_groups"], "Users/groups"),
    ]

    return {
        "snapshot": snapshot,
        "counts": counts,
        "triage": triage,
        "os_desc": os_desc,
        "meta": snapshot.meta or {},
        "warnings": warnings,
        "warnings_panel": warnings[:50],
        "warnings_overflow": max(0, len(warnings) - 50),
        "containerfile_html": Markup(containerfile_html),
        "containerfile_lines": containerfile_content.count("\n") + 1 if containerfile_content else 0,
        "tree_html": Markup(tree_html),
        "file_content_snippets_html": Markup("".join(file_content_snippets)),
        "audit_html": Markup(audit_html),
        "config_files_rendered": _prepare_config_files(snapshot),
        "containers_data": _prepare_containers(snapshot),
        "non_rpm_data": _prepare_non_rpm(snapshot),
        "summary_glance": summary_glance,
    }


# ---------------------------------------------------------------------------
# Public render entry point
# ---------------------------------------------------------------------------

def render(
    snapshot: InspectionSnapshot,
    env: Environment,
    output_dir: Path,
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

    ctx = _build_context(snapshot, output_dir, env)
    template = env.get_template("report.html.j2")
    html = template.render(ctx)
    (output_dir / "report.html").write_text(html)
