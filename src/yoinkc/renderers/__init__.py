"""
Renderers consume the full snapshot and a Jinja2 environment, writing to output_dir.
"""

from pathlib import Path

from jinja2 import Environment, FileSystemLoader

from ..schema import InspectionSnapshot

from .containerfile import render as render_containerfile
from .audit_report import render as render_audit_report
from .html_report import render as render_html_report
from .readme import render as render_readme
from .kickstart import render as render_kickstart
from .secrets_review import render as render_secrets_review


def run_all(snapshot: InspectionSnapshot, output_dir: Path) -> None:
    """Run all renderers. output_dir is created if it does not exist."""
    output_dir = Path(output_dir)
    output_dir.mkdir(parents=True, exist_ok=True)
    templates_dir = Path(__file__).resolve().parent.parent / "templates"
    env = Environment(
        loader=FileSystemLoader(str(templates_dir)),
        autoescape=True,
    )
    render_containerfile(snapshot, env, output_dir)
    render_audit_report(snapshot, env, output_dir)
    render_html_report(snapshot, env, output_dir)
    render_readme(snapshot, env, output_dir)
    render_kickstart(snapshot, env, output_dir)
    render_secrets_review(snapshot, env, output_dir)
