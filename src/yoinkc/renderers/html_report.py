"""Self-contained report.html renderer: dashboard, category cards, drill-down, searchable warnings."""

import re
from pathlib import Path
from typing import List

from jinja2 import Environment

from ..schema import ConfigFileKind, InspectionSnapshot

# Max size per file to embed in the report (bytes); larger files show a truncation note
_MAX_FILE_CONTENT = 100 * 1024


def _build_output_tree(output_dir: Path) -> List[dict]:
    """Build a tree of config/ and quadlet/ for the file browser. Returns list of root nodes."""
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
    """Recursively walk a directory; return list of tree nodes (dir or file)."""
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
                out.append({"type": "file", "name": p.name, "rel_path": child_rel, "content": content, "content_id": content_id})
            else:
                sub = _walk_dir(p, prefix, child_rel)
                out.append({"type": "dir", "name": p.name, "children": sub})
    except Exception:
        pass
    return out


def _render_tree_html(nodes: List[dict], content_snippets: List[str]) -> str:
    """Render tree nodes to HTML; append file content to content_snippets for later inclusion."""
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
            content_snippets.append(f'<div id="{cid}" class="file-content-hidden"><pre class="file-content-pre">{content}</pre></div>')
            parts.append(f'<li><button type="button" class="file-entry" data-content-id="{cid}" data-path="{path_attr}">{(n.get("name", "")).replace("<", "&lt;")}</button></li>')
    return "\n".join(parts)


def _markdown_to_html(md: str) -> str:
    """Convert basic markdown to HTML for display in the report (headers, lists, code, tables)."""
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
            j = s.index("`", i + 1) if "`" in s[i + 1 :] else -1
            if j > i:
                s = s[:i] + "<code>" + s[i + 1 : j] + "</code>" + s[j + 1 :]
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
                   + len(snapshot.scheduled_tasks.generated_timer_units or [])) if snapshot.scheduled_tasks else 0
    n_containers = 0
    if snapshot.containers:
        n_containers = (len(snapshot.containers.quadlet_units or [])
                        + len(snapshot.containers.compose_files or [])
                        + len(snapshot.containers.running_containers or []))
    n_non_rpm = len(snapshot.non_rpm_software.items or []) if snapshot.non_rpm_software else 0
    n_users = len(snapshot.users_groups.users or []) + len(snapshot.users_groups.groups or []) if snapshot.users_groups else 0
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
        n_selinux = len(snapshot.selinux.custom_modules or []) + len(snapshot.selinux.boolean_overrides or []) + len(snapshot.selinux.audit_rules or []) + (1 if snapshot.selinux.fips_mode else 0)
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


def render(
    snapshot: InspectionSnapshot,
    env: Environment,
    output_dir: Path,
) -> None:
    from ._triage import compute_triage

    output_dir = Path(output_dir)
    counts = _summary_counts(snapshot)
    triage = compute_triage(snapshot, output_dir)
    os_desc = snapshot.os_release.pretty_name or snapshot.os_release.name if snapshot.os_release else "Unknown"
    containerfile_content: str = ""
    containerfile_path = output_dir / "Containerfile"
    if containerfile_path.exists():
        try:
            containerfile_content = containerfile_path.read_text()
        except Exception:
            containerfile_content = ""
    audit_report_content: str = ""
    audit_report_path = output_dir / "audit-report.md"
    if audit_report_path.exists():
        try:
            audit_report_content = audit_report_path.read_text()
        except Exception:
            audit_report_content = ""
    warnings: List[dict] = list(snapshot.warnings) if snapshot.warnings else []
    if snapshot.redactions:
        for r in snapshot.redactions:
            warnings.append({"severity": "warning", "source": "redaction", "message": f"Redacted: {r.get('path') or ''}", "detail": (r.get("remediation") or "")})

    html_parts: List[str] = []
    html_parts.append("""<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8"/>
<meta name="viewport" content="width=device-width, initial-scale=1"/>
<title>yoinkc Report</title>
<style>
:root { --bg: #0f1419; --card: #1a2332; --text: #e6edf3; --muted: #8b949e; --accent: #58a6ff; --warn: #d29922; --error: #f85149; --ok: #3fb950; }
* { box-sizing: border-box; }
body { font-family: system-ui, -apple-system, sans-serif; background: var(--bg); color: var(--text); margin: 0; padding: 1rem; line-height: 1.5; }
h1, h2, h3 { margin-top: 0; }
a { color: var(--accent); }
.banner { background: var(--card); border-radius: 8px; padding: 1rem 1.5rem; margin-bottom: 1rem; border-left: 4px solid var(--accent); }
.warning-panel { background: #2d1f1f; border: 1px solid var(--warn); border-radius: 8px; padding: 1rem; margin-bottom: 1rem; }
.warning-panel.collapsed { display: none; }
.warning-panel-header { display: flex; align-items: center; justify-content: space-between; gap: 1rem; margin-bottom: 0.5rem; }
.warning-panel h3 { color: var(--warn); margin: 0; flex: 1; }
.warning-panel-dismiss { background: var(--card); border: 1px solid var(--warn); color: var(--warn); border-radius: 6px; padding: 0.35rem 0.75rem; cursor: pointer; font-size: 0.9rem; font-weight: 500; white-space: nowrap; }
.warning-panel-dismiss:hover { background: var(--warn); color: var(--bg); border-color: var(--warn); }
.warning-row { display: flex; align-items: flex-start; gap: 0.5rem; padding: 0.4rem 0; border-bottom: 1px solid rgba(210,153,34,0.15); }
.warning-row.dismissed { display: none; }
.warning-row:last-child { border-bottom: none; }
.warning-row .warning-msg { flex: 1; font-size: 0.9rem; }
.warning-row-dismiss { background: none; border: none; color: var(--muted); cursor: pointer; font-size: 1.1rem; line-height: 1; padding: 0.15rem 0.35rem; border-radius: 4px; flex-shrink: 0; }
.warning-row-dismiss:hover { color: var(--warn); background: rgba(210,153,34,0.15); }
.cards { display: grid; grid-template-columns: repeat(auto-fill, minmax(200px, 1fr)); gap: 1rem; margin-bottom: 1.5rem; }
.card { background: var(--card); border-radius: 8px; padding: 1rem; cursor: pointer; border: 1px solid transparent; transition: border-color .15s; }
.card:hover { border-color: var(--accent); }
.card h4 { margin: 0 0 .5rem 0; font-size: .95rem; }
.card .count { font-size: 1.5rem; font-weight: 600; }
.card .status { font-size: .8rem; color: var(--muted); margin-top: .25rem; }
.section { display: none; background: var(--card); border-radius: 8px; padding: 1.5rem; margin-top: 1rem; }
.section.visible { display: block; }
table { width: 100%; border-collapse: collapse; }
th, td { text-align: left; padding: .5rem .75rem; border-bottom: 1px solid #30363d; }
th { color: var(--muted); font-weight: 500; }
.tabs { display: flex; gap: .5rem; margin-bottom: 1rem; flex-wrap: wrap; }
.tabs button { background: var(--card); border: 1px solid #30363d; color: var(--text); padding: .5rem 1rem; border-radius: 6px; cursor: pointer; }
.tabs button.active { background: var(--accent); border-color: var(--accent); color: #fff; }
#warnings-list { max-height: 400px; overflow-y: auto; }
.search { margin-bottom: 1rem; }
.search input { width: 100%; max-width: 400px; padding: .5rem; border-radius: 6px; border: 1px solid #30363d; background: var(--bg); color: var(--text); }
.badge { display: inline-block; padding: .15rem .5rem; border-radius: 4px; font-size: .75rem; }
.badge.ok { background: #238636; color: #fff; }
.badge.warn { background: #9e6a03; color: #fff; }
.badge.error { background: #da3633; color: #fff; }
.containerfile-pre { background: #0d1117; border: 1px solid #30363d; border-radius: 6px; padding: 1rem; overflow: auto; max-height: 70vh; font-size: 0.85rem; line-height: 1.4; white-space: pre; margin: 0; }
.file-browser-layout { display: flex; gap: 1rem; margin-top: 1rem; flex-wrap: wrap; }
.file-tree-panel { flex: 0 1 280px; min-width: 200px; max-height: 70vh; overflow-y: auto; background: #0d1117; border: 1px solid #30363d; border-radius: 6px; padding: 0.75rem; }
.file-tree-root { padding: 0; }
.file-tree-root > details { margin-bottom: 0.25rem; }
.file-tree-panel ul { list-style: none; padding-left: 1rem; margin: 0.25rem 0; }
.file-tree-panel .tree-dir summary { cursor: pointer; padding: 0.2rem 0; }
.file-tree-panel .file-entry { background: none; border: none; color: var(--accent); cursor: pointer; padding: 0.2rem 0; text-align: left; font-size: 0.9rem; }
.file-tree-panel .file-entry:hover { text-decoration: underline; }
.file-viewer-panel { flex: 1 1 400px; min-height: 200px; background: #0d1117; border: 1px solid #30363d; border-radius: 6px; padding: 1rem; overflow: auto; }
.file-viewer-panel .file-path { color: var(--muted); font-size: 0.85rem; margin-bottom: 0.5rem; }
.file-viewer-panel .file-content-pre { max-height: 65vh; margin: 0; }
.file-content-hidden { display: none; }
.diff-view { max-height: 300px; overflow: auto; font-size: 0.85em; line-height: 1.5; background: #0d1117; padding: 0.5rem; border-radius: 4px; }
.diff-add { color: #3fb950; background: rgba(46,160,67,0.15); display: block; }
.diff-del { color: #f85149; background: rgba(248,81,73,0.15); display: block; }
.diff-hdr { color: #8b949e; display: block; font-weight: bold; }
.diff-hunk { color: #79c0ff; display: block; }
.summary-hero { font-size: 1.1rem; margin-bottom: 1.5rem; color: var(--muted); }
.summary-meta { display: grid; gap: 0.5rem; margin-bottom: 1.5rem; padding: 1rem; background: #0d1117; border-radius: 8px; border: 1px solid #30363d; }
.summary-meta dt { color: var(--muted); font-size: 0.85rem; margin-top: 0.5rem; }
.summary-meta dt:first-child { margin-top: 0; }
.summary-meta dd { margin: 0.15rem 0 0 0; }
.summary-glance { display: grid; grid-template-columns: repeat(auto-fill, minmax(140px, 1fr)); gap: 0.75rem; margin-bottom: 1.5rem; }
.summary-glance-item { background: #0d1117; border: 1px solid #30363d; border-radius: 6px; padding: 0.75rem 1rem; }
.summary-glance-item .num { font-size: 1.5rem; font-weight: 600; color: var(--accent); }
.summary-glance-item .label { font-size: 0.8rem; color: var(--muted); margin-top: 0.2rem; }
.summary-next { margin-top: 1.5rem; padding-top: 1rem; border-top: 1px solid #30363d; }
.audit-pre { background: #0d1117; border: 1px solid #30363d; border-radius: 6px; padding: 1rem; overflow: auto; max-height: 50vh; font-size: 0.85rem; margin: 0.5rem 0; }
.audit-table { margin: 0.5rem 0; }
.audit-section { max-height: 70vh; overflow-y: auto; }
</style>
</head>
<body>
<h1>yoinkc Inspection Report</h1>
<div class="banner">
  <strong>Host:</strong> """ + os_desc + """ &nbsp;|&nbsp;
  <span style="color:#3fb950">&#10003; """ + str(triage["automatic"]) + """ automatic</span> &nbsp;|&nbsp;
  <span style="color:#d29922">&#9888; """ + str(triage["fixme"]) + """ FIXME</span> &nbsp;|&nbsp;
  <span style="color:#f85149">&#9679; """ + str(triage["manual"]) + """ manual</span> &nbsp;|&nbsp;
  <strong>Warnings:</strong> <span id="banner-warning-count">""" + str(len(warnings)) + """</span>
</div>
""")

    if warnings:
        panel_warnings = warnings[:50]
        overflow = len(warnings) - len(panel_warnings)
        html_parts.append("""<div id="warning-banner" class="warning-panel" data-overflow-count=\"""" + str(overflow) + """\">
  <div class="warning-panel-header">
    <h3>Warnings &amp; items to review (<span id="warning-banner-count">""" + str(len(warnings)) + """</span>)</h3>
    <button type="button" class="warning-panel-dismiss" id="warning-dismiss-all" aria-label="Dismiss All">Dismiss All</button>
  </div>
  <div id="warning-rows">""")
        for idx, w in enumerate(panel_warnings):
            msg = (w.get("message") or w.get("detail") or "—").replace("<", "&lt;").replace(">", "&gt;")
            sev = (w.get("severity") or "warning")
            html_parts.append(
                f'<div class="warning-row" data-warning-idx="{idx}">'
                f'<span class="badge {sev}">{sev}</span>'
                f'<span class="warning-msg">{msg}</span>'
                f'<button type="button" class="warning-row-dismiss" aria-label="Dismiss">&times;</button>'
                f'</div>'
            )
        if overflow > 0:
            html_parts.append(f'<div class="warning-row" style="color:var(--muted);font-size:0.85rem;justify-content:center;">… and {overflow} more in the Warnings tab</div>')
        html_parts.append("""  </div>
</div>
""")

    html_parts.append("""
<div class="cards">
  <div class="card" data-section="packages"><h4>Packages</h4><div class="count">""" + str(counts["packages_added"]) + """ added</div><div class="status">""" + str(counts["packages_removed"]) + """ removed</div></div>
  <div class="card" data-section="services"><h4>Services</h4><div class="count">""" + str(counts["services_enabled"]) + """ enabled</div><div class="status">""" + str(counts["services_disabled"]) + """ disabled</div></div>
  <div class="card" data-section="config"><h4>Config</h4><div class="count">""" + str(counts["config_files"]) + """ files</div><div class="status">rpm_va: """ + str(counts["rpm_va"]) + """</div></div>
  <div class="card" data-section="network"><h4>Network</h4><div class="count">""" + str(counts["network"]) + """</div><div class="status">connections / firewall</div></div>
  <div class="card" data-section="storage"><h4>Storage</h4><div class="count">""" + str(counts["storage"]) + """</div><div class="status">fstab entries</div></div>
  <div class="card" data-section="scheduled_tasks"><h4>Scheduled</h4><div class="count">""" + str(counts["scheduled_tasks"]) + """</div><div class="status">cron / timers</div></div>
  <div class="card" data-section="containers"><h4>Containers</h4><div class="count">""" + str(counts["containers"]) + """</div><div class="status">quadlet / compose</div></div>
  <div class="card" data-section="non_rpm"><h4>Non-RPM</h4><div class="count">""" + str(counts["non_rpm"]) + """</div><div class="status">items</div></div>
  <div class="card" data-section="kernel_boot"><h4>Kernel/Boot</h4><div class="count">""" + str(counts["kernel_boot"]) + """</div><div class="status">configs</div></div>
  <div class="card" data-section="selinux"><h4>SELinux</h4><div class="count">""" + str(counts["selinux"]) + """</div><div class="status">customizations</div></div>
  <div class="card" data-section="users_groups"><h4>Users/Groups</h4><div class="count">""" + str(counts["users_groups"]) + """</div><div class="status">non-system</div></div>
  <div class="card" data-section="warnings"><h4>Warnings</h4><div class="count" id="warnings-card-count">""" + str(len(warnings)) + """</div><div class="status" id="warnings-card-status">Review required</div></div>
  <div class="card" data-section="containerfile"><h4>Containerfile</h4><div class="count">""" + (str(containerfile_content.count("\n") + 1) if containerfile_content else "0") + """ lines</div><div class="status">Generated</div></div>
  <div class="card" data-section="output_files"><h4>Output files</h4><div class="count">config / quadlet</div><div class="status">Browse</div></div>
  <div class="card" data-section="audit"><h4>Audit report</h4><div class="count">Full report</div><div class="status">Markdown</div></div>
</div>

<div class="tabs">
  <button class="active" data-tab="summary">Summary</button>
  <button data-tab="packages">Packages</button>
  <button data-tab="services">Services</button>
  <button data-tab="config">Config</button>
  <button data-tab="network">Network</button>
  <button data-tab="storage">Storage</button>
  <button data-tab="scheduled_tasks">Scheduled</button>
  <button data-tab="containers">Containers</button>
  <button data-tab="non_rpm">Non-RPM</button>
  <button data-tab="kernel_boot">Kernel/Boot</button>
  <button data-tab="selinux">SELinux</button>
  <button data-tab="users_groups">Users/Groups</button>
  <button data-tab="warnings">Warnings</button>
  <button data-tab="containerfile">Containerfile</button>
  <button data-tab="output_files">Output files</button>
  <button data-tab="audit">Audit report</button>
</div>
""")

    # Summary section (prettier: run info + at-a-glance + next steps)
    meta = snapshot.meta or {}
    host_root = (meta.get("host_root") or "").replace("<", "&lt;").replace(">", "&gt;")
    hostname = (meta.get("hostname") or "—").replace("<", "&lt;").replace(">", "&gt;")
    timestamp = (meta.get("timestamp") or "—").replace("<", "&lt;").replace(">", "&gt;")
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
    glance_html = "".join(
        f'<div class="summary-glance-item"><div class="num">{n}</div><div class="label">{lbl}</div></div>'
        for n, lbl in summary_glance
    )
    html_parts.append(
        '<div id="section-summary" class="section visible"><h2>Summary</h2>'
        '<p class="summary-hero">Inspection overview and run metadata. Use the cards or tabs above to drill into any category.</p>'
        '<p class="summary-portable" style="font-size:0.9rem;color:var(--muted);">This report is self-contained and portable: all content (Containerfile, audit, config/ and quadlet/ file browser) is embedded. You can copy or share <code>report.html</code> alone.</p>'
        '<dl class="summary-meta">'
        f'<dt>Host root</dt><dd><code>{host_root}</code></dd>'
        f'<dt>Hostname</dt><dd>{hostname}</dd>'
        f'<dt>Inspection time (UTC)</dt><dd>{timestamp}</dd>'
        f'<dt>OS</dt><dd>{os_desc}</dd>'
        "</dl>"
        '<h3>At a glance</h3>'
        f'<div class="summary-glance">{glance_html}</div>'
        '<div class="summary-next">'
        "<p><strong>Next steps:</strong> Review the <strong>Audit report</strong> tab, fix any FIXMEs in the Containerfile, run <code>podman build -t my-image .</code>, then deploy with <code>bootc switch my-image:latest</code>.</p>"
        "</div></div>"
    )

    # Packages section
    html_parts.append('<div id="section-packages" class="section"><h2>Packages</h2>')
    if snapshot.rpm and getattr(snapshot.rpm, "no_baseline", False):
        html_parts.append("<p><em>No baseline — showing all packages. Pull the base image or provide --baseline-packages.</em></p>")
    if snapshot.rpm and snapshot.rpm.packages_added:
        html_parts.append("<table><thead><tr><th>Name</th><th>Version</th><th>Release</th><th>Arch</th></tr></thead><tbody>")
        for p in snapshot.rpm.packages_added[:100]:
            html_parts.append(f"<tr><td>{p.name}</td><td>{p.version}</td><td>{p.release}</td><td>{p.arch}</td></tr>")
        html_parts.append("</tbody></table>")
        if len(snapshot.rpm.packages_added) > 100:
            html_parts.append(f"<p>... and {len(snapshot.rpm.packages_added) - 100} more.</p>")
    else:
        html_parts.append("<p>No added packages.</p>")
    html_parts.append("</div>")

    # Services section
    html_parts.append('<div id="section-services" class="section"><h2>Services</h2>')
    service_rows = [s for s in (snapshot.services.state_changes or []) if s.action != "unchanged"] if snapshot.services else []
    if service_rows:
        html_parts.append("<table><thead><tr><th>Unit</th><th>Current</th><th>Default</th><th>Action</th></tr></thead><tbody>")
        for s in service_rows:
            html_parts.append(f"<tr><td>{s.unit}</td><td>{s.current_state}</td><td>{s.default_state}</td><td>{s.action}</td></tr>")
        html_parts.append("</tbody></table>")
    else:
        html_parts.append("<p>No service changes.</p>")
    html_parts.append("</div>")

    # Config section
    html_parts.append('<div id="section-config" class="section"><h2>Configuration files</h2>')
    if snapshot.config and snapshot.config.files:
        html_parts.append("<table><thead><tr><th>Path</th><th>Kind</th><th>rpm -Va flags</th><th>Diff</th></tr></thead><tbody>")
        for f in snapshot.config.files:
            diff_cell = ""
            if f.diff_against_rpm:
                diff_lines = f.diff_against_rpm.splitlines()[:80]
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
                if len(f.diff_against_rpm.splitlines()) > 80:
                    colored.append(f'<span class="diff-hdr">... {len(f.diff_against_rpm.splitlines()) - 80} more lines</span>')
                diff_cell = '<pre class="diff-view">' + "\n".join(colored) + "</pre>"
            flags_cell = f"<code>{f.rpm_va_flags}</code>" if f.rpm_va_flags else ""
            html_parts.append(f"<tr><td><code>{f.path}</code></td><td>{f.kind.value}</td><td>{flags_cell}</td><td>{diff_cell}</td></tr>")
        html_parts.append("</tbody></table>")
    else:
        html_parts.append("<p>No config files.</p>")
    html_parts.append("</div>")

    # Network section
    html_parts.append('<div id="section-network" class="section"><h2>Network</h2>')
    net = snapshot.network
    if net and (net.connections or net.firewall_zones or net.firewall_direct_rules
                or net.ip_routes or net.ip_rules or net.resolv_provenance):
        # --- Connections table ---
        if net.connections:
            html_parts.append("<h3>Connections</h3>")
            html_parts.append('<table class="data-table"><thead><tr>'
                              '<th>Name</th><th>Method</th><th>Type</th><th>Deployment</th>'
                              '</tr></thead><tbody>')
            for c in net.connections:
                if c.method == "static":
                    deploy = '<span style="color:#3fb950;font-weight:bold">Bake into image</span>'
                elif c.method == "dhcp":
                    deploy = '<span style="color:#d29922;font-weight:bold">Kickstart at deploy</span>'
                else:
                    deploy = '<span style="color:var(--muted)">Review</span>'
                html_parts.append(f'<tr><td><code>{c.name}</code></td>'
                                  f'<td>{c.method}</td><td>{c.type}</td><td>{deploy}</td></tr>')
            html_parts.append("</tbody></table>")

        # --- Firewall zones ---
        if net.firewall_zones:
            html_parts.append('<h3>Firewall zones <span style="color:#3fb950;font-size:0.8em">(bake into image)</span></h3>')
            for z in net.firewall_zones:
                html_parts.append(f"<h4>{z.name}</h4>")
                if z.services:
                    html_parts.append(f"<p>Services: <code>{', '.join(z.services)}</code></p>")
                if z.ports:
                    html_parts.append(f"<p>Ports: <code>{', '.join(z.ports)}</code></p>")
                if z.rich_rules:
                    html_parts.append(f"<p>Rich rules ({len(z.rich_rules)}):</p><ul>")
                    for r in z.rich_rules:
                        escaped = r.replace("<", "&lt;").replace(">", "&gt;")
                        html_parts.append(f"<li><code>{escaped[:300]}</code></li>")
                    html_parts.append("</ul>")

        # --- Direct rules ---
        if net.firewall_direct_rules:
            html_parts.append('<h3>Firewall direct rules <span style="color:#3fb950;font-size:0.8em">(bake into image)</span></h3>')
            html_parts.append("<ul>")
            for r in net.firewall_direct_rules:
                html_parts.append(f"<li>{r.ipv} {r.chain}: <code>{r.args}</code></li>")
            html_parts.append("</ul>")

        # --- resolv.conf ---
        if net.resolv_provenance:
            prov = net.resolv_provenance
            if prov == "networkmanager":
                label = "NetworkManager-managed"
                note = "DHCP-assigned DNS — kickstart at deploy time"
                color = "#d29922"
            elif prov == "systemd-resolved":
                label = "systemd-resolved"
                note = "system resolver — kickstart at deploy time"
                color = "#d29922"
            else:
                label = "hand-edited"
                note = "bake /etc/resolv.conf into image or manage at deploy"
                color = "#3fb950"
            html_parts.append(f'<h3>DNS</h3><p>resolv.conf provenance: '
                              f'<span style="color:{color};font-weight:bold">{label}</span> — {note}</p>')

        # --- IP routes / rules ---
        if net.ip_routes:
            static_rt = [r for r in net.ip_routes if "proto static" in r]
            if static_rt:
                html_parts.append(f"<h3>Static routes ({len(static_rt)})</h3><ul>")
                for r in static_rt:
                    html_parts.append(f"<li><code>{r}</code></li>")
                html_parts.append("</ul>")
        if net.ip_rules:
            html_parts.append(f"<h3>Policy routing rules ({len(net.ip_rules)})</h3><ul>")
            for r in net.ip_rules:
                html_parts.append(f"<li><code>{r}</code></li>")
            html_parts.append("</ul>")
    else:
        html_parts.append("<p>No network config captured.</p>")
    html_parts.append("</div>")

    # Storage section
    html_parts.append('<div id="section-storage" class="section"><h2>Storage</h2>')
    if snapshot.storage and (snapshot.storage.fstab_entries or []):
        html_parts.append("<table><thead><tr><th>Device</th><th>Mount</th><th>Type</th></tr></thead><tbody>")
        for e in (snapshot.storage.fstab_entries or [])[:30]:
            html_parts.append(f"<tr><td>{e.device}</td><td>{e.mount_point}</td><td>{e.fstype}</td></tr>")
        html_parts.append("</tbody></table>")
    else:
        html_parts.append("<p>No fstab entries.</p>")
    html_parts.append("</div>")

    # Scheduled tasks section
    html_parts.append('<div id="section-scheduled_tasks" class="section"><h2>Scheduled tasks</h2>')
    st = snapshot.scheduled_tasks
    has_tasks = st and (st.cron_jobs or st.systemd_timers or st.generated_timer_units or st.at_jobs)
    if has_tasks:
        # Existing systemd timers
        local_timers = [t for t in st.systemd_timers if t.source == "local"]
        vendor_timers = [t for t in st.systemd_timers if t.source == "vendor"]
        if local_timers or vendor_timers:
            html_parts.append("<h3>Existing systemd timers</h3>")
            html_parts.append('<table><tr><th>Timer</th><th>Schedule</th><th>ExecStart</th><th>Source</th></tr>')
            for t in local_timers:
                html_parts.append(
                    f'<tr style="background:#e8f5e9"><td>{t.name}</td>'
                    f'<td>{t.on_calendar}</td>'
                    f'<td><code>{t.exec_start}</code></td>'
                    f'<td><span style="background:#4caf50;color:#fff;padding:2px 8px;border-radius:4px">local</span></td></tr>')
            for t in vendor_timers:
                html_parts.append(
                    f'<tr><td>{t.name}</td>'
                    f'<td>{t.on_calendar}</td>'
                    f'<td><code>{t.exec_start}</code></td>'
                    f'<td><span style="background:#90a4ae;color:#fff;padding:2px 8px;border-radius:4px">vendor</span></td></tr>')
            html_parts.append("</table>")

        # Cron-converted timers
        if st.generated_timer_units:
            html_parts.append("<h3>Cron-converted timers</h3>")
            html_parts.append('<table><tr><th>Name</th><th>Cron Expression</th><th>Source File</th></tr>')
            for u in st.generated_timer_units:
                html_parts.append(
                    f'<tr style="background:#fff3e0"><td>{u.name}</td>'
                    f'<td><code>{u.cron_expr}</code></td>'
                    f'<td><code>{u.source_path}</code></td></tr>')
            html_parts.append("</table>")

        # Cron jobs
        if st.cron_jobs:
            html_parts.append("<h3>Cron jobs</h3>")
            html_parts.append("<ul>")
            for j in st.cron_jobs:
                html_parts.append(f'<li><code>{j.path}</code> ({j.source})</li>')
            html_parts.append("</ul>")

        # At jobs
        if st.at_jobs:
            html_parts.append("<h3>At jobs</h3>")
            html_parts.append('<table><tr><th>File</th><th>User</th><th>Command</th></tr>')
            for a in st.at_jobs:
                cmd = a.command[:117] + "..." if len(a.command) > 120 else a.command
                html_parts.append(
                    f'<tr><td><code>{a.file}</code></td>'
                    f'<td>{a.user}</td>'
                    f'<td><code>{cmd}</code></td></tr>')
            html_parts.append("</table>")
    else:
        html_parts.append("<p>No scheduled tasks.</p>")
    html_parts.append("</div>")

    # Containers section
    html_parts.append('<div id="section-containers" class="section"><h2>Containers</h2>')
    ct = snapshot.containers
    has_ct = ct and (ct.quadlet_units or ct.compose_files or ct.running_containers)
    if has_ct:
        if ct.quadlet_units:
            html_parts.append("<h3>Quadlet units</h3>")
            html_parts.append('<table><tr><th>Unit</th><th>Image</th><th>Path</th></tr>')
            for u in ct.quadlet_units:
                img_display = f'<code>{u.image}</code>' if u.image else '<em>none</em>'
                html_parts.append(
                    f'<tr><td>{u.name}</td>'
                    f'<td>{img_display}</td>'
                    f'<td><code>{u.path}</code></td></tr>')
            html_parts.append("</table>")

        if ct.compose_files:
            html_parts.append("<h3>Compose files</h3>")
            for c in ct.compose_files:
                html_parts.append(f'<h4><code>{c.path}</code></h4>')
                if c.images:
                    html_parts.append('<table><tr><th>Service</th><th>Image</th></tr>')
                    for img in c.images:
                        html_parts.append(
                            f'<tr><td>{img.service}</td>'
                            f'<td><code>{img.image}</code></td></tr>')
                    html_parts.append("</table>")
                else:
                    html_parts.append("<p>No image references found.</p>")

        if ct.running_containers:
            html_parts.append("<h3>Running containers (podman)</h3>")
            html_parts.append('<table><tr><th>Name</th><th>Image</th><th>Status</th><th>Mounts</th><th>Networks</th></tr>')
            for r in ct.running_containers:
                name = r.name or r.id[:12]
                mount_summary = ", ".join(
                    f'{m.source}&rarr;{m.destination}' for m in r.mounts[:3]
                )
                if len(r.mounts) > 3:
                    mount_summary += f" +{len(r.mounts)-3} more"
                net_summary = ", ".join(
                    f'{n}: {info.get("ip","")}'
                    for n, info in r.networks.items()
                )
                html_parts.append(
                    f'<tr><td><strong>{name}</strong></td>'
                    f'<td><code>{r.image}</code></td>'
                    f'<td>{r.status}</td>'
                    f'<td style="font-size:0.85em">{mount_summary or "<em>none</em>"}</td>'
                    f'<td style="font-size:0.85em">{net_summary or "<em>none</em>"}</td></tr>')
            html_parts.append("</table>")
    else:
        html_parts.append("<p>No container workloads.</p>")
    html_parts.append("</div>")

    # Non-RPM section
    html_parts.append('<div id="section-non_rpm" class="section"><h2>Non-RPM software</h2>')
    nr = snapshot.non_rpm_software
    if nr and nr.items:
        elf_items = [i for i in nr.items if i.get("lang")]
        venv_items = [i for i in nr.items if i.get("method") == "python venv"]
        git_items = [i for i in nr.items if i.get("method") == "git repository"]
        pip_items = [i for i in nr.items if i.get("method") == "pip dist-info"]
        other_items = [i for i in nr.items
                       if not i.get("lang") and i.get("method") not in ("python venv", "git repository", "pip dist-info")]

        if elf_items:
            html_parts.append("<h3>Compiled binaries</h3>")
            html_parts.append('<table><tr><th>Path</th><th>Language</th><th>Linking</th><th>Shared Libraries</th></tr>')
            lang_colors = {"go": "#00ADD8", "rust": "#DEA584", "c/c++": "#555"}
            for i in elf_items:
                lang = i.get("lang", "")
                color = lang_colors.get(lang, "#999")
                linking = "static" if i.get("static") else "dynamic"
                link_color = "#4caf50" if i.get("static") else "#ff9800"
                libs = ", ".join(i.get("shared_libs", [])[:5])
                html_parts.append(
                    f'<tr><td><code>{i.get("path","")}</code></td>'
                    f'<td><span style="background:{color};color:#fff;padding:2px 8px;border-radius:4px">{lang}</span></td>'
                    f'<td><span style="background:{link_color};color:#fff;padding:2px 8px;border-radius:4px">{linking}</span></td>'
                    f'<td style="font-size:0.85em">{libs or "<em>none</em>"}</td></tr>')
            html_parts.append("</table>")

        if venv_items:
            html_parts.append("<h3>Python virtual environments</h3>")
            for v in venv_items:
                ssp = v.get("system_site_packages", False)
                badge = ('<span style="background:#f44336;color:#fff;padding:2px 8px;border-radius:4px">system-site-packages</span>'
                         if ssp else
                         '<span style="background:#4caf50;color:#fff;padding:2px 8px;border-radius:4px">isolated</span>')
                html_parts.append(f'<h4><code>{v.get("path","")}</code> {badge}</h4>')
                pkgs = v.get("packages", [])
                if pkgs:
                    html_parts.append('<table><tr><th>Package</th><th>Version</th></tr>')
                    for p in pkgs[:20]:
                        html_parts.append(f'<tr><td>{p.get("name","")}</td><td>{p.get("version","")}</td></tr>')
                    if len(pkgs) > 20:
                        html_parts.append(f'<tr><td colspan="2"><em>+{len(pkgs)-20} more</em></td></tr>')
                    html_parts.append("</table>")
                else:
                    html_parts.append("<p>No packages found.</p>")

        if git_items:
            html_parts.append("<h3>Git-managed directories</h3>")
            html_parts.append('<table><tr><th>Path</th><th>Remote</th><th>Branch</th><th>Commit</th></tr>')
            for i in git_items:
                commit = i.get("git_commit", "")[:12]
                remote = i.get("git_remote", "")
                html_parts.append(
                    f'<tr><td><code>{i.get("path","")}</code></td>'
                    f'<td><code>{remote}</code></td>'
                    f'<td>{i.get("git_branch","")}</td>'
                    f'<td><code>{commit}</code></td></tr>')
            html_parts.append("</table>")

        if pip_items:
            html_parts.append("<h3>System pip packages</h3>")
            html_parts.append('<table><tr><th>Package</th><th>Version</th><th>Path</th></tr>')
            for i in pip_items[:20]:
                html_parts.append(
                    f'<tr><td>{i.get("name","")}</td>'
                    f'<td>{i.get("version","")}</td>'
                    f'<td><code>{i.get("path","")}</code></td></tr>')
            html_parts.append("</table>")

        if other_items:
            html_parts.append("<h3>Other non-RPM items</h3>")
            html_parts.append('<table><tr><th>Path</th><th>Confidence</th><th>Method</th></tr>')
            for i in other_items[:20]:
                html_parts.append(
                    f'<tr><td><code>{i.get("path","") or i.get("name","")}</code></td>'
                    f'<td>{i.get("confidence","")}</td>'
                    f'<td>{i.get("method","")}</td></tr>')
            html_parts.append("</table>")
    else:
        html_parts.append("<p>No non-RPM software.</p>")
    html_parts.append("</div>")

    # Kernel/Boot section
    html_parts.append('<div id="section-kernel_boot" class="section"><h2>Kernel and boot</h2>')
    kb = snapshot.kernel_boot
    if kb and (kb.cmdline or kb.sysctl_overrides or kb.non_default_modules
               or kb.modules_load_d or kb.modprobe_d or kb.dracut_conf):
        if kb.cmdline:
            cmdline_escaped = kb.cmdline[:300].replace("<", "&lt;").replace(">", "&gt;")
            html_parts.append(f"<p><strong>cmdline:</strong> <code>{cmdline_escaped}</code></p>")

        if kb.non_default_modules:
            total = len(kb.loaded_modules or [])
            default_count = total - len(kb.non_default_modules)
            html_parts.append(f"<h3>Non-default loaded modules ({len(kb.non_default_modules)})</h3>")
            html_parts.append('<table class="data-table"><thead><tr>'
                              '<th>Module</th><th>Size</th><th>Used by</th>'
                              '</tr></thead><tbody>')
            for m in kb.non_default_modules:
                name = m.get("name", "?")
                size = m.get("size", "")
                used = m.get("used_by", "")
                html_parts.append(f'<tr><td><code>{name}</code></td><td>{size}</td><td>{used}</td></tr>')
            html_parts.append("</tbody></table>")
            if default_count > 0:
                html_parts.append(f"<p>{default_count} module(s) at expected defaults (not shown).</p>")

        if kb.sysctl_overrides:
            html_parts.append(f"<h3>Non-default sysctl values ({len(kb.sysctl_overrides)})</h3>")
            html_parts.append('<table class="data-table"><thead><tr>'
                              '<th>Key</th><th>Runtime</th><th>Default</th><th>Source</th>'
                              '</tr></thead><tbody>')
            for s in kb.sysctl_overrides:
                key = s.get("key", "?")
                runtime = s.get("runtime", "?")
                default = s.get("default", "—")
                source = s.get("source", "")
                html_parts.append(
                    f'<tr><td><code>{key}</code></td>'
                    f'<td style="color:#d29922;font-weight:bold">{runtime}</td>'
                    f'<td>{default}</td><td><code>{source}</code></td></tr>'
                )
            html_parts.append("</tbody></table>")

        if kb.modules_load_d:
            html_parts.append(f"<p>modules-load.d: {len(kb.modules_load_d)} file(s)</p>")
        if kb.modprobe_d:
            html_parts.append(f"<p>modprobe.d: {len(kb.modprobe_d)} file(s)</p>")
        if kb.dracut_conf:
            html_parts.append(f"<p>dracut.conf.d: {len(kb.dracut_conf)} file(s)</p>")
    else:
        html_parts.append("<p>No kernel/boot customizations detected.</p>")
    html_parts.append("</div>")

    # SELinux section
    html_parts.append('<div id="section-selinux" class="section"><h2>SELinux / Security</h2>')
    se = snapshot.selinux
    if se and (se.mode or se.custom_modules or se.boolean_overrides or se.audit_rules or se.fips_mode):
        if se.mode:
            html_parts.append(f"<p><strong>Mode:</strong> {se.mode}</p>")
        if se.fips_mode:
            html_parts.append("<p><strong>FIPS mode:</strong> enabled</p>")

        if se.custom_modules:
            html_parts.append(f"<h3>Custom policy modules ({len(se.custom_modules)})</h3>")
            html_parts.append("<ul>")
            for m in se.custom_modules:
                html_parts.append(f"<li><code>{m}</code></li>")
            html_parts.append("</ul>")

        non_default = [b for b in (se.boolean_overrides or []) if b.get("non_default")]
        unchanged_count = len(se.boolean_overrides or []) - len(non_default)
        if non_default:
            html_parts.append(f"<h3>Non-default booleans ({len(non_default)})</h3>")
            html_parts.append('<table class="data-table"><thead><tr>'
                              '<th>Boolean</th><th>Current</th><th>Default</th><th>Description</th>'
                              '</tr></thead><tbody>')
            for b in non_default:
                name = b.get("name", "?")
                cur = b.get("current", "?")
                dflt = b.get("default", "?")
                desc = b.get("description", "")
                cur_cls = "color:#3fb950" if cur == "on" else "color:#f85149"
                html_parts.append(
                    f'<tr><td><code>{name}</code></td>'
                    f'<td style="{cur_cls};font-weight:bold">{cur}</td>'
                    f'<td>{dflt}</td><td>{desc}</td></tr>'
                )
            html_parts.append("</tbody></table>")
        if unchanged_count > 0:
            html_parts.append(f"<p>{unchanged_count} boolean(s) at default values (not shown).</p>")

        if se.audit_rules:
            html_parts.append(f"<p>Audit rule files: {len(se.audit_rules)}</p>")
        if se.pam_configs:
            html_parts.append(f"<p>PAM configs: {len(se.pam_configs)}</p>")
    else:
        html_parts.append("<p>No SELinux/security customizations detected.</p>")
    html_parts.append("</div>")

    # Users/Groups section
    html_parts.append('<div id="section-users_groups" class="section"><h2>Users and groups</h2>')
    if snapshot.users_groups and (snapshot.users_groups.users or snapshot.users_groups.groups):
        for u in (snapshot.users_groups.users or []):
            html_parts.append(f"<p>User: {u.get('name') or ''} (uid {u.get('uid') or ''})</p>")
        for g in (snapshot.users_groups.groups or []):
            html_parts.append(f"<p>Group: {g.get('name') or ''} (gid {g.get('gid') or ''})</p>")
    else:
        html_parts.append("<p>No non-system users/groups.</p>")
    html_parts.append("</div>")

    # Containerfile section (rendered content from output_dir/Containerfile)
    containerfile_escaped = (containerfile_content or "(Containerfile not generated)").replace("<", "&lt;").replace(">", "&gt;")
    html_parts.append('<div id="section-containerfile" class="section"><h2>Containerfile</h2><p>Generated image definition. Build with <code>podman build -t my-image .</code></p><pre class="containerfile-pre">' + containerfile_escaped + "</pre></div>")

    # Output files browser (config/, quadlet/)
    output_tree = _build_output_tree(output_dir)
    file_content_snippets: List[str] = []
    tree_html = _render_tree_html(output_tree, file_content_snippets)
    html_parts.append(
        '<div id="section-output_files" class="section"><h2>Output files</h2>'
        "<p>Browse <code>config/</code> and <code>quadlet/</code> generated in the output directory. Click a file to view its contents.</p>"
        '<div class="file-browser-layout">'
        '<div class="file-tree-panel"><div class="file-tree-root">' + tree_html + "</div></div>"
        '<div class="file-viewer-panel"><div class="file-path" id="file-viewer-path">Click a file</div><div id="file-viewer-content"></div></div>'
        "</div>"
        + "".join(file_content_snippets)
        + "</div>"
    )

    # Audit report section (audit-report.md rendered as HTML)
    audit_html = _markdown_to_html(audit_report_content) if audit_report_content else "<p>(Audit report not generated.)</p>"
    html_parts.append(
        '<div id="section-audit" class="section"><h2>Audit report</h2>'
        '<p>Full audit findings (from <code>audit-report.md</code>).</p>'
        f'<div class="audit-section">{audit_html}</div></div>'
    )

    # Warnings section
    html_parts.append('<div id="section-warnings" class="section"><h2>Warnings</h2><div class="search"><input type="text" id="warn-search" placeholder="Search warnings..."/></div><ul id="warnings-list">')
    for w in warnings:
        msg = (w.get("message") or w.get("detail") or "—")
        detail = (w.get("detail") or "")
        html_parts.append(f"<li><strong>{msg}</strong><br/><small>{detail}</small></li>")
    html_parts.append("</ul></div>")

    html_parts.append("""
<script>
(function(){
  var sections = document.querySelectorAll('.section');
  var cards = document.querySelectorAll('.card[data-section]');
  var tabs = document.querySelectorAll('.tabs button[data-tab]');
  function show(id) {
    if (!id) return;
    sections.forEach(function(s){ s.classList.remove('visible'); });
    var el = document.getElementById('section-' + id);
    if (el) el.classList.add('visible');
    tabs.forEach(function(t){ t.classList.toggle('active', t.getAttribute('data-tab') === id); });
  }
  cards.forEach(function(c){
    c.addEventListener('click', function(){ show(c.getAttribute('data-section')); });
  });
  tabs.forEach(function(t){
    t.addEventListener('click', function(){ show(t.getAttribute('data-tab')); });
  });
  show('summary');
  var warningBanner = document.getElementById('warning-banner');
  var bannerCount = document.getElementById('banner-warning-count');
  var panelCount = document.getElementById('warning-banner-count');

  var overflowCount = parseInt(warningBanner ? warningBanner.getAttribute('data-overflow-count') : '0', 10) || 0;

  function updateWarningCounts() {
    if (!warningBanner) return;
    var visibleRows = warningBanner.querySelectorAll('.warning-row[data-warning-idx]:not(.dismissed)');
    var panelVisible = visibleRows.length;
    var total = panelVisible + overflowCount;
    if (bannerCount) bannerCount.textContent = total;
    if (panelCount) panelCount.textContent = total;
    var cardCount = document.getElementById('warnings-card-count');
    if (cardCount) cardCount.textContent = total;
    var statusEl = document.getElementById('warnings-card-status');
    if (statusEl) statusEl.textContent = panelVisible > 0 ? 'Review required' : (overflowCount > 0 ? 'Panel cleared' : 'All dismissed');
    if (panelVisible === 0) {
      warningBanner.classList.add('collapsed');
    }
  }

  if (warningBanner) {
    warningBanner.querySelectorAll('.warning-row-dismiss').forEach(function(btn){
      btn.addEventListener('click', function(){
        this.closest('.warning-row').classList.add('dismissed');
        updateWarningCounts();
      });
    });
    var dismissAll = document.getElementById('warning-dismiss-all');
    if (dismissAll) {
      dismissAll.addEventListener('click', function(){
        warningBanner.querySelectorAll('.warning-row').forEach(function(row){
          row.classList.add('dismissed');
        });
        updateWarningCounts();
      });
    }
  }
  var search = document.getElementById('warn-search');
  if (search) {
    search.addEventListener('input', function(){
      var q = (this.value || '').toLowerCase();
      var listItems = document.querySelectorAll('#warnings-list li');
      listItems.forEach(function(li){
        li.style.display = (q === '' || (li.textContent || '').toLowerCase().indexOf(q) >= 0) ? '' : 'none';
      });
    });
  }
  document.querySelectorAll('.file-entry').forEach(function(btn){
    btn.addEventListener('click', function(){
      var id = this.getAttribute('data-content-id');
      var path = this.getAttribute('data-path');
      var pathEl = document.getElementById('file-viewer-path');
      var contentEl = document.getElementById('file-viewer-content');
      var src = id ? document.getElementById(id) : null;
      if (pathEl) pathEl.textContent = path || 'Click a file';
      if (contentEl) contentEl.innerHTML = src ? src.innerHTML : '';
    });
  });
})();
</script>
</body>
</html>""")

    (output_dir / "report.html").write_text("\n".join(html_parts))
