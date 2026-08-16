# Release Notes: v0.9.0-beta.2

## What's new in v0.9.0-beta.2

This is a correctness release. It closes the integration gaps left when the `FindingKind` disposition model (Actionable, Advisory, Inventory) shipped in v0.9.0-beta.1 ahead of the surfaces that needed to display it. The common thread: advisory findings, things inspectah reports but never asks you to decide about, were being treated as decisions across several surfaces. Every fix below addresses one instance of that.

### Advisories now render as advisories, everywhere

- **Refine web UI stopped treating advisories as toggleable** — the frontend read a finding's include flag with a `?? true` fallback, so an advisory or inventory finding, neither of which carries that key, rendered with a live checkbox that the server silently discarded on click. The web contract now models all three dispositions: advisories render with their type and rationale and no toggle, inventory findings render as inventory, and a refused `SetInclude` on the backend now returns 422 with a message naming the item and why, instead of a bare 500.
- **Config-borne advisories now appear on every surface** — modernization notices (sysvinit scripts and the rest of the pattern catalog) and cross-tree symlink findings were carried on a disposition that every include-filtered table treated as excluded, so they reached no report at all. They now appear in the HTML audit report, the markdown audit report, the TUI (with the `ℹ` marker and rationale), and the refine web UI (as advisory rows, not toggleable config files). A host whose only config finding was an advisory no longer gets hidden behind an "all configuration files excluded" empty state.
- **Orphan full-shadow services get the same treatment** — a preset-matched service whose unit file is fully shadowed in `/etc/systemd/system/` has no state change to act on, but was rendering as an ordinary included row and dropped from the group advisory badge. It now renders in the advisory list with rationale and counts toward the badge, matching the audit report.

### Section stats, counts, and defaults now agree with the disposition model

- **Advisories no longer count as excluded** — section stats bucketed every finding by its include flag, so an advisory landed in the excluded count. Stats now carry a third `advisory` bucket, and the refine stats bar, the TUI status bar, the group badges, and the export dialog all report it as its own thing rather than folding it into "excluded."
- **Normalization no longer overwrites advisory dispositions** — opening a snapshot in refine was applying tier-based include defaults over advisory config dispositions, erasing the rationale and letting export copy a flagged file into the image. Advisory and inventory findings now pass through normalization unchanged.
- **A resumed stale toggle can no longer bake an advisory file into the image** — `resume_from` replays an autosaved timeline without re-validating each op. A session saved before advisory toggles were refused could replay a `SetInclude` against a config entry unconditionally, rewriting a modernization or cross-tree symlink finding as included in the projection export renders from. The projection now preserves advisory and inventory findings, matching the merge layer.
- **Snapshot deserialization no longer defaults to actionable-and-included** — network connections, firewall zones, firewall direct rules, and the tuned profile selection were defaulting to "actionable, included" when their `disposition` key was absent from a snapshot. They now default correctly (inventory for network data, excluded for the tuned selection), so `--from-snapshot` re-renders no longer bake inventory-only items into the Containerfile.

### Also fixed

- **Two shell-injection paths in Containerfile rendering closed.** The npm global prefix and the pip package list are interpolated into comment lines, and neither was filtered; a newline in either value could close the comment and turn the rest into an active `RUN` instruction. Both are now escaped for their comment context. **These predate v0.9.0-beta.1** — they are not beta.2 regressions, just found and fixed during this pass. A related fix quotes the venv directory name at all three of its interpolation sites, closing a build break when the name contained a space.
- **Language package pins are keyboard-reachable** — Enter and Space now toggle a pin from an expanded npm global package row, matching the announcement behavior of a mouse click. Previously the pin was reachable only by tabbing into the row's checkbox.
- **Path normalization fixes** — npm global and system gem collectors now strip all leading slashes from environment paths instead of just the first, and the renderer restores exactly one. The renderer's shell metacharacter guard for language-package paths and package names now also rejects `;`, `$`, backtick, `|`, and `&`, not just quotes and newlines.

### Schema version

No schema change in this release. Schema version remains 22, as set in v0.9.0-beta.1.

### Binaries

Pre-built binaries for 3 platforms:
- `inspectah-darwin-arm64` -- macOS on Apple Silicon
- `inspectah-linux-arm64-bin` -- Linux on ARM64 (static musl binary)
- `inspectah-linux-amd64` -- Linux on x86_64 (static musl binary)

**Full changelog:** https://github.com/marrusl/inspectah/compare/v0.9.0-beta.1...v0.9.0-beta.2
