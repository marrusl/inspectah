# Factor Spec

## Metadata

| Field      | Value                                                        |
|------------|--------------------------------------------------------------|
| Date       | 2026-08-15                                                   |
| Status     | Proposed                                                     |
| Supersedes | `2026-06-27-factor-pre-spec.md`                              |
| Basis      | 2026-08-15 product session: every open question in the pre-spec (§10) was resolved or dissolved there. This spec records those decisions; it does not reopen them. |
| Verified   | Code citations checked against inspectah commit `fdbf3508`   |

## 1. Problem Statement

A team migrating a heterogeneous fleet to image mode has an architecture
question: how many images, what goes in each, and how do the
Containerfiles relate. By the time that team reaches factor, they have
already answered it. They grouped machines into fleets, aggregated each
group, and triaged each aggregate through refine. Those groupings are the
architecture, expressed one decision at a time.

**Factor materializes the architecture the user expressed through refine
groupings, exactly.** Each refined aggregate becomes a role. What every
input has in common becomes the base. Factor's mathematics is set
intersection over typed equality keys: deterministic, explainable, and
reproducible. Same inputs, same architecture.

The division of labor across the pipeline is strict. Prevalence and
variance are refine's vocabulary: they exist so a human can triage fuzzy
fleet reality into decisions. Factor consumes the decisions. Inclusion in
a refined aggregate means the user decided it belongs; factor treats that
as authoritative and adds no second layer of statistical judgment. The
Containerfile and build-context output are exact artifacts, and the input
that produces them is exact too.

Positioning: factor turns the fleet groupings a team has already refined
into a role-separated image architecture. Refine curates what is true;
factor materializes how to build it.

## 2. Input Model

### Directory or explicit file list

Factor accepts either form as positional args: a directory of refined
aggregates, or an explicit list of aggregate files. Both forms consume
the same input: the exported output of `inspectah refine` run on
aggregates, one per intended role grouping. Each input arrives
pre-refined. Noise is filtered, variants are picked, dispositions are
set. Factor performs no re-triage.

The input format is the versioned refined-aggregate export contract owned
by the aggregate convergence workstream (§8). Factor is a consumer of
that contract, never a definer of it.

### Single input is an error

Factoring one aggregate is a no-op with extra steps: there is nothing to
intersect and only one role to emit. Factor errors on a single input, and
the error points upstream:

> factor needs at least two refined aggregates. With one input there is
> nothing to intersect. Group machines into fleets during aggregate,
> refine each fleet, then factor the set.

### Lineage validation

Compatibility is judged on each input's **resolved migration target**,
never its source OS. A Fedora-source aggregate targeting rhel9 and an
EL8-source aggregate targeting rhel9 co-factor cleanly; both resolved to
the same target lineage.

The rule: all inputs' target base images must share **registry family and
major version**. Minor versions may differ; **minors float to the latest
minor present in the input set**, and that image becomes the base FROM
(§7). The resolved target ships in the snapshot as `target_image`
(`crates/core/src/snapshot.rs:60-62`, verified), so factor reads it
rather than re-deriving it.

**CentOS Stream and RHEL form one compatibility class.** CentOS Stream N
and RHEL N are the same major for the lineage rule; a set mixing the two
is not a family or major mismatch. When a set mixes them, RHEL wins: the
base FROM is the RHEL ref at the latest minor present in the set. An
all-CentOS-Stream set uses the CentOS Stream ref.

Family or major mismatch is an error, and the message tells the user to
split the inputs by lineage and run factor once per lineage.

**Override switch:** `--force-mixed-lineage`, named here per the session's
instruction to name it at spec time. Its help text carries an explicit
warning that the resulting base intersection spans incompatible parents
and the output is not expected to build without manual repair. It exists
for spelunking, not for production runs.

## 3. The Factoring Algorithm

### One role per input

Role count derives from input count: N refined aggregates produce N roles
plus one base. Factor never invents a role the user did not express as a
grouping, and never drops one.

### Base is the true intersection

An artifact lands in the base when it appears in **every** input and
matches under its equality key in every input. 100 percent, no threshold,
no middle band. Everything else stays in the roles that declare it.

### Equality keys

"The same artifact in two aggregates" is defined per artifact type:

| Artifact type | Equality key |
|---------------|--------------|
| Packages      | name only |
| Config files  | path + content hash |
| Services      | unit name + enablement state |
| Quadlets      | unit name + content hash |
| Sysctls       | key + value |
| Firewall rules| full rule identity (port/service/zone tuple) |
| Users/groups  | name, with uid/gid conflict flagging (below) |

Notes on the keys:

- **Packages match on name alone.** Factor assumes a single architecture
  per factoring run; arm and amd estates serve different purposes in
  practice and get factored separately. Estates that do double duty still
  work: the Containerfile abstracts the architecture and dnf resolves
  per-arch at build time.
- **Configs compare winners.** Refine already picked one winner per path
  within each fleet; the intersection compares those winners across
  aggregates by content hash. Identical everywhere goes to base;
  differing content stays per-role.
- **Services carry enablement state in the key.** The same unit enabled
  in one fleet and disabled in another is not the same intent, and does
  not intersect.
- **Users and groups intersect on name.** When the same name carries
  different uid/gid across inputs, the artifact still intersects, and
  factor **flags the conflict loudly** in the composition report and the
  review queue. Silent exclusion on a strict uid/gid key is exactly the
  failure mode this rule exists to prevent: files baked into the image
  own their uid/gid numerically, so a quiet mismatch surfaces as a
  runtime permissions bug months later.

### Divergent tied configs

When a package intersects (name-identical everywhere) but one of its tied
configs differs across inputs, the package goes to base and the differing
configs stay per-role. This matches the bootc filesystem split the output
lands on: the base bakes the read-only /usr payload, roles carry the
differing /etc content.

### Partial overlap: duplicate, surface, hoist by hand

An artifact present in k of N inputs (1 < k < N) is **duplicated into
every role that declares it**. Factor does not create shared-subset roles
and does not hoist on its own.

- The composition report and (in the flagship) the canvas mark such
  artifacts **shared-across-k**: "nginx.conf identical in 3 of 5 roles."
- **Hoisting into base is always a manual act.** Promoting an artifact
  from k roles to the base changes what every fleet receives; that is the
  user's standardization decision, and factor's job is to surface the
  opportunity, not take it.

## 4. Naming

### Semantic guess by default

Factor proposes a purpose name for each role from its contents. The
method for deriving the guess is an open implementation question (§11);
the contract is only that the default proposal is semantic, not
mechanical.

### Collision escalation

When two proposed names collide, the collision escalates to the review
queue. The queue entry shows the **distinguishing delta** between the two
roles (what one has that the other lacks), and the rename field is
**pre-filled with the input's filename stem** so accepting a resolution
is one keystroke, not a naming exercise from scratch.

### Stable identity, separate from display

- Every role carries a **stable role ID** distinct from its display name.
  Adjustments, provenance, and archives reference the ID; humans see the
  name.
- Renames are in-place, cheap, and available everywhere a role name is
  shown. Provenance metadata survives rename.
- **No timestamps in role names.** Provenance carries time; names carry
  purpose.

### Opt-in role naming

A load-time flag, `--name-roles`, replaces semantic guessing with an
interactive naming pass over the inputs.

Invoked, it first prints the full numbered list of inputs, then prompts
for each input in sequence. Each prompt's bracket default is
`normalize(stem)`: the input's filename stem, lowercased, with spaces,
underscores, and dots converted to hyphens, runs of hyphens collapsed,
and leading and trailing hyphens stripped. An entry that normalizes to
nothing, to `.` or `..`, or that begins with a leading hyphen is
rejected, and length is capped; an invalid entry re-prompts with the
normalized form as the new default.

Duplicate stems are pre-checked before prompting begins and announced up
front, then resolved interactively with disambiguated defaults. On a
non-TTY run, `--name-roles` takes `normalize(stem)` for every input
silently, except for one loud summary line listing the names used; an
unnormalizable stem is a hard failure, and duplicate stems are a hard
error naming both source files.

A name that survives a prompt, including a bare Enter accepting the
default, is explicit and permanent: the semantic-guess default above
never names a role the flag already covered. Ctrl+C aborts the whole
load, not just the current prompt.

A future non-interactive form is noted but not built here: a repeatable
`--role-name <stem>=<name>` mapping keyed on filename stem, never on
positional order.

## 5. Tied Changes

Moving or hoisting an artifact drags its dependents. Ties come in two
tiers with different automation contracts:

### Explicit ties: auto-follow

Ties already explicit in the contract follow automatically:

| Tie | Evidence source (verified at `fdbf3508`) |
|-----|------------------------------------------|
| package to config | rpm ownership; `ConfigFileEntry.package` (`crates/core/src/types/config.rs:44`) |
| service to owning package | `ServiceStateChange.owning_package` (`crates/core/src/types/services.rs:75`) |
| drop-in to service | `SystemdDropIn.unit` (`crates/core/src/types/services.rs:104-107`) |
| quadlet to image | `QuadletUnit.image` (`crates/core/src/types/containers.rs:28`) |
| sysctl to source file | `SysctlOverride.source` (`crates/core/src/types/kernelboot.rs:23`) |

### Inferred ties: preview only

Softer correlations (firewall rule to the service it fronts, workload
heuristics) are **preview-only, never automatic**. They render as
suggestions with their evidence; the user commits or dismisses.

### Evidence edges in the contract

The refined-aggregate export carries **typed evidence edges**, not a
frozen tie graph. Factor derives its tie behavior from the evidence at
run time, so tie policy can improve without a contract change. Package
references on evidence edges are normalized to `name.arch`, per the
repo's package-identity rule. The current fields hold bare package names
(`Option<String>`, verified above), so canonical `name.arch` refs are a
contract ask on the convergence workstream (§8), not something factor
papers over locally.

## 6. Merge and Hoist

Both are **user-initiated**; factor never merges or hoists on its own.

### Merge (role into role)

- The result is the **set union** of both roles.
- Conflicts exist only where both sides carry the same equality key with
  different content. Each conflict resolves by **pick-a-winner**, the
  same interaction refine uses for content variants. Nothing is averaged
  or auto-preferred.
- Ties recompute after the merge, so dependents follow their anchors into
  the merged role.

### Hoist (artifact into base)

- Promotes an artifact from the roles that carry it into the base.
  Surfaced by shared-across-k marking (§3); executed only by the user.
- Hoisting a package re-evaluates its tied configs under the divergent
  tied configs rule: identical configs hoist along, differing configs
  stay per-role.
- In the flagship UI the gesture is a drag onto the base; in the
  precursor it is an adjustment entry (§9).

## 7. Output Model

### Role archives with a typed provenance envelope

Factor emits build contexts only; it never invokes podman, buildah, or
any other build tool. The primary output is an archive tarball
containing one directory per build context: `base/` plus one directory
per role. Each directory is self-contained, a Containerfile and its COPY
payload, with a deterministic default base ref in FROM, overridable per
the FROM rules below. Building and tagging those contexts is the user's
step, with their own tools.

Each archive set also carries a **typed provenance envelope**:

- `export_schema_version` of the consumed refined aggregates
- source-aggregate identities: label, host counts, merged-at,
  completeness
- refine provenance: generation, export timestamp, decision-state hash
- integrity digest over the archive payload
- factor run metadata: input list, equality-key version

Human-readable reports (composition report, proposal report) ride along
in the archive set; **they are not the contract**. The envelope is.

### Containerfile rendering: shallow chain

Archives store role dependency intent as data (a DAG). The rendered build
is deliberately flatter than the data model allows:

- **One base Containerfile.** Its FROM is the shared target lineage at
  the latest minor in the input set (§2).
- **Sibling role Containerfiles, each exactly one hop FROM the base
  image.** No role-on-role stacking.

Deep inheritance chains multiply rebuild fan-out and blur ownership,
while bootc lifecycle operations (install, upgrade, rollback) act on
whole images at the image boundary. The shallow chain keeps every role
image one rebuild away from its base and one owner away from
accountability. Composing sibling roles into a single image at build
time is a possible later export mode, not the primary model.

### FROM lines: inherited and pinned

- The base FROM is **inherited from the inputs' resolved target lineage**
  and **pinned** to a specific minor, matching the repo's
  pinned-minor-with-floor resolution posture
  (`crates/core/src/baseline.rs:399-442`, verified: per-family registry
  mapping with version clamping and the EL8-to-EL9 floor).
- Role Containerfiles FROM the base image by reference.
- Per-role overrides are explicit: a user can set any role's FROM to a
  tag or digest of their choosing.
- `:latest` stays opt-in everywhere, never a default.

### Config authoring target

Factor authors user configs to `/etc`, never to `/usr/etc`. Package
defaults already ship in the base image and are not overwritten; the
COPY payload lands only the content refine decided belongs in `/etc`.

### Structural lints

Factor lints the proposed output before export. Ordered by expected
catch rate:

1. **Unbacked /var state.** No mutable /var file content baked into the
   image; allow only declarative directory provisioning. tmpfiles.d is
   the primary mechanism, and factor carries no special /var logic
   beyond this lint: normalizing tmpfiles.d-versus-direct-copy divergence
   for the same path is refine's job (one declarative story per /var
   path per fleet), and divergence that survives into factor's inputs
   surfaces through the ordinary equality-key conflict handling (§3).
   (The repo already models the underlying advisory: the unbacked-var
   advisory in `crates/collect/src/inspectors/storage.rs:126-145`,
   verified.)
2. **Merge-hostile or host-state /etc content.** The repo force-excludes
   `/etc/fstab` and `/etc/crypttab` today
   (`crates/refine/src/normalize.rs:184-213`, verified); the lint extends
   that judgment to role content.
3. **Services and timers incompatible with image mode.** The repo
   enumerates known-incompatible package-manager units in
   `crates/core/src/baseline.rs:117-144` (verified).
4. **Mixed lineage or invalid parent FROM.** Load-time validation (§2)
   catches mismatched inputs; this lint catches per-role FROM overrides
   that break the lineage after the fact.
5. **Broken explicit ties.** A service placed without its drop-ins, a
   package without its owned config, a quadlet without its related
   artifacts.
6. **Wants-symlink hygiene.** No handcrafted wants-symlink trees; use
   either explicit service-state actions or generated preset policy.
   (The renderer emits explicit `RUN systemctl enable` today,
   `crates/pipeline/src/render/containerfile.rs:937-945`, verified.)

## 8. Two-Schema Split

Factor's persistence splits into two schemas with different stability
promises:

### Refined-aggregate export: facts

The export contract carries **fleet facts only**: typed artifacts,
equality-key inputs (content hashes, variant selection), evidence edges,
lineage, provenance. It is owned by the **aggregate contract convergence
workstream**, and it is the surface the 1.0 stability promise covers.

The 2026-08-15 session produced a contract-asks list for that workstream
(stable artifact IDs per type, `name.arch` package refs on evidence
edges, content-hash and variant exposure in JSON, provenance envelope
fields, target-lineage preservation through export, quadlet path
normalization, first-class IDs for firewall direct rules, /usr entries
preserved through aggregate merge). This spec depends on those asks and
records them here as dependencies only; **their normative definition
belongs to the export contract, not to this spec**. One is verified
directly: aggregate merge currently rebuilds the unmanaged section with
`usr_entries: Vec::new()` (`crates/core/src/aggregate/merge.rs:1819-1824`,
verified), so the /usr preservation ask is real work, not paperwork.

Factor requires **no prevalence data** from the export: no host lists,
machine counts, or cohort data. Whether raw prevalence stays in the
export for report and audit surfaces is the convergence track's design
call, not a factor requirement.

### Factor adjustments file: decisions

Factor's own save file records **the user's adjustments**: renames,
hoists, moves, merges, exceptions. Every entry is keyed to stable
artifact IDs and stable role IDs, never to display names or array
positions.

### Re-factor replay

Re-running factor against updated refined aggregates:

1. Re-runs the set math on the new inputs from scratch.
2. Replays every adjustment whose IDs still resolve.
3. Reports the adjustments that no longer apply, with the reason (ID
   gone, artifact no longer present, role vanished).

Nothing is silently dropped and nothing stale is silently applied. This
is what makes factor re-runnable quarter over quarter instead of a
one-time migration exercise.

## 9. Scope Ladder

### Propose-only precursor (late 0.9.x, green-lit)

A CLI-only precursor ships in the 0.9.x line:

- `inspectah factor <dir>` (or an explicit list of aggregate files) runs
  the intersection and emits:
  - the **proposal report**: named roles, base contents, composition and
    overlap (shared-across-k with hoist candidates), naming collisions,
    uid/gid flags, tie previews, lint results. Legible and falsifiable:
    every placement traces to an input and an equality key.
  - the **role archives** with provenance envelopes (§7).
  - the **saved adjustments file** (§8), so a precursor run is
    re-runnable and diffable from day one.
- **Explicit preview framing.** Every precursor surface labels its output
  formats as preview. Factor output formats sit **outside the 1.0
  stability promise** regardless of when they ship.
- No canvas, no editor, no interactive surface.

### 1.x flagship

The full factor product: the canvas (roles by artifact types), the
interactive editor (moves, merges, hoists, renames with tie previews),
and the report surfaces, built on the same algorithm, contracts, and
adjustments file the precursor established. The canvas and editor get
their own UI spec; interaction design material recorded at the session
(cell summary objects, layered canvas, non-drag canonical move flow,
accessibility contract) is input to that spec, not a commitment of this
one.

### Parked

The TUI is out of factor's scope entirely. If it ever returns it is
inexpensive follow-up work after the flagship, post-1.x.

## 10. The /usr Walk

The /usr walk is a **general inspectah feature**, not a factor feature:
collection, single-host report, aggregate, refine, and export all handle
it upstream of factor. /usr entries follow the standard finding process:
Actionable, default-include on a single host, and the existing aggregate
zone machinery elsewhere (100 percent prevalence auto-includes, partial
prevalence lands in a review zone). There is no separate resolution-state
concept for /usr. The presentation is designed in
`2026-08-15-usr-walk-presentation-design.md` (this directory);
implementation targets v0.9.0-beta.3.

Factor is a **future consumer of these ordinary decisions**. By the time
an aggregate reaches factor, each /usr entry carries the same
include/exclude decision as any other artifact family, and factor
materializes it the same way: included entries become COPY content in
the owning role or base.

## 11. Open Questions

Decisions the session did not make, recorded here rather than invented:

1. **Semantic-name derivation.** What evidence feeds the purpose-name
   guess and how candidates are ranked.
2. **uid/gid conflict resolution mechanics.** The flag is loud by
   decision; whether resolution is pick-a-winner like config variants or
   a per-role divergence is undecided.
3. **Adjustments-file location and naming.** Alongside the archives, in
   a dot-directory, or user-specified.
4. **Proposal report format.** The required contents are settled (§9);
   the format (markdown, HTML, both) is not.
5. **Precursor CLI surface.** The subcommand split, if any (run vs
   render vs replay), is undecided. Input forms (§2), the mixed-lineage
   override, and `--name-roles` (§4) are settled; further flags beyond
   those remain open.

## 12. Glossary

| Term | Definition |
|------|------------|
| **Role** | The materialization of one refined aggregate: a named, purpose-driven grouping of artifacts with a stable ID. |
| **Base** | The true intersection across all inputs; the parent image every role builds FROM. |
| **Equality key** | The per-artifact-type definition of "the same artifact in two aggregates" (§3). |
| **Shared-across-k** | An artifact present in k of N inputs, duplicated into each declaring role and surfaced as a hoist candidate. |
| **Hoist** | The user's act of promoting a shared artifact from roles into the base. |
| **Evidence edge** | A typed relation in the export contract (package ownership, unit membership) from which factor derives tie behavior. |
| **Adjustments file** | Factor's save file of user decisions, keyed to stable IDs, replayed on re-factor. |
| **Composition report** | The human-readable account of the factoring: role contents, overlap, flags, lint results. |
| **Shallow chain** | The rendered build layout: one base Containerfile, sibling role Containerfiles one hop FROM base. |
| **Provenance envelope** | The typed metadata contract each archive set carries (§7). |
| **Refined aggregate** | The exported output of `inspectah refine` on an aggregate; pre-refined intent and factor's only input. |
