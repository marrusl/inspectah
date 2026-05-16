# Host Validation Evidence - Slice 2a + 2b

**Date:** 2026-05-14T13:14:07-04:00
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
drwxr-xr-x. 3 root root     188 May 14 13:14 .
drwxr-xr-x. 6 root root      68 May 14 13:13 ..
-rw-r--r--. 1 root root   16628 May 14 13:14 audit-report.md
drwxr-xr-x. 3 root root      17 May 14 13:14 config
-rw-r--r--. 1 root root     566 May 14 13:14 Containerfile
-rw-r--r--. 1 root root  590372 May 14 13:14 inspection-snapshot.json
-rw-r--r--. 1 root root     877 May 14 13:14 kickstart-suggestion.ks
-rw-r--r--. 1 root root    2414 May 14 13:14 README.md
-rw-r--r--. 1 root root 2914301 May 14 13:14 report.html
-rw-r--r--. 1 root root     399 May 14 13:14 secrets-review.md
```

### Rust scan output
```
total 196
drwxr-xr-x. 2 root root     38 May 14 13:14 .
drwxr-xr-x. 6 root root     68 May 14 13:13 ..
-rw-r--r--. 1 root root 198051 May 14 13:14 inspection-snapshot.json
```

## Section Parity

- **services:** DIVERGENCE (1710 lines)
- **storage:** DIVERGENCE (94 lines)
- **kernelboot:** DIVERGENCE (944 lines)

## Conclusion

[Review diffs above and fill in assessment]
