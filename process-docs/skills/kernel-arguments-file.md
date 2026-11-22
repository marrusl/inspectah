# The kernel arguments file

inspectah carries the operator's kernel arguments from the host's
`/proc/cmdline` into the image as
`/usr/lib/bootc/kargs.d/inspectah-migrated.toml` (`KARGS_PATH`), written
by `kargs_toml` in `crates/pipeline/src/render/configtree.rs`. It is an
offer of the stager's decision (`StagedFileKind::KargsFile`), so both
export paths write it through `staged_writes` and the Containerfile's
kernel section copies it only when it is staged.

## bootc's format

bootc reads each `kargs.d/*.toml` as a table with a top-level `kargs`
array of strings and an optional `match-architectures` array, and its
parser refuses any other key:

```toml
# Migrated from kernel cmdline by inspectah
kargs = ["nosmt=force", "hugepages=16"]
```

Until beta.7 the writer emitted a `[kargs]` table with one
`append = ["..."]` line per argument. bootc does not read that, and with
two or more arguments it is not even valid TOML (a duplicate key). The
layout came over from the Go port and was never parsed by a test. **Test
the file by parsing it as bootc does**, never by `contains`: the unit test
in `configtree.rs` deserializes it into a `deny_unknown_fields` struct,
and `crates/cli/tests/kargs_file_export_test.rs` parses both export
paths' files with `toml`. `toml` is a dev-dependency only; production
builds the array with `users::toml_string`, the TOML basic-string escaper
the blueprint fragment uses.

## One model of a kernel argument

`safety::kernel_args` parses the command line the way the kernel's
`next_arg` (`lib/cmdline.c`) does, into `KernelArg` values with spans
into the line: the raw argument, the name before the first `=`, and the
value after it. A double quote opening the whole argument
(`"rd.iscsi.password=a b"`) and one opening the value
(`dyndbg="file x.c +p"`) are both quoting; whitespace inside quotes does
not end the argument. Every consumer reads arguments through it, never
by splitting on whitespace or matching raw text:

- **The boot loader filter** (`is_bootloader_karg`) compares the parsed
  name against `KARGS_BOOTLOADER_NAMES`, with `-` equal to `_` as the
  kernel's `parameq` has it, so `"root=UUID=x"` and `root="UUID=x"` are
  dropped like `root=UUID=x`. A raw prefix check let a quoted `root=`
  into kargs.d, where it can redirect root selection.
- **`operator_kargs`** also drops an argument holding a control
  character, or the byte `0xA0`, which the kernel's `isspace` counts as
  whitespace inside a UTF-8 character where `kernel_args` cannot split.
  It does not apply the shell filter: the file is TOML, and a retained
  argument is written in its original spelling through the escaper.
- **The `/etc/default/grub` comparison** (`settle_default_grub` in
  `crates/pipeline/src/default_grub.rs`) reads `GRUB_CMDLINE_LINUX` and
  `GRUB_CMDLINE_LINUX_DEFAULT` with a reader for plain shell assignments,
  then compares their `kernel_args` with the running ones by parsed name
  and value, skipping boot loader names. It runs in
  `finalize_cross_inspector`, not in the config inspector's modernization
  pass: that pass runs beside the kernel and boot inspector and cannot
  see its `/proc/cmdline`, while finalize has both sections and runs
  before redaction, so the values compared are the host's. It also keeps
  the file out of the export whatever the collector decided, so the
  exclusion does not rest on the modernization pass, which is skipped
  when the OS version does not parse.
- **Redaction** (`redact_kernel_cmdline` in `redaction/engine.rs`) finds
  secrets with `kernel_cmdline_matches`, which detection (`scan_content`)
  uses too. Matching is per argument: every pattern searches each
  `kernel_args` argument on its own and its offsets are moved back onto
  the whole line, so no match crosses the whitespace between arguments.
  Searched as one line, `rd.iscsi.password= systemd.setenv=DB_PASSWORD=x`
  gave the empty password the next argument as its value and hid the
  `DB_PASSWORD` secret inside it. An argument whose parsed name is a
  credential name (`patterns::is_credential_name`, the generic pattern's
  own key list, with `-` read as `_` as the kernel does) is found by name
  instead of by the generic pattern: its whole non-empty value is the
  secret, whatever it holds, since the generic pattern needs a non-space
  character after the `=` (`"rd.iscsi.password=<U+2003>bravo7k"` matched
  nothing) and takes the quotes of `password=""` for a value. An empty
  value or a name with no `=` is no finding. Redaction then replaces the
  whole parsed value of each argument a secret falls in (the whole
  argument when the secret touches the name), keeping the quotes: a
  pattern can stop at the first space, so replacing only its match left
  `"rd.iscsi.password=REDACTED_PASSWORD_1 bravo"` in the snapshot. The
  cmdline hint check reads the value to the end of the argument, quotes
  aside, for the same reason. The config-file filters (NSS/PAM words,
  `#` comment lines) do not run on the command line: a dropped first
  word (`"rd.iscsi.password=files bravo7k"`) meant no finding, so no
  argument was redacted at all.

## A redacted argument is owed, never written

Redaction rewrites `/proc/cmdline` in the snapshot, so an argument such
as `rd.iscsi.password=secret` arrives as
`rd.iscsi.password=REDACTED_PASSWORD_1`. A placeholder is not a usable
value, and on an iSCSI root it can stop the machine booting. So
`split_operator_kargs` sets aside every argument whose raw text matches
`REDACTION_TOKEN_RE` (`REDACTED_<KIND>_<N>`, as `token_for` writes it):
the file never holds one, and when all operator arguments are redacted
no file is written at all.

`owed_kernel_arguments` names each set-aside argument from its parsed
name with the whole value shown as `<value>`
(`rd.iscsi.password=<value>`), never from the raw text, so nothing of
the value is repeated whatever redaction left; an argument whose name
holds a placeholder, or that has no value, is `<value>` alone. The
provisioning surfaces in `render/machine_state.rs` carry it beside the
machine-state steps: the Containerfile's Provisioning section
(`# bootc install to-disk --karg rd.iscsi.password=<value>`), the
`## Provisioning steps` section of the audit report and README, and a
count-only notice in `export_notices`, which reaches the scan's and
aggregate's stderr, the refine export's `x-export-warnings` header and
the web view's machine-state notices. The `/proc/cmdline` line in
`secrets-review.md` points at those steps. The web view's
`machine_state_steps` list is machine-state rows only and does not carry
it.

Tests for this cover both quoting forms from the collector to both
export paths (`kargs_file_export_test.rs`) and assert each word of a
multi-word secret absent on its own, not only the whole value. Pick
words no export holds otherwise: the report's stylesheet contains
`alpha`.

A new redaction form on the command line (a marker other than
`REDACTED_<KIND>_<N>`) needs `REDACTION_TOKEN_RE` widened, or its
placeholder lands in the image.
