"""
Renderers consume the full snapshot and a Jinja2 environment, writing to output_dir.
"""

from pathlib import Path
from typing import Optional

from jinja2 import Environment, FileSystemLoader

from ..schema import InspectionSnapshot
from .._util import status as _status_fn

from .containerfile import render as render_containerfile
from .containerfile._config_tree import write_redacted_dir
from .audit_report import render as render_audit_report
from .html_report import render as render_html_report
from .readme import render as render_readme
from .kickstart import render as render_kickstart
from .secrets_review import render as render_secrets_review
from .merge_notes import render_merge_notes


def run_all(
    snapshot: InspectionSnapshot,
    output_dir: Path,
    refine_mode: bool = False,
    original_snapshot_path: Optional[Path] = None,
) -> None:
    """Run all renderers. output_dir is created if it does not exist."""
    output_dir = Path(output_dir)
    output_dir.mkdir(parents=True, exist_ok=True)
    templates_dir = Path(__file__).resolve().parent.parent / "templates"
    env = Environment(
        loader=FileSystemLoader(str(templates_dir)),
        autoescape=True,
    )
    # Read the original snapshot once for both html_report and audit_report
    original_snapshot = None
    if original_snapshot_path and original_snapshot_path.exists():
        try:
            original_snapshot = InspectionSnapshot.model_validate_json(
                original_snapshot_path.read_text()
            )
        except Exception:
            pass

    _status_fn("Rendering output…")
    render_containerfile(snapshot, env, output_dir)
    write_redacted_dir(snapshot, output_dir)
    render_merge_notes(snapshot, output_dir)
    render_audit_report(snapshot, env, output_dir, original_snapshot=original_snapshot)
    render_html_report(
        snapshot, env, output_dir,
        refine_mode=refine_mode,
        original_snapshot_path=original_snapshot_path,
    )
    render_readme(snapshot, env, output_dir)
    render_kickstart(snapshot, env, output_dir)
    render_secrets_review(snapshot, env, output_dir)
    _status_fn("Done.")
