"""Tests for heuristic secret detection engine."""
import pytest
from yoinkc.heuristic import (
    shannon_entropy,
    is_secret_keyword,
    find_heuristic_candidates,
    HeuristicCandidate,
)

# --- Shannon entropy ---
def test_entropy_low_for_repeated():
    assert shannon_entropy("aaaaaaaaaa") < 1.0

def test_entropy_high_for_random():
    val = "aR$9xk!mQ2pL7bN4cK"
    assert shannon_entropy(val) > 4.0

def test_entropy_moderate_for_hex():
    val = "a8f2b9c4d5e6f7a8b9c4d5e6f7a8b9c4"
    e = shannon_entropy(val)
    assert 3.5 < e < 4.5

def test_entropy_high_for_base64():
    val = "dGhpcyBpcyBhIHRlc3Qgc3RyaW5nZm9v"
    assert shannon_entropy(val) > 4.0

def test_entropy_empty_string():
    assert shannon_entropy("") == 0.0

# --- Keyword detection ---
def test_is_secret_keyword_positive():
    for kw in ("password", "passwd", "secret", "token", "api_key", "credential", "auth", "private_key"):
        assert is_secret_keyword(kw)

def test_is_secret_keyword_case_insensitive():
    assert is_secret_keyword("PASSWORD")
    assert is_secret_keyword("Api_Key")

def test_is_secret_keyword_negative():
    for kw in ("hostname", "port", "timeout", "description"):
        assert not is_secret_keyword(kw)

# --- Candidate finding ---
def test_finds_high_confidence_keyword_plus_entropy():
    lines = ["db_password = aR$9xk!mQ2pL7bN4cK"]
    candidates = find_heuristic_candidates(lines, "/etc/app.conf", source="file")
    assert len(candidates) >= 1
    c = candidates[0]
    assert c.confidence == "high"

def test_finds_low_confidence_entropy_only():
    lines = ["config_key = a8f2b9c4d5e6f7a8b9c4d5e6f7a8b9c4d5e6"]
    candidates = find_heuristic_candidates(lines, "/etc/app.conf", source="file")
    high = [c for c in candidates if c.confidence == "high"]
    assert len(high) == 0

def test_no_finding_for_short_value():
    lines = ["timeout = 3600"]
    candidates = find_heuristic_candidates(lines, "/etc/app.conf", source="file")
    assert len(candidates) == 0

def test_no_finding_for_boolean_after_keyword():
    lines = ["secret = false"]
    candidates = find_heuristic_candidates(lines, "/etc/app.conf", source="file")
    assert len(candidates) == 0

def test_no_finding_for_numeric_after_keyword():
    lines = ["password_min_length = 12"]
    candidates = find_heuristic_candidates(lines, "/etc/app.conf", source="file")
    assert len(candidates) == 0


# --- False positive filters ---

def test_uuid_is_false_positive():
    lines = ["session_id = 550e8400-e29b-41d4-a716-446655440000"]
    candidates = find_heuristic_candidates(lines, "/etc/app.conf", source="file")
    assert len(candidates) == 0

def test_hex_checksum_is_false_positive():
    for val in ["a" * 32, "b" * 40, "c" * 64]:
        lines = [f"checksum = {val}"]
        candidates = find_heuristic_candidates(lines, "/etc/app.conf", source="file")
        assert len(candidates) == 0, f"Hex checksum {len(val)} chars should be filtered"

def test_already_redacted_is_false_positive():
    lines = ["password = REDACTED_PASSWORD_1"]
    candidates = find_heuristic_candidates(lines, "/etc/app.conf", source="file")
    assert len(candidates) == 0

def test_comment_lines_skipped():
    lines = [
        "# password = supersecretvalue12345678",
        "; token = abcdefghijklmnop12345678",
    ]
    candidates = find_heuristic_candidates(lines, "/etc/app.conf", source="file")
    assert len(candidates) == 0

def test_vendor_prefix_residual_detected():
    lines = ["config = myprefix_aB3cD4eF5gH6iJ7kL8mN9oP0qR1sT2uV3wX4"]
    candidates = find_heuristic_candidates(lines, "/etc/app.conf", source="file")
    residual = [c for c in candidates if "prefix" in c.why_flagged.lower() or "vendor" in c.why_flagged.lower()]
    assert len(residual) >= 1
