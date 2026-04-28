package pipeline

import (
	"encoding/json"
	"fmt"
	"regexp"
	"strings"

	"github.com/marrusl/inspectah/cmd/inspectah/internal/schema"
)

// RedactOptions controls redaction behavior.
type RedactOptions struct {
	// NoRedaction runs detection without modifying content. All findings
	// become flagged instead of redacted.
	NoRedaction bool

	// Sensitivity is "strict" (redact high-confidence heuristics) or
	// "moderate" (flag all heuristic findings).
	Sensitivity string
}

// redactPattern is a compiled secret-detection pattern.
type redactPattern struct {
	re        *regexp.Regexp
	typeLabel string
}

// Patterns ordered most specific first. Compiled at init time.
var redactPatterns = func() []redactPattern {
	raw := []struct {
		pattern   string
		typeLabel string
	}{
		// PEM-encoded keys and certificates
		{`(?s)-----BEGIN (?:RSA |EC |DSA |OPENSSH )?PRIVATE KEY-----.*?-----END (?:RSA |EC |DSA |OPENSSH )?PRIVATE KEY-----`, "PRIVATE_KEY"},
		{`(?s)-----BEGIN CERTIFICATE-----.*?-----END CERTIFICATE-----`, "CERTIFICATE"},

		// Shadow password hashes ($algo$salt$hash)
		{`\$[1256y]\$[A-Za-z0-9./]+\$[A-Za-z0-9./]+`, "PASSWORD_HASH"},

		// Generic password/secret assignments
		{`(?i)((?:password|passwd|secret|token|api_key|apikey|auth_token|access_key|private_key)\s*[:=]\s*)(\S+)`, "PASSWORD"},

		// AWS access key
		{`(?i)((?:aws_access_key_id|aws_secret_access_key)\s*[:=]\s*)(\S+)`, "AWS_KEY"},

		// Database connection strings
		{`(?i)jdbc:[^:]+://[^:]+:([^@\s]+)@`, "JDBC_PASSWORD"},
		{`(?i)postgres(?:ql)?://[^:]+:([^@\s]+)@`, "POSTGRES_PASSWORD"},
		{`(?i)mongodb(?:\+srv)?://[^:]+:([^@\s]+)@`, "MONGODB_PASSWORD"},
		{`(?i)redis://[^:]*:([^@\s]+)@`, "REDIS_PASSWORD"},

		// WireGuard private key
		{`(PrivateKey\s*=\s*)([A-Za-z0-9+/]{43}=)`, "WIREGUARD_KEY"},

		// WiFi PSK
		{`(psk\s*=\s*)(\S+)`, "WIFI_PSK"},
	}

	patterns := make([]redactPattern, 0, len(raw))
	for _, r := range raw {
		patterns = append(patterns, redactPattern{
			re:        regexp.MustCompile(r.pattern),
			typeLabel: r.typeLabel,
		})
	}
	return patterns
}()

// Paths whose files are always excluded entirely (never have content in output).
var excludedPaths = []string{
	"/etc/shadow",
	"/etc/shadow-",
	"/etc/gshadow",
	"/etc/gshadow-",
	"/etc/pki/tls/private/*",
	"/etc/ssl/private/*",
	"/etc/ssh/ssh_host_*_key",
}

// falsePositiveValues are common non-secret values that appear after
// "password:" or "passwd:" in config files.
var falsePositiveValues = map[string]bool{
	"files":         true,
	"compat":        true,
	"sss":           true,
	"ldap":          true,
	"nis":           true,
	"hesiod":        true,
	"systemd":       true,
	"nisplus":       true,
	"winbind":       true,
	"required":      true,
	"sufficient":    true,
	"optional":      true,
	"include":       true,
	"substack":      true,
	"pam_unix.so":   true,
	"pam_sss.so":    true,
	"pam_deny.so":   true,
	"pam_permit.so": true,
	"pam_env.so":    true,
	"requisite":     true,
}

// counterRegistry maps (typeLabel, secretValue) to deterministic sequential tokens.
type counterRegistry struct {
	counters map[string]int
	seen     map[string]string
}

func newCounterRegistry() *counterRegistry {
	return &counterRegistry{
		counters: make(map[string]int),
		seen:     make(map[string]string),
	}
}

func (r *counterRegistry) getToken(typeLabel, value string) string {
	key := typeLabel + "\x00" + value
	if tok, ok := r.seen[key]; ok {
		return tok
	}
	r.counters[typeLabel]++
	tok := fmt.Sprintf("REDACTED_%s_%d", typeLabel, r.counters[typeLabel])
	r.seen[key] = tok
	return tok
}

// isExcludedPath checks if a path matches the excluded patterns.
func isExcludedPath(path string) bool {
	normalised := "/" + strings.TrimPrefix(path, "/")
	for _, pat := range excludedPaths {
		re := "^" + strings.ReplaceAll(pat, "*", ".*") + "$"
		if matched, _ := regexp.MatchString(re, normalised); matched {
			return true
		}
	}
	return false
}

// isCommentLine returns true if the character at pos is on a comment line.
func isCommentLine(text string, pos int) bool {
	// Find the start of the line
	lineStart := strings.LastIndex(text[:pos], "\n")
	if lineStart < 0 {
		lineStart = 0
	} else {
		lineStart++ // skip the newline
	}
	line := strings.TrimSpace(text[lineStart:pos])
	return strings.HasPrefix(line, "#") || strings.HasPrefix(line, "//") || strings.HasPrefix(line, ";")
}

// redactText scans text for secret patterns and replaces them.
func redactText(text, path string, registry *counterRegistry, source string) (string, []schema.RedactionFinding) {
	var findings []schema.RedactionFinding
	out := text

	for _, pat := range redactPatterns {
		matches := pat.re.FindAllStringSubmatchIndex(out, -1)
		if len(matches) == 0 {
			continue
		}

		// Process in reverse to preserve positions
		for i := len(matches) - 1; i >= 0; i-- {
			loc := matches[i]
			matchStart, matchEnd := loc[0], loc[1]

			if isCommentLine(out, matchStart) {
				continue
			}

			var replacement string
			if pat.typeLabel == "PRIVATE_KEY" {
				token := registry.getToken(pat.typeLabel, out[matchStart:matchEnd])
				replacement = token
			} else {
				// Get the captured secret value (group 2 if exists, else full match)
				secretStart, secretEnd := matchStart, matchEnd
				if len(loc) >= 6 && loc[4] >= 0 {
					secretStart = loc[4]
					secretEnd = loc[5]
				}
				secret := out[secretStart:secretEnd]

				if pat.typeLabel == "PASSWORD" && falsePositiveValues[strings.TrimSpace(strings.ToLower(secret))] {
					continue
				}

				token := registry.getToken(pat.typeLabel, secret)

				// Preserve prefix if it contains assignment syntax
				if len(loc) >= 4 && loc[2] >= 0 {
					prefix := out[loc[2]:loc[3]]
					if strings.Contains(prefix, "=") || strings.Contains(prefix, ":") {
						replacement = prefix + token
						matchStart = loc[2]
					} else {
						replacement = token
					}
				} else {
					replacement = token
				}
			}

			// Calculate line number
			var lineNum *int
			if source == "file" || source == "diff" {
				ln := strings.Count(out[:matchStart], "\n") + 1
				lineNum = &ln
			}

			findings = append(findings, schema.RedactionFinding{
				Path:            path,
				Source:          source,
				Kind:            "inline",
				Pattern:         pat.typeLabel,
				Remediation:     "value-removed",
				Replacement:     &replacement,
				Line:            lineNum,
				DetectionMethod: "pattern",
			})

			out = out[:matchStart] + replacement + out[matchEnd:]
		}
	}

	return out, findings
}

// RedactSnapshot applies pattern-based redaction to the snapshot.
func RedactSnapshot(snap *schema.InspectionSnapshot) *schema.InspectionSnapshot {
	registry := newCounterRegistry()
	var allFindings []schema.RedactionFinding

	// Config files
	if snap.Config != nil {
		for i, f := range snap.Config.Files {
			if !f.Include || f.Content == "" {
				continue
			}
			if isExcludedPath(f.Path) {
				allFindings = append(allFindings, schema.RedactionFinding{
					Path:            f.Path,
					Source:          "file",
					Kind:            "excluded",
					Pattern:         "excluded_path",
					Remediation:     "provision",
					DetectionMethod: "pattern",
				})
				snap.Config.Files[i].Include = false
				continue
			}
			redacted, findings := redactText(f.Content, f.Path, registry, "file")
			snap.Config.Files[i].Content = redacted
			allFindings = append(allFindings, findings...)
		}
	}

	// Shadow entries
	if snap.UsersGroups != nil {
		for i, entry := range snap.UsersGroups.ShadowEntries {
			redacted, findings := redactText(entry, "/etc/shadow", registry, "shadow")
			snap.UsersGroups.ShadowEntries[i] = redacted
			allFindings = append(allFindings, findings...)
		}
	}

	// Container env vars
	if snap.Containers != nil {
		for i, c := range snap.Containers.RunningContainers {
			for j, env := range c.Env {
				redacted, findings := redactText(env, fmt.Sprintf("container:%s", c.Name), registry, "container-env")
				snap.Containers.RunningContainers[i].Env[j] = redacted
				allFindings = append(allFindings, findings...)
			}
		}
	}

	// Non-RPM env files
	if snap.NonRpmSoftware != nil {
		for i, f := range snap.NonRpmSoftware.EnvFiles {
			if !f.Include || f.Content == "" {
				continue
			}
			redacted, findings := redactText(f.Content, f.Path, registry, "file")
			snap.NonRpmSoftware.EnvFiles[i].Content = redacted
			allFindings = append(allFindings, findings...)
		}
	}

	// Convert findings to JSON and append to Redactions
	for _, f := range allFindings {
		data, err := json.Marshal(f)
		if err != nil {
			continue
		}
		snap.Redactions = append(snap.Redactions, data)
	}

	return snap
}
