package pipeline

import (
	"strings"
	"testing"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
)

func TestIsExcludedPath(t *testing.T) {
	tests := []struct {
		path     string
		excluded bool
	}{
		{"/etc/shadow", true},
		{"/etc/shadow-", true},
		{"/etc/gshadow", true},
		{"/etc/pki/tls/private/server.key", true},
		{"/etc/ssl/private/ca.key", true},
		{"/etc/ssh/ssh_host_rsa_key", true},
		{"/etc/httpd/conf/httpd.conf", false},
		{"/etc/passwd", false},
		{"/etc/ssh/ssh_host_rsa_key.pub", false},
	}

	for _, tt := range tests {
		t.Run(tt.path, func(t *testing.T) {
			got := isExcludedPath(tt.path)
			if got != tt.excluded {
				t.Errorf("got %v, want %v", got, tt.excluded)
			}
		})
	}
}

func TestRedactTextPrivateKey(t *testing.T) {
	content := `Some config
-----BEGIN RSA PRIVATE KEY-----
MIIEowIBAAKCAQEA0Z3kS0tB8/asdf+1234
-----END RSA PRIVATE KEY-----
more config`

	registry := newCounterRegistry()
	redacted, findings := redactText(content, "/etc/test", registry, "file")

	if strings.Contains(redacted, "MIIEowIBAAKCAQEA0Z3kS0tB8") {
		t.Error("private key not redacted")
	}
	if !strings.Contains(redacted, "REDACTED_PRIVATE_KEY") {
		t.Error("missing redaction token")
	}
	if len(findings) == 0 {
		t.Error("expected at least one finding")
	}
	if findings[0].Pattern != "PRIVATE_KEY" {
		t.Errorf("expected PRIVATE_KEY pattern, got %s", findings[0].Pattern)
	}
}

func TestRedactTextPasswordAssignment(t *testing.T) {
	content := `db_password=supersecret123
api_key: my-api-key-value-here`

	registry := newCounterRegistry()
	redacted, findings := redactText(content, "/etc/app.conf", registry, "file")

	if strings.Contains(redacted, "supersecret123") {
		t.Error("password not redacted")
	}
	if strings.Contains(redacted, "my-api-key-value-here") {
		t.Error("api key not redacted")
	}
	if len(findings) < 2 {
		t.Errorf("expected at least 2 findings, got %d", len(findings))
	}
}

func TestRedactTextFalsePositive(t *testing.T) {
	// nsswitch.conf uses "passwd: files sss" which should not be redacted
	content := `passwd: files sss
group: files sss`

	registry := newCounterRegistry()
	_, findings := redactText(content, "/etc/nsswitch.conf", registry, "file")

	for _, f := range findings {
		if f.Pattern == "PASSWORD" {
			t.Error("false positive: 'files' should not be detected as password")
		}
	}
}

func TestRedactTextCommentLine(t *testing.T) {
	content := `# password=old_value
password=actual_secret`

	registry := newCounterRegistry()
	redacted, findings := redactText(content, "/etc/test", registry, "file")

	// The comment line should be preserved
	if !strings.Contains(redacted, "# password=old_value") {
		t.Error("comment line should not be redacted")
	}
	// The actual value should be redacted
	if strings.Contains(redacted, "actual_secret") {
		t.Error("actual password not redacted")
	}
	if len(findings) == 0 {
		t.Error("expected at least one finding for non-comment line")
	}
}

func TestRedactTextPasswordHash(t *testing.T) {
	content := `root:$6$randomsalt$hashedvalue123:19000:0:99999:7:::`

	registry := newCounterRegistry()
	redacted, _ := redactText(content, "/etc/shadow", registry, "shadow")

	if strings.Contains(redacted, "$6$randomsalt$hashedvalue123") {
		t.Error("password hash not redacted")
	}
}

func TestRedactSnapshotConfigFiles(t *testing.T) {
	snap := schema.NewSnapshot()
	snap.Config = &schema.ConfigSection{
		Files: []schema.ConfigFileEntry{
			{
				Path:    "/etc/myapp/config.yml",
				Content: "password: supersecret",
				Include: true,
			},
			{
				Path:    "/etc/shadow",
				Content: "root:$6$xyz$hash::",
				Include: true,
			},
		},
	}

	result := RedactSnapshot(snap)

	// /etc/shadow should be excluded
	for _, f := range result.Config.Files {
		if f.Path == "/etc/shadow" && f.Include {
			t.Error("/etc/shadow should be excluded")
		}
	}

	// Config file should have password redacted
	for _, f := range result.Config.Files {
		if f.Path == "/etc/myapp/config.yml" {
			if strings.Contains(f.Content, "supersecret") {
				t.Error("password not redacted in config file")
			}
		}
	}

	// Should have redaction findings
	if len(result.Redactions) == 0 {
		t.Error("expected redaction findings")
	}
}

func TestCounterRegistryDeterministic(t *testing.T) {
	r := newCounterRegistry()

	tok1 := r.getToken("PASSWORD", "secret1")
	tok2 := r.getToken("PASSWORD", "secret2")
	tok3 := r.getToken("PASSWORD", "secret1") // Same as tok1

	if tok1 != "REDACTED_PASSWORD_1" {
		t.Errorf("first token: got %q", tok1)
	}
	if tok2 != "REDACTED_PASSWORD_2" {
		t.Errorf("second token: got %q", tok2)
	}
	if tok3 != tok1 {
		t.Errorf("repeated value should return same token: got %q, want %q", tok3, tok1)
	}
}
