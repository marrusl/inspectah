import { useCallback, useEffect, useMemo, useState, useRef } from "react";
import { Button, Content, Skeleton } from "@patternfly/react-core";
import { AngleDoubleRightIcon } from "@patternfly/react-icons";

export interface ContainerfilePanelProps {
  content: string | null;
  isOpen: boolean;
  onToggle: () => void;
  loading: boolean;
}

const DEFAULT_WIDTH = 340;
const MIN_WIDTH = 200;
const MAX_WIDTH_RATIO = 0.6; // 60% of viewport

export function ContainerfilePanel({
  content,
  isOpen,
  onToggle,
  loading,
}: ContainerfilePanelProps) {
  const [panelWidth, setPanelWidth] = useState(DEFAULT_WIDTH);
  const isDragging = useRef(false);
  const dragStartX = useRef(0);
  const dragStartWidth = useRef(DEFAULT_WIDTH);

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

  const lines = useMemo(
    () => (content != null ? content.split("\n") : []),
    [content],
  );

  const lineCount = lines.filter((l) => l.length > 0).length;

  if (!isOpen) {
    return (
      <div
        className="inspectah-cf-panel inspectah-cf-panel--collapsed"
        role="complementary"
        aria-label="Containerfile preview"
      >
        <button
          className="inspectah-cf-panel__tab"
          onClick={onToggle}
          aria-label="Expand Containerfile panel"
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
      <div className="inspectah-cf-panel__body" tabIndex={0} aria-label="Containerfile preview content">
        {loading ? (
          <>
            <Skeleton width="90%" />
            <Skeleton width="70%" />
            <Skeleton width="85%" />
            <Skeleton width="60%" />
          </>
        ) : (
          <pre className="inspectah-cf-panel__code">
            <code className="inspectah-cf-panel__dockerfile">
              {lines.map((line, i) => (
                <span key={i} className="inspectah-cf-panel__line">
                  {tokenizeLine(line).map((tok, j) =>
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
              ))}
            </code>
          </pre>
        )}
      </div>
      <div className="inspectah-cf-panel__footer">
        <Content component="small">{lineCount} lines</Content>
        <Content component="small" className="inspectah-cf-panel__footer-note">
          Preview reflects package and config decisions. Context sections are
          included as-is.
        </Content>
      </div>
    </div>
  );
}
