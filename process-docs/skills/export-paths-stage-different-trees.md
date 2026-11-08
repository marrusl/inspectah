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
   through `config/` paths; it never did, and `configtree.rs` skips the
   quadlet prefix precisely so that it cannot.
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
   manifests only for high-confidence items. **A writer must stage
   exactly what the active `COPY` lines carry:** staging content for a
   commented-out block trips the reverse guard below, because the files
   ship in the export and never reach the image. The fixture carries a
   lockfile-only project for this case. Medium-confidence pip items
   still stage their `requirements.txt` under a commented block; no
   fixture exercises that yet.

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

- `unmanaged/` and `repoless-packages/` are re-extracted from the
  **source tarball**, not rendered from the snapshot, so a fixture
  without one cannot exercise them.
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
  `configtree.rs`'s `etc/containers/systemd/` prefix suppression is the
  standing example.

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
