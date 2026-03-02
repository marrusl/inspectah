"""
Tests for the unowned-file exclusion lists in the config inspector.

The goal: every file in EXCLUDE_EXACT / EXCLUDE_GLOBS should return True
from _is_excluded_unowned, and genuine operator-placed configs should
return False.
"""

from yoinkc.inspectors.config import _is_excluded_unowned


# Paths that must be excluded (system-generated noise)
_EXCLUDED = [
    # Machine identity
    "/etc/machine-id", "/etc/adjtime", "/etc/hostname", "/etc/localtime",
    "/etc/machine-info",
    # Backup files
    "/etc/.pwd.lock", "/etc/.updated", "/etc/passwd-", "/etc/shadow-",
    "/etc/group-", "/etc/gshadow-", "/etc/subuid-", "/etc/subgid-",
    # systemd symlinks and targets
    "/etc/systemd/system/default.target", "/etc/systemd/system/dbus.service",
    "/etc/systemd/user/dbus.service",
    "/etc/systemd/system/multi-user.target.wants/httpd.service",
    "/etc/systemd/system/sockets.target.wants/cockpit.socket",
    "/etc/systemd/user/default.target.wants/xdg-user-dirs-update.service",
    "/etc/systemd/system/ctrl-alt-del.target",
    # Network / DNS
    "/etc/resolv.conf", "/etc/NetworkManager/NetworkManager-intern.conf",
    # System library state
    "/etc/ld.so.cache", "/etc/ld.so.conf", "/etc/mtab", "/etc/rpc",
    "/etc/udev/hwdb.bin",
    "/etc/tuned/active_profile", "/etc/tuned/profile_mode", "/etc/tuned/bootcmdline",
    # PKI generated
    "/etc/pki/ca-trust/extracted/java/cacerts",
    "/etc/pki/ca-trust/extracted/pem/tls-ca-bundle.pem",
    "/etc/pki/java/cacerts", "/etc/pki/tls/cert.pem",
    "/etc/pki/tls/certs/ca-bundle.crt", "/etc/pki/tls/certs/ca-bundle.trust.crt",
    "/etc/pki/product-default/69.pem",   # RHEL 9
    "/etc/pki/product-default/479.pem",  # RHEL 10
    "/etc/pki/tls/ct_log_list.cnf", "/etc/pki/tls/fips_local.cnf",
    "/etc/pki/tls/openssl.cnf",
    "/etc/pki/consumer/cert.pem", "/etc/pki/consumer/key.pem",
    "/etc/pki/entitlement/12345.pem",
    # Installer / legacy artifacts
    "/etc/sysconfig/anaconda", "/etc/sysconfig/kernel",
    "/etc/sysconfig/network", "/etc/sysconfig/selinux",
    "/etc/sysconfig/network-scripts/readme-ifcfg-rh.txt",
    "/etc/sysconfig/network-scripts/readme-something-else.txt",
    # SSH host keys
    "/etc/ssh/ssh_host_rsa_key", "/etc/ssh/ssh_host_rsa_key.pub",
    "/etc/ssh/ssh_host_ed25519_key", "/etc/ssh/ssh_host_ecdsa_key.pub",
    # LVM metadata
    "/etc/lvm/archive/vg0_00000-12345.vg", "/etc/lvm/backup/vg0",
    "/etc/lvm/devices/system.devices",
    # Alternatives
    "/etc/alternatives/python", "/etc/alternatives/python3", "/etc/alternatives/java",
    # SELinux policy store (compiled, not operator config)
    "/etc/selinux/targeted/policy/policy.33",
    "/etc/selinux/targeted/contexts/files/file_contexts.bin",
    "/etc/selinux/targeted/contexts/files/file_contexts",
    "/etc/selinux/targeted/contexts/files/file_contexts.homedirs",
    "/etc/selinux/targeted/contexts/default_contexts",
    "/etc/selinux/targeted/contexts/dbus_contexts",
    "/etc/selinux/targeted/contexts/users/unconfined_u",
    "/etc/selinux/targeted/setrans.conf",
    "/etc/selinux/targeted/seusers",
    "/etc/selinux/targeted/.policy.sha512",
    "/etc/selinux/targeted/booleans.subs_dist",
    # Firewalld backups
    "/etc/firewalld/zones/public.xml.old", "/etc/firewalld/direct.xml.old",
    # Package manager
    "/etc/dnf/dnf.conf", "/etc/yum.conf", "/etc/npmrc",
    "/etc/dnf/protected.d/selinux-policy-targeted.conf",
    # Bootloader
    "/etc/kernel/cmdline",
    # NVMe identity
    "/etc/nvme/hostnqn", "/etc/nvme/hostid",
    # RHSM
    "/etc/rhsm/syspurpose/syspurpose.json",
    # authselect / dconf distro defaults
    "/etc/dconf/db/distro.d/20-authselect",
    "/etc/dconf/db/distro.d/locks/20-authselect",
    # Modprobe blacklists (crypto-policies / kernel)
    "/etc/modprobe.d/sctp-blacklist.conf",
    "/etc/modprobe.d/tipc_diag-blacklist.conf",
    # gnupg profile scripts
    "/etc/profile.d/gnupg2.sh", "/etc/profile.d/gnupg2.csh",
    # PAM base configs (not RPM-owned on RHEL 10)
    "/etc/pam.d/chfn", "/etc/pam.d/chsh", "/etc/pam.d/login",
    "/etc/pam.d/remote", "/etc/pam.d/runuser", "/etc/pam.d/runuser-l",
    "/etc/pam.d/su", "/etc/pam.d/su-l",
    # udisks2 defaults
    "/etc/udisks2/udisks2.conf", "/etc/udisks2/mount_options.conf.example",
    # logrotate from packages
    "/etc/logrotate.d/kvm_stat",
]

# Paths that must NOT be excluded (genuine operator configs)
_GENUINE = [
    "/etc/httpd/conf/httpd.conf", "/etc/nginx/nginx.conf",
    "/etc/myapp/config.yaml", "/etc/cron.d/backup",
    "/etc/sudoers.d/wheel", "/etc/pam.d/custom-service",
    "/etc/sysconfig/myapp",
    "/etc/NetworkManager/conf.d/99-unmanaged-devices.conf",
    "/etc/ssh/sshd_config",
    "/etc/firewalld/zones/public.xml", "/etc/firewalld/direct.xml",
    "/etc/yum.repos.d/rhel.repo",
    "/etc/tuned/recommend.d/custom.conf",
    "/etc/systemd/system/myapp.service", "/etc/systemd/system/myapp.timer",
    "/etc/selinux/config",
]


def test_system_generated_files_excluded():
    """All known system-generated paths must be excluded from the unowned list."""
    failures = [p for p in _EXCLUDED if not _is_excluded_unowned(p)]
    assert not failures, f"Should be excluded but were not: {failures}"


def test_genuine_operator_configs_not_excluded():
    """Genuine operator-placed configs must not be excluded."""
    failures = [p for p in _GENUINE if _is_excluded_unowned(p)]
    assert not failures, f"Should NOT be excluded but were: {failures}"
