import { useEffect, useRef, useState } from "react";
import { Button, ExpandableSection, Label } from "@patternfly/react-core";
import type { NotCarriedRow } from "../api/types";

/** DOM id of the group, the target of the Export dialog's review link. */
export const NOT_CARRIED_GROUP_ID = "not-carried-group";

export interface NotCarriedGroupProps {
  /** `not_carried_packages` from the view, rendered in array order. */
  rows: NotCarriedRow[];
  /** One-click include: the caller sends `SetInclude` with `row.item_id`. */
  onInclude: (row: NotCarriedRow) => void;
  isPending?: boolean;
  /** Rows (by `notCarriedRowKey`) whose Include request is in flight. */
  pendingKeys?: ReadonlySet<string>;
  /** When true, expand, scroll into view and focus the toggle, once. */
  reveal?: boolean;
  /** Called after a reveal was handled, so the caller can clear it. */
  onRevealed?: () => void;
}

const NO_PENDING: ReadonlySet<string> = new Set();

/** Stable row identity: `name.arch`, as in the package list. */
export function notCarriedRowKey(row: NotCarriedRow): string {
  return `${row.item_id.key.name}.${row.item_id.key.arch}`;
}

/**
 * Auto packages the export leaves out (the Containerfile's `# Not carried`
 * block), each with a one-click include. The server orders the rows (unit
 * owners first) and decides which rows own an enabled unit; this component
 * re-derives neither. An empty list renders nothing, because the payload
 * cannot tell "nothing listed" from "the classifier fell back".
 */
export function NotCarriedGroup({
  rows,
  onInclude,
  isPending = false,
  pendingKeys = NO_PENDING,
  reveal = false,
  onRevealed,
}: NotCarriedGroupProps) {
  const [expanded, setExpanded] = useState(false);
  const sectionRef = useRef<HTMLElement>(null);

  useEffect(() => {
    if (!reveal) return;
    setExpanded(true);
    onRevealed?.();
    // Two frames: the section change that brought the packages view up
    // focuses the view's first row in the next frame (App.tsx), which would
    // scroll this group back out of sight if it ran after us.
    requestAnimationFrame(() =>
      requestAnimationFrame(() => {
        const section = sectionRef.current;
        if (!section) return;
        section.scrollIntoView({ behavior: "smooth", block: "start" });
        section.querySelector("button")?.focus({ preventScroll: true });
      }),
    );
  }, [reveal, onRevealed]);

  if (rows.length === 0) return null;

  const owners = rows.filter((r) => r.owns_enabled_unit).length;
  const toggleText =
    owners > 0
      ? `Not carried (${rows.length}) · ${owners} own an enabled unit`
      : `Not carried (${rows.length})`;

  return (
    <section
      id={NOT_CARRIED_GROUP_ID}
      ref={sectionRef}
      data-testid="not-carried-group"
      className="inspectah-not-carried"
    >
      <ExpandableSection
        toggleText={toggleText}
        isExpanded={expanded}
        onToggle={(_e, isExpanded) => setExpanded(isExpanded)}
      >
        <p
          className="inspectah-not-carried__help"
          data-testid="not-carried-help"
        >
          Auto-installed packages the export leaves out, listed in the
          Containerfile's <code># Not carried</code> block. Most are packages
          nothing carried in the image needs. A package that owns an enabled
          unit on the source host is carried by default, so it is listed here
          only if you excluded it or it is held back: it may need a package the
          image cannot install, such as one from a repo disabled on the host.
          The Containerfile gives each held package's reason. Including a
          package adds it to the install list, lets dnf resolve its dependencies
          and restores its enabled units. For held packages, resolve the reason
          shown in the Containerfile first.
        </p>
        <ul className="inspectah-not-carried__list">
          {rows.map((row) => {
            const key = notCarriedRowKey(row);
            return (
              <li
                key={key}
                data-testid={`not-carried-row-${key}`}
                className={
                  row.owns_enabled_unit
                    ? "inspectah-not-carried__row inspectah-not-carried__row--owner"
                    : "inspectah-not-carried__row"
                }
              >
                <div className="inspectah-not-carried__main">
                  <span className="inspectah-not-carried__name">
                    {row.display_name}
                  </span>
                  <span className="inspectah-not-carried__repo">
                    {row.source_repo}
                  </span>
                  {row.owns_enabled_unit && (
                    <span className="inspectah-not-carried__units">
                      <Label
                        color="orange"
                        isCompact
                        data-testid="not-carried-owner-label"
                      >
                        Enabled on source host
                      </Label>
                      {row.enabled_units.map((unit) => (
                        <code key={unit}>{unit}</code>
                      ))}
                    </span>
                  )}
                </div>
                <Button
                  className="inspectah-not-carried__include"
                  variant="secondary"
                  size="sm"
                  aria-label={`Include ${row.display_name}`}
                  isDisabled={isPending || pendingKeys.has(key)}
                  onClick={() => onInclude(row)}
                >
                  Include
                </Button>
              </li>
            );
          })}
        </ul>
      </ExpandableSection>
    </section>
  );
}
