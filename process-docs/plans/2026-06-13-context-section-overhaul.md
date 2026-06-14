# Context Section Layout Overhaul — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace generic ContextItem rendering in three reference sections (Version Changes, Networking, Kernel & Boot) with purpose-built layouts.

**Architecture:** Networking and Kernel & Boot are Rust adapter-only changes — restructure items into `ContextSubsection`s that `ContextList` already renders. Version Changes gets a new React component reading from `ViewResponse.version_changes`. Two cross-cutting frontend fixes: sidebar count for subsection-only sections, and subsection accessibility semantics.

**Tech Stack:** Rust (adapter.rs), React + PatternFly + vitest (frontend), `@patternfly/react-table` for the version changes table.

**Spec:** `process-docs/specs/proposed/2026-06-13-context-section-overhaul.md`

---

### Task 1: Sidebar count fix for subsection-only sections

**Owner:** Kit (frontend)

**Files:**
- Modify: `crates/web/ui/src/components/Sidebar.tsx`
- Test: `crates/web/ui/src/components/__tests__/Sidebar.test.tsx` (or inline)

**Context:** `sectionCount()` in `Sidebar.tsx` returns `sec.items.length`. When networking and kernel/boot move items into subsections (top-level `items` empty), sidebar shows `0`. This task must land before the adapter changes.

- [ ] **Step 1: Write the failing test**

Create or add to `crates/web/ui/src/components/__tests__/Sidebar.test.tsx`. If no test file exists for `sectionCount`, test it as an exported helper or through component rendering.

```typescript
it("counts subsection items when top-level items is empty", () => {
  const section: ReferenceSection = {
    id: "network",
    display_name: "Network",
    items: [],
    subsections: [
      { id: "connections", display_name: "Connections", items: [
        { id: "eth0", title: "eth0", subtitle: null, detail: null, searchable_text: "eth0" },
        { id: "eth1", title: "eth1", subtitle: null, detail: null, searchable_text: "eth1" },
      ]},
      { id: "firewall", display_name: "Firewall", items: [
        { id: "public", title: "public", subtitle: null, detail: null, searchable_text: "public" },
      ]},
    ],
  };
  // sectionCount should return "3" (sum of subsection items)
});

it("uses top-level items count when items are present", () => {
  const section: ReferenceSection = {
    id: "storage",
    display_name: "Storage",
    items: [
      { id: "lvm", title: "lvm", subtitle: null, detail: null, searchable_text: "lvm" },
    ],
    subsections: [],
  };
  // sectionCount should return "1"
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd crates/web/ui && npx vitest run src/components/__tests__/Sidebar.test.tsx`

Expected: FAIL — subsection-only section returns "0".

- [ ] **Step 3: Implement the fix**

In `crates/web/ui/src/components/Sidebar.tsx`, update `sectionCount()`:

```typescript
function sectionCount(
  sections: ReferenceSection[] | null,
  id: string,
): string | undefined {
  if (!sections) return "...";
  const lookupId = id === "compose" ? "containers" : id;
  const sec = sections.find((s) => s.id === lookupId);
  if (!sec) return "0";
  const topLevel = sec.items.length;
  if (topLevel > 0) return String(topLevel);
  const subTotal = (sec.subsections ?? []).reduce(
    (sum, sub) => sum + sub.items.length,
    0,
  );
  return String(subTotal);
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd crates/web/ui && npx vitest run src/components/__tests__/Sidebar.test.tsx`

Expected: PASS

- [ ] **Step 5: Commit**

```
git add crates/web/ui/src/components/Sidebar.tsx crates/web/ui/src/components/__tests__/Sidebar.test.tsx
git commit -m "fix(refine): sidebar counts subsection items for subsection-only sections"
```

---

### Task 2: ContextList subsection accessibility

**Owner:** Kit (frontend)

**Files:**
- Modify: `crates/web/ui/src/components/ContextList.tsx`
- Test: `crates/web/ui/src/components/__tests__/ContextList.test.tsx`

**Context:** Subsection labels are plain `<div>` elements — no semantic structure for screen readers. Upgrade to `<section>` + `<h4>` + `aria-labelledby`.

- [ ] **Step 1: Write the failing test**

```typescript
it("renders subsection labels as h4 inside a section with aria-labelledby", () => {
  const section: ReferenceSection = {
    id: "network",
    display_name: "Network",
    items: [],
    subsections: [
      { id: "connections", display_name: "Connections", items: [
        { id: "eth0", title: "eth0", subtitle: null, detail: null, searchable_text: "eth0" },
      ]},
    ],
  };
  render(<ContextList section={section} />);
  const heading = screen.getByRole("heading", { level: 4, name: "Connections" });
  expect(heading).toBeInTheDocument();
  const sectionEl = heading.closest("section");
  expect(sectionEl).toHaveAttribute("aria-labelledby", "subsection-connections");
  expect(heading).toHaveAttribute("id", "subsection-connections");
});

it("subsection region contains its list items", () => {
  const section: ReferenceSection = {
    id: "network",
    display_name: "Network",
    items: [],
    subsections: [
      { id: "connections", display_name: "Connections", items: [
        { id: "eth0", title: "eth0", subtitle: null, detail: null, searchable_text: "eth0" },
      ]},
    ],
  };
  render(<ContextList section={section} />);
  const region = screen.getByRole("list", { name: "Connections context items" });
  expect(region).toBeInTheDocument();
  expect(within(region).getByTestId("context-item-eth0")).toBeInTheDocument();
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd crates/web/ui && npx vitest run src/components/__tests__/ContextList.test.tsx`

Expected: FAIL — no `<h4>` or `<section>` elements.

- [ ] **Step 3: Implement the fix**

In `ContextList.tsx`, replace the subsection rendering block:

```tsx
{subsections.map((sub) => (
  <section
    key={sub.id}
    className="inspectah-context-subsection"
    aria-labelledby={`subsection-${sub.id}`}
  >
    <h4
      id={`subsection-${sub.id}`}
      className="inspectah-context-subsection__label"
    >
      {sub.display_name}
    </h4>
    <div role="list" aria-label={`${sub.display_name} context items`}>
      {sub.items.map((item) => (
        <ContextItem key={item.id} item={item} />
      ))}
    </div>
  </section>
))}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd crates/web/ui && npx vitest run src/components/__tests__/ContextList.test.tsx`

Expected: PASS

- [ ] **Step 5: Run full frontend suite**

Run: `cd crates/web/ui && npx vitest run`

Expected: All tests pass. Some existing tests may need updating if they assert on the old `<div>` structure — fix any that break.

- [ ] **Step 6: Commit**

```
git add crates/web/ui/src/components/ContextList.tsx crates/web/ui/src/components/__tests__/ContextList.test.tsx
git commit -m "fix(refine): add semantic heading and region to ContextList subsections"
```

---

### Task 3: Networking adapter — subsections by type

**Owner:** Tang (Rust)

**Files:**
- Modify: `crates/web/src/adapter.rs` — `web_network_section()`
- Test: inline `#[cfg(test)]` in `adapter.rs` or `crates/web/tests/`

**Context:** `web_network_section()` currently pushes all items into a flat `items` vec. Restructure to emit 5 `ContextSubsection`s: Connections, Firewall, Routes & Rules, DNS & Hosts, Proxy. Top-level `items` stays empty. The `ContextList` frontend already renders subsections — no frontend changes needed (sidebar fix from Task 1 handles counts).

**Field mapping (exhaustive against `RefNetwork`):**
- `connections` → "Connections" subsection
- `firewall_zones` + `firewall_direct_rules` → "Firewall" subsection
- `static_routes` + `ip_routes` + `ip_rules` → "Routes & Rules" subsection
- `resolv_provenance` + `hosts_additions` → "DNS & Hosts" subsection
- `proxy_env` → "Proxy" subsection

Empty subsections are omitted.

- [ ] **Step 1: Write the failing test**

Add a test in the adapter test module (or `crates/web/tests/`) that constructs a `RefNetwork` with all fields populated and asserts the section has 5 subsections with correct `id`, `display_name`, and item counts. Also test that empty subsections are omitted.

```rust
#[test]
fn web_network_section_groups_into_subsections() {
    let data = RefNetwork {
        connections: vec![/* one RefNMConnection */],
        firewall_zones: vec![/* one RefFirewallZone */],
        firewall_direct_rules: vec![/* one RefFirewallDirectRule */],
        static_routes: vec![/* one RefStaticRoute */],
        ip_routes: vec!["10.0.0.0/8 via 10.0.0.1".to_string()],
        ip_rules: vec!["from 10.0.0.0/8 lookup 100".to_string()],
        resolv_provenance: "NetworkManager".to_string(),
        hosts_additions: vec!["10.0.0.5 db.local".to_string()],
        proxy_env: vec![/* one RefProxyEnv */],
    };
    let section = web_network_section(&data);
    assert!(section.items.is_empty(), "top-level items must be empty");
    assert_eq!(section.subsections.len(), 5);
    assert_eq!(section.subsections[0].id, "connections");
    assert_eq!(section.subsections[0].items.len(), 1);
    // Verify representative field values (not just counts)
    assert_eq!(section.subsections[0].items[0].title, "eth0");
    assert!(section.subsections[0].items[0].subtitle.as_deref()
        .unwrap().contains("ethernet"));

    assert_eq!(section.subsections[1].id, "firewall");
    assert_eq!(section.subsections[1].items.len(), 2); // zone + direct rule
    // Zone detail must contain zone content
    assert!(section.subsections[1].items[0].detail.is_some());

    assert_eq!(section.subsections[2].id, "routes_rules");
    assert_eq!(section.subsections[2].items.len(), 3); // static + ip_route + ip_rule
    // IP route subtitle must say "ip route"
    let ip_route_item = section.subsections[2].items.iter()
        .find(|i| i.subtitle.as_deref() == Some("ip route")).unwrap();
    assert!(!ip_route_item.searchable_text.is_empty());

    assert_eq!(section.subsections[3].id, "dns_hosts");
    assert_eq!(section.subsections[3].items.len(), 2); // resolv + hosts

    assert_eq!(section.subsections[4].id, "proxy");
    assert_eq!(section.subsections[4].items.len(), 1);
}

#[test]
fn web_network_section_omits_empty_subsections() {
    let data = RefNetwork {
        connections: vec![/* one connection */],
        ..Default::default()
    };
    let section = web_network_section(&data);
    assert_eq!(section.subsections.len(), 1, "only non-empty subsections");
    assert_eq!(section.subsections[0].id, "connections");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p inspectah-web -- web_network_section`

Expected: FAIL — section currently has flat items, no subsections.

- [ ] **Step 3: Rewrite `web_network_section()`**

Restructure the function to build items into per-subsection vecs, then assemble only non-empty subsections. Each item's `ContextItem` construction stays identical — only the container changes from flat `items` to subsection vecs.

```rust
pub fn web_network_section(data: &RefNetwork) -> ReferenceSection {
    let mut subsections = Vec::new();

    // --- Connections ---
    let conn_items: Vec<ContextItem> = data.connections.iter().map(|conn| {
        ContextItem {
            id: conn.name.clone(),
            title: conn.name.clone(),
            subtitle: Some(format!("{} ({})", conn.conn_type, conn.method)),
            detail: None,
            searchable_text: format!("{} {} {} {}", conn.name, conn.conn_type, conn.method, conn.path),
        }
    }).collect();
    if !conn_items.is_empty() {
        subsections.push(ContextSubsection {
            id: "connections".to_string(),
            display_name: "Connections".to_string(),
            items: conn_items,
        });
    }

    // --- Firewall (zones + direct rules) ---
    let mut fw_items = Vec::new();
    // [zone items — same ContextItem construction as current code]
    // [direct rule items — same ContextItem construction as current code]
    if !fw_items.is_empty() {
        subsections.push(ContextSubsection {
            id: "firewall".to_string(),
            display_name: "Firewall".to_string(),
            items: fw_items,
        });
    }

    // --- Routes & Rules ---
    let mut route_items = Vec::new();
    // [static_routes, ip_routes, ip_rules — same construction]
    if !route_items.is_empty() {
        subsections.push(ContextSubsection {
            id: "routes_rules".to_string(),
            display_name: "Routes & Rules".to_string(),
            items: route_items,
        });
    }

    // --- DNS & Hosts ---
    let mut dns_items = Vec::new();
    // [resolv_provenance, hosts_additions — same construction]
    if !dns_items.is_empty() {
        subsections.push(ContextSubsection {
            id: "dns_hosts".to_string(),
            display_name: "DNS & Hosts".to_string(),
            items: dns_items,
        });
    }

    // --- Proxy ---
    let mut proxy_items = Vec::new();
    // [proxy_env — same construction]
    if !proxy_items.is_empty() {
        subsections.push(ContextSubsection {
            id: "proxy".to_string(),
            display_name: "Proxy".to_string(),
            items: proxy_items,
        });
    }

    ReferenceSection {
        id: "network".to_string(),
        display_name: "Network".to_string(),
        items: Vec::new(), // top-level empty; sidebar fix handles counts
        subsections,
        empty_reason: None,
    }
}
```

**Critical:** Copy each ContextItem construction verbatim from the current code. Do not change field mappings, subtitles, detail content, or searchable_text — only the container structure changes.

- [ ] **Step 4: Run tests**

Run: `cargo test -p inspectah-web`

Expected: PASS. Also run `cargo clippy -p inspectah-web -- -W clippy::all`.

- [ ] **Step 5: Commit**

```bash
git add crates/web/src/adapter.rs
git commit -m "refactor(refine): group network section into subsections by type"
```

---

### Task 4: Kernel & Boot adapter — customizations vs defaults

**Owner:** Tang (Rust)

**Files:**
- Modify: `crates/web/src/adapter.rs` — `web_kernel_boot_section()`
- Test: inline `#[cfg(test)]` in `adapter.rs` or `crates/web/tests/`

**Context:** Same pattern as Task 3. Split the flat list into two subsections:
- **Customizations:** tuned profile, sysctl overrides, non-default kernel modules, modules-load.d, modprobe.d, dracut.conf.d, custom tuned profiles
- **Defaults / Context:** cmdline, GRUB defaults, locale, timezone, alternatives

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn web_kernel_boot_section_splits_customizations_and_defaults() {
    // Construct RefKernelBoot with:
    //   tuned_active = Some("throughput-performance")
    //   sysctl_overrides = [one override with key="vm.swappiness"]
    //   cmdline = Some("BOOT_IMAGE=...")
    //   locale = Some("en_US.UTF-8")
    let data = RefKernelBoot { /* populate fields */ };
    let section = web_kernel_boot_section(&data);

    assert!(section.items.is_empty(), "top-level items must be empty");
    assert_eq!(section.subsections.len(), 2);
    assert_eq!(section.subsections[0].id, "customizations");
    assert_eq!(section.subsections[0].display_name, "Customizations");
    // Verify tuned and sysctl landed in customizations
    assert!(section.subsections[0].items.iter()
        .any(|i| i.title == "Active tuned profile"));
    assert!(section.subsections[0].items.iter()
        .any(|i| i.title == "vm.swappiness"));

    assert_eq!(section.subsections[1].id, "defaults_context");
    assert_eq!(section.subsections[1].display_name, "Defaults / Context");
    // Verify cmdline and locale landed in defaults
    assert!(section.subsections[1].items.iter()
        .any(|i| i.title == "Kernel cmdline"));
    assert!(section.subsections[1].items.iter()
        .any(|i| i.title == "Locale"));
}

#[test]
fn web_kernel_boot_section_omits_empty_customizations() {
    // Construct RefKernelBoot with only cmdline and locale (no customizations).
    let data = RefKernelBoot {
        cmdline: Some("BOOT_IMAGE=...".to_string()),
        locale: Some("en_US.UTF-8".to_string()),
        ..Default::default()
    };
    let section = web_kernel_boot_section(&data);
    assert_eq!(section.subsections.len(), 1, "only non-empty subsections");
    assert_eq!(section.subsections[0].id, "defaults_context");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p inspectah-web -- web_kernel_boot`

- [ ] **Step 3: Rewrite `web_kernel_boot_section()`**

Same pattern as Task 3. Build two vecs (`customization_items`, `default_items`), push each `ContextItem` to the appropriate vec, assemble non-empty subsections.

**Customizations vec receives:** tuned_active, sysctl_overrides, non_default_modules, modules_load_d snippets, modprobe_d snippets, dracut_conf snippets, custom_tuned_profiles.

**Defaults vec receives:** cmdline, grub_defaults, locale, timezone, alternatives.

Each `ContextItem` construction stays identical to the current code.

- [ ] **Step 4: Run tests**

Run: `cargo test -p inspectah-web && cargo clippy -p inspectah-web -- -W clippy::all`

- [ ] **Step 5: Commit**

```bash
git add crates/web/src/adapter.rs
git commit -m "refactor(refine): split kernel & boot into customizations vs defaults"
```

---

### Task 5: EVR formatting utility

**Owner:** Kit (frontend)

**Files:**
- Create: `crates/web/ui/src/components/evrFormat.ts`
- Test: `crates/web/ui/src/components/__tests__/evrFormat.test.ts`

**Context:** The `VersionChangesTable` needs to format epoch:version strings using the same pairwise logic as the Rust adapter's `format_evr_pair()`. Extract this as a pure utility function so the table component stays clean.

**Pairwise rule (from `format_evr_pair()` in `adapter.rs`):**
1. Normalize: treat empty epoch as `"0"`.
2. `show_epoch = (base_norm !== host_norm) || (base_norm !== "0")` — show epoch on BOTH sides when either side has a meaningful (non-`"0"`) epoch, or when epochs differ.
3. When showing epoch: display `"epoch:version"`, substituting `"0"` for empty epoch.
4. When not showing epoch: display version alone.

- [ ] **Step 1: Write the failing test**

```typescript
// crates/web/ui/src/components/__tests__/evrFormat.test.ts
import { formatEvrPair } from "../evrFormat";

describe("formatEvrPair", () => {
  it("omits epoch when both sides are 0 or empty", () => {
    expect(formatEvrPair("", "2.4.51", "", "2.4.57")).toEqual(["2.4.51", "2.4.57"]);
    expect(formatEvrPair("0", "2.4.51", "0", "2.4.57")).toEqual(["2.4.51", "2.4.57"]);
  });

  it("shows epoch on both sides when either has non-zero epoch", () => {
    expect(formatEvrPair("1", "2.4.51", "0", "2.4.57")).toEqual(["1:2.4.51", "0:2.4.57"]);
    expect(formatEvrPair("0", "2.4.51", "1", "2.4.57")).toEqual(["0:2.4.51", "1:2.4.57"]);
  });

  it("shows epoch on both sides when epochs differ even if both non-zero", () => {
    expect(formatEvrPair("1", "2.4.51", "2", "2.4.57")).toEqual(["1:2.4.51", "2:2.4.57"]);
  });

  it("shows epoch on both sides when one is non-zero and other is empty", () => {
    expect(formatEvrPair("1", "2.4.51", "", "2.4.57")).toEqual(["1:2.4.51", "0:2.4.57"]);
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd crates/web/ui && npx vitest run src/components/__tests__/evrFormat.test.ts`

Expected: FAIL — module does not exist.

- [ ] **Step 3: Implement**

```typescript
// crates/web/ui/src/components/evrFormat.ts

/** Pairwise EVR formatting matching the Rust adapter's format_evr_pair(). */
export function formatEvrPair(
  baseEpoch: string,
  baseVersion: string,
  hostEpoch: string,
  hostVersion: string,
): [string, string] {
  const norm = (e: string) => (e === "" ? "0" : e);
  const baseNorm = norm(baseEpoch);
  const hostNorm = norm(hostEpoch);
  const showEpoch = baseNorm !== hostNorm || baseNorm !== "0";

  const fmt = (epoch: string, version: string) => {
    if (showEpoch) {
      const e = epoch === "" ? "0" : epoch;
      return `${e}:${version}`;
    }
    return version;
  };

  return [fmt(baseEpoch, baseVersion), fmt(hostEpoch, hostVersion)];
}
```

- [ ] **Step 4: Run tests**

Run: `cd crates/web/ui && npx vitest run src/components/__tests__/evrFormat.test.ts`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/web/ui/src/components/evrFormat.ts \
       crates/web/ui/src/components/__tests__/evrFormat.test.ts
git commit -m "feat(refine): add pairwise EVR formatting utility"
```

---

### Task 6: VersionChangesTable component

**Owner:** Kit (frontend)

**Files:**
- Create: `crates/web/ui/src/components/VersionChangesTable.tsx`
- Test: `crates/web/ui/src/components/__tests__/VersionChangesTable.test.tsx`

**Context:** New component that renders version changes as a grouped table. Reads `VersionChangeEntry[]` from `viewData.version_changes`. Receives `empty_reason` from the reference section for empty state rendering.

**Dependencies:** Task 5 (EVR utility). `@patternfly/react-table` (PatternFly 6 compatible — the project already uses `@patternfly/react-core` v6) must be installed — check `package.json` first; if absent, run `npm install @patternfly/react-table` in `crates/web/ui/`.

**Files (exhaustive):**
- Create: `crates/web/ui/src/components/VersionChangesTable.tsx`
- Test: `crates/web/ui/src/components/__tests__/VersionChangesTable.test.tsx`
- Modify: `crates/web/ui/package.json` (if adding dependency)
- Modify: `crates/web/ui/package-lock.json` (if adding dependency)
- Modify: `crates/web/ui/src/App.css` (or relevant CSS file — add table styles)

- [ ] **Step 1: Check and install PatternFly table dependency**

```bash
cd crates/web/ui
grep react-table package.json || npm install @patternfly/react-table
```

- [ ] **Step 2: Write the failing tests**

```typescript
// crates/web/ui/src/components/__tests__/VersionChangesTable.test.tsx
import { render, screen } from "@testing-library/react";
import { VersionChangesTable } from "../VersionChangesTable";
import type { VersionChangeEntry } from "../../api/types";

const downgrade: VersionChangeEntry = {
  name: "httpd", arch: "x86_64",
  host_version: "2.4.57", base_version: "2.4.51",
  host_epoch: "", base_epoch: "",
  direction: "downgrade",
};
const upgrade: VersionChangeEntry = {
  name: "podman", arch: "x86_64",
  host_version: "4.6.1", base_version: "4.9.0",
  host_epoch: "", base_epoch: "",
  direction: "upgrade",
};

describe("VersionChangesTable", () => {
  it("renders downgrades before upgrades", () => {
    render(<VersionChangesTable entries={[upgrade, downgrade]} />);
    const rows = screen.getAllByTestId(/^context-item-/);
    expect(rows[0]).toHaveAttribute("data-testid", "context-item-httpd.x86_64");
    expect(rows[1]).toHaveAttribute("data-testid", "context-item-podman.x86_64");
  });

  it("shows group headers with counts", () => {
    render(<VersionChangesTable entries={[downgrade, upgrade]} />);
    expect(screen.getByText(/Downgrades \(1\)/)).toBeInTheDocument();
    expect(screen.getByText(/Upgrades \(1\)/)).toBeInTheDocument();
  });

  it("omits empty groups", () => {
    render(<VersionChangesTable entries={[upgrade]} />);
    expect(screen.queryByText(/Downgrades/)).not.toBeInTheDocument();
    expect(screen.getByText(/Upgrades \(1\)/)).toBeInTheDocument();
  });

  it("renders data_unavailable empty state", () => {
    render(<VersionChangesTable entries={[]} emptyReason="data_unavailable" />);
    expect(screen.getByText(/not available/i)).toBeInTheDocument();
  });

  it("renders zero_drift empty state", () => {
    render(<VersionChangesTable entries={[]} emptyReason="zero_drift" />);
    expect(screen.getByText(/match the target baseline/i)).toBeInTheDocument();
  });

  it("renders default empty state when no reason", () => {
    render(<VersionChangesTable entries={[]} />);
    expect(screen.getByText(/No Version Changes/i)).toBeInTheDocument();
  });

  it("applies pairwise EVR formatting", () => {
    const epochEntry: VersionChangeEntry = {
      name: "pkg", arch: "x86_64",
      host_version: "1.0", base_version: "2.0",
      host_epoch: "1", base_epoch: "0",
      direction: "downgrade",
    };
    render(<VersionChangesTable entries={[epochEntry]} />);
    expect(screen.getByText("1:1.0")).toBeInTheDocument();
    expect(screen.getByText("0:2.0")).toBeInTheDocument();
  });

  it("data rows have focusable context-item testids", () => {
    render(<VersionChangesTable entries={[downgrade]} />);
    const row = screen.getByTestId("context-item-httpd.x86_64");
    expect(row).toHaveAttribute("tabindex", "-1");
  });
});
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cd crates/web/ui && npx vitest run src/components/__tests__/VersionChangesTable.test.tsx`

Expected: FAIL — module does not exist.

- [ ] **Step 4: Implement the component**

Create `crates/web/ui/src/components/VersionChangesTable.tsx`:

```tsx
import { EmptyState, EmptyStateBody } from "@patternfly/react-core";
import { CubesIcon } from "@patternfly/react-icons";
import { Table, Thead, Tbody, Tr, Th, Td } from "@patternfly/react-table";
import type { VersionChangeEntry } from "../api/types";
import { formatEvrPair } from "./evrFormat";

export interface VersionChangesTableProps {
  entries: VersionChangeEntry[];
  emptyReason?: string | null;
  revealItemId?: string;
}

export function VersionChangesTable({
  entries,
  emptyReason,
  revealItemId,
}: VersionChangesTableProps) {
  if (entries.length === 0) {
    const copyMap: Record<string, string> = {
      zero_drift: "All packages match the target baseline versions.",
      data_unavailable: "Version change data is not available for this snapshot.",
    };
    const title = emptyReason && copyMap[emptyReason]
      ? copyMap[emptyReason]
      : "No Version Changes data in this snapshot";
    return <EmptyState titleText={title} icon={CubesIcon} headingLevel="h3" />;
  }

  const downgrades = entries.filter((e) => e.direction === "downgrade");
  const upgrades = entries.filter((e) => e.direction === "upgrade");

  const renderGroup = (
    label: string,
    items: VersionChangeEntry[],
    variant: "danger" | "success",
  ) => {
    if (items.length === 0) return null;
    const arrow = variant === "danger" ? "▼" : "▲";
    return (
      <Tbody>
        <Tr
          aria-label={`${items.length} ${label.toLowerCase()}`}
          className={`inspectah-vc-group-header inspectah-vc-group-header--${variant}`}
        >
          <Td colSpan={3} className="inspectah-vc-group-header__cell">
            {arrow} {label} ({items.length})
          </Td>
        </Tr>
        {items.map((vc) => {
          const id = `${vc.name}.${vc.arch}`;
          const [baseFmt, hostFmt] = formatEvrPair(
            vc.base_epoch, vc.base_version,
            vc.host_epoch, vc.host_version,
          );
          const isRevealed = revealItemId === id;
          return (
            <Tr
              key={id}
              data-testid={`context-item-${id}`}
              tabIndex={-1}
              className={isRevealed ? "inspectah-vc-row--revealed" : undefined}
            >
              <Td dataLabel="Package">{vc.name}.{vc.arch}</Td>
              <Td dataLabel="Host Version" className="inspectah-vc-version">
                {hostFmt}
              </Td>
              <Td dataLabel="Target Version" className="inspectah-vc-version">
                {baseFmt}
              </Td>
            </Tr>
          );
        })}
      </Tbody>
    );
  };

  return (
    <Table variant="compact" aria-label="Version changes">
      <Thead>
        <Tr>
          <Th>Package</Th>
          <Th>Host Version</Th>
          <Th>Target Version</Th>
        </Tr>
      </Thead>
      {renderGroup("Downgrades", downgrades, "danger")}
      {renderGroup("Upgrades", upgrades, "success")}
    </Table>
  );
}
```

Add CSS for the group headers and version cells. Create or append to the appropriate CSS file used by the refine UI:

```css
.inspectah-vc-group-header--danger .inspectah-vc-group-header__cell {
  font-weight: 600;
  font-size: var(--pf-t--global--font--size--xs);
  text-transform: uppercase;
  letter-spacing: 0.5px;
  color: var(--pf-t--global--color--status--danger--default);
  border-bottom: 2px solid var(--pf-t--global--color--status--danger--default);
}
.inspectah-vc-group-header--success .inspectah-vc-group-header__cell {
  font-weight: 600;
  font-size: var(--pf-t--global--font--size--xs);
  text-transform: uppercase;
  letter-spacing: 0.5px;
  color: var(--pf-t--global--color--status--success--default);
  border-bottom: 2px solid var(--pf-t--global--color--status--success--default);
}
.inspectah-vc-version {
  font-family: var(--pf-t--global--font--family--mono);
}
.inspectah-vc-row--revealed {
  background: var(--pf-t--global--background--color--primary--default);
}
```

- [ ] **Step 5: Run tests**

Run: `cd crates/web/ui && npx vitest run src/components/__tests__/VersionChangesTable.test.tsx`

Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/web/ui/src/components/VersionChangesTable.tsx \
       crates/web/ui/src/components/__tests__/VersionChangesTable.test.tsx \
       crates/web/ui/src/App.css \
       crates/web/ui/package.json \
       crates/web/ui/package-lock.json
git commit -m "feat(refine): add VersionChangesTable grouped table component"
```

---

### Task 7: MainContent integration + App.tsx focus fix

**Owner:** Kit (frontend)

**Files:**
- Modify: `crates/web/ui/src/components/MainContent.tsx`
- Modify: `crates/web/ui/src/App.tsx`
- Test: existing tests + integration assertions

**Context:** Wire `VersionChangesTable` into `MainContent.tsx` replacing `ContextList` for the `version_changes` section. Update `App.tsx` section-entry focus to target `[data-testid^="context-item-"]` instead of `[role="row"]` for the version_changes section.

- [ ] **Step 1: Write the failing tests**

Add tests for MainContent integration and App-level focus contract:

```typescript
// MainContent integration test (add to existing MainContent tests)
it("renders VersionChangesTable instead of ContextList for version_changes", () => {
  // Render MainContent with activeSection="version_changes"
  // and viewData containing version_changes entries.
  // Assert: VersionChangesTable renders (e.g., table with group header)
  // Assert: ContextList does NOT render for this section
});

// App-level focus contract tests (add to App.test.tsx or new file)
it("plain section entry focuses first data row, not group header", () => {
  // Render App with version_changes active, no revealItemId.
  // Assert: document.activeElement has data-testid
  //   matching "context-item-{first-downgrade-name}.{arch}"
  // Assert: document.activeElement does NOT have class
  //   "inspectah-vc-group-header"
});

it("reveal navigation focuses the targeted data row", () => {
  // Render App with version_changes active and
  //   revealItemId="podman.x86_64"
  // Assert: element with data-testid="context-item-podman.x86_64"
  //   has focus or is scrolled into view
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd crates/web/ui && npx vitest run src/components/__tests__/MainContent.test.tsx`

- [ ] **Step 3: Update MainContent.tsx**

In the `version_changes` section case, replace the `ContextList` rendering:

```tsx
if (activeSection === "version_changes") {
  const section = sections?.find((s) => s.id === "version_changes");
  if (!section) {
    return <p>Section data not available.</p>;
  }
  const emptyReason = section.empty_reason ?? undefined;

  return (
    <>
      <Content>
        <h2>{SECTION_LABELS.version_changes}</h2>
      </Content>
      <VersionChangesTable
        entries={viewData?.version_changes ?? []}
        emptyReason={emptyReason}
        revealItemId={revealItemId}
      />
    </>
  );
}
```

Add the import: `import { VersionChangesTable } from "./VersionChangesTable";`

- [ ] **Step 4: Update App.tsx focus query**

In the `useEffect` that focuses the first row on section entry, update the query order. Currently:

```typescript
const firstRow = container.querySelector('[role="row"]') as HTMLElement | null;
if (firstRow) { firstRow.focus(); return; }
const firstContextItem = container.querySelector('[data-testid^="context-item-"]') ...
```

For the `version_changes` section only, try `[data-testid^="context-item-"]` before `[role="row"]`. Other sections keep existing behavior:

```typescript
// Scoped to version_changes per the approved spec
const preferContextItem = activeSection === "version_changes";

if (preferContextItem) {
  const firstContextItem = container.querySelector(
    '[data-testid^="context-item-"]',
  ) as HTMLElement | null;
  if (firstContextItem) {
    firstContextItem.focus();
    return;
  }
}

const firstRow = container.querySelector(
  '[role="row"]',
) as HTMLElement | null;
if (firstRow) {
  firstRow.focus();
  return;
}

if (!preferContextItem) {
  const firstContextItem = container.querySelector(
    '[data-testid^="context-item-"]',
  ) as HTMLElement | null;
  if (firstContextItem) {
    firstContextItem.focus();
    return;
  }
}
```

This is scoped to `version_changes` only, matching the spec. Other sections are unaffected. The `activeSection` variable is already in scope in the `useEffect`.

- [ ] **Step 5: Run full test suite**

Run: `cd crates/web/ui && npx vitest run`

Expected: All tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/web/ui/src/components/MainContent.tsx \
       crates/web/ui/src/App.tsx \
       crates/web/ui/src/components/__tests__/
git commit -m "feat(refine): wire VersionChangesTable into MainContent, fix section-entry focus"
```

---

### Task 8: Contract snapshots + final verification

**Owner:** Tang (Rust) and Kit (frontend) — split as needed

**Files:**
- Update: `crates/web/tests/snapshots/` — any contract snapshots affected by subsection structure changes
- Verify: full workspace test suite

- [ ] **Step 1: Update `api_test.rs` assertions for subsection structure**

`crates/web/tests/api_test.rs` contains flat-list assertions for `network` and `kernel_boot` sections that check `items` counts and field values. These will fail because items have moved to subsections. Update the assertions to:
- Check `items` is empty for `network` and `kernel_boot` sections
- Check `subsections` array has the expected structure
- Verify representative items exist in the correct subsections

Read the failing tests first to understand the exact assertions, then update them to match the new subsection structure.

- [ ] **Step 2: Run full Rust test suite**

Run: `cargo test --workspace`

Fix any snapshot mismatches with `cargo insta review` or by updating expected values. The networking and kernel/boot reference sections now have subsections instead of flat items — contract snapshots must reflect this.

- [ ] **Step 2: Run full frontend test suite**

Run: `cd crates/web/ui && npx vitest run`

Fix any failures.

- [ ] **Step 3: Run clippy across workspace**

Run: `cargo clippy --workspace -- -W clippy::all`

Expected: zero warnings.

- [ ] **Step 4: Run TypeScript type check**

Run: `cd crates/web/ui && npx tsc --noEmit`

Expected: zero errors.

- [ ] **Step 5: Commit any snapshot updates**

```
git commit -m "test(refine): update contract snapshots for subsection structure"
```

- [ ] **Step 6: Final behavior verification**

Run both test suites one more time as a gate:

```bash
cargo test --workspace 2>&1 | tail -5
cd crates/web/ui && npx vitest run 2>&1 | tail -5
```

Expected: zero failures in both. If any fail, diagnose and fix before marking complete.
