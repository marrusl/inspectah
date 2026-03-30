# Documentation Backlog
**Maintained by:** Mango
**Last updated:** 2026-03-30

## Priority 1 (Do Soon)

- [ ] **README split** — README.md is 562 lines and growing. Split into multiple docs following Diataxis framework:
  - `docs/getting-started.md` — Install, first run, quickstart for one host
  - `docs/how-to/` — Task-oriented guides (fleet aggregation, refining findings, building images, architecting layers)
  - `docs/reference/cli.md` — Complete CLI flag reference extracted from argparse
  - `docs/explanation/architecture.md` — How yoinkc works (inspector/renderer pipeline, baseline subtraction, layer ordering)
  - Keep README as landing page with project overview + links to detailed docs

- [ ] **Cross-repo coherence** — Ensure yoinkc and driftify READMEs reference each other clearly:
  - yoinkc README should mention driftify as the testing/validation companion tool
  - driftify README already links to yoinkc but could add examples showing the full workflow (driftify → yoinkc inspect → yoinkc fleet → yoinkc architect)
  - Shared glossary of terms across both projects (drift profile, inspector, baseline, architect layer, fleet aggregation, prevalence)

- [ ] **Changelog ownership setup** — Establish CHANGELOG.md maintenance workflow:
  - Create initial CHANGELOG.md for both yoinkc and driftify
  - Follow conventional commits format (Added, Changed, Fixed, Removed sections per version)
  - Decide: automated from git log, manual curation, or hybrid approach?
  - Document changelog update process in CONTRIBUTING.md

## Priority 2 (Do When Time Allows)

- [ ] **API reference docs** — Document HTTP endpoints for `yoinkc refine` and `yoinkc architect`:
  - Refine server: `GET /`, `POST /api/re-render`, `GET /api/tarball`
  - Architect server: `GET /` (index), `GET /api/health`, `GET /api/topology`, `POST /api/move`, `POST /api/copy`, `GET /api/preview/{layer}`, `GET /api/export`
  - Include request/response schemas, error codes, example curl commands
  - Decision: separate `docs/reference/api.md` or embed in how-to guides?

- [ ] **Container wrapper documentation** — Document run-yoinkc.sh, run-fleet-test.sh, and run-architect-test.sh usage patterns:
  - When to use container wrapper vs native install
  - Environment variable reference (YOINKC_IMAGE, YOINKC_HOSTNAME, YOINKC_OUTPUT_DIR, etc.)
  - Port exposure details for refine/architect when running via container
  - How wrapper scripts map to direct `yoinkc` commands

- [ ] **Man page strategy decision** — Evaluate and decide on man page generation:
  - Option 1: argparse-manpage (auto-generate from CLI definitions)
  - Option 2: Hand-written groff/mdoc
  - Option 3: No man pages (rely on `--help` and web docs)
  - Consider: RPM/Homebrew packaging will eventually want man pages

- [ ] **Troubleshooting guide** — Common failure modes and solutions:
  - "Baseline not available" scenarios and `--baseline-packages` workaround
  - RHEL registry authentication setup (`podman login registry.redhat.io`)
  - Container preflight check failures (rootless vs rootful, missing --pid=host, etc.)
  - Version drift warnings and cross-major-version migration guidance
  - Build failures with subscription certs

## Priority 3 (Nice to Have)

- [ ] **MkDocs site setup evaluation** — Assess whether doc volume justifies a static site:
  - Pros: Better navigation, search, versioning, dark mode
  - Cons: Adds build/deploy complexity, requires GitHub Pages or similar hosting
  - Decision criteria: Wait until docs exceed ~10 pages or external users request it

- [ ] **Example-driven how-to library** — Expand how-to guides with real-world scenarios:
  - "How to migrate a fleet of web servers with custom RPMs and firewall rules"
  - "How to handle secrets during migration (redaction, review, secure injection)"
  - "How to test a bootc image locally before deploying to production"
  - "How to handle storage migration (NFS mounts, local data directories)"

- [ ] **Video/screencast documentation** — For visual learners:
  - Quickstart: inspect → refine → build in under 5 minutes
  - Fleet workflow: multi-host aggregation with prevalence tuning
  - Architect demo: layer decomposition for multi-role fleets

- [ ] **Glossary page** — Centralized terminology reference:
  - bootc, ostree, composefs, image mode vs package mode
  - Baseline subtraction, prevalence, drift profile, inspector, renderer
  - Fleet aggregation, layer decomposition, derived images
  - Keep it beginner-friendly with links to upstream docs for deep dives

- [ ] **Integration examples** — Show yoinkc in broader workflows:
  - CI/CD pipeline integration (GitLab CI, GitHub Actions, Jenkins)
  - Ansible playbook for running yoinkc across a fleet
  - Image registry setup and bootc deployment examples
  - Connecting to bootc-image-builder for disk image generation

## Deferred (Known Gaps, No Priority Yet)

- Formal design docs for features still in `docs/specs/proposed/` — wait until implemented
- Upstream bootc/Podman change impact tracking — Dash's domain, not pure docs
- SEO optimization and repo metadata (badges, description) — ask Mark before changing
- Deprecation/removal policy for old flags or output formats — establish when needed
