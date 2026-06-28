# RPM Ownership: Path Proof vs Name Heuristic

When filtering pip packages to exclude RPM-managed ones, use `rpm -qf
<dist-info-path>` to prove the RPM database actually owns that specific
path. Do NOT use the `python3-<normalized-name>` name heuristic against
`RpmState.installed_packages`.

## Why

The name heuristic suppresses real user-managed packages. A venv
`requests` gets dropped if `python3-requests` RPM is installed anywhere
on the host, even when the venv copy is not RPM-owned. The correct
contract is path ownership: exclude only when the RPM database owns the
specific detected `dist-info` path.

## Related constraint

`RpmState.owned_paths` is filtered to `/etc` during construction (for
config file detection). It cannot answer ownership questions for paths
under `/opt`, `/srv`, `/usr/local`, or system site-packages. Use
`rpm -qf` via the executor for those paths.

## Where this applies

- `crates/collect/src/inspectors/nonrpm.rs` — pip RPM filtering
- `crates/collect/src/inspectors/nonrpm.rs` — unmanaged file RPM
  exclusion (also uses `rpm -qf`)
