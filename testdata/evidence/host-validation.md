# Host Validation Evidence - Slice 2c (all 11 sections)

**Date:** 2026-05-15T07:51:05-04:00
**Hostname:** CentOS9

## Host Details

- **OS:** CentOS Stream 9
- **Kernel:** 5.14.0-686.el9.aarch64
- **Architecture:** aarch64
- **Go inspectah version:** inspectah wrapper 0.7.0
  commit: unknown
  built:  2026-05-10T03:49:08Z
- **Rust inspectah version:** inspectah 0.8.0-alpha.1
commit: unknown
date:   unknown

## Scan Results

### Go scan output
```
total 3464
drwxr-xr-x. 3 root root     188 May 15 07:51 .
drwxr-xr-x. 6 root root      68 May 15 07:50 ..
-rw-r--r--. 1 root root   16628 May 15 07:51 audit-report.md
drwxr-xr-x. 3 root root      17 May 15 07:51 config
-rw-r--r--. 1 root root     566 May 15 07:51 Containerfile
-rw-r--r--. 1 root root  590369 May 15 07:51 inspection-snapshot.json
-rw-r--r--. 1 root root     877 May 15 07:51 kickstart-suggestion.ks
-rw-r--r--. 1 root root    2414 May 15 07:51 README.md
-rw-r--r--. 1 root root 2914298 May 15 07:51 report.html
-rw-r--r--. 1 root root     399 May 15 07:51 secrets-review.md
```

### Rust scan output
```
total 1332
drwxr-xr-x. 2 root root      38 May 15 07:51 .
drwxr-xr-x. 6 root root      68 May 15 07:50 ..
-rw-r--r--. 1 root root 1361615 May 15 07:51 inspection-snapshot.json
```

## Section Parity

- **rpm:** DIVERGENCE (7220 lines)
- **services:** DIVERGENCE (1710 lines)
- **storage:** DIVERGENCE (94 lines)
- **kernelboot:** DIVERGENCE (944 lines)
- **network:** DIVERGENCE (4 lines)
- **containers:** DIVERGENCE (10 lines)
- **users-groups:** DIVERGENCE (23 lines)
- **scheduled-tasks:** DIVERGENCE (190 lines)
- **config:** DIVERGENCE (3631 lines)
- **selinux:** DIVERGENCE (80 lines)
- **non-rpm-software:** DIVERGENCE (30 lines)

## Conclusion

[Review diffs above and fill in assessment]
