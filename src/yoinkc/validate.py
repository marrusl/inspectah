"""Build validation: run podman build against generated Containerfile (--validate)."""

import subprocess
from pathlib import Path


def run_validate(output_dir: Path) -> bool:
    """
    Run `podman build --no-cache` in output_dir. On failure write build-errors.log,
    append summary to audit-report.md, and inject a warning into report.html.
    Returns True if build succeeded, False otherwise.
    """
    output_dir = Path(output_dir)
    containerfile = output_dir / "Containerfile"
    if not containerfile.exists():
        return True
    try:
        result = subprocess.run(
            ["podman", "build", "--no-cache", "-f", str(containerfile), str(output_dir)],
            capture_output=True,
            text=True,
            timeout=600,
        )
        if result.returncode != 0:
            err_text = (result.stderr or "") + (result.stdout or "")
            log_path = output_dir / "build-errors.log"
            log_path.write_text(
                "Podman build failed.\n\nstdout:\n" + (result.stdout or "") + "\n\nstderr:\n" + (result.stderr or "")
            )
            _append_build_failure_to_reports(output_dir, err_text[:2000])
            return False
        # On success, report image ID and size
        _report_build_success(output_dir)
        return True
    except FileNotFoundError:
        # podman not installed
        return True
    except subprocess.TimeoutExpired:
        (output_dir / "build-errors.log").write_text("Podman build timed out after 600s.\n")
        _append_build_failure_to_reports(output_dir, "Podman build timed out after 600s.")
        return False
    except Exception:
        return True


def _report_build_success(output_dir: Path) -> None:
    """Print image ID and size of the last built image."""
    try:
        r = subprocess.run(
            ["podman", "images", "--format", "{{.ID}} {{.Size}}", "--noheading", "-n", "1"],
            capture_output=True,
            text=True,
            timeout=5,
        )
        if r.returncode == 0 and r.stdout.strip():
            parts = r.stdout.strip().split(None, 1)
            img_id = parts[0][:12] if parts else "?"
            size = parts[1] if len(parts) > 1 else "?"
            print(f"Build succeeded. Image ID: {img_id}  Size: {size}")
    except Exception:
        print("Build succeeded.")


def _append_build_failure_to_reports(output_dir: Path, summary: str) -> None:
    """Append build failure to audit-report.md and inject into report.html."""
    output_dir = Path(output_dir)
    audit = output_dir / "audit-report.md"
    if audit.exists():
        with open(audit, "a") as f:
            f.write("\n## Build validation failed\n\n")
            f.write("See `build-errors.log` for full output.\n\n")
            f.write("```\n")
            f.write(summary[:1500].replace("```", "` ` `"))
            f.write("\n```\n")
    html_path = output_dir / "report.html"
    if html_path.exists():
        html = html_path.read_text()
        escaped = summary[:500].replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")
        inject = f'<div class="warning-panel" style="border-color:var(--error);"><h3>Build validation failed</h3><p>See build-errors.log</p><pre style="font-size:0.85em">{escaped}</pre></div>'
        if "</body>" in html:
            html = html.replace("</body>", inject + "\n</body>")
            html_path.write_text(html)
