# Secrets Review

The following items were redacted or excluded. Handle them manually (e.g. Kubernetes secret, systemd credential, env at deploy).

| Path | Pattern | Line | Remediation |
|------|---------|------|-------------|
| /etc/rhsm/rhsm.conf | PASSWORD | content | Use a secret store or inject at deploy time. |
