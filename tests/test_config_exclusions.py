"""
Tests for the unowned-file exclusion lists in the config inspector.

The goal: every file in EXCLUDE_EXACT / EXCLUDE_GLOBS should return True
from _is_excluded_unowned, and a selection of genuine operator-placed
configs should return False.
"""

import pytest
from yoinkc.inspectors.config import _is_excluded_unowned


# ---------------------------------------------------------------------------
# Should be EXCLUDED (system-generated noise)
# ---------------------------------------------------------------------------

MACHINE_IDENTITY = [
    "/etc/machine-id",
    "/etc/adjtime",
    "/etc/hostname",
    "/etc/localtime",
    "/etc/machine-info",
]

BACKUP_FILES = [
    "/etc/.pwd.lock",
    "/etc/.updated",
    "/etc/passwd-",
    "/etc/shadow-",
    "/etc/group-",
    "/etc/gshadow-",
    "/etc/subuid-",
    "/etc/subgid-",
]

SYSTEMD_SYMLINKS = [
    "/etc/systemd/system/default.target",
    "/etc/systemd/system/dbus.service",
    "/etc/systemd/user/dbus.service",
    "/etc/systemd/system/multi-user.target.wants/httpd.service",
    "/etc/systemd/system/sockets.target.wants/cockpit.socket",
    "/etc/systemd/user/default.target.wants/xdg-user-dirs-update.service",
]

NETWORK_DNS = [
    "/etc/resolv.conf",
    "/etc/NetworkManager/NetworkManager-intern.conf",
]

RUNTIME_STATE = [
    "/etc/ld.so.cache",
    "/etc/udev/hwdb.bin",
    "/etc/tuned/active_profile",
    "/etc/tuned/profile_mode",
    "/etc/tuned/bootcmdline",
]

PKI_GENERATED = [
    "/etc/pki/ca-trust/extracted/java/cacerts",
    "/etc/pki/ca-trust/extracted/pem/tls-ca-bundle.pem",
    "/etc/pki/java/cacerts",
    "/etc/pki/tls/cert.pem",
    "/etc/pki/tls/certs/ca-bundle.crt",
    "/etc/pki/tls/certs/ca-bundle.trust.crt",
    "/etc/pki/product-default/69.pem",
]

INSTALLER_ARTIFACTS = [
    "/etc/sysconfig/anaconda",
    "/etc/sysconfig/kernel",
    "/etc/sysconfig/network-scripts/readme-ifcfg-rh.txt",
    "/etc/sysconfig/network-scripts/readme-something-else.txt",
]

SSH_HOST_KEYS = [
    "/etc/ssh/ssh_host_rsa_key",
    "/etc/ssh/ssh_host_rsa_key.pub",
    "/etc/ssh/ssh_host_ed25519_key",
    "/etc/ssh/ssh_host_ecdsa_key.pub",
]

LVM_METADATA = [
    "/etc/lvm/archive/vg0_00000-12345.vg",
    "/etc/lvm/backup/vg0",
    "/etc/lvm/devices/system.devices",
]

ALTERNATIVES = [
    "/etc/alternatives/python",
    "/etc/alternatives/python3",
    "/etc/alternatives/java",
]

SELINUX_BINARY = [
    "/etc/selinux/targeted/policy/policy.33",
    "/etc/selinux/targeted/contexts/files/file_contexts.bin",
]

FIREWALLD_BACKUPS = [
    "/etc/firewalld/zones/public.xml.old",
    "/etc/firewalld/direct.xml.old",
]

DNF_STATE = [
    "/etc/dnf/dnf.conf",
    "/etc/yum.conf",
]


@pytest.mark.parametrize("path", MACHINE_IDENTITY)
def test_machine_identity_excluded(path):
    assert _is_excluded_unowned(path), f"Should be excluded: {path}"


@pytest.mark.parametrize("path", BACKUP_FILES)
def test_backup_files_excluded(path):
    assert _is_excluded_unowned(path), f"Should be excluded: {path}"


@pytest.mark.parametrize("path", SYSTEMD_SYMLINKS)
def test_systemd_symlinks_excluded(path):
    assert _is_excluded_unowned(path), f"Should be excluded: {path}"


@pytest.mark.parametrize("path", NETWORK_DNS)
def test_network_dns_excluded(path):
    assert _is_excluded_unowned(path), f"Should be excluded: {path}"


@pytest.mark.parametrize("path", RUNTIME_STATE)
def test_runtime_state_excluded(path):
    assert _is_excluded_unowned(path), f"Should be excluded: {path}"


@pytest.mark.parametrize("path", PKI_GENERATED)
def test_pki_generated_excluded(path):
    assert _is_excluded_unowned(path), f"Should be excluded: {path}"


@pytest.mark.parametrize("path", INSTALLER_ARTIFACTS)
def test_installer_artifacts_excluded(path):
    assert _is_excluded_unowned(path), f"Should be excluded: {path}"


@pytest.mark.parametrize("path", SSH_HOST_KEYS)
def test_ssh_host_keys_excluded(path):
    assert _is_excluded_unowned(path), f"Should be excluded: {path}"


@pytest.mark.parametrize("path", LVM_METADATA)
def test_lvm_metadata_excluded(path):
    assert _is_excluded_unowned(path), f"Should be excluded: {path}"


@pytest.mark.parametrize("path", ALTERNATIVES)
def test_alternatives_excluded(path):
    assert _is_excluded_unowned(path), f"Should be excluded: {path}"


@pytest.mark.parametrize("path", SELINUX_BINARY)
def test_selinux_binary_excluded(path):
    assert _is_excluded_unowned(path), f"Should be excluded: {path}"


@pytest.mark.parametrize("path", FIREWALLD_BACKUPS)
def test_firewalld_backups_excluded(path):
    assert _is_excluded_unowned(path), f"Should be excluded: {path}"


@pytest.mark.parametrize("path", DNF_STATE)
def test_dnf_state_excluded(path):
    assert _is_excluded_unowned(path), f"Should be excluded: {path}"


# ---------------------------------------------------------------------------
# Should NOT be excluded (genuine operator-placed configs)
# ---------------------------------------------------------------------------

GENUINE_CONFIGS = [
    "/etc/httpd/conf/httpd.conf",
    "/etc/nginx/nginx.conf",
    "/etc/myapp/config.yaml",
    "/etc/cron.d/backup",
    "/etc/sudoers.d/wheel",
    "/etc/pam.d/custom-service",
    "/etc/sysconfig/myapp",          # operator-placed sysconfig, not installer
    "/etc/NetworkManager/conf.d/99-unmanaged-devices.conf",  # operator drop-in
    "/etc/ssh/sshd_config",          # operator-modified sshd config (not a host key)
    "/etc/firewalld/zones/public.xml",       # operator firewall zone
    "/etc/firewalld/direct.xml",     # operator direct rules
    "/etc/yum.repos.d/rhel.repo",
    "/etc/tuned/recommend.d/custom.conf",    # operator tuned config
    "/etc/systemd/system/myapp.service",     # operator unit file
    "/etc/systemd/system/myapp.timer",
    "/etc/selinux/config",
]


@pytest.mark.parametrize("path", GENUINE_CONFIGS)
def test_genuine_configs_not_excluded(path):
    assert not _is_excluded_unowned(path), f"Should NOT be excluded: {path}"
