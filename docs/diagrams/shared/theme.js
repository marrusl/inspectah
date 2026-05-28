/**
 * Shared theme system for inspectah D3 diagrams.
 * Matches the refine UI dark palette for visual consistency.
 * @module theme
 */

/** Semantic color tokens — each color has fill (12% opacity), stroke, and text variants. */
export const colors = {
  green:  { fill: 'rgba(34,197,94,0.12)',   stroke: '#22c55e', text: '#86efac' },
  teal:   { fill: 'rgba(45,212,191,0.12)',  stroke: '#2dd4bf', text: '#5eead4' },
  blue:   { fill: 'rgba(96,165,250,0.12)',  stroke: '#60a5fa', text: '#93c5fd' },
  purple: { fill: 'rgba(192,132,252,0.12)', stroke: '#c084fc', text: '#d8b4fe' },
  amber:  { fill: 'rgba(245,158,11,0.12)',  stroke: '#f59e0b', text: '#fde68a' },
  rose:   { fill: 'rgba(244,114,182,0.12)', stroke: '#f472b6', text: '#f9a8d4' },
  red:    { fill: 'rgba(239,68,68,0.15)',   stroke: '#ef4444', text: '#fca5a5' },
  orange: { fill: 'rgba(249,115,22,0.12)',  stroke: '#f97316', text: '#fdba74' },
};

/** Background color for diagram canvas. */
export const bg = '#0f1729';

/** Surface color for cards and panels. */
export const surface = '#182038';

/** Border color for separators and outlines. */
export const border = '#2a3a5c';

/** Primary text color. */
export const text = '#e0e6f0';

/** Dimmed text color for secondary labels. */
export const textDim = '#8899bb';

/**
 * Inject CSS custom properties for the theme into the document.
 * Idempotent — re-calling updates the existing style element.
 */
export function injectCSS() {
  const id = 'inspectah-diagram-theme';
  let style = document.getElementById(id);
  if (!style) {
    style = document.createElement('style');
    style.id = id;
    document.head.appendChild(style);
  }

  const colorVars = Object.entries(colors)
    .map(([name, c]) => [
      `--color-${name}-fill: ${c.fill};`,
      `--color-${name}-stroke: ${c.stroke};`,
      `--color-${name}-text: ${c.text};`,
    ].join('\n'))
    .join('\n');

  style.textContent = `
:root {
  --bg: ${bg};
  --surface: ${surface};
  --border: ${border};
  --text: ${text};
  --text-dim: ${textDim};
  ${colorVars}
}
body {
  background: var(--bg);
  color: var(--text);
  margin: 0;
  font-family: 'Inter', -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
  overflow: hidden;
}
`;
}

/**
 * Add reusable SVG filter definitions (glow, arrowheads) to an SVG element.
 * @param {d3.Selection} svg — D3 selection of the root <svg> element.
 */
export function createSVGFilters(svg) {
  const defs = svg.append('defs');

  // Glow filter — soft colored bloom around highlighted elements
  const glow = defs.append('filter')
    .attr('id', 'glow')
    .attr('x', '-50%').attr('y', '-50%')
    .attr('width', '200%').attr('height', '200%');
  glow.append('feGaussianBlur')
    .attr('stdDeviation', '3')
    .attr('result', 'blur');
  glow.append('feComposite')
    .attr('in', 'SourceGraphic')
    .attr('in2', 'blur')
    .attr('operator', 'over');

  // Arrowhead marker — for directional edges
  defs.append('marker')
    .attr('id', 'arrowhead')
    .attr('viewBox', '0 0 10 7')
    .attr('refX', 10).attr('refY', 3.5)
    .attr('markerWidth', 8).attr('markerHeight', 6)
    .attr('orient', 'auto-start-reverse')
    .append('path')
    .attr('d', 'M 0 0 L 10 3.5 L 0 7 z')
    .attr('fill', border);

  // Highlighted arrowhead — brighter version for active edges
  defs.append('marker')
    .attr('id', 'arrowhead-active')
    .attr('viewBox', '0 0 10 7')
    .attr('refX', 10).attr('refY', 3.5)
    .attr('markerWidth', 8).attr('markerHeight', 6)
    .attr('orient', 'auto-start-reverse')
    .append('path')
    .attr('d', 'M 0 0 L 10 3.5 L 0 7 z')
    .attr('fill', text);
}

/** CSS rules for common diagram elements — nodes, labels, tooltips, focus rings. */
export const diagramStyles = `
.node rect,
.node circle {
  transition: filter 0.2s ease, opacity 0.2s ease;
  cursor: pointer;
}
.node:hover rect,
.node:hover circle {
  filter: url(#glow);
}
.node text {
  font-size: 13px;
  fill: var(--text);
  pointer-events: none;
  user-select: none;
}
.node text.label-dim {
  fill: var(--text-dim);
  font-size: 11px;
}
.edge {
  fill: none;
  stroke: var(--border);
  stroke-width: 1.5;
  transition: stroke 0.2s ease, stroke-width 0.2s ease;
}
.edge.active {
  stroke: var(--text);
  stroke-width: 2;
}

/* Tooltip */
.diagram-tooltip {
  position: fixed;
  pointer-events: none;
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 6px;
  padding: 8px 12px;
  font-size: 13px;
  color: var(--text);
  max-width: 320px;
  line-height: 1.45;
  box-shadow: 0 4px 24px rgba(0,0,0,0.5);
  z-index: 1000;
  opacity: 0;
  transition: opacity 0.15s ease;
}
.diagram-tooltip.visible {
  opacity: 1;
}

/* Focus rings — 2px visible outline for keyboard nav */
.node:focus-visible rect,
.node:focus-visible circle,
[tabindex]:focus-visible {
  outline: 2px solid #60a5fa;
  outline-offset: 2px;
}
.node:focus:not(:focus-visible) rect,
.node:focus:not(:focus-visible) circle {
  outline: none;
}

/* Reduced motion */
@media (prefers-reduced-motion: reduce) {
  .node rect, .node circle, .edge, .diagram-tooltip {
    transition: none !important;
  }
}
`;
