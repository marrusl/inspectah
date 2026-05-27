import { useCallback, useEffect, useMemo, useState, useRef } from "react";
import { Button, Content, Skeleton } from "@patternfly/react-core";
import { AngleDoubleRightIcon } from "@patternfly/react-icons";
import { useContainerfileDiff } from "../hooks/useContainerfileDiff";

export interface ContainerfilePanelProps {
  content: string | null;
  isOpen: boolean;
  onToggle: () => void;
  loading: boolean;
  /** When true, redact crypt(3) hashes in chpasswd lines. */
  sessionIsSensitive?: boolean;
}

const DEFAULT_WIDTH = 340;
const MIN_WIDTH = 200;
const MAX_WIDTH_RATIO = 0.6; // 60% of viewport

/** Regex matching crypt(3) hash patterns ($6$..., $y$..., $5$...). */
const CRYPT_HASH_RE = /(\$(?:6|5|y)\$[^\s'"\\]+)/g;

export function ContainerfilePanel({
  content,
  isOpen,
  onToggle,
  loading,
  sessionIsSensitive = false,
}: ContainerfilePanelProps) {
  const [panelWidth, setPanelWidth] = useState(DEFAULT_WIDTH);
  const [hashesRevealed, setHashesRevealed] = useState(false);
  const isDragging = useRef(false);
  const dragStartX = useRef(0);
  const dragStartWidth = useRef(DEFAULT_WIDTH);
  const panelBodyRef = useRef<HTMLDivElement>(null);
  const scrollTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Resize drag handlers
  const handleDragStart = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    isDragging.current = true;
    dragStartX.current = e.clientX;
    dragStartWidth.current = panelWidth;
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
  }, [panelWidth]);

  useEffect(() => {
    const handleDragMove = (e: MouseEvent) => {
      if (!isDragging.current) return;
      // Panel is on the right, so dragging left increases width
      const delta = dragStartX.current - e.clientX;
      const maxWidth = window.innerWidth * MAX_WIDTH_RATIO;
      const newWidth = Math.min(maxWidth, Math.max(MIN_WIDTH, dragStartWidth.current + delta));
      setPanelWidth(newWidth);
    };

    const handleDragEnd = () => {
      if (!isDragging.current) return;
      isDragging.current = false;
      document.body.style.cursor = "";
      document.body.style.userSelect = "";
    };

    document.addEventListener("mousemove", handleDragMove);
    document.addEventListener("mouseup", handleDragEnd);
    return () => {
      document.removeEventListener("mousemove", handleDragMove);
      document.removeEventListener("mouseup", handleDragEnd);
    };
  }, []);

  // Dockerfile keywords for CSS-based highlighting
  const DOCKERFILE_KEYWORDS = useMemo(
    () =>
      new Set([
        "FROM",
        "RUN",
        "CMD",
        "LABEL",
        "MAINTAINER",
        "EXPOSE",
        "ENV",
        "ADD",
        "COPY",
        "ENTRYPOINT",
        "VOLUME",
        "USER",
        "WORKDIR",
        "ARG",
        "ONBUILD",
        "STOPSIGNAL",
        "HEALTHCHECK",
        "SHELL",
        "AS",
      ]),
    [],
  );

  // Auto-collapse below 1280px on runtime viewport changes.
  // Initial narrow-viewport state is handled by the parent (App.tsx)
  // synchronously during useState initialization.
  useEffect(() => {
    const mq = window.matchMedia("(max-width: 1279px)");
    const handler = (e: MediaQueryListEvent) => {
      if (e.matches && isOpen) onToggle();
    };
    mq.addEventListener("change", handler);
    return () => mq.removeEventListener("change", handler);
  }, [isOpen, onToggle]);

  /** Split a line into tokens: keyword spans get a CSS class, everything else is plain text. */
  const tokenizeLine = useCallback(
    (line: string): Array<{ text: string; isKeyword: boolean }> => {
      const match = line.match(/^(\s*)([A-Z]+)(.*)/);
      if (match && DOCKERFILE_KEYWORDS.has(match[2])) {
        const tokens: Array<{ text: string; isKeyword: boolean }> = [];
        if (match[1]) tokens.push({ text: match[1], isKeyword: false });
        tokens.push({ text: match[2], isKeyword: true });
        if (match[3]) tokens.push({ text: match[3], isKeyword: false });
        return tokens;
      }
      return [{ text: line, isKeyword: false }];
    },
    [DOCKERFILE_KEYWORDS],
  );

  const { diffResult, hasPendingChanges, pruneRemovingLine, clearHighlight } = useContainerfileDiff(content, isOpen);

  /** Apply crypt(3) hash redaction to a line's text when sensitive. */
  const redactLine = useCallback(
    (text: string): string => {
      if (!sessionIsSensitive || hashesRevealed) return text;
      if (!CRYPT_HASH_RE.test(text)) return text;
      CRYPT_HASH_RE.lastIndex = 0;
      return text.replace(CRYPT_HASH_RE, (match) => {
        const prefix = match.match(/^(\$[^$]+\$)/);
        return prefix ? `${prefix[1]}<REDACTED>` : "$<REDACTED>";
      });
    },
    [sessionIsSensitive, hashesRevealed],
  );

  // Removal animation lifecycle: glow -> collapse -> prune
  // + reduced motion support: immediate prune for removing, 2s clear for added
  const removingTimers = useRef<Map<string, ReturnType<typeof setTimeout>>>(new Map());
  const addedTimers = useRef<Map<string, ReturnType<typeof setTimeout>>>(new Map());
  useEffect(() => {
    const prefersReducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

    const removing = diffResult.lines.filter((l) => l.state === "removing");
    for (const dl of removing) {
      // Skip if already tracked
      if (removingTimers.current.has(dl.id)) continue;

      if (prefersReducedMotion) {
        // Reduced motion: prune immediately
        pruneRemovingLine(dl.id);
      } else {
        // Phase 1: glow for 300ms, then collapse
        const glowTimer = setTimeout(() => {
          const el = document.querySelector(`[data-line-id="${dl.id}"]`) as HTMLElement | null;
          if (el) {
            // Set explicit max-height before collapsing
            el.style.maxHeight = `${el.scrollHeight}px`;
            // Force reflow so the browser registers the initial max-height
            void el.offsetHeight;
            el.classList.add("inspectah-cf-line--collapsing");
          }

          // Phase 2: prune after collapse transition (500ms) or fallback (1.5s)
          let pruned = false;
          const prune = () => {
            if (pruned) return;
            pruned = true;
            removingTimers.current.delete(dl.id);
            pruneRemovingLine(dl.id);
          };

          if (el) {
            el.addEventListener("transitionend", prune, { once: true });
          }

          // Fallback timeout in case transitionend doesn't fire
          const fallback = setTimeout(prune, 1500);
          removingTimers.current.set(dl.id, fallback);
        }, 300);

        removingTimers.current.set(dl.id, glowTimer);
      }
    }

    const added = diffResult.lines.filter((l) => l.state === "added");
    for (const dl of added) {
      // Skip if already tracked
      if (addedTimers.current.has(dl.id)) continue;

      if (prefersReducedMotion) {
        // Reduced motion: clear highlight after 2s
        const timer = setTimeout(() => {
          addedTimers.current.delete(dl.id);
          clearHighlight(dl.id);
        }, 2000);
        addedTimers.current.set(dl.id, timer);
      }
    }

    // Cleanup on unmount
    return () => {
      for (const timer of removingTimers.current.values()) {
        clearTimeout(timer);
      }
      for (const timer of addedTimers.current.values()) {
        clearTimeout(timer);
      }
    };
  }, [diffResult, pruneRemovingLine, clearHighlight]);

  // Auto-scroll to first changed line after diff updates
  useEffect(() => {
    if (!diffResult.hasChanges) return;

    // Debounce: clear previous scroll timeout
    if (scrollTimeoutRef.current) clearTimeout(scrollTimeoutRef.current);

    scrollTimeoutRef.current = setTimeout(() => {
      const panelBody = panelBodyRef.current;
      if (!panelBody) return;

      const firstChanged = panelBody.querySelector("[data-line-id]");
      if (!firstChanged) return;

      // Check if already visible within the panel body
      const bodyRect = panelBody.getBoundingClientRect();
      const lineRect = firstChanged.getBoundingClientRect();
      if (lineRect.top >= bodyRect.top && lineRect.bottom <= bodyRect.bottom) return;

      // Single scrollTo targeting ~1/3 from top of the panel.
      const el = firstChanged as HTMLElement;
      const targetTop = el.offsetTop - Math.round(panelBody.clientHeight / 3);
      const prefersReducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
      panelBody.scrollTo({
        top: Math.max(0, targetTop),
        behavior: prefersReducedMotion ? "auto" : "smooth",
      });
    }, 150);

    return () => {
      if (scrollTimeoutRef.current) clearTimeout(scrollTimeoutRef.current);
    };
  }, [diffResult]);

  const lineCount = diffResult.lines.filter(
    (l) => l.state !== "removing" && l.text.length > 0,
  ).length;

  /** Build the aria-live summary text. */
  const diffSummary = useMemo(() => {
    if (!diffResult.hasChanges) return "";
    const parts: string[] = [];
    if (diffResult.addedCount > 0) {
      parts.push(`${diffResult.addedCount} line${diffResult.addedCount === 1 ? "" : "s"} added`);
    }
    if (diffResult.removedCount > 0) {
      parts.push(`${diffResult.removedCount} line${diffResult.removedCount === 1 ? "" : "s"} removed`);
    }
    return `Containerfile updated: ${parts.join(", ")}`;
  }, [diffResult.hasChanges, diffResult.addedCount, diffResult.removedCount]);

  if (!isOpen) {
    const tabClass = hasPendingChanges
      ? "inspectah-cf-panel__tab inspectah-cf-panel__tab--has-changes"
      : "inspectah-cf-panel__tab";
    const tabLabel = hasPendingChanges
      ? "Expand Containerfile panel, pending changes"
      : "Expand Containerfile panel";
    return (
      <div
        className="inspectah-cf-panel inspectah-cf-panel--collapsed"
        role="complementary"
        aria-label="Containerfile preview"
      >
        <button
          className={tabClass}
          onClick={onToggle}
          aria-label={tabLabel}
          title="Ctrl+E"
        >
          <span className="inspectah-cf-panel__tab-label">Containerfile</span>
        </button>
      </div>
    );
  }

  return (
    <div
      className="inspectah-cf-panel inspectah-cf-panel--open"
      role="complementary"
      aria-label="Containerfile preview"
      style={{ flexBasis: `${panelWidth}px` }}
    >
      {/* Resize drag handle */}
      <div
        className="inspectah-cf-panel__drag-handle"
        onMouseDown={handleDragStart}
        role="separator"
        aria-orientation="vertical"
        aria-label="Resize Containerfile panel"
      />
      <div className="inspectah-cf-panel__header">
        <Content component="h3">Containerfile</Content>
        <Button
          variant="plain"
          aria-label="Collapse Containerfile panel"
          onClick={onToggle}
          icon={<AngleDoubleRightIcon />}
          size="sm"
        />
      </div>
      <div ref={panelBodyRef} className="inspectah-cf-panel__body" tabIndex={0} aria-label="Containerfile preview content">
        {loading ? (
          <>
            <Skeleton width="90%" />
            <Skeleton width="70%" />
            <Skeleton width="85%" />
            <Skeleton width="60%" />
          </>
        ) : (
          <>
            <pre className="inspectah-cf-panel__code">
              <code className="inspectah-cf-panel__dockerfile">
                {diffResult.lines.map((dl) => {
                  const displayText = redactLine(dl.text);
                  const lineClasses = [
                    "inspectah-cf-panel__line",
                    dl.state === "added" ? "inspectah-cf-line--added" : "",
                    dl.state === "removing" ? "inspectah-cf-line--removing" : "",
                  ]
                    .filter(Boolean)
                    .join(" ");

                  const isChanged = dl.state === "added" || dl.state === "removing";
                  return (
                    <span
                      key={dl.id}
                      className={lineClasses}
                      {...(isChanged ? { "data-line-id": dl.id } : {})}
                      {...(dl.state === "removing" ? { "aria-hidden": "true" } : {})}
                    >
                      {tokenizeLine(displayText).map((tok, j) =>
                        tok.isKeyword ? (
                          <span
                            key={j}
                            className="inspectah-cf-panel__keyword"
                          >
                            {tok.text}
                          </span>
                        ) : (
                          <span key={j}>{tok.text}</span>
                        ),
                      )}
                      {"\n"}
                    </span>
                  );
                })}
              </code>
            </pre>
            <span className="inspectah-sr-only" aria-live="polite">
              {diffSummary}
            </span>
          </>
        )}
      </div>
      <div className="inspectah-cf-panel__footer">
        <Content component="small">{lineCount} lines</Content>
        {sessionIsSensitive && (
          <button
            onClick={() => setHashesRevealed((p) => !p)}
            style={{
              background: "none",
              border: "none",
              cursor: "pointer",
              textDecoration: "underline",
              fontSize: "var(--pf-t--global--font--size--xs)",
              padding: 0,
            }}
          >
            {hashesRevealed ? "Redact hashes" : "Reveal hashes"}
          </button>
        )}
        <Content component="small" className="inspectah-cf-panel__footer-note">
          Preview reflects package and config decisions. Context sections are
          included as-is.
        </Content>
      </div>
    </div>
  );
}
