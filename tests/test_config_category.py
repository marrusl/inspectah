"""Tests for config file path classification."""
import pytest
from yoinkc.inspectors.config import classify_config_path
from yoinkc.schema import ConfigCategory


@pytest.mark.parametrize("path, expected", [
    # tmpfiles
    ("/etc/tmpfiles.d/myapp.conf", ConfigCategory.TMPFILES),
    ("/etc/tmpfiles.d/nested/file.conf", ConfigCategory.TMPFILES),
    # environment
    ("/etc/environment", ConfigCategory.ENVIRONMENT),
    ("/etc/profile.d/custom.sh", ConfigCategory.ENVIRONMENT),
    ("/etc/profile.d/proxy.sh", ConfigCategory.ENVIRONMENT),
    # audit
    ("/etc/audit/rules.d/custom.rules", ConfigCategory.AUDIT),
    # library path
    ("/etc/ld.so.conf.d/custom.conf", ConfigCategory.LIBRARY_PATH),
    # journal
    ("/etc/systemd/journald.conf.d/rate-limit.conf", ConfigCategory.JOURNAL),
    # logrotate
    ("/etc/logrotate.d/myapp", ConfigCategory.LOGROTATE),
    # automount
    ("/etc/auto.master", ConfigCategory.AUTOMOUNT),
    ("/etc/auto.misc", ConfigCategory.AUTOMOUNT),
    ("/etc/auto.nfs", ConfigCategory.AUTOMOUNT),
    # sysctl
    ("/etc/sysctl.d/99-custom.conf", ConfigCategory.SYSCTL),
    ("/etc/sysctl.conf", ConfigCategory.SYSCTL),
    # crypto policy
    ("/etc/crypto-policies/config", ConfigCategory.CRYPTO_POLICY),
    ("/etc/crypto-policies/policies/custom.pol", ConfigCategory.CRYPTO_POLICY),
    ("/etc/crypto-policies/policies/custom/subpolicy.pol", ConfigCategory.CRYPTO_POLICY),
    # identity — nsswitch, sssd, kerberos, ipa
    ("/etc/nsswitch.conf", ConfigCategory.IDENTITY),
    ("/etc/sssd/sssd.conf", ConfigCategory.IDENTITY),
    ("/etc/sssd/conf.d/custom.conf", ConfigCategory.IDENTITY),
    ("/etc/krb5.conf", ConfigCategory.IDENTITY),
    ("/etc/krb5.conf.d/ipa.conf", ConfigCategory.IDENTITY),
    ("/etc/krb5.conf.d/custom.conf", ConfigCategory.IDENTITY),
    ("/etc/ipa/ipa.conf", ConfigCategory.IDENTITY),
    ("/etc/ipa/ca/ca.crt", ConfigCategory.IDENTITY),
    # limits
    ("/etc/security/limits.conf", ConfigCategory.LIMITS),
    ("/etc/security/limits.d/elasticsearch.conf", ConfigCategory.LIMITS),
    ("/etc/security/limits.d/custom.conf", ConfigCategory.LIMITS),
    # other — no match
    ("/etc/nginx/nginx.conf", ConfigCategory.OTHER),
    ("/etc/ssh/sshd_config", ConfigCategory.OTHER),
    ("/etc/fstab", ConfigCategory.OTHER),
    # edge cases
    ("/etc/profile.d.bak", ConfigCategory.OTHER),  # not a directory prefix
    ("/etc/sysctl.conf.bak", ConfigCategory.OTHER),  # exact match only
    ("/etc/environment.d/50-custom.conf", ConfigCategory.OTHER),  # systemd env generators, different from /etc/environment
    ("/etc/security/sudo.conf", ConfigCategory.OTHER),  # not /etc/security/limits
])
def test_classify_config_path(path, expected):
    assert classify_config_path(path) == expected
