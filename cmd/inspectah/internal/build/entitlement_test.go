package build

import (
	"crypto/ecdsa"
	"crypto/elliptic"
	"crypto/rand"
	"crypto/x509"
	"crypto/x509/pkix"
	"encoding/pem"
	"math/big"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func writeFakeCert(t *testing.T, dir string, name string, expiry time.Time) {
	t.Helper()
	key, err := ecdsa.GenerateKey(elliptic.P256(), rand.Reader)
	require.NoError(t, err)
	tmpl := &x509.Certificate{
		SerialNumber: big.NewInt(1),
		Subject:      pkix.Name{CommonName: "test"},
		NotBefore:    time.Now().Add(-1 * time.Hour),
		NotAfter:     expiry,
	}
	der, err := x509.CreateCertificate(rand.Reader, tmpl, tmpl, &key.PublicKey, key)
	require.NoError(t, err)
	os.MkdirAll(dir, 0755)
	f, err := os.Create(filepath.Join(dir, name))
	require.NoError(t, err)
	defer f.Close()
	pem.Encode(f, &pem.Block{Type: "CERTIFICATE", Bytes: der})
}

func TestDiscoverCerts_BundledInTarball(t *testing.T) {
	dir := t.TempDir()
	entDir := filepath.Join(dir, "entitlement")
	writeFakeCert(t, entDir, "123.pem", time.Now().Add(24*time.Hour))

	result, err := DiscoverCerts(DiscoverOpts{OutputDir: dir})
	require.NoError(t, err)
	assert.Equal(t, DiscoveryCertsFound, result.Status)
	assert.Equal(t, entDir, result.EntitlementDir)
}

func TestDiscoverCerts_WithRHSM(t *testing.T) {
	dir := t.TempDir()
	entDir := filepath.Join(dir, "entitlement")
	rhsmDir := filepath.Join(dir, "rhsm")
	writeFakeCert(t, entDir, "123.pem", time.Now().Add(24*time.Hour))
	os.MkdirAll(rhsmDir, 0755)

	result, err := DiscoverCerts(DiscoverOpts{OutputDir: dir})
	require.NoError(t, err)
	assert.Equal(t, rhsmDir, result.RHSMDir)
}

func TestDiscoverCerts_NoEntitlements(t *testing.T) {
	result, err := DiscoverCerts(DiscoverOpts{SkipEntitlements: true})
	require.NoError(t, err)
	assert.Equal(t, DiscoveryNoCerts, result.Status)
}

func TestDiscoverCerts_ExplicitDirInvalid(t *testing.T) {
	_, err := DiscoverCerts(DiscoverOpts{EntitlementsDir: "/nonexistent/path"})
	assert.Error(t, err)
	assert.ErrorContains(t, err, "does not exist")
}

func TestDiscoverCerts_MutualExclusion(t *testing.T) {
	_, err := DiscoverCerts(DiscoverOpts{
		EntitlementsDir:  "/some/path",
		SkipEntitlements: true,
	})
	assert.ErrorContains(t, err, "mutually exclusive")
}

func TestValidateCertExpiry_Valid(t *testing.T) {
	dir := t.TempDir()
	writeFakeCert(t, dir, "valid.pem", time.Now().Add(24*time.Hour))

	err := ValidateCertExpiry(dir, false)
	assert.NoError(t, err)
}

func TestValidateCertExpiry_Expired(t *testing.T) {
	dir := t.TempDir()
	writeFakeCert(t, dir, "expired.pem", time.Now().Add(-1*time.Hour))

	err := ValidateCertExpiry(dir, false)
	assert.ErrorContains(t, err, "expired")
}

func TestValidateCertExpiry_IgnoreExpired(t *testing.T) {
	dir := t.TempDir()
	writeFakeCert(t, dir, "expired.pem", time.Now().Add(-1*time.Hour))

	err := ValidateCertExpiry(dir, true)
	assert.NoError(t, err)
}
