# RPM Output: Never Split Paths on Whitespace

`rpm` quotes nothing in its output and paths may contain spaces. Any
parser that recovers a path by splitting a line on whitespace is wrong,
including "take the first field," which looks safe because the path
comes first.

Ask `rpm` for one value per line instead:

```rust
// crates/collect/src/rpm_ownership.rs
const OWNED_PATHS_QUERY_FORMAT: &str = "[%{FILENAMES}\\n]";
exec.run("rpm", &["--query", "--all", "--qf", OWNED_PATHS_QUERY_FORMAT]);
```

The whole line is then the path. Do not `trim()` it either: leading and
trailing spaces are legal in a path.

## Why positional parsing of `--dump` cannot be fixed

`rpm --query --all --dump` emits:

```
PATH SIZE MTIME DIGEST MODE OWNER GROUP ISCONFIG ISDOC RDEV SYMLINK
```

Splitting on whitespace and taking field 0 truncates any owned path
containing a space. Counting ten fields from the right does not rescue
it: the trailing `SYMLINK` field is a link target, which can contain
spaces too. There is no delimiter that separates the ends reliably, so
`--dump` cannot be parsed positionally at all. Only ask for `--dump` if
something actually consumes the size/mode/digest columns.

## This is not a hypothetical

Firmware packages name files after the hardware they drive, spaces
included. `brcmfmac-firmware` on RHEL 10 owns:

```
/usr/lib/firmware/brcm/brcmfmac43241b4-sdio.Intel Corp.-VALLEYVIEW C0 PLATFORM.txt.xz
```

Under first-field parsing this truncated to `...-sdio.Intel`, so the real
path was missing from the owned set and the `/usr` walk reported the file
as unmanaged content for the user to act on. That host had 33 such paths
against 62 total reported `/usr` entries: 53 percent false positives, in
shipped code, because every unit test fixture used space-free paths.

**Test fixtures for anything that parses paths must include a path with
an embedded space.** That single omission is what let this ship.

## The rpmdb records pre-usrmerge paths

A second way a path that rpm owns can miss the owned set. Packages built
without usrmerge record `/lib/...`, `/lib64/...`, `/bin/...` or
`/sbin/...` (on RHEL 10, `kernel-core` records `/lib/modules/<ver>` and
libgcc records `/lib64/libgcc_s.so.1`), while a walk of the filesystem
sees `/usr/lib/...`. `rpm -qf` resolves the symlink and answers correctly;
a set lookup does not. `build_rpm_owned_set` therefore inserts the `/usr`
form of any path under `USRMERGE_LEGACY_PREFIXES` beside the recorded
form. Any new consumer that compares walked paths against rpm file lists
must go through that function, not its own parse of `rpm` output.

## Batched `--whatprovides` output says nothing about which argument it answers

`rpm -q --whatprovides cap1 cap2 cap3` prints a flat list of provider
NEVRAs, one or more per argument, with no marker between arguments. A
capability with two providers and two capabilities with one provider each
print the same thing. The output of a batch therefore cannot be mapped back
to the capability each line answers.

The dependency graph behind leaf/auto classification (`classify_deps_rpm`
in `inspectors/rpm/mod.rs`) once resolved every unique capability of every
added package in batches of 50 and credited every provider in a batch to
every capability in it. That was harmless while each batch held one
package's capabilities, because all those providers were that package's
dependencies anyway. Once the batches mixed capabilities from many
packages, each package depended on the providers of its neighbours'
capabilities, and the closure `classify_leaf_auto` takes spread that to
almost everything. On a RHEL 9.8 host, 25 of 26 leaves had the same
114-package dependency list: `tree`, `htop` and `wget` each "depended on"
`nginx-core`, `httpd-core` and `rsyslog`. `pulled_in_by_included` reads
that tree, so the service tiers and the disabled-repo selection then
treated packages as present that nothing installs.

**The requirement: each capability maps to its own providers only.** The
collector now builds one provides index from the rpmdb and looks each
capability up in it:

```rust
const RPM_PROVIDES_INDEX_FORMAT: &str = "[%{=NAME}.%{=ARCH}\\t%{PROVIDENAME}\\n]";
exec.run("rpm", &["-qa", "--queryformat", RPM_PROVIDES_INDEX_FORMAT]);
```

One line per provide, identity first, so a line splits on its first tab and
the arch comes straight from the header rather than from parsing a NEVRA.
The `=` modifier repeats a scalar for every array element; without it rpm
rejects scalar and array tags in one `[...]`. A failed index returns `None`
so classification falls back to dnf, rather than reading as a graph with no
edges.

Capabilities are matched by name with version constraints stripped, as
`--whatprovides <name>` matches them. A capability with several providers
credits all of them, which over-counts where only one of a set of
alternatives is needed.

## What the dependency tree counts as pulled in

`leaf_dep_tree` feeds `pulled_in_by_included`, which decides which
excluded packages are still in the image. It has to count everything dnf
installs alongside a leaf, not only hard requirements, or packages dnf
does install read as not carried. An edge `a -> b` in the graph means
installing `a` with dnf defaults installs `b`. The requires query asks for
three tags, each in its own `[...]` group with a one-letter tag first:

```rust
const RPM_DEPS_FORMAT: &str = "@@%{NAME}\\n\
                               [R\\t%{REQUIRENAME}\\n]\
                               [W\\t%{RECOMMENDNAME}\\n]\
                               [S\\t%{SUPPLEMENTNAME}\\n]";
```

A package with no entries for a tag prints nothing for that group
(measured on rpm 4.14.3 in UBI 8 and 4.16.1.3 in UBI 9), so no `(none)`
marker is glued to the next `@@` header.

**Weak dependencies are installed on rhel-bootc.** Measured 2026-11-08 on
`registry.redhat.io/rhel9/rhel-bootc:9.8` and `rhel10/rhel-bootc:10.2`:
both run dnf4 with `install_weak_deps` in effect (default, not set in
`dnf.conf`), and `dnf install -y httpd` printed `Installing weak
dependencies: mod_http2, mod_lua` on both. So:

- **Requires** (`R`): an edge to each added provider.
- **File requirements** (`/usr/bin/perl`, `/etc/mime.types`): no package
  lists these in its provides; `--whatprovides` answers them from file
  lists. The tree does the same from `rpm.file_ownership`, which the
  inspector already queries (now before the tree, not after), plus
  explicit file provides in the index (bash provides `/bin/sh`). Both
  sides are compared in their `/usr` form. No extra process: a single
  `rpm -qf a b c` would have the same attribution problem as batched
  `--whatprovides`, and adding `%{FILENAMES}` to the provides index would
  duplicate the ownership query. Ownership is keyed by name, so a
  multilib pair that both list a path are both credited, as
  `--whatprovides` would.
- **Recommends** (`W`): treated as Requires.
- **Supplements** (`S`): the reverse. `b` Supplements `x` means `b` is
  installed when `x` is, so the edge runs from each added provider of `x`
  to `b`, and only when the whole expression is satisfied on the host.
- **Rich (boolean) dependencies** (`rich_dep.rs`): evaluated against the
  installed host, not solved. `(a if c)` pulls `a` only when some
  installed package (base image included) provides `c`, and never pulls
  `c`; `unless` and `else` likewise. `(a or b)` pulls the first installed
  operand for a requirement and every installed one for a Supplements.
  `and` and `with` pull every operand; `without` pulls its left side.

**Still excluded:** Suggests and Enhances (dnf does not install them by
default). A rich expression that mixes operators in one group, which rpm
rejects as well, is dropped. The tree is reachability only: it does not
record which kind of edge put a package there.

A test for each of these must fail on the previous graph: a fixture
whose only route to a package is a requirement form the old parser
dropped, not one also reachable through a plain capability.

A test of provider resolution needs several capabilities with different
providers in one batch. A fixture where every capability has the same
provider, or only one capability exists, passes under per-batch
attribution, which is how this shipped.

## Where this applies

- `crates/collect/src/inspectors/rpm/mod.rs`: `ProviderIndex`, the
  provides index behind `classify_deps_rpm`, and `parse_sentinel_deps`;
  `rich_dep.rs` for rich dependencies
- `crates/collect/src/rpm_ownership.rs` — `build_rpm_owned_set`, consumed
  by the `/usr` walk (`inspectors/nonrpm.rs`) and by `/var` directory
  backing analysis (`inspectors/storage.rs`)

## Not affected

Call sites that pass a path *to* `rpm` and read only the exit code are
fine. `RealExecutor::run` builds a fixed argv via `Command::new().args()`
and never a shell string, so a space in an argument is not a delimiter.
`is_rpm_owned_path` and `filter_rpm_owned_gems_with_gemdir` in
`inspectors/nonrpm.rs` both work this way.

## Related

- [mock-executor-key-format](mock-executor-key-format.md) — the mock key
  is `cmd + " " + args.join(" ")`. Build it from the same constant the
  collector uses; a drifted key returns exit 127, which this collector
  turns into an empty owned set, which reports the entire tree as
  unmanaged.
- [rpm-ownership-vs-name-heuristic](rpm-ownership-vs-name-heuristic.md) —
  which ownership signal to trust.
