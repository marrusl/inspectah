/**
 * Shared color system and theme utilities for inspectah diagrams.
 * Matches the inspectah refine UI dark palette.
 * @module theme
 */

import * as d3 from 'https://cdn.jsdelivr.net/npm/d3@7/+esm';

/** Semantic color palette — each key maps to fill, stroke, and text values. */
export const colors = {
  green:  { fill: 'rgba(34,197,94,0.12)',   stroke: '#22c55e', text: '#86efac' },
  teal:   { fill: 'rgba(45,212,191,0.12)',  stroke: '#2dd4bf', text: '#5eead4' },
  blue:   { fill: 'rgba(96,165,250,0.12)',  stroke: '#60a5fa', text: '#93c5fd' },
  purple: { fill: 'rgba(192,132,252,0.12)', stroke: '#c084fc', text: '#d8b4fe' },
  amber:  { fill: 'rgba(245,158,11,0.12)',  stroke: '#f59e0b', text: '#fde68a' },
  rose:   { fill: 'rgba(192,132,252,0.12)', stroke: '#f472b6', text: '#f9a8d4' },
  red:    { fill: 'rgba(239,68,68,0.15)',   stroke: '#ef4444', text: '#fca5a5' },
  orange: { fill: 'rgba(249,115,22,0.12)',  stroke: '#f97316', text: '#fdba74' },
};

/** Page / canvas background. */
export const bg = '#0f1729';
/** Elevated surface (cards, panels). */
export const surface = '#182038';
/** Border / separator color. */
export const border = '#2a3a5c';
/** Primary text. */
export const text = '#e0e6f0';
/** Dimmed / secondary text. */
export const textDim = '#8899bb';

/**
 * CSS string containing common diagram styles.
 * Covers nodes, labels, tooltips, focus rings, and reduced-motion overrides.
 */
export const diagramStyles = `
  :root {
    --diagram-bg: ${bg};
    --diagram-surface: ${surface};
    --diagram-border: ${border};
    --diagram-text: ${text};
    --diagram-text-dim: ${textDim};
    --diagram-focus-ring: #60a5fa;
    --diagram-transition: 200ms;
    --diagram-font: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Oxygen, Ubuntu, Cantarell, 'Helvetica Neue', Arial, sans-serif;
    --diagram-font-mono: 'SFMono-Regular', Consolas, 'Liberation Mono', Menlo, monospace;
  }

  @media (prefers-reduced-motion: reduce) {
    :root {
      --diagram-transition: 0ms;
    }
  }

  * { box-sizing: border-box; }

  html, body {
    margin: 0;
    padding: 0;
    background: var(--diagram-bg);
    color: var(--diagram-text);
    font-family: var(--diagram-font);
    overflow: hidden;
    width: 100%;
    height: 100%;
  }

  svg {
    display: block;
    width: 100%;
    height: 100%;
  }

  /* --- Node styles --- */
  .node rect, .node circle, .node ellipse {
    transition: filter var(--diagram-transition) ease,
                stroke-width var(--diagram-transition) ease;
    cursor: pointer;
  }

  .node:hover rect, .node:hover circle, .node:hover ellipse {
    filter: url(#glow);
    stroke-width: 2;
  }

  .node text {
    fill: var(--diagram-text);
    font-family: var(--diagram-font);
    pointer-events: none;
  }

  .node-label {
    fill: var(--diagram-text);
    font-size: 13px;
    font-weight: 500;
  }

  .node-sublabel {
    fill: var(--diagram-text-dim);
    font-size: 11px;
  }

  /* --- Links / edges --- */
  .link {
    fill: none;
    stroke: var(--diagram-border);
    stroke-width: 1.5;
    transition: stroke var(--diagram-transition) ease;
  }

  .link:hover {
    stroke: var(--diagram-text-dim);
  }

  /* --- Focus rings (a11y) --- */
  .node:focus, [role="treeitem"]:focus, [role="button"]:focus {
    outline: none;
  }

  .node:focus rect, .node:focus circle, .node:focus ellipse,
  [role="treeitem"]:focus rect, [role="treeitem"]:focus circle,
  [role="button"]:focus rect, [role="button"]:focus circle {
    stroke: var(--diagram-focus-ring) !important;
    stroke-width: 2px !important;
    filter: drop-shadow(0 0 4px rgba(96,165,250,0.5));
  }

  /* --- Tooltip --- */
  .diagram-tooltip {
    position: fixed;
    padding: 8px 12px;
    background: ${surface};
    border: 1px solid ${border};
    border-radius: 6px;
    color: ${text};
    font-size: 12px;
    font-family: var(--diagram-font);
    line-height: 1.5;
    pointer-events: none;
    opacity: 0;
    transition: opacity var(--diagram-transition) ease;
    max-width: 320px;
    z-index: 1000;
    box-shadow: 0 4px 12px rgba(0,0,0,0.4);
  }

  .diagram-tooltip.visible {
    opacity: 1;
  }

  /* --- Expand/collapse content --- */
  .expand-panel {
    background: ${surface};
    border: 1px solid ${border};
    border-radius: 8px;
    padding: 16px;
    font-size: 13px;
    line-height: 1.6;
    color: ${text};
    max-width: 400px;
    box-shadow: 0 8px 24px rgba(0,0,0,0.4);
  }

  .expand-panel h3 {
    margin: 0 0 8px 0;
    font-size: 14px;
    font-weight: 600;
  }

  .expand-panel p {
    margin: 0 0 6px 0;
  }

  .expand-panel code {
    font-family: var(--diagram-font-mono);
    font-size: 12px;
    background: rgba(96,165,250,0.1);
    padding: 1px 5px;
    border-radius: 3px;
  }

  /* --- Chrome overlays --- */
  .diagram-title-overlay {
    position: fixed;
    top: 16px;
    left: 20px;
    z-index: 100;
    pointer-events: none;
  }

  .diagram-title-overlay h1 {
    margin: 0;
    font-size: 16px;
    font-weight: 700;
    color: ${text};
    letter-spacing: 0.5px;
  }

  .diagram-title-overlay .subtitle {
    margin: 2px 0 0 0;
    font-size: 12px;
    color: ${textDim};
  }

  .diagram-back-link {
    position: fixed;
    top: 56px;
    left: 20px;
    z-index: 100;
    font-size: 12px;
    color: ${textDim};
    text-decoration: none;
    transition: color var(--diagram-transition) ease;
  }

  .diagram-back-link:hover,
  .diagram-back-link:focus {
    color: var(--diagram-focus-ring);
    outline: none;
  }

  .diagram-legend {
    position: fixed;
    top: 16px;
    right: 20px;
    z-index: 100;
    display: flex;
    gap: 14px;
    flex-wrap: wrap;
    justify-content: flex-end;
  }

  .diagram-legend-item {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 11px;
    color: ${textDim};
  }

  .diagram-legend-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    flex-shrink: 0;
  }

  .diagram-hint-bar {
    position: fixed;
    bottom: 12px;
    left: 50%;
    transform: translateX(-50%);
    z-index: 100;
    font-size: 11px;
    color: ${textDim};
    background: rgba(15,23,41,0.8);
    padding: 6px 16px;
    border-radius: 20px;
    border: 1px solid ${border};
    pointer-events: none;
  }

  .diagram-fullscreen-btn {
    position: fixed;
    top: 50px;
    right: 20px;
    z-index: 100;
    font-size: 11px;
    color: ${textDim};
    background: ${surface};
    border: 1px solid ${border};
    border-radius: 6px;
    padding: 5px 12px;
    cursor: pointer;
    transition: color var(--diagram-transition) ease,
                border-color var(--diagram-transition) ease;
  }

  .diagram-fullscreen-btn:hover,
  .diagram-fullscreen-btn:focus {
    color: var(--diagram-focus-ring);
    border-color: var(--diagram-focus-ring);
    outline: none;
  }
`;

/**
 * Inject theme CSS variables and common diagram styles into the document.
 * Safe to call multiple times — subsequent calls are no-ops.
 */
export function injectCSS() {
  if (document.getElementById('diagram-theme-styles')) return;
  const style = document.createElement('style');
  style.id = 'diagram-theme-styles';
  style.textContent = diagramStyles;
  document.head.appendChild(style);
}

/**
 * Add SVG filter definitions (glow effect, arrowhead markers) to an SVG element.
 * @param {d3.Selection} svg - D3 selection of the root SVG element.
 */
export function createSVGFilters(svg) {
  const defs = svg.append('defs');

  // Glow filter
  const glow = defs.append('filter')
    .attr('id', 'glow')
    .attr('x', '-50%').attr('y', '-50%')
    .attr('width', '200%').attr('height', '200%');

  glow.append('feGaussianBlur')
    .attr('stdDeviation', '3')
    .attr('result', 'blur');

  glow.append('feMerge')
    .selectAll('feMergeNode')
    .data(['blur', 'SourceGraphic'])
    .join('feMergeNode')
      .attr('in', d => d);

  // Arrowhead markers for each color
  const markerColors = {
    default: border,
    ...Object.fromEntries(
      Object.entries(colors).map(([k, v]) => [k, v.stroke])
    ),
  };

  for (const [name, color] of Object.entries(markerColors)) {
    defs.append('marker')
      .attr('id', `arrow-${name}`)
      .attr('viewBox', '0 0 10 10')
      .attr('refX', 9)
      .attr('refY', 5)
      .attr('markerWidth', 8)
      .attr('markerHeight', 8)
      .attr('orient', 'auto-start-reverse')
      .append('path')
        .attr('d', 'M 0 0 L 10 5 L 0 10 z')
        .attr('fill', color);
  }
}
