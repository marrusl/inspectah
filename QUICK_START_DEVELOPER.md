# inspectah Developer Quick Start

## TL;DR - Key Facts

- **Language**: Python 3.11+ (NOT Go)
- **CLI Framework**: argparse (NOT Cobra)
- **Data Layer**: Pydantic v2 (schemas: `src/inspectah/schema.py`)
- **Plugin Pattern**: Inspectors collect → Renderers output (both in registry)
- **Orchestration**: Pipeline in `pipeline.py` runs inspectors sequentially, then renderers
- **Testing**: pytest with fixtures in `tests/fixtures/` mock `/etc` trees
- **Build**: setuptools (pyproject.toml), container at `ghcr.io/marrusl/inspectah:latest`

## Critical Files (Read in Order)

1. **Entry Point**: `src/inspectah/__main__.py`
   - Calls `parse_args()` → matches command → calls handler
   - Handlers: `_run_scan()`, `_run_fleet()`, `run_refine()`, `run_architect()`

2. **Commands**: `src/inspectah/cli.py`
   - argparse subcommand setup
   - Flag definitions (--target-version, --output-file, --from-snapshot, etc.)

3. **Schema**: `src/inspectah/schema.py`
   - `InspectionSnapshot` — single source of truth
   - Pydantic models: `RpmSection`, `ConfigSection`, `ServiceSection`, etc.
   - All data flows through this schema

4. **Pipeline**: `src/inspectah/pipeline.py`
   - Orchestrator: detects OS → resolves baseline → runs 11 inspectors → runs renderers
   - Each inspector wrapped in `_safe_run()` for error handling
   - Tracks warnings

5. **Inspectors**: `src/inspectah/inspectors/`
   - 11 modules: `rpm.py`, `config.py`, `service.py`, `network.py`, `storage.py`, etc.
   - Each has `run_XXXXX()` function returning Pydantic model or None

6. **Renderers**: `src/inspectah/renderers/`
   - 8 modules: `audit_report.py`, `html_report.py`, `containerfile/`, etc.
   - Each has `render()` function consuming snapshot, producing output

## Commands Structure

```bash
inspectah [inspect]              # Scan host, produce tarball/dir
inspectah refine *.tar.gz        # Interactive browser editor
inspectah fleet dir/             # Merge N host snapshots
inspectah architect ./fleets/    # Plan layer decomposition
```

## Adding a New Analyzer (Step-by-Step)

### 1. Schema (schema.py)
```python
class YourEntry(BaseModel):
    name: str
    severity: Optional[str] = None
    include: bool = True

class YourSection(BaseModel):
    items: List[YourEntry] = Field(default_factory=list)
    warnings: List[str] = Field(default_factory=list)

# Add to InspectionSnapshot:
class InspectionSnapshot(BaseModel):
    your_section: Optional[YourSection] = None  # <-- Add here
```

### 2. Inspector (inspectors/your_analyzer.py)
```python
def run_your_analyzer(
    host_root: Path,
    executor: CommandExecutor,
    warnings: list = None,
    **kwargs
) -> Optional[YourSection]:
    """Inspect something."""
    try:
        items = []
        result = executor.run(["some", "command"])
        for line in result.stdout.splitlines():
            items.append(YourEntry(name=line, ...))
        return YourSection(items=items)
    except (PermissionError, OSError) as e:
        if warnings:
            warnings.append(make_warning("your_analyzer", str(e)))
        return None
```

### 3. Register Inspector (pipeline.py)
```python
_TOTAL_STEPS = 12  # Increment this

_section_banner("Your Analyzer", 12, _TOTAL_STEPS)
snapshot.your_section = _safe_run(
    "your_analyzer",
    lambda: run_your_analyzer(host_root, executor, warnings=w, ...),
    None,
    w
)
```

### 4. Renderer (renderers/your_analyzer.py)
```python
def render(snapshot: InspectionSnapshot, env, output_dir: Path) -> None:
    if not snapshot.your_section:
        return
    template = env.get_template("your_analyzer.j2")
    output = template.render(section=snapshot.your_section)
    (output_dir / "your_analyzer_report.md").write_text(output)
```

### 5. Register Renderer (renderers/__init__.py)
```python
from .your_analyzer import render as render_your_analyzer

def run_all(...):
    # ... existing ...
    render_your_analyzer(snapshot, env, output_dir)
```

### 6. Template (templates/your_analyzer.j2)
```jinja2
# Your Analyzer Report

{% for item in section.items %}
- {{ item.name }}: {{ item.severity }}
{% endfor %}
```

### 7. Tests (tests/test_your_analyzer.py)
```python
from inspectah.inspectors.your_analyzer import run_your_analyzer
from inspectah.schema import YourSection

def test_basic(tmp_path, executor_mock):
    result = run_your_analyzer(tmp_path, executor_mock)
    assert isinstance(result, YourSection)

def test_error_handling(tmp_path, executor_mock):
    executor_mock.side_effect = PermissionError()
    result = run_your_analyzer(tmp_path, executor_mock, warnings=[])
    assert result is None
```

## Running Locally

```bash
# Setup
python -m venv .venv
source .venv/bin/activate
pip install -e ".[dev]"

# Test
pytest tests/
pytest -xvs tests/test_your_analyzer.py

# Run (requires root, container, chroot)
sudo python -m inspectah inspect --output-file output.tar.gz

# From snapshot (no root needed)
python -m inspectah inspect --from-snapshot output.tar.gz --output-dir refine/
```

## Key Patterns

### Error Handling
```python
try:
    result = do_something()
except (PermissionError, OSError) as e:
    warnings.append(make_warning("section", str(e)))
    return None  # Default for missing data

except Exception as e:
    print(f"Error: {e}", file=sys.stderr)
    if os.environ.get("INSPECTAH_DEBUG"):
        traceback.print_exc()
    return 1
```

### Running Commands (Always Use Executor)
```python
# WRONG: result = subprocess.run(...)
# RIGHT:
result = executor.run(["rpm", "-qa"])
if result.returncode != 0:
    raise RuntimeError(f"Failed: {result.stderr}")
```

### Subprocess Result Object
```python
result = executor.run([...])
# Properties:
result.stdout   # str
result.stderr   # str
result.returncode  # int
```

## Checklist for New Feature

- [ ] Schema defined in `schema.py`
- [ ] Inspector function in `inspectors/your_name.py`
- [ ] Inspector registered in `pipeline.py` (_TOTAL_STEPS incremented)
- [ ] Renderer function in `renderers/your_name.py`
- [ ] Renderer registered in `renderers/__init__.py`
- [ ] Template created in `templates/your_name.j2`
- [ ] Tests written in `tests/test_your_name.py`
- [ ] All tests passing: `pytest tests/`
- [ ] Manual test with `--inspect-only` flag
- [ ] Verify output in `report.html` and `audit-report.md`

## Project Layout

```
src/inspectah/
├── __main__.py           # CLI entry point
├── cli.py                # argparse setup
├── pipeline.py           # Orchestrator (run_pipeline function)
├── schema.py             # Pydantic models
├── inspectors/
│   ├── __init__.py       # run_XXXXX functions
│   ├── rpm.py
│   ├── config.py
│   ├── service.py
│   └── ... (9 more)
├── renderers/
│   ├── __init__.py       # run_all() main entry
│   ├── audit_report.py
│   ├── html_report.py
│   └── ... (6 more)
├── templates/            # Jinja2 templates
├── preflight.py          # Startup checks
├── baseline.py           # Resolve base image
├── heuristic.py          # Smart classification
├── redact.py             # Secrets masking
└── packaging.py          # Tarball handling

tests/
├── conftest.py           # pytest fixtures
├── fixtures/             # Mock /etc trees
├── test_*.py             # ~20 test modules
└── e2e/                  # Browser tests

docs/
├── reference/            # CLI flag documentation
└── explanation/          # Architecture docs
```

## Important Notes

1. **No Cobra** — inspectah uses argparse, not a Go CLI framework
2. **No Go Code** — 100% Python
3. **Pydantic v2** — use Field(), BaseModel, not v1 syntax
4. **Container-First** — All commands run inside podman with chroot to /host
5. **Read-Only** — Never modifies the inspected host
6. **Type Safety** — Everything flows through schema types
7. **Baseline Required** — Without baseline, all packages included (risky)
8. **Warnings Tracked** — Appear in snapshot.warnings and HTML report

## Common Tasks

### Add a new flag
1. Edit `cli.py` → add `parser.add_argument(...)`
2. Pass via `args.your_flag` to handler
3. Handler passes to `run_pipeline()` or other function

### Add a new output format
1. Create `renderers/your_format.py` with `render()` function
2. Register in `renderers/__init__.py` in `run_all()`
3. Add template in `templates/your_format.j2`

### Add a new preflight check
1. Edit `preflight.py` → add `check_your_requirement()` function
2. Call in `__main__.py` if `not args.skip_preflight`

## Documentation

- **Full design**: `design.md` (technical deep-dive)
- **CLI reference**: `docs/reference/cli.md` (all flags)
- **Architecture**: `docs/explanation/architecture.md` (how inspectors/renderers/baseline work)
- **Schema**: See docstrings in `schema.py`
- **Implementation plan**: `IMPLEMENTATION_PLAN.md` (this repo's full analysis)
