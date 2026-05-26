import { useState, useMemo, useCallback, useRef, useEffect, forwardRef, useImperativeHandle } from "react";
import { SearchInput } from "@patternfly/react-core";
import type { DecisionItemKind } from "./DecisionItem";
import { itemId as getItemId } from "./DecisionItem";
import type { ContextSection, UserDecision } from "../api/types";

export interface GlobalSearchResult {
  /** Section ID (e.g. "packages", "configs", "services"). */
  sectionId: string;
  /** Human-readable section label. */
  sectionLabel: string;
  /** Display title of the item. */
  title: string;
  /** Unique item ID within the section. */
  itemId: string;
}

/** Section ID to human-readable label. */
const SECTION_LABELS: Record<string, string> = {
  packages: "Packages",
  configs: "Config Files",
  services: "Services",
  containers: "Containers",
  compose: "Compose",
  users_groups: "Users & Groups",
  network: "Network",
  storage: "Storage",
  scheduled_tasks: "Scheduled Tasks",
  non_rpm_software: "Non-RPM Software",
  kernel_boot: "Kernel & Boot",
  selinux: "Security & Access Control",
  system_tuning: "System Tuning",
};

export interface GlobalSearchProps {
  /** All decision items from packages section. */
  packageItems: DecisionItemKind[];
  /** All decision items from configs section. */
  configItems: DecisionItemKind[];
  /** User decisions for the users_groups section. */
  userDecisions?: UserDecision[];
  /** Context sections (services, containers, etc.). */
  contextSections: ContextSection[] | null;
  /** Called when user selects a result -- navigates to section + item. */
  onNavigate: (sectionId: string, itemId: string) => void;
}

/** Imperative handle exposed by GlobalSearch for focusing the input. */
export interface GlobalSearchHandle {
  focus: () => void;
}

/** Set of section IDs that match a search query. Empty set means no active filter. */
export type MatchingSections = Set<string>;

function itemName(item: DecisionItemKind): string {
  if (item.type === "package") {
    return `${item.data.entry.name}.${item.data.entry.arch}`;
  }
  return item.data.entry.path;
}

function itemSearchText(item: DecisionItemKind): string {
  if (item.type === "package") {
    const e = item.data.entry;
    const reasons = (item.data.attention ?? []).map((a) => a.detail ?? "").join(" ");
    return `${e.name} ${e.arch} ${e.version} ${e.source_repo} ${reasons}`.toLowerCase();
  }
  const e = item.data.entry;
  const reasons = (item.data.attention ?? []).map((a) => a.detail ?? "").join(" ");
  return `${e.path} ${e.kind} ${e.category} ${e.package ?? ""} ${reasons}`.toLowerCase();
}

/**
 * Sidebar-integrated global search.
 * Always-visible search input at the top of the sidebar. Typing filters
 * sections inline -- matching sections are highlighted, non-matching hidden.
 * Ctrl+K focuses this input.
 */
export const GlobalSearch = forwardRef<GlobalSearchHandle, GlobalSearchProps>(
  function GlobalSearch({ packageItems, configItems, userDecisions, contextSections, onNavigate }, ref) {
    const [query, setQuery] = useState("");
    const [selectedIndex, setSelectedIndex] = useState(0);
    const inputRef = useRef<HTMLInputElement>(null);
    const resultsRef = useRef<HTMLDivElement>(null);

    useImperativeHandle(ref, () => ({
      focus: () => inputRef.current?.focus(),
    }));

    // Build searchable index of all items
    const allItems = useMemo((): GlobalSearchResult[] => {
      const results: GlobalSearchResult[] = [];

      for (const item of packageItems) {
        results.push({
          sectionId: "packages",
          sectionLabel: SECTION_LABELS.packages,
          title: itemName(item),
          itemId: getItemId(item),
        });
      }

      for (const item of configItems) {
        results.push({
          sectionId: "configs",
          sectionLabel: SECTION_LABELS.configs,
          title: itemName(item),
          itemId: getItemId(item),
        });
      }

      if (userDecisions) {
        for (const user of userDecisions) {
          results.push({
            sectionId: "users_groups",
            sectionLabel: SECTION_LABELS.users_groups,
            title: user.name,
            itemId: `users:${user.name}`,
          });
        }
      }

      if (contextSections) {
        for (const section of contextSections) {
          const label = SECTION_LABELS[section.id] ?? section.display_name;
          for (const ci of section.items) {
            results.push({
              sectionId: section.id,
              sectionLabel: label,
              title: ci.title,
              itemId: ci.id,
            });
          }
          // Index subsection items so they are searchable too.
          if (section.subsections) {
            for (const sub of section.subsections) {
              for (const ci of sub.items) {
                results.push({
                  sectionId: section.id,
                  sectionLabel: label,
                  title: ci.title,
                  itemId: ci.id,
                });
              }
            }
          }
        }
      }

      return results;
    }, [packageItems, configItems, userDecisions, contextSections]);

    // Build searchable text keyed by a composite of sectionId + itemId.
    // Context section items use bare IDs (e.g. "/" for a mount point) that
    // can collide across sections, so the section prefix is required to
    // keep each entry's search text distinct.
    const searchTextMap = useMemo(() => {
      const map = new Map<string, string>();
      for (const item of packageItems) {
        map.set(getItemId(item), itemSearchText(item));
      }
      for (const item of configItems) {
        map.set(getItemId(item), itemSearchText(item));
      }
      if (userDecisions) {
        for (const user of userDecisions) {
          map.set(
            `users:${user.name}`,
            `${user.name} ${user.uid} ${user.classification} ${user.shell} ${user.home}`.toLowerCase(),
          );
        }
      }
      if (contextSections) {
        for (const section of contextSections) {
          for (const ci of section.items) {
            map.set(`${section.id}:${ci.id}`, ci.searchable_text.toLowerCase());
          }
          if (section.subsections) {
            for (const sub of section.subsections) {
              for (const ci of sub.items) {
                map.set(`${section.id}:${ci.id}`, ci.searchable_text.toLowerCase());
              }
            }
          }
        }
      }
      return map;
    }, [packageItems, configItems, userDecisions, contextSections]);

    // Filter results
    const filtered = useMemo(() => {
      if (!query.trim()) return [];
      const q = query.toLowerCase();
      return allItems
        .filter((r) => {
          // Decision items are keyed by bare itemId; context items use a
          // "sectionId:itemId" composite key to avoid cross-section collisions.
          const text =
            searchTextMap.get(r.itemId) ??
            searchTextMap.get(`${r.sectionId}:${r.itemId}`) ??
            r.title.toLowerCase();
          return text.includes(q) || r.title.toLowerCase().includes(q);
        })
        .slice(0, 50); // Cap at 50 results
    }, [allItems, searchTextMap, query]);

    // Clamp selected index
    useEffect(() => {
      if (selectedIndex >= filtered.length) {
        setSelectedIndex(Math.max(0, filtered.length - 1));
      }
    }, [filtered.length, selectedIndex]);

    const handleSelect = useCallback(
      (result: GlobalSearchResult) => {
        onNavigate(result.sectionId, result.itemId);
        setQuery("");
      },
      [onNavigate],
    );

    const handleKeyDown = useCallback(
      (e: React.KeyboardEvent) => {
        if (e.key === "ArrowDown") {
          e.preventDefault();
          setSelectedIndex((prev) => Math.min(prev + 1, filtered.length - 1));
          return;
        }
        if (e.key === "ArrowUp") {
          e.preventDefault();
          setSelectedIndex((prev) => Math.max(prev - 1, 0));
          return;
        }
        if (e.key === "Enter" && filtered.length > 0) {
          e.preventDefault();
          handleSelect(filtered[selectedIndex]);
          return;
        }
        if (e.key === "Escape") {
          e.preventDefault();
          setQuery("");
          inputRef.current?.blur();
          return;
        }
      },
      [filtered, selectedIndex, handleSelect],
    );

    // Scroll selected item into view
    useEffect(() => {
      if (resultsRef.current) {
        const selected = resultsRef.current.querySelector(
          `[data-result-index="${selectedIndex}"]`,
        );
        if (typeof selected?.scrollIntoView === "function") {
          selected.scrollIntoView({ block: "nearest" });
        }
      }
    }, [selectedIndex]);

    return (
      <div className="inspectah-sidebar__search" data-testid="sidebar-search">
        <SearchInput
          ref={inputRef}
          placeholder="Search all sections..."
          value={query}
          onChange={(_e, val) => {
            setQuery(val);
            setSelectedIndex(0);
          }}
          onClear={() => setQuery("")}
          onKeyDown={handleKeyDown}
          aria-label="Search all sections"
          data-testid="global-search-input"
        />
        {query.trim() && (
          <div
            ref={resultsRef}
            role="listbox"
            aria-label="Search results"
            data-testid="global-search-results"
            style={{
              marginTop: "var(--pf-t--global--spacer--xs)",
              maxHeight: 240,
              overflowY: "auto",
            }}
          >
            {filtered.length === 0 ? (
              <div
                style={{
                  padding: "var(--pf-t--global--spacer--sm)",
                  textAlign: "center",
                  color: "var(--pf-t--global--color--200)",
                  fontSize: "var(--pf-t--global--font--size--sm)",
                }}
              >
                No results found
              </div>
            ) : (
              filtered.map((result, idx) => (
                <div
                  key={`${result.sectionId}:${result.itemId}`}
                  role="option"
                  aria-selected={idx === selectedIndex}
                  data-result-index={idx}
                  data-testid={`global-search-result-${idx}`}
                  onClick={() => handleSelect(result)}
                  style={{
                    padding: "var(--pf-t--global--spacer--xs) var(--pf-t--global--spacer--sm)",
                    cursor: "pointer",
                    background:
                      idx === selectedIndex
                        ? "var(--pf-t--global--background--color--primary--hover)"
                        : "transparent",
                    color: "inherit",
                    borderRadius: "var(--pf-t--global--border--radius--small)",
                    display: "flex",
                    alignItems: "center",
                    gap: "var(--pf-t--global--spacer--sm)",
                    fontSize: "var(--pf-t--global--font--size--sm)",
                  }}
                >
                  <span
                    style={{
                      fontSize: "var(--pf-t--global--font--size--xs)",
                      opacity: 0.7,
                      minWidth: 70,
                    }}
                  >
                    {result.sectionLabel}
                  </span>
                  <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                    {result.title}
                  </span>
                </div>
              ))
            )}
          </div>
        )}
      </div>
    );
  },
);
