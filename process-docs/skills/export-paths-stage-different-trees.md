# Two export paths, one Containerfile renderer

There are two ways a user gets a buildable export, they stage their
trees with different code, and both render their Containerfile from the
same `crates/pipeline/src/render/containerfile.rs`. Every `COPY` the
renderer can emit therefore has to be satisfied twice, by two writers
that nothing forces to agree.

| | CLI scan tarball | refine web download |
|---|---|---|
| staged by | `render::render_all` (`crates/pipeline/src/render/mod.rs`) | `render_refine_export` (`crates/refine/src/session.rs`) |
| reached via | `inspectah scan` → `run_pipeline` | `POST /api/tarball` |
| extra steps | none | uploads, payload re-extraction from the source tarball, aggregate variants, compose, and a **top-level allowlist that deletes anything not named in it** |

## The failure this produces

A `COPY` whose source one path writes and the other does not. The build
dies at that step naming a path, with nothing earlier in the log
pointing at the export, and `x-export-warnings` empty. Four instances
to date, all found by hand on a real host rather than by the suite:

1. `config/usr/lib/tmpfiles.d/inspectah-var.conf` — written by
   `render_all` only. Fixed by extracting `write_tmpfiles_conf`, which
   now carries the warning comment that is the short form of this file.
2. `quadlet/` — written by `write_config_tree` on **both** paths, then
   deleted on the web path by the step-`2h` allowlist. The comment
   justifying the deletion claimed the Containerfile reached quadlets
   through `config/` paths; it never did, and `configtree.rs` leaves
   every recorded quadlet to the containers section precisely so that it
   cannot.
3. `language-packages/<eco>/<hash>/…` — the mirror image. The manifest
   writer lived in the refine crate, so the **CLI** tarball emitted the
   `COPY` and wrote nothing. Moved to
   `render::language_packages::write_language_package_manifests`,
   beside the renderer that emits those lines, and called from both.
4. An uploaded RPM whose staged name could not be proved: the export
   emitted the `COPY` anyway. That one now demotes to a FIXME.

5. A high-confidence npm project held one manifest (a lockfile with no
   `package.json`), so the renderer emitted an active `COPY` for a file
   the writer never staged. `npm ci` needs both files anyway (npm 11
   exits `EUSAGE` with either alone), so the collector now marks a
   project high only when it read both, and the writer stages npm
   manifests only for high-confidence items. The renderer and the writer
   both ask `npm_item_is_active`, which also requires both manifests in
   `manifest_files`, because a snapshot the earlier collector wrote can
   still say high over one file. The below-high block carries no `COPY`
   at all, commented or not, since nothing stages its files; it names a
   missing manifest instead. **A writer must stage
   exactly what the active `COPY` lines carry:** staging content for a
   commented-out block trips the reverse guard below, because the files
   ship in the export and never reach the image. The fixture carries a
   lockfile-only project for this case. Medium-confidence pip items
   still stage their `requirements.txt` under a commented block; no
   fixture exercises that yet.
6. A package from a repo the host keeps disabled installs from that repo
   with `--enablerepo`, so its block carries no `COPY` of a cached RPM.
   All three `repoless-packages/` writers (the scan tarball's, refine's
   payload extraction, refine's upload copy) still staged one. They now
   ask `render::repoless::route_for`, the one routing decision the
   renderer reads too, and stage a file only for the route that copies
   it. The scan tarball's writer moved to
   `render::repoless::write_cached_rpms` for the reason in the next
   section, and the guard calls it.

## What to do when adding a staged directory

Put the writer in `crates/pipeline/src/render/` beside the function
that emits its `COPY` lines, and call it from both paths. A writer that
can only be called from one of them is the defect, written down.

If you add a top-level name to the refine export, add it to
`allowed_top_level` in the same change. Every name in that list is one
the renderer can `COPY`; omitting one does not produce a smaller
archive, it produces an archive that cannot build.

## The guard, and the fixture that makes it mean something

`crates/cli/tests/copy_source_exists_test.rs` asserts the invariant on
both paths: every `COPY <src>` the renderer emits has a `<src>` in the
export tree rendered alongside it.

**The fixture is half of it.** A snapshot with no quadlets, no drop-ins
and no language packages satisfies the invariant vacuously, which is how
defects 2 and 3 survived two green suites for months. The fixture there
is collector-shaped, built through the three calls `scan.rs` makes, and
a companion test fails if the renderer stops emitting
`COPY quadlet/`, `COPY drop-ins/` or `COPY language-packages/npm/` over
it. Extend that list when you add a staged directory.

Two things the guard does **not** cover, deliberately:

- `unmanaged/` is re-extracted from the **source tarball**, not
  rendered from the snapshot, so a fixture without one cannot exercise
  it. `repoless-packages/` is covered since 2026-11-08: the fixture
  carries two cached repo-less RPMs, one `@commandline` and one from a
  disabled repo, the CLI half stages them through `write_cached_rpms`,
  and the refine half exports from a source tarball packed from that
  tree, with the disabled-repo RPM added as a stale scan would have left
  it.
- ~~A file the export silently drops with no `COPY` emitted for it.~~
  **Covered since 2026-11-04** by `assert_every_staged_file_is_copied` in
  the same file, which asks the mirror question: does some `COPY` carry
  *this staged file* into the image. It found a real one — deleting
  `COPY flatpak/ /usr/share/inspectah/flatpak/` leaves the provision
  service copied and enabled while the manifest it reads at boot never
  ships, and the build still succeeds.
- **Still uncovered:** a file the export never stages *and* never
  references. Both guards read what is on disk or in the Containerfile,
  so content that disappears before either sees it is invisible to both.
  The historical example is `configtree.rs`'s old
  `etc/containers/systemd/` prefix suppression, which dropped every
  configuration entry under the quadlet directory, drop-ins and stray
  files included, until 2026-11-22. It is pinned now by two tests in
  `copy_source_exists_test.rs` that name the files they expect rather
  than reading the tree: `non_quadlet_files_under_the_quadlet_directory_are_carried_on_both_paths`
  checks those files reach `config/` on both export paths, and
  `an_excluded_quadlet_is_not_carried_back_as_configuration` checks a
  quadlet excluded in refine is not staged again through its config row.
  Any other loss of this kind still needs its own named-file test.

## An unmanaged payload's mode crosses four writers

`COPY` gives the image file the mode of its build-context copy, so an
unmanaged payload's permission bits have to survive every hop, and until
2026-11-23 they survived none past the first: every member packed 0644,
which stripped exec bits from `/opt` scripts and made a 0600 `.env`
world-readable. The mode is read once, from the host file, and then
carried, never re-derived:

1. **Captured** by `bundle_unmanaged_files` (`render/unmanaged.rs`), which
   sets the bundled copy's mode from the host file masked with
   `PAYLOAD_MODE_BITS` (0o777: setuid, setgid and sticky dropped). The
   snapshot's `permissions` field is not what ships; it is the walk's
   `stat` record.
2. **Packed** by `create_tarball` (`render/tarball.rs`) from the on-disk
   mode, for regular files under `unmanaged/` only
   (`is_unmanaged_payload`, compared by component). Everything else the
   render wrote packs 0644, so a restrictive umask never reaches `config/`.
3. **Re-staged** by refine's `extract_payload_dirs_with_budget`
   (`session.rs`), which sets the extracted copy to the member's mode, so
   its re-pack through step 2 keeps it.
4. **Applied** by `TarballExtractor::extract` (`build/extract.rs`), which
   sets the build-context copy to the member's mode for the same paths.

A new writer of `unmanaged/` files carries the mode or the image gets the
writer's umask. `unmanaged_files_keep_the_host_mode_on_both_export_paths`
(`crates/cli/tests/unmanaged_redaction_test.rs`) checks every hop on both
paths.

Ownership crosses none of the four: every member packs as uid 0, and no
`COPY` has `--chown`, so every payload is `root:root` in the image. Until
ownership is carried, `render/unmanaged_owners.rs` names each carried
regular file whose recorded `uid` or `gid` is not 0, or whose recorded
mode has setuid, setgid or sticky, in the audit report's Software
section, with a count in the Containerfile's unmanaged block and on the
scan's stderr. The Containerfile's count takes its files from
`unmanaged::copied_files`, the grouped `COPY` decisions the renderer
emits, never a per-file path check: a refused `COPY` carries nothing,
and a directory `COPY` carries a file whose own path would be refused,
along with anything else under it by path component. The "not copied"
FIXMEs read the same decision (`unmanaged::is_copied`), so a FIXME is
emitted only for a path no successful `COPY` carries, nested files,
directories and symlinks under a carried ancestor included. The `ln`
lines read it too: a symlink a `COPY` carries gets no `ln` line, since
the `COPY` already recreates it with its recorded target, and every `ln`
the renderer emits is `ln -sfn`. Without `-n`, `ln -sf` onto an existing
symlink to a directory follows it and creates the link inside that
directory (`current -> v2` becomes `v2/v2 -> v2`), in GNU and BSD `ln`
alike; `a_rendered_ln_replaces_a_link_to_a_directory_instead_of_following_it`
runs the rendered command to check. A real directory at the destination
still gets the link inside it; `-T` would fail the build instead, but is
GNU only. The ownership count reads the walk's `stat` record (`provenance`), not the
bundled copy, which has already lost the special bits. Directories are
not named: the walk records files only. A change that starts carrying
ownership or special bits must shrink or remove this notice in the same
commit, or it reports a loss that no longer happens.

## The COPY invariant has two directions and they need different granularity

Learned 2026-11-04 by running mutations against both halves. Getting this
wrong in either direction produces a guard that is useless or that people
weaken.

- **Forward** (every `COPY` source exists) is checked **per COPY line**.
  A missing source stops `podman build` at that line, so each line is
  independently load-bearing.
- **Reverse** (every staged file is carried) is checked **per staged
  file, with directory-prefix matching**, and must not be checked per
  COPY line. The config tree is copied by root: `write_config_tree`
  returns the roots it materialized and the renderer emits one
  `COPY config/<root>/ /<root>/` for each, so deeper lines are
  redundant. `COPY config/etc/systemd/system/ ...` and
  `COPY config/usr/lib/bootc/kargs.d/...` can both be deleted with no
  observable effect, because `COPY config/etc/ /etc/` and
  `COPY config/usr/ /usr/` already carry those paths.

**The general lesson, which cost a wrong finding in a report before it was
caught:** a surviving mutant is only evidence of a coverage gap once you
have shown the mutation changes observable behaviour. A mutation that
deletes genuinely redundant code is an *equivalent mutant*, and the suite
is right not to fail. Check equivalence before writing the finding up.

## The same check against a real export

```
grep '^COPY ' Containerfile | sed "s/^COPY //" | while read -r line; do
  src=$(printf '%s' "$line" | awk '{print $1}' | sed "s/^'//; s/'$//")
  [ -e "$src" ] || echo "MISSING $src"
done
```

Run it on an extracted export before `podman build`. It costs a second
and names every missing source at once, where the build names only the
first.
