---
name: report-section-gating
description: The HTML report hides a section twice over, once in base.html when its data is absent and once in section.html's macro, which swallows the caller's whole body when the section failed. Text that does not come from the section's data, such as the sysctl explanation, needs both gates opened, or the HTML says less than the audit.
---

# HTML Report Section Gating

`templates/report/base.html` includes most sections behind
`{% if has_<x> or <x>_state == "failed" %}`, and
`templates/report/section.html`'s `section()` macro prints a generic
"Data collection failed for this section." body **in place of**
`caller()` when `state == "failed"`. Both gates assume everything inside
a section comes from that section's own data, which is right for most of
them: no data, nothing to say.

**It is wrong for text built from a model that also covers the
no-data case.** The sysctl explanation is one: `sysctl_report()` returns
`Unavailable { reason }` when `kernel_boot` (or its `sysctl`) is `None`,
and `Aggregate` on a fleet whether or not the merge kept a kernel/boot
section. The audit prints its Kernel & Boot heading unconditionally for
exactly that reason. With the HTML gates left in place, a host whose
kernel/boot section was absent or failed, and a fleet whose inputs had
none, got the audit's line in Markdown and in the refine view's DTO but
nothing, or only the generic failed body, in the HTML.

How the Kernel & Boot section handles it, and how any similar section
should:

1. **Include the template unconditionally** in `base.html`. The section
   then renders with its count badge at 0 and only the model-driven block.
2. **Put the model-driven block in a macro** in the section's template
   (`sysctl_block()` in `kernel.html`) and print it in the normal body.
3. **Pass the same macro's output as `failed_extra`** to `section()`. A
   failed section prints it after the generic failed body. MiniJinja
   macro output is already-escaped markup, so this needs no `|safe`, and
   the test that the kernel template marks nothing safe still holds.

Do not drop the failed-body branch to get the same effect: every other
section relies on it to say nothing from data that was not collected.

## Testing

A real scan cannot produce either state for kernel/boot: the scan
command always runs `KernelbootInspector`, and that inspector returns
its section as `Degraded` when a source fails, never `Failed`. The
sysctl harness (`crates/cli/tests/support/sysctl_hosts.rs`) builds them
through the real `collect()` path instead: `scan_without_kernel_boot`
leaves the inspector out, and `scan_with_failed_kernel_boot` runs a stub
under the `KernelBoot` id that returns `Failed`, which `collect()`
records the way it records a panic. `merge_snapshots` over two
`scan_without_kernel_boot` hosts gives the aggregate case. The tests
compare the HTML block's text with the audit's line, not with a
hand-written expectation, so the two surfaces cannot drift apart.
