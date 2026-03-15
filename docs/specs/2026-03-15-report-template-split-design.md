# Report Template Split — Design Spec

**Date:** 2026-03-15
**Status:** Approved
**Scope:** Split `templates/report.html.j2` (2,556 lines) into Jinja2 include
partials — one per content section, plus CSS, JS, and macros.

## Motivation

`report.html.j2` is the largest file in yoinkc. It contains embedded CSS
(220 lines), JavaScript (656 lines), 3 macros, and 20+ content sections in a
single template. Fleet prevalence UI added significant complexity. Splitting
into partials improves navigability without changing the rendered output — the
HTML report remains a single self-contained file.

## Approach

Use Jinja2 `{% include %}` to split content sections into partials under a
`templates/report/` subdirectory. Macros move to a shared file and are imported
via `{% from ... import ... %}` in each partial that needs them. The main
`report.html.j2` becomes a ~50-line skeleton of includes.

## File Layout

```
templates/
├── patternfly.css                  # existing, unchanged
├── report.html.j2                  # skeleton (~50 lines)
└── report/
    ├── _macros.html.j2             # section, fleet_cell, data_table
    ├── _css.html.j2                # custom CSS (220 lines)
    ├── _js.html.j2                 # toolbar, exclusions, sidebar JS (656 lines)
    ├── _sidebar.html.j2            # sidebar navigation
    ├── _banner.html.j2             # banner + fleet banner
    ├── _summary.html.j2            # readiness summary / triage
    ├── _file_browser.html.j2       # file browser section
    ├── _packages.html.j2           # packages (169 lines)
    ├── _services.html.j2           # services (90 lines)
    ├── _config.html.j2             # config files (82 lines)
    ├── _non_rpm.html.j2            # non-RPM software (137 lines)
    ├── _containers.html.j2         # containers (138 lines)
    ├── _users_groups.html.j2       # users / groups (101 lines)
    ├── _scheduled_jobs.html.j2     # scheduled jobs (99 lines)
    ├── _kernel_boot.html.j2        # kernel / boot (90 lines)
    ├── _selinux.html.j2            # SELinux (78 lines)
    ├── _secrets.html.j2            # secrets (22 lines)
    ├── _network.html.j2            # network (126 lines)
    ├── _storage.html.j2            # storage (13 lines)
    ├── _warnings.html.j2           # warnings (27 lines)
    ├── _audit_report.html.j2       # audit report with all 13 subsections (270 lines)
    ├── _containerfile.html.j2      # containerfile section (~17 lines)
    └── _toolbar.html.j2            # sticky toolbar markup
```

### Design Decisions

- **Partials in `report/` subdirectory** — keeps `templates/` clean (it also
  has `patternfly.css`). Leading underscores signal these aren't standalone
  templates.
- **Audit report as one partial** — its 13 subsections are tiny (12-41 lines
  each) and form a cohesive unit. Splitting further would create ~13 files
  averaging 20 lines each.
- **CSS and JS in their own partials** — JS alone is 656 lines (the single
  biggest block). Extracting both makes the skeleton readable.
- **Toolbar markup separate from JS** — the sticky toolbar HTML (L1877-1898)
  is distinct from the `<script>` block (L1899-2554).

## Macro Handling

Jinja2's `{% include %}` gives partials access to context variables but NOT
to macros defined in the parent template. Macros must be shared via
`{% import %}`.

**`report/_macros.html.j2`** contains:
- `section(id, title, visible=false)` — PF6 card wrapper using `{{ caller() }}`
- `fleet_cell(item)` — prevalence bar cell, conditional on `fleet_meta`
- `data_table(headers)` — PF6 table wrapper using `{{ caller() }}`

Each partial imports only the macros it uses:

| Macro usage                              | Partials                                                    |
|------------------------------------------|-------------------------------------------------------------|
| `section` + `fleet_cell`                 | packages, services, config, containers, scheduled_jobs, network |
| `section` only                           | summary, file_browser, non_rpm, users_groups, kernel_boot, selinux, secrets, storage, warnings, audit_report |
| None                                     | css, js, sidebar, banner, containerfile, toolbar            |

Note: `data_table` is defined in `_macros.html.j2` but never called anywhere
in the template. It is preserved as dead code; no partial imports it.

Import pattern:
```jinja2
{% from "report/_macros.html.j2" import section, fleet_cell, data_table %}
```

## Context Flow

No changes to the renderer (`html_report.py`). The context dict passed to
`template.render()` is unchanged. `{% include %}` gives each partial access
to all context variables automatically:

- `snapshot`, `counts`, `fleet_meta`, `triage`, `os_desc`, `meta`
- `warnings`, `warnings_panel`, `warnings_overflow`
- `containerfile_html`, `containerfile_lines`
- `tree_html`, `file_content_snippets_html`
- `config_files_rendered`, `non_rpm_data`, `containers_data`
- `triage_detail`, `patternfly_css`

## Skeleton Structure

After splitting, `report.html.j2` becomes:

```jinja2
{# report.html.j2 — yoinkc self-contained inspection report.
   Split into partials under report/. See report/_macros.html.j2 for shared macros.
   Rendered by renderers/html_report.py via _build_context().
#}
<!DOCTYPE html>
<html lang="en" class="pf-v6-theme-dark">
<head>
  <meta charset="UTF-8">
  <title>yoinkc inspection report — {{ meta.hostname }}</title>
  <style>{{ patternfly_css }}</style>
  {% include "report/_css.html.j2" %}
</head>
<body>
<div class="pf-v6-c-page">
  <header class="pf-v6-c-masthead">
    {# masthead markup kept inline — small, structural #}
    ...
  </header>
  {% include "report/_sidebar.html.j2" %}
  <main class="pf-v6-c-page__main">
  <section class="pf-v6-c-page__main-section">
    {% include "report/_banner.html.j2" %}
    {% include "report/_summary.html.j2" %}
    {% include "report/_file_browser.html.j2" %}
    {% include "report/_packages.html.j2" %}
    {% include "report/_services.html.j2" %}
    {% include "report/_config.html.j2" %}
    {% include "report/_non_rpm.html.j2" %}
    {% include "report/_containers.html.j2" %}
    {% include "report/_users_groups.html.j2" %}
    {% include "report/_scheduled_jobs.html.j2" %}
    {% include "report/_kernel_boot.html.j2" %}
    {% include "report/_selinux.html.j2" %}
    {% include "report/_secrets.html.j2" %}
    {% include "report/_network.html.j2" %}
    {% include "report/_storage.html.j2" %}
    {% include "report/_warnings.html.j2" %}
    {% include "report/_audit_report.html.j2" %}
    {% include "report/_containerfile.html.j2" %}
  </section>
  </main>
</div>
{% include "report/_toolbar.html.j2" %}
{% include "report/_js.html.j2" %}
</body>
</html>
```

The exact skeleton structure (which HTML elements wrap the includes) must be
verified against the original template during implementation. The above is
illustrative — the implementer should read the original and preserve the
exact element nesting.

## Source Line Map

| Lines     | Content                          | Destination               |
|-----------|----------------------------------|---------------------------|
| 1-16      | Comment header                   | `report.html.j2` (comment)|
| 17-28     | `section` macro                  | `_macros.html.j2`         |
| 29-48     | `fleet_cell` macro               | `_macros.html.j2`         |
| 49-56     | `data_table` macro               | `_macros.html.j2`         |
| 57-63     | DOCTYPE, html, head open         | `report.html.j2`          |
| 64        | PatternFly CSS embed             | `report.html.j2`          |
| 65-284    | Custom CSS                       | `_css.html.j2`            |
| 285-288   | Head close, body open, page div  | `report.html.j2`          |
| 289-297   | Masthead header                  | `report.html.j2` (inline) |
| 298-340   | Sidebar navigation               | `_sidebar.html.j2`        |
| 341-380   | Banner                           | `_banner.html.j2`         |
| 381-417   | Summary                          | `_summary.html.j2`        |
| 418-434   | File browser                     | `_file_browser.html.j2`   |
| 435-603   | Packages                         | `_packages.html.j2`       |
| 604-693   | Services                         | `_services.html.j2`       |
| 694-775   | Config                           | `_config.html.j2`         |
| 776-912   | Non-RPM software                 | `_non_rpm.html.j2`        |
| 913-1050  | Containers                       | `_containers.html.j2`     |
| 1051-1151 | Users / Groups                   | `_users_groups.html.j2`   |
| 1152-1250 | Scheduled jobs                   | `_scheduled_jobs.html.j2` |
| 1251-1340 | Kernel / Boot                    | `_kernel_boot.html.j2`    |
| 1341-1418 | SELinux                          | `_selinux.html.j2`        |
| 1419-1440 | Secrets                          | `_secrets.html.j2`        |
| 1441-1566 | Network                          | `_network.html.j2`        |
| 1567-1579 | Storage                          | `_storage.html.j2`        |
| 1580-1606 | Warnings                         | `_warnings.html.j2`       |
| 1607-1853 | Audit report (13 subsections)    | `_audit_report.html.j2`   |
| 1855-1871 | Containerfile                    | `_containerfile.html.j2`  |
| 1873-1876 | Closing tags (section/main/page) | `report.html.j2`          |
| 1877-1898 | Sticky toolbar markup            | `_toolbar.html.j2`        |
| 1899-2554 | JavaScript                       | `_js.html.j2`             |
| 2555-2556 | Close body, html                 | `report.html.j2`          |

## Backward Compatibility

Zero breaking changes:

- `env.get_template("report.html.j2")` continues to work — same file, same
  path.
- `FileSystemLoader` already points at `templates/`, so the `report/`
  subdirectory is automatically discoverable for `{% include %}` and
  `{% import %}`.
- The rendered HTML output is byte-identical — includes are resolved at
  render time.
- No changes to `html_report.py` or any other Python code.

## Testing Impact

- **`test_renderer_outputs.py::TestHtmlReport`** tests rendered HTML
  (structure, section IDs, warnings panel, etc.). Since output doesn't
  change, these are the correctness check.
- **No new tests** needed — the existing suite verifies output identity.

## Migration Strategy

Single atomic commit:

1. Create `templates/report/` directory with all partials
2. Replace `report.html.j2` content with the skeleton
3. Run tests to verify output identity
