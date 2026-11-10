# Archive extraction confinement

Any code that writes a path taken from a tarball, or from a snapshot that came
out of one, under a root directory must keep the write inside that root. The
refine export does this twice: step 2e copies `unmanaged/` and
`repoless-packages/` entries out of the source scan tarball, and step 2c writes
aggregate variant files named by snapshot paths. Both inputs come from the same
supplied tarball, so an allowlist built from the snapshot is not a check: the
attacker writes both halves of it.

## The primitives

`crates/pipeline/src/build/extract.rs` holds the shared pieces. Use them; do not
write another extractor's worth of checks inline.

| Primitive | Closes |
|---|---|
| `confined_rel(&str) -> Result<ConfinedRel, ArchiveViolation>` | `..`, root, prefix and leading `.` components. A component test, not a substring test: `foo..bar` is a legal name. |
| `SeenPaths::record(ConfinedRel, EntryKind)` | A second entry at a path already written (symlink then file, file then symlink). Repeated directory entries are allowed. Keys compare by component, so `a//b` equals `a/b`. |
| `create_confined_dir(root, ConfinedRel) -> io::Result<PathBuf>` | A symlink or non-directory at any ancestor, checked on disk with `symlink_metadata`, creating missing components one at a time with `create_dir`. Also catches a link an earlier export step left under the root, which no in-memory record would see. |
| `EntryKind::classify(tar::EntryType, &str)` | Hardlinks and special files (char, block, FIFO, unknown). |
| `OpenOptions::new().write(true).create_new(true)` on the leaf, inline at the call site | Any existing leaf, including a symlink, dangling or not. `O_EXCL` never follows a final link. |

`ConfinedRel` is a newtype so `create_confined_dir` and `SeenPaths` cannot be
handed an unchecked path. `ConfinedRel::parent()` of a one-component path is
empty, and `create_confined_dir` treats that as the root itself.

### Snapshot fields that name one file

A snapshot string that names a single file or directory under a fixed export
directory is the same class with a smaller input: a manifest file name in
`language_packages.rs`, a user name in `users/home/{name}/.ssh`, a timer or
quadlet unit name in `configtree.rs`. `Path::join` with an absolute name
replaces the base, and `..` leaves it. Check such a value with
`is_single_normal_component` before the join, and skip a refused one with a
warning (or a Containerfile FIXME where the Containerfile would otherwise
`COPY` the missing file). A multi-component snapshot path, such as a config
file's, goes through `validate_path` or `confined_rel` instead.

## Why each part is needed

- **Component check alone** misses a symlink entry followed by a regular file at
  the same path: every component is ordinary, and `File::create` follows the link.
- **A seen set alone** misses a symlink at `opt/d` followed by a file at
  `opt/d/x`: the two paths differ, and `create_dir_all` succeeds through a
  symlinked directory.
- **`create_new` alone** refuses an existing leaf, not a symlinked ancestor.
- **Canonicalize after the write is not containment.** It finds the escape after
  the victim was truncated and overwritten. `TarballExtractor` still has one, as
  defense in depth; do not copy it as the check.

On a case-insensitive filesystem (default macOS), `opt/x` and `opt/X` are one
file but two seen-set keys. The on-disk checks (`create_confined_dir` and
`create_new`) still refuse that case, which is one reason they are not optional.

## Symlink targets stay unrestricted for unmanaged files

The collector records an unmanaged symlink's target with `readlink -f`, so every
real target is an absolute path. Refusing absolute or `..` targets, as
`TarballExtractor` does for `inspectah build`, would fail every export from a
host with an unmanaged symlink. Do not apply that rule here. Targets are safe to
leave alone because nothing in the extraction writes through a link, and the
packer (`render/tarball.rs`) stores a link with `read_link` rather than following
it. Unmanaged symlinks are kept, not skipped: the export tree must agree with its
snapshot.

## The session loader's limits

`crates/refine/src/tarball.rs` `extract_safe` unpacks a supplied tarball with
`unpack_in` under entry-count and size limits. Count `entry.size()`, never
`entry.header().size()`: the `tar` crate extracts the PAX `size` when one is
present, so the ustar header can say 0 for an entry that unpacks to gigabytes.
The loader then reads `inspection-snapshot.json`, which extraction may have
left as a symlink to anything (`/dev/zero`, a FIFO): check it with
`symlink_metadata` before opening, and bound the read.

### Bound the decompressed stream, not only the entries

The `tar` crate (0.4.45) reads GNU long-name, GNU long-link and PAX records
whole into memory with an unbounded `read_to_end` before it returns the entry
they belong to. No check on an entry can bound them: the allocation is done by
the time there is an entry. So any reader of an untrusted tarball wraps the gzip
decoder in `BudgetReader` (`crates/pipeline/src/build/extract.rs`) before
handing it to `tar::Archive`, and keeps the `BudgetProbe`:

- **Every consumed byte counts**: headers, padding, extension records and
  skipped entry data. The refine budget is `MAX_STREAM_SIZE`, the unpacked
  limit plus `STREAM_OVERHEAD_PER_ENTRY` (8 KiB) per allowed entry.
- **Exhaustion is an error, never an end of stream**, so a cut-off archive is
  not mistaken for a complete one.
- **Classify errors with the probe.** `tar` wraps the reader's error in its
  own, so ask `probe.exhausted()` and map that to `ArchiveSafety`
  (`tarball::stream_error`). That includes errors from copying entry data,
  which are otherwise reported as write errors.
- **Keep the per-entry checks.** The stream bound is a backstop on memory;
  the entry-size and count limits still give the precise refusal.
- **Test with a small injected budget** (`extract_safe_with_budget`,
  `extract_payload_dirs_with_budget`), a record twice the budget, one case
  each for PAX, long name and long link. Before the bound, the long-name and
  long-link cases fail with an ordinary unpack error, not acceptance, so
  assert the `ArchiveSafety` variant, not just an error.

The bound caps memory at about the budget, not far below it: one record can
still be most of 590 MiB. `inspectah build`'s `TarballExtractor` and
`aggregate`'s snapshot reader do not use the wrapper yet.

## Trust assumption

The ancestor walk and the leaf open are not atomic. That is safe only because
the export root is a fresh `tempfile::tempdir()` that this process alone writes.
If a root ever becomes caller-supplied, this needs `openat`-style resolution
(`openat2(RESOLVE_BENEATH)` on Linux) instead.

## The guard in `copy_proved_uploads`

Its `symlink_metadata(&dest_dir).is_dir()` check has no known reaching input
once 2c and 2e are confined. Keep it: `create_dir_all` writes through a
symlinked directory, and the next export step that can leave one there will not
announce itself.

## Testing a confinement fix

Call the writer directly with a controlled root (`sandbox/export`) and a seeded
victim outside it (`sandbox/victim/authorized_keys`). Every refusal case asserts
three things: the call errs, the victim still reads its original bytes, and a
walk of the sandbox outside the root is unchanged (catches files and directories
created outside). Check containment before the error, so a failure reports the
damage. Build traversal headers by writing the name field directly, since
`Header::set_path` refuses `..`. Include positive cases (an absolute-target
symlink survives intact, a `foo..bar` name extracts) to catch over-refusal. The
cases live in `session.rs` under `export_write_confinement`.

Test each protection on its own: an ancestor-check test must aim at a leaf that
does not exist, or `create_new` refuses the existing victim and masks a missing
ancestor check; a leaf test must plant the link on disk, not in the tarball, or
the seen set refuses it first.
