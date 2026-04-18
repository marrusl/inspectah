"""Tests for --sensitivity and --no-redaction CLI flags."""
import pytest
from inspectah.cli import parse_args

def test_sensitivity_default_strict():
    args = parse_args(["inspect", "--from-snapshot", "test.json"])
    assert args.sensitivity == "strict"

def test_sensitivity_moderate():
    args = parse_args(["inspect", "--from-snapshot", "test.json", "--sensitivity", "moderate"])
    assert args.sensitivity == "moderate"

def test_sensitivity_strict_explicit():
    args = parse_args(["inspect", "--from-snapshot", "test.json", "--sensitivity", "strict"])
    assert args.sensitivity == "strict"

def test_sensitivity_invalid():
    with pytest.raises(SystemExit):
        parse_args(["inspect", "--from-snapshot", "test.json", "--sensitivity", "paranoid"])

def test_no_redaction_flag():
    args = parse_args(["inspect", "--from-snapshot", "test.json", "--no-redaction"])
    assert args.no_redaction is True

def test_no_redaction_default_false():
    args = parse_args(["inspect", "--from-snapshot", "test.json"])
    assert args.no_redaction is False

def test_sensitivity_and_no_redaction_mutual_exclusion():
    with pytest.raises(SystemExit):
        parse_args(["inspect", "--from-snapshot", "test.json",
                    "--sensitivity", "moderate", "--no-redaction"])

def test_backward_compat_bare_flags():
    args = parse_args(["--from-snapshot", "test.json", "--sensitivity", "moderate"])
    assert args.command == "inspect"
    assert args.sensitivity == "moderate"


def test_sensitivity_passed_to_pipeline(tmp_path):
    args = parse_args(["inspect", "--from-snapshot", str(tmp_path / "test.json"),
                       "--output-dir", str(tmp_path / "output"),
                       "--sensitivity", "moderate"])
    assert args.sensitivity == "moderate"
    assert args.no_redaction is False


def test_no_redaction_passed_to_pipeline(tmp_path):
    args = parse_args(["inspect", "--from-snapshot", str(tmp_path / "test.json"),
                       "--output-dir", str(tmp_path / "output"),
                       "--no-redaction"])
    assert args.no_redaction is True
