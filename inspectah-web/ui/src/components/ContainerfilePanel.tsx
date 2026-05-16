import { useEffect, useRef, useCallback } from "react";
import { Button, Content, Skeleton } from "@patternfly/react-core";
import { AngleDoubleRightIcon } from "@patternfly/react-icons";
import hljs from "highlight.js/lib/core";
import dockerfile from "highlight.js/lib/languages/dockerfile";

hljs.registerLanguage("dockerfile", dockerfile);

export interface ContainerfilePanelProps {
  content: string | null;
  isOpen: boolean;
  onToggle: () => void;
  loading: boolean;
}

export function ContainerfilePanel({
  content,
  isOpen,
  onToggle,
  loading,
}: ContainerfilePanelProps) {
  const codeRef = useRef<HTMLElement>(null);

  // Keyboard shortcut: Ctrl+E
  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      if (e.ctrlKey && e.key === "e") {
        e.preventDefault();
        onToggle();
      }
    },
    [onToggle],
  );

  useEffect(() => {
    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [handleKeyDown]);

  // Auto-collapse below 1280px
  useEffect(() => {
    const mq = window.matchMedia("(max-width: 1280px)");
    const handler = (e: MediaQueryListEvent) => {
      if (e.matches && isOpen) onToggle();
    };
    mq.addEventListener("change", handler);
    return () => mq.removeEventListener("change", handler);
  }, [isOpen, onToggle]);

  const highlighted =
    content != null
      ? hljs.highlight(content, { language: "dockerfile" }).value
      : "";

  const lineCount =
    content != null ? content.split("\n").filter((l) => l.length > 0).length : 0;

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
    >
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
      <div className="inspectah-cf-panel__body">
        {loading ? (
          <>
            <Skeleton width="90%" />
            <Skeleton width="70%" />
            <Skeleton width="85%" />
            <Skeleton width="60%" />
          </>
        ) : (
          <pre className="inspectah-cf-panel__code">
            <code
              ref={codeRef}
              className="hljs language-dockerfile"
              dangerouslySetInnerHTML={{ __html: highlighted }}
            />
          </pre>
        )}
      </div>
      <div className="inspectah-cf-panel__footer">
        <Content component="small">{lineCount} lines</Content>
      </div>
    </div>
  );
}
