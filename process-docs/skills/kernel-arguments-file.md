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

## Which arguments

`safety::operator_kargs` splits the command line the way the kernel
does: on whitespace except inside double quotes, so
`dyndbg="file x.c +p"` is one argument and is carried with its quotes.
It drops boot loader and installer arguments (`is_bootloader_karg`) and
any argument holding a line break or other control character. It does
not apply the shell filter: the file is TOML, not a `RUN` line, and
escaping is the escaper's job. Refusing `"` or a space there silently
dropped real arguments.

## A redacted argument is owed, never written

Redaction rewrites `/proc/cmdline` in the snapshot, so an argument such
as `rd.iscsi.password=secret` arrives as
`rd.iscsi.password=REDACTED_PASSWORD_1`. A placeholder is not a usable
value, and on an iSCSI root it can stop the machine booting. So
`split_operator_kargs` sets aside every argument matching
`REDACTION_TOKEN_RE` (`REDACTED_<KIND>_<N>`, as `token_for` writes it):
the file never holds one, and when all operator arguments are redacted
no file is written at all.

`owed_kernel_arguments` names each set-aside argument with its
placeholders shown as `<value>` (`rd.iscsi.password=<value>`), and the
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

A new redaction form on the command line (a marker other than
`REDACTED_<KIND>_<N>`) needs `REDACTION_TOKEN_RE` widened, or its
placeholder lands in the image.
