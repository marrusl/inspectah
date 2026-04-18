# inspectah output

Generated from **Red Hat Enterprise Linux 9.7 (Plow)**.

**Host:** `input`
**Inspected:** 2026-03-23T13:38:34.246523+00:00

## Findings summary

| Category | Count |
|---|---|
| Packages added (beyond base image) | 225 |
| New from base image | 78 |
| Configs modified (RPM-owned) | 19 |
| Configs unowned | 47 |
| Services changed | 7 (6 enabled, 1 disabled) |
| Non-RPM software items | 11 |
| Container workloads | 4 quadlet, 1 compose |
| Secrets redacted | 1 |
| Warnings | 3 |
| FIXME items | 15 |

## Build

```bash
podman build -t my-bootc-image:latest .
```

## Deploy

```bash
# Custom kernel args detected — verify they are baked into the image
# or pass them via the bootloader configuration at deploy time.
# Switch an existing system to the new image:
bootc switch my-bootc-image:latest

# Or install to a new disk:
bootc install to-disk --enforce-container-sigpolicy /dev/sdX
```

Review `kickstart-suggestion.ks` for deployment-time settings (hostname, DHCP, DNS).

## Artifacts

| File | Description |
|---|---|
| `Containerfile` | Image definition |
| `config/` | Files to COPY into the image |
| `audit-report.md` | Full findings (markdown) |
| `report.html` | Interactive report (open in browser) |
| `secrets-review.md` | Redacted items requiring manual handling |
| `kickstart-suggestion.ks` | Suggested deploy-time settings |
| `inspection-snapshot.json` | Raw data for re-rendering (`--from-snapshot`) |

## FIXME items (resolve before production)

1. FIXME: dynamic C/C++ binary at /opt/myapp — needs: libpython3.14.so.1.0, libc.so.6
2. FIXME: Go binary at /usr/local/bin/driftify-probe (statically linked)
3. FIXME: verify npm packages in /opt/webapp install correctly
4. FIXME: unknown provenance — determine upstream source and installation method for /opt/tools
5. FIXME: unknown provenance — determine upstream source and installation method for /usr/local/bin/bundle
6. FIXME: unknown provenance — determine upstream source and installation method for /usr/local/bin/bundler
7. FIXME: unknown provenance — determine upstream source and installation method for /usr/local/bin/tilt
8. FIXME: unknown provenance — determine upstream source and installation method for /usr/local/share
9. FIXME: unknown provenance — determine upstream source and installation method for /usr/local/bin/deploy.sh
10. FIXME: human user 'mrussell' deferred to kickstart/provisioning
11. FIXME: human user 'appuser' deferred to kickstart/provisioning
12. FIXME: 2 sudoers rule(s) — review and bake into /etc/sudoers.d/
13. FIXME: if these modules are needed, add them to /etc/modules-load.d/ in the image
14. FIXME: 1 custom fcontext rule(s) detected — apply in image
15. FIXME: review kickstart-suggestion.ks for deployment-time config

## Warnings

- **rpm:** 1 package(s) will be downgraded by the base image — review the Version Changes section.
- **network:** ip route failed — static route information unavailable.
- **network:** ip rule failed — policy routing rule information unavailable.

## User Creation Strategies

bootc performs a three-way merge on `/etc` during image updates. Users baked into `/etc/passwd` in the image can conflict with runtime changes. Declarative and deploy-time approaches avoid this.

| Strategy | What it does | When to use | Risk |
|----------|-------------|-------------|------|
| **sysusers** | systemd-sysusers drop-in creates users at boot | Service accounts (nologin shell) | Users not visible until first boot |
| **useradd** | Explicit `RUN useradd` in Containerfile | Accounts needing precise control in the image | Conflicts with bootc `/etc` merge on updates |
| **kickstart** | User directives in kickstart at deploy time | Human users, site-specific accounts | Users missing if kickstart not applied |
| **blueprint** | bootc-image-builder TOML customization | When using image-builder as build pipeline | Only works with bootc-image-builder |

**Recommendation:** Use **sysusers** for service accounts and **kickstart** or identity management (FreeIPA, SSSD) for human users. Use `--user-strategy` to override the per-classification defaults if you want a single strategy for all users.

See [`audit-report.md`](audit-report.md) or [`report.html`](report.html) for full details.
