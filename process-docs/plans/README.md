# Implementation plans — status index

**The status marker at the top of each plan is the authority; this table is a
convenience. If they ever disagree, trust the marker** — it carries the
evidence, and this table carries only the verdict.

**Do not read checkbox state as status.** Every plan here has unticked boxes
and none are fully ticked, including plans whose work demonstrably shipped.
Implementers ran the work and never came back to tick them.

| Plan | Classification | Classified |
|---|---|---|
| [2026-05-31-refine-projection-consolidation.md](2026-05-31-refine-projection-consolidation.md) | Executed | 2026-11-04 |
| [2026-05-31-tui-refine.md](2026-05-31-tui-refine.md) | Executed, feature later removed | 2026-11-04 |
| [2026-06-03-unified-include-default-model.md](2026-06-03-unified-include-default-model.md) | Executed | 2026-11-04 |
| [2026-06-08-html-audit-report-redesign.md](2026-06-08-html-audit-report-redesign.md) | Executed | 2026-11-04 |
| [2026-06-08-preserve-consolidation-and-no-redaction.md](2026-06-08-preserve-consolidation-and-no-redaction.md) | Executed | 2026-11-04 |
| [2026-06-09-fleet-leaf-intersection.md](2026-06-09-fleet-leaf-intersection.md) | Executed | 2026-11-04 |
| [2026-06-10-scan-output-rethink.md](2026-06-10-scan-output-rethink.md) | Executed | 2026-11-04 |
| [2026-06-11-anaconda-gap-classifier.md](2026-06-11-anaconda-gap-classifier.md) | Executed | 2026-11-04 |
| [2026-06-11-group-rendering-plan.md](2026-06-11-group-rendering-plan.md) | Executed | 2026-11-04 |
| [2026-06-12-mandatory-baseline-requirement.md](2026-06-12-mandatory-baseline-requirement.md) | Executed | 2026-11-04 |
| [2026-06-13-context-section-overhaul.md](2026-06-13-context-section-overhaul.md) | Executed | 2026-11-04 |
| [2026-06-15-fleet-to-aggregate-rename.md](2026-06-15-fleet-to-aggregate-rename.md) | Executed | 2026-11-04 |
| [2026-06-15-package-group-dep-visibility.md](2026-06-15-package-group-dep-visibility.md) | Executed | 2026-11-04 |
| [2026-06-27-non-rpm-replication-plan1-collector-and-tier1.md](2026-06-27-non-rpm-replication-plan1-collector-and-tier1.md) | Executed | 2026-11-04 |
| [2026-06-27-non-rpm-replication-plan2-tier2-tier3.md](2026-06-27-non-rpm-replication-plan2-tier2-tier3.md) | Executed | 2026-11-04 |
| [2026-06-27-non-rpm-replication-plan3-refine-ui.md](2026-06-27-non-rpm-replication-plan3-refine-ui.md) | Executed | 2026-11-04 |
| [2026-06-27-non-rpm-replication-plan4-compose-and-aggregate.md](2026-06-27-non-rpm-replication-plan4-compose-and-aggregate.md) | Executed | 2026-11-04 |
| [2026-07-02-extended-findings-companion.md](2026-07-02-extended-findings-companion.md) | Executed | 2026-11-04 |
| [2026-07-07-refine-ux-convergence.md](2026-07-07-refine-ux-convergence.md) | Executed | 2026-11-04 |
| [2026-07-08-language-package-detection-v2.md](2026-07-08-language-package-detection-v2.md) | Executed | 2026-11-04 |
| [2026-07-08-language-package-detection-v2-fix-round.md](2026-07-08-language-package-detection-v2-fix-round.md) | Executed | 2026-11-04 |
| [2026-08-15-usr-walk-presentation-plan.md](2026-08-15-usr-walk-presentation-plan.md) | **Superseded — do not execute** | 2026-11-04 |
| [2026-10-27-refine-ui-fixup-phase-0.md](2026-10-27-refine-ui-fixup-phase-0.md) | Executed (phase 0 only) | 2026-11-04 |
| [ansible-role-inspectah-plan.md](ansible-role-inspectah-plan.md) | Executed (in the role repo) | 2026-11-04 |
| [crates-reorganization.md](crates-reorganization.md) | Executed | 2026-11-04 |

## Reading the classifications

- **Executed** — the work is in the tree. The marker names the module, test
  or commit that shows it.
- **Executed, feature later removed** — the plan ran, and the thing it built
  was deleted afterwards. The TUI is the only case.
- **Executed (phase 0 only)** — a staged plan whose later phases are separate
  releases with their own plans.
- **Superseded** — replaced by a later design. The marker names the successor.

## Two things that mislead readers here

**A dead plan does not mean a dead feature.** The `/usr` walk is the standing
example: its presentation plan is superseded, but the detector shipped and is
live. Check the feature before concluding anything from a plan's status.

**Several plans name paths that no longer resolve.** Three mechanical events
moved files out from under the plan text — the `fleet` to `aggregate` rename
(`da83875b`), the `crates/tui` removal (`1f5db703`), and files that simply
landed somewhere other than planned. A path that does not exist is usually a
renamed file, not absent work. The markers call these out per plan.

## Adding a plan

New plans go in unmarked. Add the marker and a row here when the work lands,
or when someone later establishes what happened to it. Keep the marker form
consistent with the existing ones:

```
> ## <Classification> (<date>)
>
> **Checkbox state below is not evidence** — the boxes were never ticked.
> Classified from the tree:
> - <artifact, test, or commit>
```
