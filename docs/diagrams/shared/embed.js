/**
 * Iframe detection, fullscreen behavior, and the diagram shell.
 * Every diagram page uses createDiagramShell() for consistent chrome.
 * @module embed
 */

import * as d3 from 'https://cdn.jsdelivr.net/npm/d3@7/+esm';
import { injectCSS, createSVGFilters, diagramStyles, bg, surface, border, text, textDim } from './theme.js';
import { setupZoom, centerView } from './interactions.js';
import { checkReducedMotion } from './accessibility.js';

/**
 * Detect whether the diagram is running inside an iframe.
 * @returns {boolean}
 */
export function isEmbedded() {
  try {
    return window.self !== window.top;
  } catch {
    // Cross-origin iframe — treat as embedded
    return true;
  }
}

/**
 * Wire up a fullscreen toggle button.
 * Uses the Fullscreen API when available, falls back to window.open.
 *
 * @param {string} buttonSelector — CSS selector for the toggle button.
 * @param {string} fallbackUrl — URL to open in a new window if Fullscreen API is unavailable.
 * @returns {{ cleanup: () => void }}
 */
export function setupFullscreen(buttonSelector, fallbackUrl) {
  const btn = document.querySelector(buttonSelector);
  if (!btn) return { cleanup() {} };

  function toggle() {
    if (document.fullscreenElement) {
      document.exitFullscreen();
    } else if (document.documentElement.requestFullscreen) {
      document.documentElement.requestFullscreen();
    } else {
      window.open(fallbackUrl, '_blank');
    }
  }

  function onFullscreenChange() {
    btn.textContent = document.fullscreenElement
      ? 'Exit fullscreen (Esc)'
      : 'Fullscreen';
    btn.setAttribute(
      'aria-label',
      document.fullscreenElement ? 'Exit fullscreen' : 'Enter fullscreen',
    );
  }

  btn.addEventListener('click', toggle);
  document.addEventListener('fullscreenchange', onFullscreenChange);

  function cleanup() {
    btn.removeEventListener('click', toggle);
    document.removeEventListener('fullscreenchange', onFullscreenChange);
  }

  return { cleanup };
}

/**
 * Send a message to the parent frame (for iframe embed scenarios).
 * @param {{ type: string, [key: string]: unknown }} event — structured event data.
 */
export function notifyParent(event) {
  if (!isEmbedded()) return;
  try {
    window.parent.postMessage({ source: 'inspectah-diagram', ...event }, '*');
  } catch {
    // Silently fail on cross-origin restrictions
  }
}

/**
 * Create the complete standalone diagram page shell.
 * Every diagram calls this once — it provides the title overlay, legend,
 * back-to-docs link, fullscreen button, hint bar, theme injection, and
 * the SVG element with zoom-enabled group.
 *
 * @param {{ title: string, subtitle: string, backUrl?: string, legendItems: Array<{ color: string, label: string }> }} config
 * @returns {{ svg: d3.Selection, g: d3.Selection, width: number, height: number, zoom: d3.ZoomBehavior, cleanup: () => void }}
 */
export function createDiagramShell({ title, subtitle, backUrl = '../index.html', legendItems = [] }) {
  // 1. Inject theme
  injectCSS();

  // 2. Inject diagram element styles
  const styleEl = document.createElement('style');
  styleEl.textContent = diagramStyles + shellStyles;
  document.head.appendChild(styleEl);

  // 3. Build the page structure
  const container = document.createElement('div');
  container.className = 'diagram-shell';
  document.body.appendChild(container);

  // --- Title overlay (top-left) ---
  const titleOverlay = document.createElement('div');
  titleOverlay.className = 'shell-title-overlay';
  titleOverlay.innerHTML = `
    <h1 class="shell-title">inspectah</h1>
    <p class="shell-subtitle">${escapeHtml(subtitle)}</p>
    <a href="${escapeHtml(backUrl)}" class="shell-back-link" aria-label="Back to documentation">
      <span aria-hidden="true">&larr;</span> Back to docs
    </a>
  `;
  container.appendChild(titleOverlay);

  // --- Legend (top-right) ---
  if (legendItems.length > 0) {
    const legend = document.createElement('div');
    legend.className = 'shell-legend';
    legend.setAttribute('role', 'list');
    legend.setAttribute('aria-label', 'Diagram legend');
    legend.innerHTML = legendItems
      .map(item => `
        <div class="shell-legend-item" role="listitem">
          <span class="shell-legend-dot" style="background: ${escapeHtml(item.color)};"></span>
          <span class="shell-legend-label">${escapeHtml(item.label)}</span>
        </div>
      `)
      .join('');
    container.appendChild(legend);
  }

  // --- Fullscreen button (top-right, below legend) ---
  const fsBtn = document.createElement('button');
  fsBtn.className = 'shell-fullscreen-btn';
  fsBtn.id = 'fullscreen-toggle';
  fsBtn.textContent = 'Fullscreen';
  fsBtn.setAttribute('aria-label', 'Enter fullscreen');
  container.appendChild(fsBtn);

  // --- Hint bar (bottom-center) ---
  const hint = document.createElement('div');
  hint.className = 'shell-hint';
  hint.setAttribute('role', 'status');
  hint.setAttribute('aria-live', 'polite');
  hint.textContent = 'Click a node to expand details. Scroll to zoom. Drag to pan.';
  container.appendChild(hint);

  // 4. Create SVG
  const width = window.innerWidth;
  const height = window.innerHeight;

  const svg = d3.select(container)
    .append('svg')
    .attr('class', 'diagram-svg')
    .attr('width', width)
    .attr('height', height)
    .attr('role', 'img')
    .attr('aria-label', `${title}: ${subtitle}`);

  // 5. SVG filter defs
  createSVGFilters(svg);

  // 6. Zoom-enabled inner group
  const g = svg.append('g').attr('class', 'diagram-canvas');

  // 7. Set up zoom
  const { zoom, cleanup: cleanupZoom } = setupZoom(svg, g);

  // 8. Fullscreen wiring
  const { cleanup: cleanupFs } = setupFullscreen('#fullscreen-toggle', window.location.href);

  // 9. Reduced-motion listener — add a class for CSS to key off
  const motionQuery = window.matchMedia('(prefers-reduced-motion: reduce)');
  function onMotionChange(e) {
    document.body.classList.toggle('reduced-motion', e.matches);
  }
  onMotionChange(motionQuery);
  motionQuery.addEventListener('change', onMotionChange);

  // 10. Resize handler
  function onResize() {
    const w = window.innerWidth;
    const h = window.innerHeight;
    svg.attr('width', w).attr('height', h);
  }
  window.addEventListener('resize', onResize);

  function cleanup() {
    cleanupZoom();
    cleanupFs();
    motionQuery.removeEventListener('change', onMotionChange);
    window.removeEventListener('resize', onResize);
  }

  return { svg, g, width, height, zoom, cleanup };
}

// --- Internal helpers ---

/**
 * Escape HTML entities to prevent XSS in template strings.
 * @param {string} str
 * @returns {string}
 */
function escapeHtml(str) {
  const div = document.createElement('div');
  div.textContent = str;
  return div.innerHTML;
}

/** CSS for the diagram shell chrome elements. */
const shellStyles = `
.diagram-shell {
  position: relative;
  width: 100vw;
  height: 100vh;
  overflow: hidden;
}
.diagram-svg {
  display: block;
}

/* Title overlay */
.shell-title-overlay {
  position: fixed;
  top: 20px;
  left: 24px;
  z-index: 100;
  pointer-events: auto;
}
.shell-title {
  font-size: 18px;
  font-weight: 700;
  color: var(--text);
  margin: 0;
  letter-spacing: -0.02em;
}
.shell-subtitle {
  font-size: 13px;
  color: var(--text-dim);
  margin: 4px 0 0;
}
.shell-back-link {
  display: inline-block;
  margin-top: 10px;
  font-size: 12px;
  color: var(--text-dim);
  text-decoration: none;
  transition: color 0.15s ease;
}
.shell-back-link:hover {
  color: var(--text);
}
.shell-back-link:focus-visible {
  outline: 2px solid #60a5fa;
  outline-offset: 2px;
}

/* Legend */
.shell-legend {
  position: fixed;
  top: 20px;
  right: 24px;
  z-index: 100;
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.shell-legend-item {
  display: flex;
  align-items: center;
  gap: 8px;
}
.shell-legend-dot {
  width: 10px;
  height: 10px;
  border-radius: 50%;
  flex-shrink: 0;
}
.shell-legend-label {
  font-size: 12px;
  color: var(--text-dim);
  white-space: nowrap;
}

/* Fullscreen button */
.shell-fullscreen-btn {
  position: fixed;
  top: auto;
  bottom: 52px;
  right: 24px;
  z-index: 100;
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 6px;
  color: var(--text-dim);
  font-size: 12px;
  padding: 6px 14px;
  cursor: pointer;
  transition: color 0.15s ease, border-color 0.15s ease;
}
.shell-fullscreen-btn:hover {
  color: var(--text);
  border-color: var(--text-dim);
}
.shell-fullscreen-btn:focus-visible {
  outline: 2px solid #60a5fa;
  outline-offset: 2px;
}

/* Hint bar */
.shell-hint {
  position: fixed;
  bottom: 16px;
  left: 50%;
  transform: translateX(-50%);
  z-index: 100;
  font-size: 12px;
  color: var(--text-dim);
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 6px 18px;
  white-space: nowrap;
  pointer-events: none;
  opacity: 0.85;
}

/* Reduced-motion class applied by JS */
body.reduced-motion .shell-back-link,
body.reduced-motion .shell-fullscreen-btn {
  transition: none;
}
`;
