"""
Pipeline orchestrator: run inspectors (or load snapshot), redact, optionally
bundle subscription certs, then produce a tarball or write to a directory.
"""

import json
import os
import shutil
import sys
import tempfile
from pathlib import Path
from typing import Callable, List, Optional

from .heuristic import find_heuristic_candidates, apply_noise_control, HeuristicCandidate
from .subscription import bundle_subscription_certs
from .packaging import create_tarball, get_output_stamp
from .redact import redact_snapshot
from .schema import InspectionSnapshot, RedactionFinding, SCHEMA_VERSION

# Subscription cert paths excluded from heuristic scanning
_SUBSCRIPTION_CERT_PREFIXES = (
    "/etc/pki/entitlement/",
    "/etc/rhsm/",
)


def load_snapshot(path: Path) -> InspectionSnapshot:
    """Load and deserialize an inspection snapshot from JSON."""
    data = json.loads(path.read_text())
    file_version = data.get("schema_version", 1)
    if file_version != SCHEMA_VERSION:
        raise ValueError(
            f"Snapshot was created by a different version of yoinkc "
            f"(schema v{file_version}, expected v{SCHEMA_VERSION}). "
            f"Re-run the inspection to generate a new snapshot."
        )
    return InspectionSnapshot.model_validate(data)


def save_snapshot(snapshot: InspectionSnapshot, path: Path) -> None:
    """Serialize snapshot to JSON."""
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(snapshot.model_dump_json(indent=2))


def _is_subscription_cert_path(path: str) -> bool:
    """Check if a path is under a subscription cert directory."""
    normalised = "/" + path.lstrip("/")
    return any(normalised.startswith(prefix) for prefix in _SUBSCRIPTION_CERT_PREFIXES)


def _run_heuristic_pass(
    snapshot: InspectionSnapshot,
    sensitivity: str,
    no_redaction: bool,
) -> InspectionSnapshot:
    """Run heuristic secret detection on snapshot content.

    Scans config files, container env vars, and timer commands for
    heuristic candidates.  Skips subscription cert paths.

    Candidates are converted to RedactionFinding with detection_method="heuristic".
    In strict mode, high-confidence candidates get kind="inline" (unless no_redaction).
    All others get kind="flagged".
    """
    all_candidates: List[HeuristicCandidate] = []

    # Scan config files
    if snapshot.config and snapshot.config.files:
        for entry in snapshot.config.files:
            if _is_subscription_cert_path(entry.path):
                continue
            if not entry.content:
                continue
            lines = entry.content.splitlines()
            candidates = find_heuristic_candidates(lines, entry.path, source="file")
            all_candidates.extend(candidates)

    # Scan container env vars
    if snapshot.containers and snapshot.containers.running_containers:
        for container in snapshot.containers.running_containers:
            name = container.name or container.id[:12]
            for env_line in container.env:
                candidates = find_heuristic_candidates(
                    [env_line],
                    f"containers:running/{name}:env",
                    source="container-env",
                )
                all_candidates.extend(candidates)

    # Scan timer commands
    if snapshot.scheduled_tasks:
        if snapshot.scheduled_tasks.generated_timer_units:
            for unit in snapshot.scheduled_tasks.generated_timer_units:
                for field_name, content in [("command", unit.command), ("service_content", unit.service_content)]:
                    if not content:
                        continue
                    lines = content.splitlines()
                    candidates = find_heuristic_candidates(
                        lines,
                        f"scheduled:timer/{unit.name}:{field_name}",
                        source="timer-cmd",
                    )
                    all_candidates.extend(candidates)

        if snapshot.scheduled_tasks.systemd_timers:
            for timer in snapshot.scheduled_tasks.systemd_timers:
                if timer.source != "local":
                    continue
                if not timer.service_content:
                    continue
                lines = timer.service_content.splitlines()
                candidates = find_heuristic_candidates(
                    lines,
                    f"scheduled:systemd_timer/{timer.name}:service_content",
                    source="timer-cmd",
                )
                all_candidates.extend(candidates)

    # Apply noise control
    noise_result = apply_noise_control(all_candidates)

    # Convert to RedactionFinding
    new_findings: List[RedactionFinding] = []
    for candidate in noise_result.reported:
        should_redact = (
            sensitivity == "strict"
            and candidate.confidence == "high"
            and not no_redaction
        )
        kind = "inline" if should_redact else "flagged"
        remediation = "value-removed" if should_redact else ""

        new_findings.append(RedactionFinding(
            path=candidate.path,
            source=candidate.source,
            kind=kind,
            pattern="heuristic",
            remediation=remediation,
            line=candidate.line_number,
            detection_method="heuristic",
            confidence=candidate.confidence,
        ))

    if new_findings:
        updated_redactions = list(snapshot.redactions) + new_findings
        return snapshot.model_copy(update={"redactions": updated_redactions})
    return snapshot


def _print_secrets_summary(snapshot: InspectionSnapshot) -> None:
    """Print secrets handling summary to stderr."""

    findings = [r for r in snapshot.redactions if isinstance(r, RedactionFinding)]
    if not findings:
        return

    excluded_regen = [f for f in findings if f.kind == "excluded" and f.remediation == "regenerate"]
    excluded_prov = [f for f in findings if f.kind == "excluded" and f.remediation == "provision"]
    inline = [f for f in findings if f.kind == "inline"]
    inline_files = len({f.path for f in inline if f.source == "file"})
    flagged = [f for f in findings if f.kind == "flagged"]

    # Break down inline by detection method
    inline_pattern = [f for f in inline if f.detection_method == "pattern"]
    inline_heuristic = [f for f in inline if f.detection_method == "heuristic"]

    print("Secrets handling:", file=sys.stderr)
    if excluded_regen:
        n = len(excluded_regen)
        print(f"  Excluded (regenerate on target): {n} file{'s' if n != 1 else ''}", file=sys.stderr)
    if excluded_prov:
        n = len(excluded_prov)
        print(f"  Excluded (provision from store): {n} file{'s' if n != 1 else ''}", file=sys.stderr)
    if inline:
        n = len(inline)
        suffix = f" ({len(inline_pattern)} pattern, {len(inline_heuristic)} heuristic)" if inline_heuristic else ""
        print(f"  Inline-redacted: {n} value{'s' if n != 1 else ''} in {inline_files} file{'s' if inline_files != 1 else ''}{suffix}", file=sys.stderr)
    if flagged:
        flagged_heuristic = [f for f in flagged if f.detection_method == "heuristic"]
        n = len(flagged_heuristic)
        if n:
            print(f"  Flagged for review: {n} heuristic finding{'s' if n != 1 else ''}", file=sys.stderr)
    legacy = [r for r in snapshot.redactions if not isinstance(r, RedactionFinding)]
    if legacy:
        print(f"  Legacy (untyped): {len(legacy)} entries", file=sys.stderr)
    print("  Details: secrets-review.md | Placeholders: redacted/", file=sys.stderr)


def run_pipeline(
    *,
    host_root: Path,
    run_inspectors: Optional[Callable[[Path], InspectionSnapshot]],
    run_renderers: Callable[[InspectionSnapshot, Path], None],
    from_snapshot_path: Optional[Path] = None,
    inspect_only: bool = False,
    output_file: Optional[Path] = None,
    output_dir: Optional[Path] = None,
    no_subscription: bool = False,
    cwd: Optional[Path] = None,
    sensitivity: str = "strict",
    no_redaction: bool = False,
) -> InspectionSnapshot:
    """Run the yoinkc pipeline.

    Output modes (mutually exclusive):
    - output_file: write tarball to this path
    - output_dir: write files to this directory
    - neither: write tarball to CWD with auto-generated name

    inspect_only: save snapshot to CWD and exit early.
    cwd: override working directory for default output paths (testing).
    sensitivity: "strict" (redact high-confidence heuristics) or "moderate" (flag all).
    no_redaction: run detection without modifying content; all findings become flagged.
    """
    working_dir = cwd or Path.cwd()

    # Load or build the snapshot
    if from_snapshot_path is not None:
        snapshot = load_snapshot(from_snapshot_path)
        if no_redaction:
            # Run redaction to generate findings, then restore original content
            original_snapshot = snapshot.model_copy(deep=True)
            redacted = redact_snapshot(snapshot)
            # Keep findings but mark all as flagged, restore original content
            flagged_findings = []
            for r in redacted.redactions:
                if isinstance(r, RedactionFinding):
                    flagged_findings.append(r.model_copy(update={"kind": "flagged"}))
                else:
                    flagged_findings.append(r)
            snapshot = original_snapshot.model_copy(update={"redactions": flagged_findings})
        else:
            snapshot = redact_snapshot(snapshot)
    else:
        assert run_inspectors is not None, "run_inspectors required when not loading from snapshot"
        snapshot = run_inspectors(host_root)
        if no_redaction:
            original_snapshot = snapshot.model_copy(deep=True)
            redacted = redact_snapshot(snapshot)
            flagged_findings = []
            for r in redacted.redactions:
                if isinstance(r, RedactionFinding):
                    flagged_findings.append(r.model_copy(update={"kind": "flagged"}))
                else:
                    flagged_findings.append(r)
            snapshot = original_snapshot.model_copy(update={"redactions": flagged_findings})
        else:
            snapshot = redact_snapshot(snapshot)

    # Run heuristic pass after pattern redaction
    snapshot = _run_heuristic_pass(snapshot, sensitivity, no_redaction)

    # --inspect-only: save snapshot and return
    if inspect_only:
        save_snapshot(snapshot, working_dir / "inspection-snapshot.json")
        return snapshot

    # Render into a temp directory
    tmp_dir = Path(tempfile.mkdtemp(prefix="yoinkc-"))
    try:
        save_snapshot(snapshot, tmp_dir / "inspection-snapshot.json")
        run_renderers(snapshot, tmp_dir)
        _print_secrets_summary(snapshot)

        # Bundle subscription certs (skip in --from-snapshot mode where
        # host filesystem may not be mounted)
        if not no_subscription and from_snapshot_path is None:
            bundle_subscription_certs(host_root, tmp_dir)

        # Output: tarball or directory
        if output_dir is not None:
            output_dir.mkdir(parents=True, exist_ok=True)
            for item in tmp_dir.iterdir():
                dest = output_dir / item.name
                if item.is_dir():
                    shutil.copytree(item, dest, dirs_exist_ok=True)
                else:
                    shutil.copy2(item, dest)
        else:
            # Prefer the hostname captured by the inspectors over re-reading
            # /etc/hostname, which is empty on RHEL hosts using hostnamectl.
            meta_hostname = snapshot.meta.get("hostname") or None
            stamp = get_output_stamp(hostname=meta_hostname, host_root=host_root)
            if output_file is None:
                output_file = working_dir / f"{stamp}.tar.gz"
            create_tarball(tmp_dir, output_file, prefix=stamp)
            name = output_file.name
            is_fleet = "fleet" in snapshot.meta
            print(f"\nOutput: {name}\n")
            print("Next steps:")
            if not is_fleet:
                scp_host = meta_hostname or "TARGET_HOST"
                host_cwd = os.environ.get("YOINKC_HOST_CWD")
                scp_path = f"{host_cwd}/{name}" if host_cwd else name
                print(f"  Copy to workstation:    scp {scp_host}:{scp_path} .")
            print(f"  Interactive refinement: yoinkc refine {name}")
            print(f"  Build the image:        ./yoinkc-build {name} my-image:latest")
    except Exception:
        print(
            f"Error during output. Rendered files preserved at: {tmp_dir}",
            file=sys.stderr,
        )
        raise
    else:
        shutil.rmtree(tmp_dir)

    return snapshot
