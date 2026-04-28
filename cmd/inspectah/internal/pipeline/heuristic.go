package pipeline

import (
	"math"
	"regexp"
	"strings"
)

// HeuristicCandidate is a potential secret found by heuristic analysis.
type HeuristicCandidate struct {
	Path       string
	Line       int
	Value      string
	Confidence string // "high" or "low"
	Reason     string
}

// Entropy thresholds (bits per character)
const (
	entropyThresholdMixed  = 4.0
	entropyThresholdHex    = 3.5
	entropyThresholdBase64 = 4.5
	minValueLength         = 8
	maxValueLength         = 256
)

// Secret keywords that indicate a value is likely a secret.
var secretKeywords = map[string]bool{
	"password":    true,
	"passwd":      true,
	"secret":      true,
	"token":       true,
	"api_key":     true,
	"apikey":      true,
	"auth_token":  true,
	"access_key":  true,
	"private_key": true,
	"credential":  true,
	"passphrase":  true,
	"signing_key": true,
}

// Heuristic false positive values.
var heuristicFalsePositives = map[string]bool{
	"true": true, "false": true, "yes": true, "no": true,
	"none": true, "null": true, "undefined": true,
	"localhost": true, "0.0.0.0": true, "127.0.0.1": true,
	"example.com": true, "changeme": true,
}

// Paths excluded from heuristic scanning (subscription certs).
var heuristicExcludedPrefixes = []string{
	"/etc/pki/entitlement/",
	"/etc/rhsm/",
	"/etc/pki/consumer/",
}

var (
	hexChecksumRe     = regexp.MustCompile(`^[0-9a-f]{32}$|^[0-9a-f]{40}$|^[0-9a-f]{64}$|^[0-9A-F]{32}$|^[0-9A-F]{40}$|^[0-9A-F]{64}$`)
	kvAssignmentRe    = regexp.MustCompile(`(?i)([A-Za-z_][A-Za-z0-9_]*)\s*[:=]\s*(\S+)`)
	vendorPrefixRe    = regexp.MustCompile(`^[a-zA-Z]{2,8}_[a-zA-Z0-9]{16,}$`)
	alreadyRedactedRe = regexp.MustCompile(`\[?REDACTED`)
)

// shannonEntropy computes Shannon entropy in bits per character.
func shannonEntropy(s string) float64 {
	if len(s) == 0 {
		return 0.0
	}
	freq := make(map[rune]int)
	for _, ch := range s {
		freq[ch]++
	}
	length := float64(len([]rune(s)))
	var entropy float64
	for _, count := range freq {
		p := float64(count) / length
		entropy -= p * math.Log2(p)
	}
	return entropy
}

// isSecretKeyword checks if a key contains a secret-related keyword.
func isSecretKeyword(key string) bool {
	lower := strings.ToLower(key)
	for kw := range secretKeywords {
		if strings.Contains(lower, kw) {
			return true
		}
	}
	return false
}

// isHeuristicExcluded checks if a path is excluded from heuristic scanning.
func isHeuristicExcluded(path string) bool {
	normalised := "/" + strings.TrimPrefix(path, "/")
	for _, prefix := range heuristicExcludedPrefixes {
		if strings.HasPrefix(normalised, prefix) {
			return true
		}
	}
	return false
}

// FindHeuristicCandidates scans content for heuristic secret indicators.
func FindHeuristicCandidates(content, path string) []HeuristicCandidate {
	if isHeuristicExcluded(path) {
		return nil
	}

	var candidates []HeuristicCandidate

	for lineNo, line := range strings.Split(content, "\n") {
		trimmed := strings.TrimSpace(line)
		if trimmed == "" || strings.HasPrefix(trimmed, "#") || strings.HasPrefix(trimmed, "//") {
			continue
		}
		if alreadyRedactedRe.MatchString(trimmed) {
			continue
		}

		// Check key=value assignments
		for _, match := range kvAssignmentRe.FindAllStringSubmatch(trimmed, -1) {
			if len(match) < 3 {
				continue
			}
			key := match[1]
			value := match[2]

			// Strip quotes
			value = strings.Trim(value, `"'`)

			if len(value) < minValueLength || len(value) > maxValueLength {
				continue
			}
			if heuristicFalsePositives[strings.ToLower(value)] {
				continue
			}
			if hexChecksumRe.MatchString(value) {
				continue
			}

			entropy := shannonEntropy(value)

			confidence := ""
			reason := ""

			if isSecretKeyword(key) && entropy > entropyThresholdMixed {
				confidence = "high"
				reason = "secret keyword + high entropy"
			} else if isSecretKeyword(key) {
				confidence = "low"
				reason = "secret keyword"
			} else if entropy > entropyThresholdBase64 && vendorPrefixRe.MatchString(value) {
				confidence = "high"
				reason = "vendor prefix + high entropy"
			} else if entropy > entropyThresholdBase64 {
				confidence = "low"
				reason = "high entropy value"
			}

			if confidence != "" {
				candidates = append(candidates, HeuristicCandidate{
					Path:       path,
					Line:       lineNo + 1,
					Value:      value,
					Confidence: confidence,
					Reason:     reason,
				})
			}
		}
	}

	return candidates
}
