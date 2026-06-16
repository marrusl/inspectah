## Summary

Remove the `--no-baseline` code path. Baseline extraction (pulling the target container image) is now mandatory. If the pull fails, the scan exits with a classified error message and specific remediation guidance.

### What changed

- **`--no-baseline` flag removed** — passing it produces a clap "unknown argument" error
- **Pull failure classification** — five error categories (registry unreachable, auth required, image not found, TLS/cert, unknown) with tailored remediation per category
- **Exit code 3** for pull failures (distinct from exit 1 general error, exit 2 incomplete scan, exit 130 SIGINT)
- **Credential sanitization** — live pull progress and error excerpts redact bearer tokens, basic auth, and authorization headers
- **`SCHEMA_VERSION` bumped to 19** — old tarballs (schema ≤18) are rejected by the existing version gate
- **`no_baseline` field removed** from `InspectionSnapshot`, `RpmSection`, all renderers, fleet merge, refine projection, and web adapter
- **Frontend cleanup** — removed "Baseline unavailable" banner and empty state (`sysctl_no_baseline` per-inspector concept preserved)
- **Docs updated** — README adds prerequisites and exit code table, all `--no-baseline` references removed from docs, CLI reference, and shell completions
- **CHANGELOG updated** with removal, schema bump, and pull failure classification

### Breaking changes

- `--no-baseline` CLI flag removed
- Schema version 18 → 19 (old tarballs not loadable)
- Exit code 3 is new

### Disconnected / air-gapped environments

Every error message includes the workaround:
```
podman save -o baseline.tar <image-ref>
podman load -i baseline.tar
```

### Note

`refine_server_lifecycle` test failure is pre-existing on `main` — not introduced by this branch.
