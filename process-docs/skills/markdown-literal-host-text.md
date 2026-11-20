# Host text in Markdown reports goes through `md_literal`

A host value interpolated into a Markdown report (a key, a value, a file
name, a link target, an unparsed line, a degradation reason) is data, not
Markdown. Printed bare, `<img src=x>` becomes an image, `[x](http://..)` a
link, a `|` splits a table row, and a newline in a file name ends the list
item or row it sits in.

`crates/pipeline/src/render/md_literal.rs` renders a value as an inline code
span nothing can reinterpret:

- `md_code(value)` for running text, `md_code_cell(value)` inside a GFM table
  cell. Never wrap a value in backticks by hand.
- Control characters other than tab, and U+2028/U+2029, become `\u{..}`
  escapes. Tab stays.
- The fence is one backtick longer than the longest backtick run in the
  value. The span is padded with one space at each end only when it has to
  be: the value holds a backtick, or begins and ends with a space. CommonMark
  strips exactly one space from each end of such a span, so padding keeps
  the literal exact; an all-space value is never stripped and is not padded.
- `md_code_cell` writes `|` as `\|`. GFM splits cells before inline parsing,
  honors that escape inside a code span and removes the backslash. Its
  pipe unescaping pairs each backslash with the byte after it and removes
  only the backslash of an unpaired `\|`, so a host `\|` cannot be written
  with backslashes alone: `\\|` reads back as two backslashes and a pipe.
  A backslash immediately before a pipe is therefore written `\u{5c}`, the
  helper's escape form, then the pipe as `\|`. No other backslash changes,
  and none is doubled: inside a code span one is already literal.
- An empty value prints `(empty)`.

Test it by parsing the rendered report with a GFM-conformant parser, not by
string matching: `crates/cli/tests/sysctl_scan_test.rs`
(`case_19_literal_rendering_audit`) parses the audit with `comrak` (a
`crates/cli` dev-dependency, `default-features = false`, table extension on)
and asserts each cell's code-node literal equals the value and the section
holds no HTML, link or image node.

The audit's `#### Sysctl` block uses it for every interpolation, and the
Warnings list in `README.md` and `audit-report.md` prints each warning
message as one `md_code` span: a message is prose with host values
already interpolated by the collector (repo file names, key paths, unit
names), so the renderer cannot escape only the host part. The inspector
name `README.md` prints in bold before each message is an `md_code` span
too: the scan sets it from code, but an imported snapshot can hold any
text there. The audit's Unowned Config Files list prints each path with
`md_code`. `crates/cli/tests/warning_markdown_test.rs` parses the reports;
`audit_host_paths_render_literally` is a table with one row per audit
list, so a site moved to `md_code` gets a row there.

These audit sites in `crates/pipeline/src/render/audit.rs` still print
host text raw inside hand-written backticks (pre-existing, not yet moved
to the helper): Modified RPM-Owned Files (each file's bold path), the kernel
command line, the duplicate-group note's package name, the SELinux summary
label, firewall direct rules (`args`), static routes, `/etc/hosts`
additions, proxy entries (source and line), storage credential references
(mount point and path), the unit-file outcome table's path, and the
`/var`/`/srv` path list.

## The HTML report is not Markdown

The HTML report's sysctl block (`templates/report/kernel.html`) states
the same sentences, but never through `md_code`: its backticks would
print as text. Every value goes through `md_literal::literal_text`, the
escaping `md_code` itself calls (control characters other than tab and
U+2028/U+2029 as `\u{..}`, `(empty)` for ""), with no fence and no pipe
escape. `report.rs` (`sysctl_runs`) turns each copy template into runs of
text: the copy's own backtick spans and every filled `{hole}` are `code`
runs, except `(empty)`, which is plain text as in the audit. The template
prints every run through MiniJinja's HTML autoescape, inside `<code>` for
a code run. Nothing in the block is marked `|safe`;
`sysctl_block_has_no_safe_filter` guards that.

MiniJinja's autoescape writes `/` as `&#x2f;` (and `'` as `&#x27;`), so a
raw `html.contains("/etc/sysctl.d/..")` never matches, and a negative
assertion on such a string passes whatever the page holds. Decode the
entities first, as `html_text` in `crates/cli/tests/sysctl_scan_test.rs`
does.
