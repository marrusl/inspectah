# inspectah Developer Documentation

This directory contains comprehensive documentation for developers working on the inspectah codebase.

## Documents

### 1. **QUICK_START_DEVELOPER.md** — Start here!
- 15-minute overview of the codebase
- Key patterns and examples
- Step-by-step: how to add a new analyzer
- Common tasks and checklists
- **Best for**: Getting oriented, quick reference

### 2. **IMPLEMENTATION_PLAN.md** — The full blueprint
- 900+ lines of detailed analysis
- Every major component explained
- Data structures and schemas
- All 11 inspectors documented
- All 8 renderers documented
- Complete API reference
- **Best for**: Deep understanding, implementation guide

### 3. **ARCHITECTURE_DIAGRAM.md** — Visual reference
- ASCII flowcharts showing data flow
- Inspector anatomy (how they work)
- Renderer anatomy (how they output)
- Schema relationships
- Fleet/Refine/Architect modes
- **Best for**: Understanding relationships, visual learners

## Key Takeaways (60 seconds)

1. **Language**: Python 3.11+ (not Go)
2. **CLI**: argparse (not Cobra)
3. **Data**: Pydantic v2 schema (strongly typed)
4. **Pattern**: 11 Inspectors → 1 Schema → 8 Renderers
5. **Entry**: `src/inspectah/__main__.py::main()`
6. **Schema**: `src/inspectah/schema.py::InspectionSnapshot` (single source of truth)
7. **Inspectors**: `src/inspectah/inspectors/*.py` (each returns Pydantic model)
8. **Renderers**: `src/inspectah/renderers/*.py` (each consumes snapshot)
9. **Tests**: pytest with mock `/etc` trees in `tests/fixtures/`
10. **Build**: setuptools (pyproject.toml)

## File Organization

```
inspectah/
├── QUICK_START_DEVELOPER.md          ← Start here (15 min)
├── IMPLEMENTATION_PLAN.md             ← Full reference (900 lines)
├── ARCHITECTURE_DIAGRAM.md            ← Visual flows
├── README_DEVELOPER.md                ← You are here
├── src/inspectah/
│   ├── __main__.py                   ← CLI entry point
│   ├── cli.py                        ← Command registration
│   ├── schema.py                     ← Data model contract
│   ├── pipeline.py                   ← Orchestration
│   ├── inspectors/                   ← Data collection (11 modules)
│   ├── renderers/                    ← Output generation (8 modules)
│   ├── templates/                    ← Jinja2 templates
│   ├── preflight.py                  ← Startup checks
│   ├── baseline.py                   ← Base image resolution
│   └── ... (other support modules)
├── tests/
│   ├── conftest.py                   ← Fixtures
│   ├── test_*.py                     ← ~20 test modules
│   ├── fixtures/                     ← Mock /etc trees
│   └── e2e/                          ← Browser tests
└── pyproject.toml                     ← Build configuration
```

## Common Workflows

### Understanding a Component
1. Read QUICK_START_DEVELOPER.md for the overview
2. Check ARCHITECTURE_DIAGRAM.md for the flow
3. Reference IMPLEMENTATION_PLAN.md section for details

### Adding a New Analyzer
1. QUICK_START_DEVELOPER.md → "Adding a New Analyzer" section (step-by-step)
2. IMPLEMENTATION_PLAN.md → Section 13 (detailed context)
3. Copy existing inspector template (e.g., `inspectors/config.py`)

### Understanding Data Flow
1. ARCHITECTURE_DIAGRAM.md → "Data Flow Through Schema" section
2. IMPLEMENTATION_PLAN.md → Section 5 "Key Types & Interfaces"
3. Read `src/inspectah/schema.py` docstrings

### Adding a New Command
1. Edit `src/inspectah/cli.py` (add argparse subcommand)
2. Add handler in `src/inspectah/__main__.py`
3. Implement handler function

### Understanding Error Handling
1. QUICK_START_DEVELOPER.md → "Key Patterns" section
2. IMPLEMENTATION_PLAN.md → Section 8 "Error Handling Pattern"
3. Read `src/inspectah/preflight.py` for examples

## Quick Reference: File Purposes

| File | Purpose |
|------|---------|
| `__main__.py` | Parses args, matches command, calls handler |
| `cli.py` | argparse setup, flag definitions |
| `schema.py` | Pydantic models (data contract) |
| `pipeline.py` | Orchestrates: preflight → OS detect → baseline → inspectors → renderers |
| `inspectors/rpm.py` | Query packages, repos, GPG keys |
| `inspectors/config.py` | Find modified /etc files |
| `inspectors/service.py` | systemd services |
| `inspectors/network.py` | Network configuration |
| `inspectors/storage.py` | Block devices, filesystems |
| `inspectors/scheduled_tasks.py` | Cron, systemd timers |
| `inspectors/container.py` | Podman images, containers, quadlets |
| `inspectors/non_rpm_software.py` | Non-packaged software (/opt, venvs, etc.) |
| `inspectors/kernel_boot.py` | GRUB, kernel params |
| `inspectors/selinux.py` | SELinux policy, contexts |
| `inspectors/users_groups.py` | Users, groups, sudoers |
| `renderers/__init__.py` | run_all() — coordinates all renderers |
| `renderers/containerfile/` | Generates Dockerfile |
| `renderers/audit_report.py` | Markdown audit report |
| `renderers/html_report.py` | Interactive HTML dashboard |
| `renderers/readme.py` | Build instructions |
| `renderers/kickstart.py` | Anaconda installer config |
| `renderers/secrets_review.py` | Redacted sensitive data |
| `preflight.py` | Startup checks (podman, root, registry, privileges) |
| `baseline.py` | Resolves base bootc image package list |
| `heuristic.py` | Smart classification of non-RPM software |
| `redact.py` | Masks passwords, SSH keys, tokens |
| `packaging.py` | Tarball/directory output |
| `validation.py` | Runs `podman build` to verify output |
| `subscription.py` | Bundles RHEL subscription certs |

## Key Concepts

### Inspector Pattern
Each inspector:
1. Takes `host_root`, `executor`, `warnings`, optional context kwargs
2. Runs commands via `executor.run(["cmd", "args"])`
3. Parses output into strongly-typed Pydantic model
4. Returns `Optional[XxxxSection]` (None if data unavailable)
5. Appends errors to `warnings` list

### Renderer Pattern
Each renderer:
1. Takes `snapshot` (full inspection data), `env` (Jinja2), `output_dir`
2. Checks if relevant data exists (e.g., `if not snapshot.rpm`)
3. Loads Jinja2 template
4. Renders template with snapshot data
5. Writes output file(s) to `output_dir`

### Schema Pattern
- Single `InspectionSnapshot` class contains all data
- Every field is Pydantic-validated
- Serializes to/from JSON cleanly
- Supports partial fills (optional sections)
- Fleet support via `FleetPrevalence` on every item

### Error Handling Pattern
- Try/except catches `PermissionError`, `OSError`
- Appends to `warnings` list instead of crashing
- Returns default/None so pipeline continues
- Generic catch in main with debug support (`INSPECTAH_DEBUG=1`)

## Testing

### Run All Tests
```bash
pytest tests/
```

### Run Single Test Module
```bash
pytest tests/test_preflight.py -xvs
```

### Run with Coverage
```bash
pytest --cov=src/inspectah tests/
```

### Key Test Patterns
- Use fixtures from `tests/conftest.py`
- Mock executors with `executor_mock`
- Load sample snapshots from `tests/fixtures/`
- Assert on schema validation

## Development Setup

```bash
# Clone repo
git clone https://github.com/marrusl/inspectah.git
cd inspectah

# Create venv
python -m venv .venv
source .venv/bin/activate

# Install dev dependencies
pip install -e ".[dev]"

# Run tests
pytest tests/

# Run locally (requires root or container)
sudo python -m inspectah inspect --output-file test.tar.gz
```

## When to Use Which Document

| Task | Document |
|------|----------|
| First time understanding codebase | QUICK_START_DEVELOPER.md |
| Deep dive on a component | IMPLEMENTATION_PLAN.md |
| Understanding data flow | ARCHITECTURE_DIAGRAM.md |
| Adding new analyzer | QUICK_START_DEVELOPER.md (step 1-7) |
| Understanding error handling | QUICK_START_DEVELOPER.md or IMPLEMENTATION_PLAN.md §8 |
| Understanding schema | IMPLEMENTATION_PLAN.md §5 |
| Understanding pipeline | ARCHITECTURE_DIAGRAM.md or IMPLEMENTATION_PLAN.md §12 |
| Understanding inspectors | IMPLEMENTATION_PLAN.md §9-10 |
| Understanding renderers | IMPLEMENTATION_PLAN.md §6-7, 11 |

## Further Reading

Official documentation in the repo:
- `design.md` — Full technical design document
- `docs/reference/cli.md` — Complete CLI flag reference
- `docs/explanation/architecture.md` — How inspectors/renderers/baseline work

Source code:
- Start with `src/inspectah/__main__.py`
- Then read `src/inspectah/pipeline.py`
- Then inspect a specific inspector or renderer

## Support

Questions? Start here:

1. **Terminology clarification** → QUICK_START_DEVELOPER.md §1 (TL;DR)
2. **How does X work?** → ARCHITECTURE_DIAGRAM.md (visual flows)
3. **Where's X code?** → IMPLEMENTATION_PLAN.md §15 (file locations)
4. **How to add Y?** → QUICK_START_DEVELOPER.md (step-by-step)
5. **Deep technical question** → IMPLEMENTATION_PLAN.md (full section)

---

**Last Updated**: April 2026  
**inspectah Version**: 0.5.1  
**Python**: 3.11+
