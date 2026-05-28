# Service Intent Inference Implementation Plan

> **Revision 2 (2026-05-19):** Addresses all 11 must-fix items plus one
> consensus should-fix from the round 1 plan review. Changes:
>
> 1. **Task 1/2 boundary pinned** — Task 1 Step 5 owns the parse-time
>    gate, typed preset comparison, and `resolve_preset_default()` in
>    `services.rs`. Task 2 adds only `owning_package` resolution to the
>    same file. The boundary is now explicit per-function.
> 2. **Task 3 split** into Task 3a (plain baseline names — collector fix)
>    and Task 3b (renderer helper seam — new module). They are independent
>    and can be implemented in either order.
> 3. **`implied_action()` compile-through** — Task 1 Step 6 now shows the
>    exact `containerfile.rs` transformation from `sc.action.as_str()` to
>    `sc.implied_action()` with the full match block.
> 4. **Masked unit preset bypass** — Task 1 Step 5 now shows the explicit
>    code path where `Masked` skips divergence checking and always enters
>    `state_changes` with `default_state: None` when no preset rule matches.
> 5. **`rpm -qf` "not owned" guard** — Task 2 Step 3 now includes the
>    `!stdout.contains("not owned")` check matching the Go reference.
> 6. **Preset string-to-enum mapping** — Task 1 Step 5 now shows the
>    `resolve_preset_default()` function that maps preset rule `action`
>    strings ("enable"/"disable") to `PresetDefault` enum values.
> 7. **Serde roundtrip proof expanded** — Task 1 Step 1 now includes an
>    explicit `Option<PresetDefault>` roundtrip test covering `Some` and
>    `None` variants through JSON.
> 8. **Proven-present tier test added** — Task 4 Step 5 adds a test proving
>    baseline-present and included-installable packages emit with zero
>    omissions and zero advisories.
> 9. **Pure degraded-mode test added** — Task 4 Step 5 adds a test proving
>    `BaselineUnavailable` fires in isolation (not stacked with another reason).
> 10. **Advisory-survives-defer test added** — Task 4 Step 5 adds a negative
>     proof that advisory services are NOT suppressed by config-tree deferral.
> 11. **Stacked advisory test body shown** — Task 4 Step 1's stacked advisory
>     test now includes the full assertion body verifying multi-reason rendering.
> 12. **Review checkpoint after Task 1 added** (should-fix consensus) — a
>     lightweight review gate after the type migration, the riskiest single slice.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace string-based service filtering with typed intent inference so inspectah only persists durable operator service intent, renders omission/advisory decisions from a single renderer authority seam, and exposes those same decisions in refine UI.

**Architecture:** Land the work in seven slices that keep the truth boundary explicit: (1) migrate the service snapshot contract to typed state/preset enums and mechanical call-site updates, (2) resolve `owning_package` during collection, (3a) fix the RPM collector to emit plain baseline names, (3b) extract the shared renderer helper seam for package-truth and omission/advisory decisions, (4) make the renderer the single authority for service omissions and advisories, (5) teach the Rust web layer to consume renderer output without recomputing it, and (6) add the minimal UI/API shape needed to render real service subsections. Tasks 3a and 3b are independent and can be implemented in either order. The renderer remains the sole owner of omission/advisory decisions; web/UI are presentation only.

**Tech Stack:** Rust workspace crates (`inspectah-core`, `inspectah-collect`, `inspectah-pipeline`, `inspectah-web`, `inspectah-refine`), serde, axum, existing mock executor/test helpers, TypeScript/React, PatternFly 6, Vitest, npm.

**Spec:** `docs/specs/proposed/2026-05-19-service-intent-inference-design.md`

**Anchor Commit:** `f130d84`

### Contract decisions

- **Schema migration assumption:** bump `SCHEMA_VERSION` to `16`, keep `MIN_SCHEMA` at `12`, and do **not** add compatibility deserialization for legacy service JSON. Old tarballs that still carry the pre-typed `services` shape are intentionally unsupported and must be re-scanned.
- **Package-name namespace assumption:** keep the field name `baseline_package_names`, but repopulate it from `BaselinePackageEntry.name` (plain RPM `Name:` values), not the canonical `name.arch` keys used inside `BaselineData.packages`.
- **Renderer authority assumption:** add a public renderer helper module at `inspectah-pipeline/src/render/service_intent.rs` so both `containerfile.rs` and `inspectah-web` consume the same `ServiceOmission` / `ServiceAdvisory` output rather than duplicating logic.
- **Review checkpoints:** (a) stop after Task 1 for a lightweight review of the type migration — this is the riskiest single slice and touches every crate; (b) stop after Task 4 and get a full code review before touching `inspectah-web` / `inspectah-web/ui`. That keeps the contract migration and renderer truth boundary reviewed before presentation work starts.

### Working directory

Run every command below from:

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
```

---

## File Map

### Core contract and collector truth gate

| File | Responsibility |
|------|----------------|
| `inspectah-core/src/types/services.rs` | Typed service snapshot schema: `ServiceUnitState`, `PresetDefault`, `ServiceAction`, `ServiceStateChange::implied_action()`, no service serde backfills |
| `inspectah-core/src/snapshot.rs` | Schema version bump to `16` and migration comment documenting re-scan policy |
| `inspectah-core/tests/parity_gate.rs` | Replace legacy Go-v13 services roundtrip expectation with explicit re-scan rejection proof and typed contract tests |
| `inspectah-collect/src/inspectors/services.rs` | Parse-time service state gate, warning contract, typed preset comparison, `owning_package` resolution |
| `inspectah-collect/tests/services_test.rs` | Intent gate matrix, warning payload proofs, no-op snapshot proof, owning-package fallback proofs |
| `inspectah-collect/tests/parity_test.rs` | Structural collector assertions updated from strings to typed service states |

### Mechanical compile-through after typed service contract

| File | Responsibility |
|------|----------------|
| `inspectah-pipeline/src/render/containerfile.rs` | Replace `sc.action` reads with `sc.implied_action()` until full renderer authority lands |
| `inspectah-pipeline/src/render/configtree.rs` | Keep service/drop-in test fixtures compiling against typed service structs |
| `inspectah-pipeline/tests/smoke_render.rs` | Update service fixtures to typed `ServiceUnitState` / `PresetDefault` |
| `inspectah-pipeline/tests/failure_policy.rs` | Update degraded service fixtures to typed contract |
| `inspectah-pipeline/tests/redaction_new_surfaces_test.rs` | Keep service-section fixture literals compiling |
| `inspectah-web/src/handlers.rs` | Mechanical typed-state subtitle/action updates in existing service normalization before subsection work |
| `inspectah-web/tests/api_test.rs` | Update rich snapshot fixtures and schema version assertions |
| `inspectah-refine/src/normalize.rs` | Keep service-related helper fixtures/comments accurate after schema bump |
| `inspectah-refine/tests/phase6_integration_test.rs` | Update typed service literals used in refine integration coverage |
| `inspectah-pipeline/src/validate.rs` | Validation tests/messages updated for schema `16` |

### Task 3a: Plain baseline names (collector fix)

| File | Responsibility |
|------|----------------|
| `inspectah-collect/src/inspectors/rpm/mod.rs` | Build plain-name `baseline_package_names` instead of canonical `name.arch` values |

### Task 3b: Renderer helper seam (new module)

| File | Responsibility |
|------|----------------|
| `inspectah-pipeline/src/render/service_intent.rs` | Shared service render decisions: `effective_target_packages()`, `is_package_installable()`, `ServiceOmission`, `ServiceAdvisory`, `AdvisoryReason`, `render_service_intent()` |
| `inspectah-pipeline/src/render/mod.rs` | Export the new `service_intent` module |
| `inspectah-pipeline/src/render/containerfile.rs` | Delegate service omission/advisory classification to `service_intent.rs` and render comments from that plan |
| `inspectah-pipeline/tests/service_intent_test.rs` | Focused helper and render-decision proofs, including stacked advisories and suppress-before-defer |

### Web/backend and UI consumption

| File | Responsibility |
|------|----------------|
| `inspectah-web/src/handlers.rs` | Typed subtitles plus `ContextSubsection` DTOs; map renderer omissions/advisories and service warnings into supplemental subsections |
| `inspectah-web/tests/api_test.rs` | `/api/snapshot/sections` proofs for service subsections and typed subtitle output |
| `inspectah-web/ui/src/api/types.ts` | TypeScript mirrors of `ContextSubsection` / updated `ContextSection` |
| `inspectah-web/ui/src/components/ContextList.tsx` | Render main context items plus subsection headings/lists without treating subsection-only sections as empty |
| `inspectah-web/ui/src/components/__tests__/ContextList.test.tsx` | UI proof that subsection headings/items render after main services and do not replace actionable items |

---

### Task 1: Migrate the service contract to typed intent states

**Files:**
- Modify: `inspectah-core/src/types/services.rs`
- Modify: `inspectah-core/src/snapshot.rs`
- Modify: `inspectah-core/tests/parity_gate.rs`
- Modify: `inspectah-collect/src/inspectors/services.rs`
- Modify: `inspectah-collect/tests/services_test.rs`
- Modify: `inspectah-collect/tests/parity_test.rs`
- Modify: `inspectah-pipeline/src/render/containerfile.rs`
- Modify: `inspectah-pipeline/src/render/configtree.rs`
- Modify: `inspectah-pipeline/tests/smoke_render.rs`
- Modify: `inspectah-pipeline/tests/failure_policy.rs`
- Modify: `inspectah-pipeline/tests/redaction_new_surfaces_test.rs`
- Modify: `inspectah-pipeline/src/validate.rs`
- Modify: `inspectah-web/src/handlers.rs`
- Modify: `inspectah-web/tests/api_test.rs`
- Modify: `inspectah-refine/src/normalize.rs`
- Modify: `inspectah-refine/tests/phase6_integration_test.rs`

- [ ] **Step 1: Write the failing core contract tests**

Add these tests to the existing `#[cfg(test)]` module in `inspectah-core/src/types/services.rs`:

```rust
#[test]
fn test_service_state_change_roundtrip_uses_typed_enums() {
    let section = ServiceSection {
        state_changes: vec![
            ServiceStateChange {
                unit: "firewalld.service".into(),
                current_state: ServiceUnitState::Enabled,
                default_state: Some(PresetDefault::Disable),
                include: true,
                owning_package: Some("firewalld".into()),
                fleet: None,
                attention_reason: None,
            },
            ServiceStateChange {
                unit: "cups.service".into(),
                current_state: ServiceUnitState::Masked,
                default_state: None,
                include: true,
                owning_package: Some("cups".into()),
                fleet: None,
                attention_reason: None,
            },
        ],
        enabled_units: vec!["firewalld.service".into()],
        disabled_units: vec![],
        drop_ins: vec![],
        preset_matched_units: vec![],
    };

    let json = serde_json::to_string(&section).unwrap();
    let parsed: ServiceSection = serde_json::from_str(&json).unwrap();

    assert_eq!(parsed, section);
}

#[test]
fn test_implied_action_derives_from_current_state() {
    let enabled = ServiceStateChange {
        unit: "firewalld.service".into(),
        current_state: ServiceUnitState::Enabled,
        default_state: Some(PresetDefault::Disable),
        include: true,
        owning_package: Some("firewalld".into()),
        fleet: None,
        attention_reason: None,
    };
    let disabled = ServiceStateChange {
        unit: "sshd.service".into(),
        current_state: ServiceUnitState::Disabled,
        default_state: Some(PresetDefault::Enable),
        include: true,
        owning_package: Some("openssh-server".into()),
        fleet: None,
        attention_reason: None,
    };
    let masked = ServiceStateChange {
        unit: "cups.service".into(),
        current_state: ServiceUnitState::Masked,
        default_state: None,
        include: true,
        owning_package: Some("cups".into()),
        fleet: None,
        attention_reason: None,
    };

    assert_eq!(enabled.implied_action(), ServiceAction::Enable);
    assert_eq!(disabled.implied_action(), ServiceAction::Disable);
    assert_eq!(masked.implied_action(), ServiceAction::Mask);
}

#[test]
fn test_missing_default_state_does_not_deserialize() {
    let json = r#"{
        "unit":"firewalld.service",
        "current_state":"enabled",
        "include":true,
        "owning_package":"firewalld",
        "fleet":null
    }"#;

    let err = serde_json::from_str::<ServiceStateChange>(json).unwrap_err();
    assert!(
        err.to_string().contains("default_state"),
        "expected missing-field error, got: {err}"
    );
}

#[test]
fn test_option_preset_default_serde_roundtrip() {
    // Some(Enable) roundtrips
    let with_preset = ServiceStateChange {
        unit: "firewalld.service".into(),
        current_state: ServiceUnitState::Enabled,
        default_state: Some(PresetDefault::Disable),
        include: true,
        owning_package: Some("firewalld".into()),
        fleet: None,
        attention_reason: None,
    };
    let json = serde_json::to_string(&with_preset).unwrap();
    let parsed: ServiceStateChange = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.default_state, Some(PresetDefault::Disable));

    // None roundtrips (masked unit with no preset rule)
    let without_preset = ServiceStateChange {
        unit: "cups.service".into(),
        current_state: ServiceUnitState::Masked,
        default_state: None,
        include: true,
        owning_package: Some("cups".into()),
        fleet: None,
        attention_reason: None,
    };
    let json = serde_json::to_string(&without_preset).unwrap();
    let parsed: ServiceStateChange = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.default_state, None);

    // Explicit null in JSON deserializes to None
    let null_json = r#"{
        "unit":"cups.service",
        "current_state":"masked",
        "default_state":null,
        "include":true,
        "owning_package":"cups",
        "fleet":null
    }"#;
    let parsed: ServiceStateChange = serde_json::from_str(null_json).unwrap();
    assert_eq!(parsed.default_state, None);
}
```

- [ ] **Step 2: Write the failing collector truth-gate tests**

Add these focused tests to `inspectah-collect/tests/services_test.rs`:

```rust
#[test]
fn test_intent_gate_warns_runtime_linked_bad_and_unknown_states() {
    let exec = MockExecutor::new()
        .with_command(
            "systemctl list-unit-files --type=service --no-pager",
            ExecResult {
                stdout: "UNIT FILE                                  STATE           PRESET\n\
                         transient.service                          transient       disabled\n\
                         linked.service                             linked          enabled\n\
                         broken.service                             bad             disabled\n\
                         future.service                             future-state    disabled\n\
                         \n\
                         4 unit files listed.\n"
                    .into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_dir("/usr/lib/systemd/system-preset", vec!["90-default.preset"])
        .with_file("/usr/lib/systemd/system-preset/90-default.preset", "disable *\n")
        .with_dir("/etc/systemd/system", vec![]);

    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
        baseline_data: None,
    };

    let output = ServicesInspector::new().inspect(&ctx).unwrap();
    let section = match &output.section {
        SectionData::Services(s) => s,
        other => panic!("expected Services section, got {other:?}"),
    };

    assert!(section.state_changes.is_empty(), "warning-only states must not persist");
    assert_eq!(output.warnings.len(), 4, "each warning state should emit one warning");
    assert!(output.warnings.iter().any(|w| {
        w.inspector == "services"
            && w.extra.get("unit") == Some(&serde_json::json!("linked.service"))
            && w.extra.get("raw_state") == Some(&serde_json::json!("linked"))
    }));
}

#[test]
fn test_clean_default_snapshot_produces_zero_state_changes() {
    let exec = MockExecutor::new()
        .with_command(
            "systemctl list-unit-files --type=service --no-pager",
            ExecResult {
                stdout: "UNIT FILE                                  STATE           PRESET\n\
                         dbus.service                               alias           disabled\n\
                         sssd-kcm.service                           indirect        disabled\n\
                         systemd-sysupdate.service                  indirect        disabled\n\
                         chronyd.service                            enabled         enabled\n\
                         sshd.service                               enabled         enabled\n\
                         cups.service                               disabled        disabled\n\
                         \n\
                         6 unit files listed.\n"
                    .into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_dir("/usr/lib/systemd/system-preset", vec!["90-default.preset"])
        .with_file(
            "/usr/lib/systemd/system-preset/90-default.preset",
            "enable chronyd.service\nenable sshd.service\ndisable *\n",
        )
        .with_dir("/etc/systemd/system", vec![]);

    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
        baseline_data: None,
    };

    let output = ServicesInspector::new().inspect(&ctx).unwrap();
    let section = match &output.section {
        SectionData::Services(s) => s,
        other => panic!("expected Services section, got {other:?}"),
    };

    assert!(section.state_changes.is_empty(), "clean default host should be a no-op");
    assert!(output.warnings.is_empty(), "known inert states should drop silently");
}
```

- [ ] **Step 3: Run the new tests to verify they fail**

Run:

```bash
cargo test -p inspectah-core test_service_state_change_roundtrip_uses_typed_enums -- --exact
cargo test -p inspectah-core test_option_preset_default_serde_roundtrip -- --exact
cargo test -p inspectah-collect test_intent_gate_warns_runtime_linked_bad_and_unknown_states -- --exact
```

Expected:
- the core tests fail because `ServiceUnitState`, `PresetDefault`, and `implied_action()` do not exist yet
- the collector test fails because runtime/linked/bad/unknown states are still stringly typed and do not emit structured service warnings

- [ ] **Step 4: Implement the typed core contract in `inspectah-core/src/types/services.rs`**

Replace the stringly typed fields with typed enums and a derived action method:

```rust
use super::fleet::FleetPrevalence;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServiceUnitState {
    Enabled,
    Disabled,
    Masked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PresetDefault {
    Enable,
    Disable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceAction {
    Enable,
    Disable,
    Mask,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServiceStateChange {
    pub unit: String,
    pub current_state: ServiceUnitState,
    pub default_state: Option<PresetDefault>,
    pub include: bool,
    pub owning_package: Option<String>,
    pub fleet: Option<FleetPrevalence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attention_reason: Option<String>,
}

impl ServiceStateChange {
    pub fn implied_action(&self) -> ServiceAction {
        match self.current_state {
            ServiceUnitState::Enabled => ServiceAction::Enable,
            ServiceUnitState::Disabled => ServiceAction::Disable,
            ServiceUnitState::Masked => ServiceAction::Mask,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ServiceSection {
    pub state_changes: Vec<ServiceStateChange>,
    pub enabled_units: Vec<String>,
    pub disabled_units: Vec<String>,
    pub drop_ins: Vec<SystemdDropIn>,
    pub preset_matched_units: Vec<String>,
}
```

Important implementation notes for this step:
- do **not** add `#[serde(default)]` to `ServiceStateChange` or `ServiceSection` fields
- keep `Default` only where it helps test ergonomics; the no-backfill rule is about serde, not Rust trait derivation
- leave `owning_package`, `fleet`, and `attention_reason` in place so later tasks stay surgical

- [ ] **Step 5: Rewrite the collector around typed durable states and service warnings**

In `inspectah-collect/src/inspectors/services.rs`, add a typed parse gate plus structured warning helper:

```rust
use inspectah_core::types::services::{
    PresetDefault, ServiceSection, ServiceStateChange, ServiceUnitState, SystemdDropIn,
};
use inspectah_core::types::warnings::{Warning, WarningSeverity};

enum ParsedUnitState {
    Durable(ServiceUnitState),
    SilentDrop,
    Warn(Warning),
}

fn parse_unit_state(unit: &str, raw_state: &str) -> ParsedUnitState {
    match raw_state {
        "enabled" => ParsedUnitState::Durable(ServiceUnitState::Enabled),
        "disabled" => ParsedUnitState::Durable(ServiceUnitState::Disabled),
        "masked" => ParsedUnitState::Durable(ServiceUnitState::Masked),
        "static" | "alias" | "indirect" | "generated" => ParsedUnitState::SilentDrop,
        "enabled-runtime" | "masked-runtime" | "transient" | "linked-runtime" => {
            ParsedUnitState::Warn(service_warning(unit, raw_state, "transient state is not migrated"))
        }
        "linked" => ParsedUnitState::Warn(service_warning(unit, raw_state, "linked unit requires manual handling")),
        "bad" => ParsedUnitState::Warn(service_warning(unit, raw_state, "unit file is invalid or unreadable")),
        other => ParsedUnitState::Warn(service_warning(unit, other, "unrecognized systemd state")),
    }
}

fn service_warning(unit: &str, raw_state: &str, detail: &str) -> Warning {
    Warning {
        inspector: "services".into(),
        message: format!("unit {unit} has state '{raw_state}' - {detail}"),
        severity: Some(WarningSeverity::Warning),
        extra: std::collections::HashMap::from([
            ("unit".into(), serde_json::json!(unit)),
            ("raw_state".into(), serde_json::json!(raw_state)),
        ]),
    }
}

/// Maps preset rule action strings to typed enum values.
///
/// The preset file format uses "enable" and "disable" as action keywords.
/// Any unrecognized action string maps to `Disable` (conservative default).
fn resolve_preset_default(unit: &str, rules: &[PresetRule]) -> Option<PresetDefault> {
    for rule in rules {
        if glob_match(&rule.pattern, unit) {
            return Some(match rule.action.as_str() {
                "enable" => PresetDefault::Enable,
                "disable" => PresetDefault::Disable,
                _ => PresetDefault::Disable, // unknown action → conservative
            });
        }
    }
    None
}
```

Thread the helper into the main collector loop with an explicit match:

```rust
let current_state = match parse_unit_state(&unit.unit, &unit.state) {
    ParsedUnitState::Durable(state) => state,
    ParsedUnitState::SilentDrop => continue,
    ParsedUnitState::Warn(warning) => {
        warnings.push(warning);
        continue;
    }
};
```

Then update the service loop so it:
- skips inert states silently
- emits warnings for runtime/linked/bad/unknown states
- only persists `Enabled`, `Disabled`, `Masked`
- never adds masked units to `disabled_units`
- sets `default_state: None` for masked units with no preset rule
- keeps `preset_matched_units` only for typed enabled/disabled states that match their preset

The masked unit bypass path must be explicit — masked units skip divergence checking entirely:

```rust
// After parse_unit_state produces Durable(Masked):
let (default_state, diverges) = if current_state == ServiceUnitState::Masked {
    // Masked units always enter state_changes regardless of preset.
    // default_state carries the preset if one exists (for UI context),
    // but divergence is not evaluated — masking is always operator intent.
    let preset = resolve_preset_default(&unit.unit, &preset_rules);
    (preset, true) // always "diverges" — masking is unconditional intent
} else {
    let preset = resolve_preset_default(&unit.unit, &preset_rules);
    match preset {
        Some(PresetDefault::Enable) if current_state == ServiceUnitState::Disabled => {
            (Some(PresetDefault::Enable), true)
        }
        Some(PresetDefault::Disable) if current_state == ServiceUnitState::Enabled => {
            (Some(PresetDefault::Disable), true)
        }
        Some(p) => (Some(p), false), // matches preset — no divergence
        None => (None, false),       // no preset rule — no divergence
    }
};

if !diverges {
    continue; // state matches preset default, not operator intent
}

state_changes.push(ServiceStateChange {
    unit: unit.unit.clone(),
    current_state,
    default_state,
    include: true,
    owning_package: None, // populated in Task 2
    fleet: None,
    attention_reason: None,
});
```

**Task 1/2 boundary:** Task 1 owns all changes to `services.rs` except
`populate_owning_packages()` and `query_owning_package()`. Specifically,
Task 1 adds: `ParsedUnitState`, `parse_unit_state()`,
`service_warning()`, `resolve_preset_default()`, the masked bypass path,
and the divergence-gated `state_changes.push()`. Task 2 adds only the
two ownership functions and the `populate_owning_packages()` call site.

- [ ] **Step 6: Mechanically update runtime/test call sites to use typed states**

Use this exact pattern anywhere the codebase still constructs `ServiceStateChange` with strings:

```rust
use inspectah_core::types::services::{
    PresetDefault, ServiceAction, ServiceStateChange, ServiceUnitState,
};

fn sc(
    unit: &str,
    current_state: ServiceUnitState,
    default_state: Option<PresetDefault>,
) -> ServiceStateChange {
    ServiceStateChange {
        unit: unit.into(),
        current_state,
        default_state,
        include: true,
        owning_package: None,
        fleet: None,
        attention_reason: None,
    }
}

let action = match sc.implied_action() {
    ServiceAction::Enable => "enable",
    ServiceAction::Disable => "disable",
    ServiceAction::Mask => "mask",
};
```

**`implied_action()` compile-through in `containerfile.rs`** — this is the
highest compile-break risk. The existing code at line ~598 reads:

```rust
// BEFORE (string-based):
match sc.action.as_str() {
    "enable" => {
        if config_tree_units.contains(u.as_str()) {
            deferred.push(u.clone());
        } else {
            safe_enabled.push(u.clone());
        }
    }
    "disable" => {
        safe_disabled.push(u.clone());
    }
    "mask" => {
        safe_masked.push(u.clone());
    }
    _ => {}
}
```

Transform to:

```rust
// AFTER (typed):
match sc.implied_action() {
    ServiceAction::Enable => {
        if config_tree_units.contains(u.as_str()) {
            deferred.push(u.clone());
        } else {
            safe_enabled.push(u.clone());
        }
    }
    ServiceAction::Disable => {
        safe_disabled.push(u.clone());
    }
    ServiceAction::Mask => {
        safe_masked.push(u.clone());
    }
}
```

Note: the `_ => {}` wildcard arm is removed because the enum is exhaustive.
The `action` field is also removed from `ServiceStateChange`, so any test
fixtures constructing `action: "enable".into()` must be replaced with the
typed `current_state` + `default_state` fields — `implied_action()` derives
the action from `current_state`, not from a stored string.

Apply that update in these exact files before leaving this task:
- `inspectah-pipeline/src/render/containerfile.rs`
- `inspectah-pipeline/src/render/configtree.rs`
- `inspectah-pipeline/tests/smoke_render.rs`
- `inspectah-pipeline/tests/failure_policy.rs`
- `inspectah-pipeline/tests/redaction_new_surfaces_test.rs`
- `inspectah-web/src/handlers.rs`
- `inspectah-web/tests/api_test.rs`
- `inspectah-refine/src/normalize.rs`
- `inspectah-refine/tests/phase6_integration_test.rs`
- `inspectah-collect/tests/parity_test.rs`

- [ ] **Step 7: Bump the schema and replace the legacy services parity gate**

In `inspectah-core/src/snapshot.rs`, bump the schema version and document the re-scan policy:

```rust
pub const SCHEMA_VERSION: u32 = 16;

// v15 -> v16: services switched from stringly typed state/action fields to
// typed intent inference. Legacy services payloads are intentionally not
// migrated; re-scan old tarballs instead of backfilling service data.
pub fn migrate(snap: &mut InspectionSnapshot) {
    if snap.schema_version >= SCHEMA_VERSION {
        return;
    }
    if snap.schema_version <= 14 && snap.baseline.is_none() && !snap.no_baseline {
        snap.no_baseline = true;
    }
    snap.schema_version = SCHEMA_VERSION;
}
```

Replace the old services golden roundtrip in `inspectah-core/tests/parity_gate.rs` with an explicit re-scan proof:

```rust
#[test]
fn test_legacy_go_v13_services_section_requires_rescan() {
    let golden = include_str!("../../testdata/golden/go-v13-services-section.json");
    let err = serde_json::from_str::<ServiceSection>(golden).unwrap_err();
    assert!(
        err.to_string().contains("current_state")
            || err.to_string().contains("unknown variant"),
        "legacy services payload should fail typed deserialization, got: {err}"
    );
}
```

Also update these version-sensitive tests while you are already in this task:
- `inspectah-pipeline/src/validate.rs`
- `inspectah-web/tests/api_test.rs` (`health_extended_fields`, `health_minimal_snapshot`)

- [ ] **Step 8: Run focused tests until they pass**

Run:

```bash
cargo test -p inspectah-core test_service_state_change_roundtrip_uses_typed_enums -- --exact
cargo test -p inspectah-core test_option_preset_default_serde_roundtrip -- --exact
cargo test -p inspectah-core test_implied_action_derives_from_current_state -- --exact
cargo test -p inspectah-core test_legacy_go_v13_services_section_requires_rescan -- --exact
cargo test -p inspectah-collect test_intent_gate_warns_runtime_linked_bad_and_unknown_states -- --exact
cargo test -p inspectah-collect test_clean_default_snapshot_produces_zero_state_changes -- --exact
```

Expected: all focused tests PASS.

- [ ] **Step 9: Refresh the service snapshot proof and run broader verification**

Run:

```bash
INSTA_UPDATE=always cargo test -p inspectah-collect services_snapshot -- --exact
cargo test -p inspectah-core
cargo test -p inspectah-collect
cargo test -p inspectah-pipeline test_containerfile_services -- --exact
cargo test -p inspectah-web test_normalize_services_three_way_split -- --exact
cargo test -p inspectah-web health_extended_fields -- --exact
```

Expected:
- the new collector snapshot is accepted
- `inspectah-core` and `inspectah-collect` are green
- the representative pipeline/web typed-service fixtures compile and pass

- [ ] **Step 10: Commit the typed contract slice**

```bash
git add inspectah-core/src/types/services.rs inspectah-core/src/snapshot.rs inspectah-core/tests/parity_gate.rs inspectah-collect/src/inspectors/services.rs inspectah-collect/tests/services_test.rs inspectah-collect/tests/parity_test.rs inspectah-pipeline/src/render/containerfile.rs inspectah-pipeline/src/render/configtree.rs inspectah-pipeline/tests/smoke_render.rs inspectah-pipeline/tests/failure_policy.rs inspectah-pipeline/tests/redaction_new_surfaces_test.rs inspectah-pipeline/src/validate.rs inspectah-web/src/handlers.rs inspectah-web/tests/api_test.rs inspectah-refine/src/normalize.rs inspectah-refine/tests/phase6_integration_test.rs
git commit -m "$(cat <<'EOF'
feat(services): adopt typed service intent contract

Replace stringly typed service state and action fields with typed durable-state
enums so non-actionable service states never persist in snapshots. Legacy
service payloads are intentionally unsupported and must be re-scanned.

Assisted-by: Claude Code (Opus 4.6)
EOF
)"
```

- [ ] **Step 11: REVIEW CHECKPOINT — stop here for lightweight review**

The type migration is the riskiest single slice — it touches every crate
in the workspace. Before proceeding to Task 2:

1. Run `git show --stat HEAD` to confirm the commit scope
2. Ask for a lightweight review focused on:
   - Are all `sc.action.as_str()` call sites updated to `sc.implied_action()`?
   - Does the masked bypass path always enter `state_changes`?
   - Does `resolve_preset_default()` correctly map action strings to enum values?
   - Are the serde attributes correct (`rename_all`, no `#[serde(default)]` on typed fields)?
3. Do **not** start Task 2 until the type migration is accepted

This is a quick review, not a full panel — the goal is catching compile-break
regressions before building ownership resolution on top of the new types.

---

### Task 2: Resolve `owning_package` during service collection

**Files:**
- Modify: `inspectah-collect/src/inspectors/services.rs`
- Modify: `inspectah-collect/tests/services_test.rs`

- [ ] **Step 1: Write the failing owning-package tests**

Add these tests to `inspectah-collect/tests/services_test.rs`:

```rust
#[test]
fn test_owning_package_batch_query_populates_state_changes() {
    let exec = MockExecutor::new()
        .with_command(
            "systemctl list-unit-files --type=service --no-pager",
            ExecResult {
                stdout: "UNIT FILE                                  STATE           PRESET\n\
                         httpd.service                              enabled         disabled\n\
                         firewalld.service                          disabled        enabled\n\
                         \n\
                         2 unit files listed.\n"
                    .into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_dir("/usr/lib/systemd/system-preset", vec!["90-default.preset"])
        .with_file(
            "/usr/lib/systemd/system-preset/90-default.preset",
            "disable httpd.service\nenable firewalld.service\n",
        )
        .with_command(
            "rpm -qf --queryformat %{NAME}\n /usr/lib/systemd/system/httpd.service /usr/lib/systemd/system/firewalld.service",
            ExecResult {
                stdout: "httpd\nfirewalld\n".into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_dir("/etc/systemd/system", vec![]);

    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
        baseline_data: None,
    };

    let output = ServicesInspector::new().inspect(&ctx).unwrap();
    let section = match &output.section {
        SectionData::Services(s) => s,
        other => panic!("expected Services section, got {other:?}"),
    };

    assert_eq!(section.state_changes[0].owning_package.as_deref(), Some("httpd"));
    assert_eq!(section.state_changes[1].owning_package.as_deref(), Some("firewalld"));
}

#[test]
fn test_owning_package_fallback_checks_etc_systemd_path() {
    let exec = MockExecutor::new()
        .with_command(
            "systemctl list-unit-files --type=service --no-pager",
            ExecResult {
                stdout: "UNIT FILE                                  STATE           PRESET\n\
                         custom-local.service                       enabled         disabled\n\
                         \n\
                         1 unit files listed.\n"
                    .into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_dir("/usr/lib/systemd/system-preset", vec!["90-default.preset"])
        .with_file("/usr/lib/systemd/system-preset/90-default.preset", "disable *\n")
        .with_command(
            "rpm -qf --queryformat %{NAME}\n /usr/lib/systemd/system/custom-local.service",
            ExecResult {
                stdout: "file /usr/lib/systemd/system/custom-local.service is not owned by any package\n".into(),
                exit_code: 1,
                ..Default::default()
            },
        )
        .with_command(
            "rpm -qf --queryformat %{NAME}\n /etc/systemd/system/custom-local.service",
            ExecResult {
                stdout: "custom-local\n".into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_dir("/etc/systemd/system", vec![]);

    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
        baseline_data: None,
    };

    let output = ServicesInspector::new().inspect(&ctx).unwrap();
    let section = match &output.section {
        SectionData::Services(s) => s,
        other => panic!("expected Services section, got {other:?}"),
    };

    assert_eq!(
        section.state_changes[0].owning_package.as_deref(),
        Some("custom-local")
    );
}
```

- [ ] **Step 2: Run the new tests to verify they fail**

Run:

```bash
cargo test -p inspectah-collect test_owning_package_batch_query_populates_state_changes -- --exact
cargo test -p inspectah-collect test_owning_package_fallback_checks_etc_systemd_path -- --exact
```

Expected: FAIL because `owning_package` is still always `None`.

- [ ] **Step 3: Implement batch-first `rpm -qf` ownership lookup**

In `inspectah-collect/src/inspectors/services.rs`, add a dedicated helper and call it immediately after `state_changes` is finalized and before drop-ins are scanned:

```rust
const OWNING_PACKAGE_QUERY: &str = "%{NAME}\n";

fn populate_owning_packages(exec: &dyn Executor, state_changes: &mut [ServiceStateChange]) {
    if state_changes.is_empty() {
        return;
    }

    let batch_paths: Vec<String> = state_changes
        .iter()
        .map(|sc| format!("/usr/lib/systemd/system/{}", sc.unit))
        .collect();

    let batch_args: Vec<&str> = std::iter::once("-qf")
        .chain(std::iter::once("--queryformat"))
        .chain(std::iter::once(OWNING_PACKAGE_QUERY))
        .chain(batch_paths.iter().map(|s| s.as_str()))
        .collect();

    let result = exec.run("rpm", &batch_args);
    let batch_lines: Vec<&str> = result.stdout.lines().collect();

    if result.success() && batch_lines.len() == state_changes.len() {
        for (sc, owner) in state_changes.iter_mut().zip(batch_lines.into_iter()) {
            let trimmed = owner.trim();
            // Guard: rpm -qf returns "not owned by any package" for unpackaged files.
            // Match the Go reference: !strings.Contains(pkg, "not owned")
            if !trimmed.is_empty() && !trimmed.contains("not owned") {
                sc.owning_package = Some(trimmed.to_string());
            }
        }
        return;
    }

    for sc in state_changes.iter_mut() {
        sc.owning_package = query_owning_package(exec, &sc.unit);
    }
}

fn query_owning_package(exec: &dyn Executor, unit: &str) -> Option<String> {
    for path in [
        format!("/usr/lib/systemd/system/{unit}"),
        format!("/etc/systemd/system/{unit}"),
    ] {
        let result = exec.run("rpm", &["-qf", "--queryformat", OWNING_PACKAGE_QUERY, &path]);
        if result.success() {
            let pkg = result
                .stdout
                .lines()
                .next()
                .map(|line| line.trim().to_string())
                .unwrap_or_default();
            // Guard: filter "not owned by any package" responses.
            // Matches the Go reference: !strings.Contains(pkg, "not owned")
            if !pkg.is_empty() && !pkg.contains("not owned") {
                return Some(pkg);
            }
        }
    }
    None
}
```

Then call:

```rust
populate_owning_packages(exec, &mut state_changes);
```

- [ ] **Step 4: Add the remaining fallback proofs**

Add two more collector tests in `inspectah-collect/tests/services_test.rs`:

```rust
#[test]
fn test_owning_package_batch_failure_falls_back_to_individual_queries() {
    let exec = MockExecutor::new()
        .with_command(
            "systemctl list-unit-files --type=service --no-pager",
            ExecResult {
                stdout: "UNIT FILE                                  STATE           PRESET\n\
                         httpd.service                              enabled         disabled\n\
                         firewalld.service                          disabled        enabled\n\
                         \n\
                         2 unit files listed.\n"
                    .into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_dir("/usr/lib/systemd/system-preset", vec!["90-default.preset"])
        .with_file(
            "/usr/lib/systemd/system-preset/90-default.preset",
            "disable httpd.service\nenable firewalld.service\n",
        )
        .with_command(
            "rpm -qf --queryformat %{NAME}\n /usr/lib/systemd/system/httpd.service /usr/lib/systemd/system/firewalld.service",
            ExecResult {
                stdout: "error: batch ownership lookup failed\n".into(),
                exit_code: 1,
                ..Default::default()
            },
        )
        .with_command(
            "rpm -qf --queryformat %{NAME}\n /usr/lib/systemd/system/httpd.service",
            ExecResult {
                stdout: "httpd\n".into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_command(
            "rpm -qf --queryformat %{NAME}\n /usr/lib/systemd/system/firewalld.service",
            ExecResult {
                stdout: "firewalld\n".into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_dir("/etc/systemd/system", vec![]);

    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
        baseline_data: None,
    };

    let output = ServicesInspector::new().inspect(&ctx).unwrap();
    let section = match &output.section {
        SectionData::Services(s) => s,
        other => panic!("expected Services section, got {other:?}"),
    };

    assert_eq!(section.state_changes[0].owning_package.as_deref(), Some("httpd"));
    assert_eq!(section.state_changes[1].owning_package.as_deref(), Some("firewalld"));
}

#[test]
fn test_unowned_unit_keeps_owning_package_none() {
    let exec = MockExecutor::new()
        .with_command(
            "systemctl list-unit-files --type=service --no-pager",
            ExecResult {
                stdout: "UNIT FILE                                  STATE           PRESET\n\
                         custom-local.service                       enabled         disabled\n\
                         \n\
                         1 unit files listed.\n"
                    .into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_dir("/usr/lib/systemd/system-preset", vec!["90-default.preset"])
        .with_file("/usr/lib/systemd/system-preset/90-default.preset", "disable *\n")
        .with_command(
            "rpm -qf --queryformat %{NAME}\n /usr/lib/systemd/system/custom-local.service",
            ExecResult {
                stdout: "file /usr/lib/systemd/system/custom-local.service is not owned by any package\n".into(),
                exit_code: 1,
                ..Default::default()
            },
        )
        .with_command(
            "rpm -qf --queryformat %{NAME}\n /etc/systemd/system/custom-local.service",
            ExecResult {
                stdout: "file /etc/systemd/system/custom-local.service is not owned by any package\n".into(),
                exit_code: 1,
                ..Default::default()
            },
        )
        .with_dir("/etc/systemd/system", vec![]);

    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
        baseline_data: None,
    };

    let output = ServicesInspector::new().inspect(&ctx).unwrap();
    let section = match &output.section {
        SectionData::Services(s) => s,
        other => panic!("expected Services section, got {other:?}"),
    };

    assert_eq!(section.state_changes[0].owning_package, None);
}
```

Use these exact commands for the focused red/green loop:

```bash
cargo test -p inspectah-collect test_owning_package_batch_failure_falls_back_to_individual_queries -- --exact
cargo test -p inspectah-collect test_unowned_unit_keeps_owning_package_none -- --exact
```

- [ ] **Step 5: Run broader collector verification**

Run:

```bash
cargo test -p inspectah-collect test_owning_package_batch_query_populates_state_changes -- --exact
cargo test -p inspectah-collect test_owning_package_fallback_checks_etc_systemd_path -- --exact
cargo test -p inspectah-collect test_clean_default_snapshot_produces_zero_state_changes -- --exact
cargo test -p inspectah-collect
```

Expected: all four focused tests and the full crate PASS.

- [ ] **Step 6: Commit the collector ownership slice**

```bash
git add inspectah-collect/src/inspectors/services.rs inspectah-collect/tests/services_test.rs
git commit -m "$(cat <<'EOF'
feat(services): resolve owning packages during collection

Populate service owning_package with a batch rpm -qf lookup and a per-unit
fallback so later omission and advisory logic can reason about package presence
without duplicating ownership discovery downstream.

Assisted-by: Claude Code (Opus 4.6)
EOF
)"
```

---

### Task 3a: Fix RPM collector to emit plain baseline names

> Tasks 3a and 3b are independent — they can be implemented in either order.

**Files:**
- Modify: `inspectah-collect/src/inspectors/rpm/mod.rs`

- [ ] **Step 1: Write the failing RPM collector proof for plain baseline names**

Add this test to the existing `#[cfg(test)]` module in `inspectah-collect/src/inspectors/rpm/mod.rs`:

```rust
#[test]
fn test_baseline_package_names_use_plain_rpm_names() {
    use inspectah_core::baseline::{BaselineData, BaselinePackageEntry};

    let baseline_data = BaselineData {
        image_digest: "sha256:test".into(),
        packages: std::collections::HashMap::from([
            (
                "firewalld.x86_64".into(),
                BaselinePackageEntry {
                    name: "firewalld".into(),
                    epoch: Some("0".into()),
                    version: "1.3.4".into(),
                    release: "1.el9".into(),
                    arch: "x86_64".into(),
                },
            ),
            (
                "systemd.x86_64".into(),
                BaselinePackageEntry {
                    name: "systemd".into(),
                    epoch: Some("0".into()),
                    version: "252.32".into(),
                    release: "1.el9".into(),
                    arch: "x86_64".into(),
                },
            ),
        ]),
        extracted_at: "2026-05-19T00:00:00Z".into(),
    };

    let names: Vec<String> = baseline_data
        .packages
        .values()
        .map(|pkg| pkg.name.clone())
        .collect();

    assert!(names.contains(&"firewalld".to_string()));
    assert!(names.contains(&"systemd".to_string()));
    assert!(!names.iter().any(|name| name.contains('.')));
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run:

```bash
cargo test -p inspectah-collect test_baseline_package_names_use_plain_rpm_names -- --exact
```

Expected: FAIL because the RPM collector still reuses canonical `name.arch` keys.

- [ ] **Step 3: Implement the plain-name baseline contract in `inspectah-collect/src/inspectors/rpm/mod.rs`**

Replace the current `baseline_package_names` builder with a deduped plain-name list:

```rust
let baseline_package_names = ctx.baseline_data.map(|b| {
    let mut names: Vec<String> = b
        .packages
        .values()
        .map(|pkg| pkg.name.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    names.sort();
    names
});
```

- [ ] **Step 4: Run focused verification**

Run:

```bash
cargo test -p inspectah-collect test_baseline_package_names_use_plain_rpm_names -- --exact
cargo test -p inspectah-collect
```

Expected: the focused test and the full crate PASS.

- [ ] **Step 5: Commit the plain baseline names fix**

```bash
git add inspectah-collect/src/inspectors/rpm/mod.rs
git commit -m "$(cat <<'EOF'
fix(rpm): use plain package names for baseline comparison

Repopulate baseline_package_names from BaselinePackageEntry.name (plain RPM
Name: values) instead of the canonical name.arch keys so service omission
logic can match owning_package against baseline presence correctly.

Assisted-by: Claude Code (Opus 4.6)
EOF
)"
```

---

### Task 3b: Extract renderer helper seam for service intent decisions

> Tasks 3a and 3b are independent — they can be implemented in either order.

**Files:**
- Create: `inspectah-pipeline/src/render/service_intent.rs`
- Modify: `inspectah-pipeline/src/render/mod.rs`
- Modify: `inspectah-pipeline/src/render/containerfile.rs`
- Create: `inspectah-pipeline/tests/service_intent_test.rs`

- [ ] **Step 1: Write the failing helper tests**

Create `inspectah-pipeline/tests/service_intent_test.rs` with these two proofs:

```rust
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection};
use inspectah_pipeline::render::service_intent::{
    effective_target_packages, is_package_installable,
};

#[test]
fn test_effective_target_packages_uses_plain_names_and_include_true() {
    let rpm = RpmSection {
        baseline_package_names: Some(vec!["firewalld".into(), "systemd".into()]),
        packages_added: vec![
            PackageEntry {
                name: "custom-app".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                include: true,
                source_repo: "appstream".into(),
                ..Default::default()
            },
            PackageEntry {
                name: "excluded-app".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                include: false,
                source_repo: "appstream".into(),
                ..Default::default()
            },
        ],
        ..Default::default()
    };

    let names = effective_target_packages(&rpm);

    assert!(names.contains("firewalld"));
    assert!(names.contains("systemd"));
    assert!(names.contains("custom-app"));
    assert!(!names.contains("excluded-app"));
}

#[test]
fn test_is_package_installable_matches_manual_follow_up_contract() {
    let installable = PackageEntry {
        name: "httpd".into(),
        arch: "x86_64".into(),
        state: PackageState::Added,
        include: true,
        source_repo: "appstream".into(),
        ..Default::default()
    };
    let local = PackageEntry {
        name: "local-tool".into(),
        arch: "x86_64".into(),
        state: PackageState::LocalInstall,
        include: true,
        source_repo: String::new(),
        ..Default::default()
    };
    let empty_repo = PackageEntry {
        name: "mystery".into(),
        arch: "x86_64".into(),
        state: PackageState::Added,
        include: true,
        source_repo: String::new(),
        ..Default::default()
    };

    assert!(is_package_installable(&installable));
    assert!(!is_package_installable(&local));
    assert!(!is_package_installable(&empty_repo));
}
```

- [ ] **Step 2: Run the helper tests to verify they fail**

Run:

```bash
cargo test -p inspectah-pipeline --test service_intent_test test_effective_target_packages_uses_plain_names_and_include_true -- --exact
cargo test -p inspectah-pipeline --test service_intent_test test_is_package_installable_matches_manual_follow_up_contract -- --exact
```

Expected: FAIL because the helper module does not exist yet.

- [ ] **Step 3: Create `inspectah-pipeline/src/render/service_intent.rs` with the shared helper seam**

Start the new module with these exact definitions:

```rust
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ManualFollowUpReason {
    LocalInstall,
    NoRepo,
    MissingSourceRepo,
}

fn manual_follow_up_reason(pkg: &PackageEntry) -> Option<ManualFollowUpReason> {
    match pkg.state {
        PackageState::LocalInstall => Some(ManualFollowUpReason::LocalInstall),
        PackageState::NoRepo => Some(ManualFollowUpReason::NoRepo),
        _ if pkg.source_repo.is_empty() => Some(ManualFollowUpReason::MissingSourceRepo),
        _ => None,
    }
}

pub(crate) fn manual_follow_up_line(pkg: &PackageEntry) -> Option<String> {
    match manual_follow_up_reason(pkg) {
        Some(ManualFollowUpReason::LocalInstall) => Some(format!(
            "# TODO: '{}' was installed locally (state: local_install) - provide a .rpm or custom repo.",
            if pkg.arch.is_empty() { pkg.name.clone() } else { format!("{}.{}", pkg.name, pkg.arch) }
        )),
        Some(ManualFollowUpReason::NoRepo) => Some(format!(
            "# TODO: '{}' has no repository source (state: no_repo) - provide a .rpm or custom repo.",
            if pkg.arch.is_empty() { pkg.name.clone() } else { format!("{}.{}", pkg.name, pkg.arch) }
        )),
        Some(ManualFollowUpReason::MissingSourceRepo) => Some(format!(
            "# TODO: '{}' has no recorded repository source - verify how to reinstall it in the image.",
            if pkg.arch.is_empty() { pkg.name.clone() } else { format!("{}.{}", pkg.name, pkg.arch) }
        )),
        None => None,
    }
}

pub fn is_package_installable(pkg: &PackageEntry) -> bool {
    manual_follow_up_reason(pkg).is_none()
}

pub fn effective_target_packages(rpm: &RpmSection) -> std::collections::BTreeSet<String> {
    let mut names = std::collections::BTreeSet::new();
    if let Some(baseline) = &rpm.baseline_package_names {
        names.extend(baseline.iter().cloned());
    }
    names.extend(
        rpm.packages_added
            .iter()
            .filter(|pkg| pkg.include)
            .map(|pkg| pkg.name.clone()),
    );
    names
}
```

- [ ] **Step 4: Reuse the helper in `containerfile.rs` immediately**

Update `inspectah-pipeline/src/render/containerfile.rs` so the package renderer stops open-coding installability:

```rust
use super::service_intent::{is_package_installable, manual_follow_up_line};

for pkg in &rpm.packages_added {
    if let Some(line) = manual_follow_up_line(pkg) {
        todo_lines.push(line);
    }
}

let is_fleet_snapshot = rpm.packages_added.iter().any(|pkg| pkg.fleet.is_some());
let leaf_filter: Option<std::collections::HashSet<String>> = rpm
    .leaf_packages
    .as_ref()
    .filter(|_| !is_fleet_snapshot)
    .map(|leaf_packages| leaf_packages.iter().cloned().collect());

let baseline_suppressed_set: std::collections::HashSet<String> = rpm
    .baseline_suppressed
    .as_ref()
    .map(|v| v.iter().cloned().collect())
    .unwrap_or_default();

let installable_packages: Vec<&PackageEntry> = rpm
    .packages_added
    .iter()
    .filter(|pkg| pkg.include)
    .filter(|pkg| is_package_installable(pkg))
    .filter(|pkg| {
        !baseline_suppressed_set.contains(&canonical_package_id(&pkg.name, &pkg.arch))
    })
    .filter(|pkg| {
        leaf_filter.as_ref().is_none_or(|leaf_ids| {
            leaf_ids.contains(&canonical_package_id(&pkg.name, &pkg.arch))
        })
    })
    .collect();
```

- [ ] **Step 5: Export the new module and run focused verification**

Add to `inspectah-pipeline/src/render/mod.rs`:

```rust
pub mod service_intent;
```

Then run:

```bash
cargo test -p inspectah-pipeline --test service_intent_test test_effective_target_packages_uses_plain_names_and_include_true -- --exact
cargo test -p inspectah-pipeline --test service_intent_test test_is_package_installable_matches_manual_follow_up_contract -- --exact
cargo test -p inspectah-pipeline test_containerfile_non_leaf_manual_follow_up_survives_leaf_filter -- --exact
```

Expected: all focused tests PASS.

- [ ] **Step 6: Commit the shared helper seam**

```bash
git add inspectah-pipeline/src/render/service_intent.rs inspectah-pipeline/src/render/mod.rs inspectah-pipeline/src/render/containerfile.rs inspectah-pipeline/tests/service_intent_test.rs
git commit -m "$(cat <<'EOF'
refactor(render): share service package truth helpers

Extract the package-truth seam used by both package rendering and service
decision logic so installability and target-image package presence stay aligned
as the service omission/advisory contract becomes real.

Assisted-by: Claude Code (Opus 4.6)
EOF
)"
```

---

### Task 4: Make the renderer the single authority for service omissions and advisories

**Files:**
- Modify: `inspectah-pipeline/src/render/service_intent.rs`
- Modify: `inspectah-pipeline/src/render/containerfile.rs`
- Modify: `inspectah-pipeline/tests/service_intent_test.rs`

- [ ] **Step 1: Write the failing renderer-decision tests**

Extend `inspectah-pipeline/tests/service_intent_test.rs` with these exact proofs:

```rust
use inspectah_core::types::services::{
    PresetDefault, ServiceSection, ServiceStateChange, ServiceUnitState,
};
use inspectah_pipeline::render::service_intent::{
    render_service_intent, AdvisoryReason,
};

fn state_change(
    unit: &str,
    current_state: ServiceUnitState,
    default_state: Option<PresetDefault>,
    owning_package: Option<&str>,
) -> ServiceStateChange {
    ServiceStateChange {
        unit: unit.into(),
        current_state,
        default_state,
        include: true,
        owning_package: owning_package.map(str::to_string),
        fleet: None,
        attention_reason: None,
    }
}

#[test]
fn test_service_render_plan_omits_proven_absent_service() {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        baseline_package_names: Some(vec!["firewalld".into()]),
        packages_added: vec![],
        no_baseline: false,
        ..Default::default()
    });
    snap.services = Some(ServiceSection {
        state_changes: vec![state_change(
            "sssd-kcm.service",
            ServiceUnitState::Disabled,
            Some(PresetDefault::Enable),
            Some("sssd"),
        )],
        enabled_units: vec![],
        disabled_units: vec![],
        drop_ins: vec![],
        preset_matched_units: vec![],
    });

    let plan = render_service_intent(&snap);

    assert!(plan.lines.iter().all(|line| !line.contains("sssd-kcm.service")));
    assert_eq!(plan.omissions.len(), 1);
    assert_eq!(plan.omissions[0].unit, "sssd-kcm.service");
    assert_eq!(plan.omissions[0].owning_package, "sssd");
}

#[test]
fn test_service_render_plan_stacks_package_excluded_and_baseline_unavailable() {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        baseline_package_names: Some(vec![]),
        packages_added: vec![PackageEntry {
            name: "custom-app".into(),
            arch: "x86_64".into(),
            state: PackageState::Added,
            include: false,
            source_repo: "appstream".into(),
            ..Default::default()
        }],
        no_baseline: true,
        ..Default::default()
    });
    snap.services = Some(ServiceSection {
        state_changes: vec![state_change(
            "custom-app.service",
            ServiceUnitState::Enabled,
            Some(PresetDefault::Disable),
            Some("custom-app"),
        )],
        enabled_units: vec![],
        disabled_units: vec![],
        drop_ins: vec![],
        preset_matched_units: vec![],
    });

    let plan = render_service_intent(&snap);

    assert!(plan.omissions.is_empty());
    assert_eq!(plan.advisories.len(), 1);
    assert_eq!(
        plan.advisories[0].reasons,
        vec![AdvisoryReason::PackageExcluded, AdvisoryReason::BaselineUnavailable]
    );
}
```

- [ ] **Step 2: Run the new tests to verify they fail**

Run:

```bash
cargo test -p inspectah-pipeline --test service_intent_test test_service_render_plan_omits_proven_absent_service -- --exact
cargo test -p inspectah-pipeline --test service_intent_test test_service_render_plan_stacks_package_excluded_and_baseline_unavailable -- --exact
```

Expected: FAIL because `render_service_intent()` does not exist yet.

- [ ] **Step 3: Build the renderer authority module in `service_intent.rs`**

Extend `inspectah-pipeline/src/render/service_intent.rs` with these types and the decision engine:

```rust
use inspectah_core::types::services::{ServiceAction, ServiceStateChange};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceOmission {
    pub unit: String,
    pub owning_package: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceAdvisory {
    pub unit: String,
    pub owning_package: String,
    pub reasons: Vec<AdvisoryReason>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdvisoryReason {
    PackageExcluded,
    PackageUnreachable,
    BaselineUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceRenderPlan {
    pub lines: Vec<String>,
    pub omissions: Vec<ServiceOmission>,
    pub advisories: Vec<ServiceAdvisory>,
}

enum PresenceDecision {
    Omit { owning_package: String },
    Emit { advisory_reasons: Option<(String, Vec<AdvisoryReason>)> },
}

fn systemctl_lines(verb: &str, units: &[String]) -> Vec<String> {
    if units.len() <= 3 {
        vec![format!("RUN systemctl {} {}", verb, units.join(" "))]
    } else {
        let mut lines = vec![format!("RUN systemctl {} \\", verb)];
        for (idx, unit) in units.iter().enumerate() {
            if idx < units.len() - 1 {
                lines.push(format!("    {} \\", unit));
            } else {
                lines.push(format!("    {}", unit));
            }
        }
        lines
    }
}

fn config_tree_units(snap: &InspectionSnapshot) -> std::collections::HashSet<String> {
    let mut units = std::collections::HashSet::new();
    if let Some(st) = &snap.scheduled_tasks {
        for timer in &st.systemd_timers {
            if timer.source == "local" && !timer.name.is_empty() {
                units.insert(format!("{}.timer", timer.name));
                units.insert(format!("{}.service", timer.name));
            }
        }
        for generated in &st.generated_timer_units {
            if generated.include && !generated.name.is_empty() {
                if !generated.timer_content.is_empty() {
                    units.insert(format!("{}.timer", generated.name));
                }
                if !generated.service_content.is_empty() {
                    units.insert(format!("{}.service", generated.name));
                }
            }
        }
    }
    units
}

fn render_without_rpm(snap: &InspectionSnapshot, services: &ServiceSection) -> ServiceRenderPlan {
    let config_tree = config_tree_units(snap);
    let mut enabled = Vec::new();
    let mut disabled = Vec::new();
    let mut masked = Vec::new();
    let mut deferred = Vec::new();

    for sc in services.state_changes.iter().filter(|sc| sc.include) {
        match sc.implied_action() {
            ServiceAction::Enable => {
                if config_tree.contains(sc.unit.as_str()) {
                    deferred.push(sc.unit.clone());
                } else {
                    enabled.push(sc.unit.clone());
                }
            }
            ServiceAction::Disable => disabled.push(sc.unit.clone()),
            ServiceAction::Mask => masked.push(sc.unit.clone()),
        }
    }

    let mut lines = Vec::new();
    lines.extend(systemctl_lines("enable", &enabled));
    lines.extend(systemctl_lines("disable", &disabled));
    lines.extend(systemctl_lines("mask", &masked));
    if !deferred.is_empty() {
        lines.push(format!(
            "# {} unit(s) deferred to Scheduled Tasks section: {}",
            deferred.len(),
            deferred.join(", ")
        ));
    }

    ServiceRenderPlan {
        lines,
        omissions: Vec::new(),
        advisories: Vec::new(),
    }
}

fn classify_service_presence(
    sc: &ServiceStateChange,
    rpm: &RpmSection,
    target_packages: &std::collections::BTreeSet<String>,
    baseline_unavailable: bool,
) -> PresenceDecision {
    let Some(pkg_name) = sc.owning_package.clone() else {
        return PresenceDecision::Emit {
            advisory_reasons: None,
        };
    };

    if let Some(pkg) = rpm.packages_added.iter().find(|pkg| pkg.name == pkg_name) {
        if !pkg.include {
            let mut reasons = vec![AdvisoryReason::PackageExcluded];
            if baseline_unavailable {
                reasons.push(AdvisoryReason::BaselineUnavailable);
            }
            return PresenceDecision::Emit {
                advisory_reasons: Some((pkg_name, reasons)),
            };
        }

        if !is_package_installable(pkg) {
            let mut reasons = vec![AdvisoryReason::PackageUnreachable];
            if baseline_unavailable {
                reasons.push(AdvisoryReason::BaselineUnavailable);
            }
            return PresenceDecision::Emit {
                advisory_reasons: Some((pkg_name, reasons)),
            };
        }

        return PresenceDecision::Emit {
            advisory_reasons: None,
        };
    }

    if target_packages.contains(&pkg_name) {
        return PresenceDecision::Emit {
            advisory_reasons: None,
        };
    }

    if baseline_unavailable {
        return PresenceDecision::Emit {
            advisory_reasons: Some((pkg_name, vec![AdvisoryReason::BaselineUnavailable])),
        };
    }

    PresenceDecision::Omit {
        owning_package: pkg_name,
    }
}

pub fn render_service_intent(snap: &InspectionSnapshot) -> ServiceRenderPlan {
    let mut lines = Vec::new();
    let mut omissions = Vec::new();
    let mut advisories = Vec::new();

    let services = match &snap.services {
        Some(services) => services,
        None => {
            return ServiceRenderPlan {
                lines,
                omissions,
                advisories,
            };
        }
    };

    let rpm = match &snap.rpm {
        Some(rpm) => rpm,
        None => {
            return render_without_rpm(snap, services);
        }
    };

    let target_packages = effective_target_packages(rpm);
    let baseline_unavailable = rpm.no_baseline || snap.no_baseline;
    let config_tree_units = config_tree_units(snap);

    let mut enabled = Vec::new();
    let mut disabled = Vec::new();
    let mut masked = Vec::new();
    let mut deferred = Vec::new();

    for sc in services.state_changes.iter().filter(|sc| sc.include) {
        let decision = classify_service_presence(sc, rpm, &target_packages, baseline_unavailable);
        match decision {
            PresenceDecision::Omit { owning_package } => {
                omissions.push(ServiceOmission {
                    unit: sc.unit.clone(),
                    owning_package,
                });
                lines.push(format!(
                    "# Omitted: {} (package '{}' not in target image)",
                    sc.unit, omissions.last().unwrap().owning_package
                ));
                continue;
            }
            PresenceDecision::Emit { advisory_reasons } => {
                if let Some((pkg, reasons)) = advisory_reasons {
                    advisories.push(ServiceAdvisory {
                        unit: sc.unit.clone(),
                        owning_package: pkg,
                        reasons,
                    });
                }
            }
        }

        match sc.implied_action() {
            ServiceAction::Enable => {
                if config_tree_units.contains(sc.unit.as_str()) {
                    deferred.push(sc.unit.clone());
                } else {
                    enabled.push(sc.unit.clone());
                }
            }
            ServiceAction::Disable => disabled.push(sc.unit.clone()),
            ServiceAction::Mask => masked.push(sc.unit.clone()),
        }
    }

    lines.extend(systemctl_lines("enable", &enabled));
    lines.extend(systemctl_lines("disable", &disabled));
    lines.extend(systemctl_lines("mask", &masked));
    if !deferred.is_empty() {
        lines.push(format!(
            "# {} unit(s) deferred to Scheduled Tasks section: {}",
            deferred.len(),
            deferred.join(", ")
        ));
    }

    ServiceRenderPlan {
        lines,
        omissions,
        advisories,
    }
}
```

Implementation rules for this step:
- `owning_package: None` must always be conservative emit
- `BaselineUnavailable` must stack with `PackageExcluded` / `PackageUnreachable`
- evaluate omission before config-tree deferral so proven-absent services never become deferred fiction
- advisory services remain in the main emitted action list; the advisory list is supplemental context only

- [ ] **Step 4: Delegate `containerfile.rs` to the new authority module**

Replace the hand-rolled service section in `inspectah-pipeline/src/render/containerfile.rs` with a thin wrapper:

```rust
use super::service_intent::render_service_intent;

fn services_section_lines(snap: &InspectionSnapshot) -> Vec<String> {
    let plan = render_service_intent(snap);
    if plan.lines.is_empty() {
        return Vec::new();
    }

    let mut lines = vec!["# === Service Enablement ===".into()];
    lines.extend(plan.lines);
    lines.push(String::new());
    lines
}
```

Keep the existing degraded inspector comment in `render_containerfile()` so the task stays surgical.

- [ ] **Step 5: Add the remaining focused proofs**

Add these tests to `inspectah-pipeline/tests/service_intent_test.rs` before broad verification:

```rust
use inspectah_core::types::rpm::PackageState;
use inspectah_core::types::scheduled::{ScheduledTaskSection, SystemdTimer};

#[test]
fn test_service_render_plan_emits_package_unreachable_service() {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        baseline_package_names: Some(vec!["firewalld".into()]),
        packages_added: vec![PackageEntry {
            name: "local-pkg".into(),
            arch: "x86_64".into(),
            state: PackageState::LocalInstall,
            include: true,
            source_repo: String::new(),
            ..Default::default()
        }],
        no_baseline: false,
        ..Default::default()
    });
    snap.services = Some(ServiceSection {
        state_changes: vec![state_change(
            "local-pkg.service",
            ServiceUnitState::Enabled,
            Some(PresetDefault::Disable),
            Some("local-pkg"),
        )],
        enabled_units: vec![],
        disabled_units: vec![],
        drop_ins: vec![],
        preset_matched_units: vec![],
    });

    let plan = render_service_intent(&snap);

    assert!(plan.omissions.is_empty());
    assert_eq!(plan.advisories.len(), 1);
    assert_eq!(
        plan.advisories[0].reasons,
        vec![AdvisoryReason::PackageUnreachable]
    );
    assert!(
        plan.lines
            .iter()
            .any(|line| line.contains("systemctl enable local-pkg.service"))
    );
}

#[test]
fn test_service_render_plan_keeps_unknown_owner_conservative() {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        baseline_package_names: Some(vec![]),
        packages_added: vec![],
        no_baseline: false,
        ..Default::default()
    });
    snap.services = Some(ServiceSection {
        state_changes: vec![state_change(
            "mystery.service",
            ServiceUnitState::Disabled,
            Some(PresetDefault::Enable),
            None,
        )],
        enabled_units: vec![],
        disabled_units: vec![],
        drop_ins: vec![],
        preset_matched_units: vec![],
    });

    let plan = render_service_intent(&snap);

    assert!(plan.omissions.is_empty());
    assert!(plan.advisories.is_empty());
    assert!(
        plan.lines
            .iter()
            .any(|line| line.contains("systemctl disable mystery.service"))
    );
}

#[test]
fn test_service_render_plan_suppresses_before_config_tree_deferral() {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        baseline_package_names: Some(vec!["firewalld".into()]),
        packages_added: vec![],
        no_baseline: false,
        ..Default::default()
    });
    snap.services = Some(ServiceSection {
        state_changes: vec![state_change(
            "backup.service",
            ServiceUnitState::Enabled,
            Some(PresetDefault::Disable),
            Some("backup-tools"),
        )],
        enabled_units: vec![],
        disabled_units: vec![],
        drop_ins: vec![],
        preset_matched_units: vec![],
    });
    snap.scheduled_tasks = Some(ScheduledTaskSection {
        systemd_timers: vec![SystemdTimer {
            name: "backup".into(),
            source: "local".into(),
            include: Some(true),
            ..Default::default()
        }],
        ..Default::default()
    });

    let plan = render_service_intent(&snap);

    assert_eq!(plan.omissions.len(), 1);
    assert_eq!(plan.omissions[0].unit, "backup.service");
    assert!(plan.lines.iter().any(|line| {
        line == "# Omitted: backup.service (package 'backup-tools' not in target image)"
    }));
    assert!(
        !plan.lines
            .iter()
            .any(|line| line.contains("deferred to Scheduled Tasks"))
    );
    assert!(
        !plan.lines
            .iter()
            .any(|line| line.contains("systemctl enable backup.service"))
    );
}

/// Proof: baseline-present and included-installable packages emit with zero
/// omissions and zero advisories (proven-present tier).
#[test]
fn test_service_render_plan_proven_present_emits_clean() {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        baseline_package_names: Some(vec!["firewalld".into(), "httpd".into()]),
        packages_added: vec![PackageEntry {
            name: "custom-app".into(),
            arch: "x86_64".into(),
            state: PackageState::Added,
            include: true,
            source_repo: "appstream".into(),
            ..Default::default()
        }],
        no_baseline: false,
        ..Default::default()
    });
    snap.services = Some(ServiceSection {
        state_changes: vec![
            // firewalld is in baseline — proven present
            state_change(
                "firewalld.service",
                ServiceUnitState::Enabled,
                Some(PresetDefault::Disable),
                Some("firewalld"),
            ),
            // custom-app is in packages_added with include:true — proven present
            state_change(
                "custom-app.service",
                ServiceUnitState::Enabled,
                Some(PresetDefault::Disable),
                Some("custom-app"),
            ),
        ],
        enabled_units: vec![],
        disabled_units: vec![],
        drop_ins: vec![],
        preset_matched_units: vec![],
    });

    let plan = render_service_intent(&snap);

    assert!(plan.omissions.is_empty(), "proven-present packages must not be omitted");
    assert!(plan.advisories.is_empty(), "proven-present packages must not emit advisories");
    assert!(plan.lines.iter().any(|l| l.contains("firewalld.service")));
    assert!(plan.lines.iter().any(|l| l.contains("custom-app.service")));
}

/// Proof: BaselineUnavailable fires in isolation when no other reason applies.
/// This covers the case where the owning package is not in packages_added and
/// not in baseline, but baseline is unavailable — the only advisory reason is
/// BaselineUnavailable (not stacked with PackageExcluded or PackageUnreachable).
#[test]
fn test_service_render_plan_pure_baseline_unavailable_advisory() {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        baseline_package_names: Some(vec![]),
        packages_added: vec![],
        no_baseline: true,
        ..Default::default()
    });
    snap.services = Some(ServiceSection {
        state_changes: vec![state_change(
            "unknown-pkg.service",
            ServiceUnitState::Enabled,
            Some(PresetDefault::Disable),
            Some("unknown-pkg"),
        )],
        enabled_units: vec![],
        disabled_units: vec![],
        drop_ins: vec![],
        preset_matched_units: vec![],
    });

    let plan = render_service_intent(&snap);

    assert!(plan.omissions.is_empty(), "baseline-unavailable must not omit");
    assert_eq!(plan.advisories.len(), 1);
    assert_eq!(
        plan.advisories[0].reasons,
        vec![AdvisoryReason::BaselineUnavailable],
        "must be pure BaselineUnavailable, not stacked with another reason"
    );
    assert!(plan.lines.iter().any(|l| l.contains("unknown-pkg.service")),
        "advisory services must still be emitted in the action list");
}

/// Proof: advisory services are NOT suppressed by config-tree deferral.
/// When a service has an advisory (e.g., PackageExcluded) AND matches a
/// config-tree timer, the advisory must survive — the defer logic must not
/// eat the advisory.
#[test]
fn test_service_render_plan_advisory_survives_config_tree_deferral() {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        baseline_package_names: Some(vec![]),
        packages_added: vec![PackageEntry {
            name: "scheduled-app".into(),
            arch: "x86_64".into(),
            state: PackageState::Added,
            include: false, // excluded — triggers PackageExcluded advisory
            source_repo: "appstream".into(),
            ..Default::default()
        }],
        no_baseline: false,
        ..Default::default()
    });
    snap.services = Some(ServiceSection {
        state_changes: vec![state_change(
            "scheduled-app.service",
            ServiceUnitState::Enabled,
            Some(PresetDefault::Disable),
            Some("scheduled-app"),
        )],
        enabled_units: vec![],
        disabled_units: vec![],
        drop_ins: vec![],
        preset_matched_units: vec![],
    });
    // This timer would normally cause deferral — but the advisory must still appear
    snap.scheduled_tasks = Some(ScheduledTaskSection {
        systemd_timers: vec![SystemdTimer {
            name: "scheduled-app".into(),
            source: "local".into(),
            include: Some(true),
            ..Default::default()
        }],
        ..Default::default()
    });

    let plan = render_service_intent(&snap);

    assert!(plan.omissions.is_empty(), "excluded package is advisory, not omission");
    assert_eq!(plan.advisories.len(), 1, "advisory must survive config-tree deferral");
    assert_eq!(plan.advisories[0].unit, "scheduled-app.service");
    assert!(
        plan.advisories[0].reasons.contains(&AdvisoryReason::PackageExcluded),
        "PackageExcluded reason must not be suppressed by deferral"
    );
}

/// Proof: stacked advisory renders multi-reason correctly.
/// When PackageExcluded AND BaselineUnavailable both apply, both reasons
/// must appear in the advisory and the service must still be emitted in the
/// action list (not omitted).
#[test]
fn test_service_render_plan_stacked_advisory_verifies_multi_reason() {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        baseline_package_names: Some(vec![]),
        packages_added: vec![PackageEntry {
            name: "custom-app".into(),
            arch: "x86_64".into(),
            state: PackageState::Added,
            include: false,
            source_repo: "appstream".into(),
            ..Default::default()
        }],
        no_baseline: true,
        ..Default::default()
    });
    snap.services = Some(ServiceSection {
        state_changes: vec![state_change(
            "custom-app.service",
            ServiceUnitState::Enabled,
            Some(PresetDefault::Disable),
            Some("custom-app"),
        )],
        enabled_units: vec![],
        disabled_units: vec![],
        drop_ins: vec![],
        preset_matched_units: vec![],
    });

    let plan = render_service_intent(&snap);

    // Must not be omitted — advisory services are always emitted
    assert!(plan.omissions.is_empty());
    // Must have exactly one advisory with exactly two reasons
    assert_eq!(plan.advisories.len(), 1);
    assert_eq!(plan.advisories[0].unit, "custom-app.service");
    assert_eq!(plan.advisories[0].reasons.len(), 2,
        "stacked advisory must carry both reasons");
    assert_eq!(plan.advisories[0].reasons[0], AdvisoryReason::PackageExcluded,
        "first reason must be PackageExcluded (primary)");
    assert_eq!(plan.advisories[0].reasons[1], AdvisoryReason::BaselineUnavailable,
        "second reason must be BaselineUnavailable (stacked)");
    // Service must still appear in the emitted action lines
    assert!(plan.lines.iter().any(|l| l.contains("custom-app.service")),
        "stacked-advisory services must be emitted, not suppressed");
}
```

Also update one existing inline containerfile proof in `inspectah-pipeline/src/render/containerfile.rs` so the runtime path is pinned end-to-end:

```rust
#[test]
fn test_containerfile_services_use_implied_action() {
    use inspectah_core::types::services::{
        PresetDefault, ServiceSection, ServiceStateChange, ServiceUnitState,
    };

    let mut snap = InspectionSnapshot::new();
    snap.services = Some(ServiceSection {
        state_changes: vec![
            ServiceStateChange {
                unit: "httpd.service".into(),
                current_state: ServiceUnitState::Enabled,
                default_state: Some(PresetDefault::Disable),
                include: true,
                owning_package: Some("httpd".into()),
                fleet: None,
                attention_reason: None,
            },
            ServiceStateChange {
                unit: "cups.service".into(),
                current_state: ServiceUnitState::Masked,
                default_state: None,
                include: true,
                owning_package: Some("cups".into()),
                fleet: None,
                attention_reason: None,
            },
        ],
        enabled_units: vec![],
        disabled_units: vec![],
        drop_ins: vec![],
        preset_matched_units: vec![],
    });

    let output = render_containerfile(&snap, None);
    assert!(output.contains("systemctl enable httpd.service"));
    assert!(output.contains("systemctl mask cups.service"));
}
```

- [ ] **Step 6: Run focused and broad renderer verification**

Run:

```bash
cargo test -p inspectah-pipeline --test service_intent_test test_service_render_plan_omits_proven_absent_service -- --exact
cargo test -p inspectah-pipeline --test service_intent_test test_service_render_plan_stacks_package_excluded_and_baseline_unavailable -- --exact
cargo test -p inspectah-pipeline --test service_intent_test test_service_render_plan_emits_package_unreachable_service -- --exact
cargo test -p inspectah-pipeline --test service_intent_test test_service_render_plan_keeps_unknown_owner_conservative -- --exact
cargo test -p inspectah-pipeline --test service_intent_test test_service_render_plan_suppresses_before_config_tree_deferral -- --exact
cargo test -p inspectah-pipeline --test service_intent_test test_service_render_plan_proven_present_emits_clean -- --exact
cargo test -p inspectah-pipeline --test service_intent_test test_service_render_plan_pure_baseline_unavailable_advisory -- --exact
cargo test -p inspectah-pipeline --test service_intent_test test_service_render_plan_advisory_survives_config_tree_deferral -- --exact
cargo test -p inspectah-pipeline --test service_intent_test test_service_render_plan_stacked_advisory_verifies_multi_reason -- --exact
cargo test -p inspectah-pipeline test_containerfile_services_use_implied_action -- --exact
cargo test -p inspectah-pipeline
```

Expected: all focused proofs and the full crate PASS.

- [ ] **Step 7: Commit the renderer authority slice and stop for review**

```bash
git add inspectah-pipeline/src/render/service_intent.rs inspectah-pipeline/src/render/containerfile.rs inspectah-pipeline/tests/service_intent_test.rs
git commit -m "$(cat <<'EOF'
feat(render): centralize service omission decisions

Make the renderer the single source of truth for service omissions and
advisories so Containerfile output and refine-side service context consume the
same package-presence decisions.

Assisted-by: Claude Code (Opus 4.6)
EOF
)"
git show --stat HEAD
```

Review checkpoint:
- ask for Collins + Thorn review on this commit before continuing
- do not start Task 5 until the renderer authority slice is accepted

---

### Task 5: Consume renderer omissions, advisories, and warnings in `inspectah-web`

**Files:**
- Modify: `inspectah-web/src/handlers.rs`
- Modify: `inspectah-web/tests/api_test.rs`

- [ ] **Step 1: Write the failing backend normalization tests**

Add these tests to the existing test module in `inspectah-web/src/handlers.rs`:

```rust
#[test]
fn test_normalize_services_uses_typed_subtitles() {
    let mut snap = empty_snapshot();
    snap.services = Some(ServiceSection {
        state_changes: vec![
            ServiceStateChange {
                unit: "firewalld.service".into(),
                current_state: ServiceUnitState::Enabled,
                default_state: Some(PresetDefault::Disable),
                include: true,
                owning_package: Some("firewalld".into()),
                fleet: None,
                attention_reason: None,
            },
            ServiceStateChange {
                unit: "cups.service".into(),
                current_state: ServiceUnitState::Masked,
                default_state: None,
                include: true,
                owning_package: Some("cups".into()),
                fleet: None,
                attention_reason: None,
            },
        ],
        enabled_units: vec![],
        disabled_units: vec![],
        drop_ins: vec![],
        preset_matched_units: vec![],
    });

    let section = normalize_services(&snap);

    let firewalld = section.items.iter().find(|i| i.id == "firewalld.service").unwrap();
    assert_eq!(
        firewalld.subtitle.as_deref(),
        Some("enabled (diverges from preset: disable)")
    );

    let cups = section.items.iter().find(|i| i.id == "cups.service").unwrap();
    assert_eq!(cups.subtitle.as_deref(), Some("masked (no preset rule)"));
}

#[test]
fn test_normalize_services_adds_omitted_advisory_and_warning_subsections() {
    let mut snap = empty_snapshot();
    snap.rpm = Some(RpmSection {
        baseline_package_names: Some(vec!["firewalld".into()]),
        packages_added: vec![PackageEntry {
            name: "custom-app".into(),
            arch: "x86_64".into(),
            state: PackageState::Added,
            include: false,
            source_repo: "appstream".into(),
            ..Default::default()
        }],
        no_baseline: true,
        ..Default::default()
    });
    snap.services = Some(ServiceSection {
        state_changes: vec![
            ServiceStateChange {
                unit: "custom-app.service".into(),
                current_state: ServiceUnitState::Enabled,
                default_state: Some(PresetDefault::Disable),
                include: true,
                owning_package: Some("custom-app".into()),
                fleet: None,
                attention_reason: None,
            },
            ServiceStateChange {
                unit: "sssd-kcm.service".into(),
                current_state: ServiceUnitState::Disabled,
                default_state: Some(PresetDefault::Enable),
                include: true,
                owning_package: Some("sssd".into()),
                fleet: None,
                attention_reason: None,
            },
        ],
        enabled_units: vec![],
        disabled_units: vec![],
        drop_ins: vec![],
        preset_matched_units: vec![],
    });
    snap.warnings.push(Warning {
        inspector: "services".into(),
        message: "unit linked.service has state 'linked' - linked unit requires manual handling".into(),
        severity: Some(WarningSeverity::Warning),
        extra: std::collections::HashMap::from([
            ("unit".into(), serde_json::json!("linked.service")),
            ("raw_state".into(), serde_json::json!("linked")),
        ]),
    });

    let section = normalize_services(&snap);
    let omitted = section.subsections.iter().find(|s| s.id == "omitted_services").unwrap();
    let advisories = section.subsections.iter().find(|s| s.id == "service_advisories").unwrap();
    let warnings = section.subsections.iter().find(|s| s.id == "service_warnings").unwrap();

    assert!(omitted.items.iter().any(|item| item.id == "sssd-kcm.service"));
    assert!(advisories.items.iter().any(|item| item.id == "custom-app.service"));
    assert!(warnings.items.iter().any(|item| item.id == "linked.service"));
    assert!(section.items.iter().any(|item| item.id == "custom-app.service"));
}
```

- [ ] **Step 2: Run the backend tests to verify they fail**

Run:

```bash
cargo test -p inspectah-web test_normalize_services_uses_typed_subtitles -- --exact
cargo test -p inspectah-web test_normalize_services_adds_omitted_advisory_and_warning_subsections -- --exact
```

Expected: FAIL because `ContextSection` has no subsection shape yet and `normalize_services()` still formats string fields directly.

- [ ] **Step 3: Add a subsection DTO and a section helper**

In `inspectah-web/src/handlers.rs`, add a dedicated subsection type plus a small helper so the change stays local:

```rust
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct ContextSubsection {
    pub id: String,
    pub display_name: String,
    pub items: Vec<ContextItem>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct ContextSection {
    pub id: String,
    pub display_name: String,
    pub items: Vec<ContextItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subsections: Vec<ContextSubsection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub empty_reason: Option<String>,
}

fn context_section(id: &str, display_name: &str, items: Vec<ContextItem>) -> ContextSection {
    ContextSection {
        id: id.to_string(),
        display_name: display_name.to_string(),
        items,
        subsections: Vec::new(),
        empty_reason: None,
    }
}
```

Then update every existing section builder in this file to use `context_section(...)` or set `subsections: Vec::new()` explicitly so the DTO change does not ripple unpredictably.

- [ ] **Step 4: Rebuild `normalize_services()` around typed subtitles and renderer output**

At the top of `normalize_services()`, fetch the renderer decisions once:

```rust
let render_plan = inspectah_pipeline::render::service_intent::render_service_intent(snap);
```

Use this exact subtitle mapping for actionable services:

```rust
let subtitle = match (sc.current_state, sc.default_state) {
    (ServiceUnitState::Enabled, Some(PresetDefault::Disable)) => {
        "enabled (diverges from preset: disable)".to_string()
    }
    (ServiceUnitState::Disabled, Some(PresetDefault::Enable)) => {
        "disabled (diverges from preset: enable)".to_string()
    }
    (ServiceUnitState::Masked, Some(PresetDefault::Enable)) => {
        "masked (preset default: enable)".to_string()
    }
    (ServiceUnitState::Masked, Some(PresetDefault::Disable)) => {
        "masked (preset default: disable)".to_string()
    }
    (ServiceUnitState::Masked, None) => "masked (no preset rule)".to_string(),
};
```

Then build these supplemental subsections from renderer/warning output:

```rust
let omitted_subsection = ContextSubsection {
    id: "omitted_services".into(),
    display_name: "Omitted Services".into(),
    items: render_plan.omissions.iter().map(|omission| ContextItem {
        id: omission.unit.clone(),
        title: omission.unit.clone(),
        subtitle: Some(format!(
            "omitted (package '{}' not in target image)",
            omission.owning_package
        )),
        detail: Some(format!(
            "{} was omitted because inspectah could prove '{}' is absent from the target image.",
            omission.unit, omission.owning_package
        )),
        searchable_text: format!("{} {}", omission.unit, omission.owning_package),
    }).collect(),
};

let advisory_subsection = ContextSubsection {
    id: "service_advisories".into(),
    display_name: "Service Advisories".into(),
    items: render_plan.advisories.iter().map(|advisory| ContextItem {
        id: advisory.unit.clone(),
        title: advisory.unit.clone(),
        subtitle: Some(
            advisory
                .reasons
                .iter()
                .map(|reason| match reason {
                    AdvisoryReason::PackageExcluded => "package excluded - may still be present as a dependency",
                    AdvisoryReason::PackageUnreachable => "package requires manual installation",
                    AdvisoryReason::BaselineUnavailable => "baseline unavailable - cannot verify presence",
                })
                .collect::<Vec<_>>()
                .join("; "),
        ),
        detail: Some(format!(
            "{} is still emitted in the Containerfile; owning package '{}'.",
            advisory.unit, advisory.owning_package
        )),
        searchable_text: format!("{} {}", advisory.unit, advisory.owning_package),
    }).collect(),
};

let warning_subsection = ContextSubsection {
    id: "service_warnings".into(),
    display_name: "Service Warnings".into(),
    items: snap.warnings.iter().filter(|w| w.inspector == "services").map(|warning| {
        let unit = warning.extra.get("unit").and_then(|v| v.as_str()).unwrap_or("unknown.service");
        let raw_state = warning.extra.get("raw_state").and_then(|v| v.as_str()).unwrap_or("unknown");
        ContextItem {
            id: unit.to_string(),
            title: unit.to_string(),
            subtitle: Some(format!("{raw_state} (warning)")),
            detail: Some(warning.message.clone()),
            searchable_text: format!("{unit} {raw_state} {}", warning.message),
        }
    }).collect(),
};
```

- [ ] **Step 5: Add the HTTP-level proof in `inspectah-web/tests/api_test.rs`**

Add a new API test that proves `/api/snapshot/sections` carries the new service subsection shape:

```rust
fn service_subsection_state() -> Arc<AppState> {
    let mut snap = rich_snapshot();
    snap.rpm = Some(RpmSection {
        baseline_package_names: Some(vec!["firewalld".into()]),
        packages_added: vec![PackageEntry {
            name: "custom-app".into(),
            arch: "x86_64".into(),
            state: PackageState::Added,
            include: false,
            source_repo: "appstream".into(),
            ..Default::default()
        }],
        no_baseline: true,
        ..Default::default()
    });
    snap.services = Some(ServiceSection {
        state_changes: vec![
            ServiceStateChange {
                unit: "custom-app.service".into(),
                current_state: ServiceUnitState::Enabled,
                default_state: Some(PresetDefault::Disable),
                include: true,
                owning_package: Some("custom-app".into()),
                fleet: None,
                attention_reason: None,
            },
            ServiceStateChange {
                unit: "sssd-kcm.service".into(),
                current_state: ServiceUnitState::Disabled,
                default_state: Some(PresetDefault::Enable),
                include: true,
                owning_package: Some("sssd".into()),
                fleet: None,
                attention_reason: None,
            },
        ],
        enabled_units: vec![],
        disabled_units: vec![],
        drop_ins: vec![],
        preset_matched_units: vec![],
    });
    snap.warnings.push(Warning {
        inspector: "services".into(),
        message: "unit linked.service has state 'linked' - linked unit requires manual handling".into(),
        severity: Some(WarningSeverity::Warning),
        extra: std::collections::HashMap::from([
            ("unit".into(), serde_json::json!("linked.service")),
            ("raw_state".into(), serde_json::json!("linked")),
        ]),
    });

    Arc::new(AppState {
        session: Arc::new(Mutex::new(RefineSession::new(snap))),
        sections_cache: OnceLock::new(),
    })
}

#[tokio::test]
async fn sections_include_service_subsections() {
    let app = app(service_subsection_state());
    let (status, json) = get_json(&app, "/api/snapshot/sections").await;
    assert_eq!(status, StatusCode::OK);

    let sections = json.as_array().unwrap();
    let services = sections.iter().find(|section| section["id"] == "services").unwrap();
    let subsections = services["subsections"].as_array().expect("services should expose subsections");

    assert!(subsections.iter().any(|sub| sub["id"] == "omitted_services"));
    assert!(subsections.iter().any(|sub| sub["id"] == "service_advisories"));
    assert!(subsections.iter().any(|sub| sub["id"] == "service_warnings"));
}
```

Use the same task to update `rich_snapshot()` and the health schema assertions so they still use typed service fixtures and expect schema version `16`.

- [ ] **Step 6: Run focused and broad web-backend verification**

Run:

```bash
cargo test -p inspectah-web test_normalize_services_uses_typed_subtitles -- --exact
cargo test -p inspectah-web test_normalize_services_adds_omitted_advisory_and_warning_subsections -- --exact
cargo test -p inspectah-web sections_include_service_subsections -- --exact
cargo test -p inspectah-web sections_items_have_required_fields -- --exact
cargo test -p inspectah-web
```

Expected: all focused tests and the full crate PASS.

- [ ] **Step 7: Commit the backend consumption slice**

```bash
git add inspectah-web/src/handlers.rs inspectah-web/tests/api_test.rs
git commit -m "$(cat <<'EOF'
feat(web): surface service omissions and advisories

Map renderer-owned service omissions and advisories plus collector warnings
into explicit service subsections so refine consumes the same render decisions
without recomputing them.

Assisted-by: Claude Code (Opus 4.6)
EOF
)"
```

---

### Task 6: Render service subsections in the refine UI

**Files:**
- Modify: `inspectah-web/ui/src/api/types.ts`
- Modify: `inspectah-web/ui/src/components/ContextList.tsx`
- Modify: `inspectah-web/ui/src/components/__tests__/ContextList.test.tsx`

- [ ] **Step 1: Write the failing UI tests**

Add these tests to `inspectah-web/ui/src/components/__tests__/ContextList.test.tsx`:

```tsx
it("renders subsections after main items", () => {
  const section: ContextSection = {
    id: "services",
    display_name: "Services",
    items: [
      {
        id: "firewalld.service",
        title: "firewalld.service",
        subtitle: "enabled (diverges from preset: disable)",
        detail: null,
        searchable_text: "firewalld",
      },
    ],
    subsections: [
      {
        id: "service_advisories",
        display_name: "Service Advisories",
        items: [
          {
            id: "custom-app.service",
            title: "custom-app.service",
            subtitle: "package excluded - may still be present as a dependency",
            detail: null,
            searchable_text: "custom-app",
          },
        ],
      },
    ],
  };

  render(<ContextList section={section} />);

  expect(screen.getByText("firewalld.service")).toBeInTheDocument();
  expect(screen.getByText("Service Advisories")).toBeInTheDocument();
  expect(screen.getByText("custom-app.service")).toBeInTheDocument();
});

it("does_not_show_empty_state_when_only_subsections_exist", () => {
  const section: ContextSection = {
    id: "services",
    display_name: "Services",
    items: [],
    subsections: [
      {
        id: "service_warnings",
        display_name: "Service Warnings",
        items: [
          {
            id: "linked.service",
            title: "linked.service",
            subtitle: "linked (warning)",
            detail: "unit linked.service has state 'linked' - linked unit requires manual handling",
            searchable_text: "linked warning",
          },
        ],
      },
    ],
  };

  render(<ContextList section={section} />);

  expect(screen.queryByText(/No Services data in this snapshot/i)).not.toBeInTheDocument();
  expect(screen.getByText("Service Warnings")).toBeInTheDocument();
  expect(screen.getByText("linked.service")).toBeInTheDocument();
});
```

- [ ] **Step 2: Run the UI tests to verify they fail**

Run:

```bash
npm --prefix inspectah-web/ui run test -- src/components/__tests__/ContextList.test.tsx
```

Expected: FAIL because `ContextSection` has no `subsections` field and `ContextList` only renders a single flat `items` list.

- [ ] **Step 3: Add the subsection TypeScript shape**

Update `inspectah-web/ui/src/api/types.ts`:

```ts
export interface ContextSubsection {
  id: string;
  display_name: string;
  items: ContextItem[];
}

export interface ContextSection {
  id: string;
  display_name: string;
  items: ContextItem[];
  subsections?: ContextSubsection[];
  empty_reason?: string;
}
```

- [ ] **Step 4: Render main items and subsections in `ContextList.tsx`**

Replace the empty-state guard and render path with this structure:

```tsx
import { DataList, EmptyState, EmptyStateBody, Title } from "@patternfly/react-core";

export function ContextList({ section }: ContextListProps) {
  const subsections = (section.subsections ?? []).filter((sub) => sub.items.length > 0);
  const hasAnyItems = section.items.length > 0 || subsections.length > 0;

  if (!hasAnyItems) {
    return (
      <EmptyState
        titleText={`No ${section.display_name} data in this snapshot`}
        icon={CubesIcon}
        headingLevel="h3"
      >
        <EmptyStateBody>
          This section contains no items from the scanned host.
        </EmptyStateBody>
      </EmptyState>
    );
  }

  return (
    <>
      {section.items.length > 0 && (
        <DataList aria-label={`${section.display_name} context items`} style={{ borderLeft: "3px solid var(--pf-t--global--border--color--default)", marginTop: "var(--pf-t--global--spacer--md)" }}>
          {section.items.map((item) => (
            <ContextItem key={item.id} item={item} />
          ))}
        </DataList>
      )}

      {subsections.map((subsection) => (
        <div key={subsection.id} style={{ marginTop: "var(--pf-t--global--spacer--lg)" }}>
          <Title headingLevel="h3" size="lg">
            {subsection.display_name}
          </Title>
          <DataList aria-label={`${subsection.display_name} context items`} style={{ borderLeft: "3px solid var(--pf-t--global--border--color--default)", marginTop: "var(--pf-t--global--spacer--md)" }}>
            {subsection.items.map((item) => (
              <ContextItem key={item.id} item={item} />
            ))}
          </DataList>
        </div>
      ))}
    </>
  );
}
```

- [ ] **Step 5: Run targeted and broad UI verification**

Run:

```bash
npm --prefix inspectah-web/ui run test -- src/components/__tests__/ContextList.test.tsx
npm --prefix inspectah-web/ui run test -- src/components/__tests__/EmptyStates.test.tsx
npm --prefix inspectah-web/ui run test
npm --prefix inspectah-web/ui run build
```

Expected:
- the subsection tests PASS
- empty-state behavior stays correct
- the full UI test suite and build PASS

- [ ] **Step 6: Run the final end-to-end verification set**

Run the full proof sweep before claiming completion:

```bash
cargo test -p inspectah-core
cargo test -p inspectah-collect
cargo test -p inspectah-pipeline
cargo test -p inspectah-web
npm --prefix inspectah-web/ui run test
npm --prefix inspectah-web/ui run build
```

In particular, confirm these proof-bearing cases stayed green:
- `test_option_preset_default_serde_roundtrip` (serde roundtrip)
- `test_clean_default_snapshot_produces_zero_state_changes`
- `test_owning_package_fallback_checks_etc_systemd_path`
- `test_effective_target_packages_uses_plain_names_and_include_true`
- `test_service_render_plan_proven_present_emits_clean` (proven-present tier)
- `test_service_render_plan_pure_baseline_unavailable_advisory` (pure degraded-mode)
- `test_service_render_plan_advisory_survives_config_tree_deferral` (advisory-survives-defer)
- `test_service_render_plan_stacked_advisory_verifies_multi_reason` (stacked advisory)
- `test_service_render_plan_stacks_package_excluded_and_baseline_unavailable`
- `test_normalize_services_adds_omitted_advisory_and_warning_subsections`
- `sections_include_service_subsections`

- [ ] **Step 7: Commit the UI slice**

```bash
git add inspectah-web/ui/src/api/types.ts inspectah-web/ui/src/components/ContextList.tsx inspectah-web/ui/src/components/__tests__/ContextList.test.tsx
git commit -m "$(cat <<'EOF'
feat(web-ui): render service context subsections

Show omitted services, advisory services, and service warnings as supplemental
context below the main Services list so renderer decisions stay visible without
replacing actionable service items.

Assisted-by: Claude Code (Opus 4.6)
EOF
)"
```

