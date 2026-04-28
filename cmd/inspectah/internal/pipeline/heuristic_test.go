package pipeline

import (
	"math"
	"testing"
)

func TestShannonEntropy(t *testing.T) {
	tests := []struct {
		input string
		min   float64
		max   float64
	}{
		{"", 0, 0},
		{"aaaa", 0, 0.01},
		{"abcd", 1.9, 2.1},
		{"aB3$xY9!pQ", 3.0, 4.0}, // high entropy
	}

	for _, tt := range tests {
		t.Run(tt.input, func(t *testing.T) {
			got := shannonEntropy(tt.input)
			if got < tt.min || got > tt.max {
				t.Errorf("shannonEntropy(%q) = %f, want [%f, %f]", tt.input, got, tt.min, tt.max)
			}
		})
	}
}

func TestShannonEntropyMonotonic(t *testing.T) {
	// More random strings should have higher entropy
	low := shannonEntropy("aaaabbbb")
	high := shannonEntropy("aB3$xY9!pQ2&kL")

	if high <= low {
		t.Errorf("expected high entropy (%f) > low entropy (%f)", high, low)
	}
	_ = math.Log2 // ensure math import is used
}

func TestIsSecretKeyword(t *testing.T) {
	tests := []struct {
		key    string
		secret bool
	}{
		{"password", true},
		{"DB_PASSWORD", true},
		{"api_key", true},
		{"auth_token", true},
		{"hostname", false},
		{"port", false},
		{"database", false},
	}

	for _, tt := range tests {
		t.Run(tt.key, func(t *testing.T) {
			got := isSecretKeyword(tt.key)
			if got != tt.secret {
				t.Errorf("got %v, want %v", got, tt.secret)
			}
		})
	}
}

func TestIsHeuristicExcluded(t *testing.T) {
	tests := []struct {
		path     string
		excluded bool
	}{
		{"/etc/pki/entitlement/1234.pem", true},
		{"/etc/rhsm/ca/redhat-uep.pem", true},
		{"/etc/pki/consumer/cert.pem", true},
		{"/etc/httpd/conf/httpd.conf", false},
		{"/etc/myapp/config.yml", false},
	}

	for _, tt := range tests {
		t.Run(tt.path, func(t *testing.T) {
			got := isHeuristicExcluded(tt.path)
			if got != tt.excluded {
				t.Errorf("got %v, want %v", got, tt.excluded)
			}
		})
	}
}

func TestFindHeuristicCandidatesSecretKeyword(t *testing.T) {
	content := `[database]
host = localhost
password = xK9mP2qR7vB4nL8jY3wE5tA6`

	candidates := FindHeuristicCandidates(content, "/etc/myapp.conf")

	found := false
	for _, c := range candidates {
		if c.Confidence == "high" {
			found = true
			break
		}
	}
	if !found {
		t.Error("expected high-confidence candidate for secret keyword + high entropy")
	}
}

func TestFindHeuristicCandidatesSkipsComments(t *testing.T) {
	content := `# password = not_a_real_secret
// token = also_not_real`

	candidates := FindHeuristicCandidates(content, "/etc/test")

	if len(candidates) > 0 {
		t.Errorf("expected no candidates from comment lines, got %d", len(candidates))
	}
}

func TestFindHeuristicCandidatesSkipsExcluded(t *testing.T) {
	content := `password = xK9mP2qR7vB4nL8jY3wE5tA6`

	candidates := FindHeuristicCandidates(content, "/etc/pki/entitlement/1234.pem")

	if len(candidates) > 0 {
		t.Error("subscription cert paths should be excluded")
	}
}

func TestFindHeuristicCandidatesSkipsFalsePositives(t *testing.T) {
	content := `password = localhost`

	candidates := FindHeuristicCandidates(content, "/etc/test")

	for _, c := range candidates {
		if c.Confidence == "high" {
			t.Error("'localhost' should be filtered as false positive")
		}
	}
}

func TestFindHeuristicCandidatesSkipsRedacted(t *testing.T) {
	content := `password = [REDACTED_PASSWORD_1]`

	candidates := FindHeuristicCandidates(content, "/etc/test")

	if len(candidates) > 0 {
		t.Error("already-redacted values should be skipped")
	}
}
