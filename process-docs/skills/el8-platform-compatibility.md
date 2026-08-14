# EL8 Platform Compatibility

inspectah is designed to work on both EL8 and EL9+ systems. This document captures platform-specific considerations.

## RPM Database Format

**`rpm -qa --dump`** field layout is stable across EL8 and EL9. The format has been consistent since early RPM versions:

```
/path/to/file SIZE HASH MODE OWNER GROUP MTIME CONFIG_FLAG DEVICE_FLAGS FLAGS
```

Current parsing (`rpm_ownership.rs`) extracts the first whitespace-delimited field (the path) via `line.split_whitespace().next()`. This is EL8-safe and requires no version-specific guards.

## systemd Features

EL8 ships **systemd 239** (vs. EL9's systemd 250+). The service inspector's usage is compatible with both:

- **Preset files** (`/usr/lib/systemd/system-preset/*.preset`, `/etc/systemd/system-preset/*.preset`) — same format on EL8 and EL9
- **Drop-in overrides** (`/etc/systemd/system/<unit>.d/*.conf`) — same mechanism
- **Directory directives** (`StateDirectory`, `CacheDirectory`, `LogsDirectory`) — all available in systemd 239
- **`systemctl list-unit-files`** output format — stable

**No version-specific guards needed** for systemd inspection.

## tmpfiles.d Directives

The tmpfiles.d parsing in `storage.rs` (`detect_var_dir_backing`) uses `grep -r` to search for directory paths in tmpfiles.d configuration files. This approach is directive-agnostic and works across EL8 and EL9.

**EL8-safe directives** (systemd.tmpfiles(5) from systemd 239):

| Directive | Purpose | Example |
|-----------|---------|---------|
| `d` | Create directory | `d /var/lib/myapp 0755 root root -` |
| `D` | Create/empty dir | `D /var/cache/myapp 0755 root root -` |
| `L` | Create symlink | `L /var/lib/link - - - - /target` |
| `p` | Create FIFO | `p /var/run/myapp.fifo 0600 root root -` |
| `c` | Create char dev | `c /dev/mychar 0600 root root - 1:2` |
| `b` | Create block dev | `b /dev/myblock 0600 root root - 8:0` |
| `C` | Recursive copy | `C /var/lib/dest - - - - /src` |
| `x` | Ignore path | `x /var/tmp/*` |
| `X` | Recursive ignore | `X /var/cache/*` |
| `r` | Remove file | `r /var/tmp/remove-me` |
| `R` | Recursive remove | `R /var/tmp/clean-me/` |
| `z` | Adjust ownership | `z /var/lib/myapp 0755 root root -` |
| `Z` | Recursive adjust | `Z /var/lib/myapp 0755 root root -` |

**Detection method:** The `check_tmpfiles_backing` function walks up parent directories and searches both `/etc/tmpfiles.d/` and `/usr/lib/tmpfiles.d/` for any directive that references the path. This is directive-type agnostic and requires no EL8-specific logic.

## Network Configuration

**ifcfg format** (`/etc/sysconfig/network-scripts/ifcfg-*`) is deprecated but **not removed** in EL9. It was fully removed in EL10. inspectah treats ifcfg files as inventory (not a modernization advisory) on all platforms — see Task 5 spec §6.6 and Task 12 brief.

**No version-specific behavior** needed for network inventory collection.

## tuned Profiles

**Stock profiles** are the same on EL8 and EL9. The `is_stock_tuned_profile` predicate covers both platforms.

**Custom profile detection** (added in Task 11) searches `/etc/tuned/*/tuned.conf` — same path on EL8 and EL9.

## Target Image Mapping

EL8 hosts have no bootc base image of their own. Default resolution (`resolve_from_os_release` in `crates/core/src/baseline.rs`) maps them up to the EL9 floor tag: RHEL 8.x → `registry.redhat.io/rhel9/rhel-bootc:9.6`, CentOS 8 → `quay.io/centos-bootc/centos-bootc:stream9`. The `RHEL_BOOTC_MIN` floor clamp handles the mapped-up case naturally (8.x is always below the EL9 floor).

**Correctness requirement: default resolution must produce version-pinned tags.** `MigrationContext::target_major_version()` (`crates/core/src/types/system.rs`) parses the target major from the image tag. A `:latest` default makes it return `None`, and `migration_kind()` then misclassifies EL8→EL9 as `SameStream` instead of `MajorUpgrade`. This happened once (July 2026, reverted): `:latest` is available only as an explicit `--base-image` override, never as the default.

## Summary

Inspection methods are platform-agnostic and work on both EL8 and EL9+ with no version-specific guards. The only EL8-specific logic in the codebase is the target image mapping above.
