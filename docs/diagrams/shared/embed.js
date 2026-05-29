/**
 * Embedding, fullscreen, and diagram shell for inspectah diagrams.
 * Every diagram page uses `createDiagramShell()` to get a consistent,
 * accessible chrome layer with zero per-diagram duplication.
 * @module embed
 */

import * as d3 from 'https://cdn.jsdelivr.net/npm/d3@7/+esm';
import { injectCSS, createSVGFilters } from './theme.js';
import { setupZoom, centerView } from './interactions.js';
import { checkReducedMotion } from './accessibility.js';

/**
 * Detect if the current page is embedded in an iframe.
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
 * Set up fullscreen toggling for a diagram.
 * Uses the Fullscreen API when available, falls back to opening a new window.
 *
 * @param {string} buttonSelector - CSS selector for the fullscreen toggle button.
 * @param {string} fallbackUrl    - URL to open in a new window if Fullscreen API is unavailable.
 * @returns {{ cleanup: Function }}
 */
export function setupFullscreen(buttonSelector, fallbackUrl) {
  const btn = document.querySelector(buttonSelector);
  if (!btn) return { cleanup() {} };

  function handleClick() {
    if (document.fullscreenElement) {
      document.exitFullscreen();
    } else if (document.documentElement.requestFullscreen) {
      document.documentElement.requestFullscreen();
    } else if (fallbackUrl) {
      window.open(fallbackUrl, '_blank');
    }
  }

  function handleFullscreenChange() {
    btn.textContent = document.fullscreenElement
      ? 'Exit fullscreen (Esc)'
      : 'Fullscreen';
  }

  btn.addEventListener('click', handleClick);
  document.addEventListener('fullscreenchange', handleFullscreenChange);

  function cleanup() {
    btn.removeEventListener('click', handleClick);
    document.removeEventListener('fullscreenchange', handleFullscreenChange);
  }

  return { cleanup };
}

/**
 * Send a postMessage event to the parent frame for focus restoration.
 * Used when an embedded diagram releases keyboard focus (e.g., Escape).
 *
 * @param {{ type: string, [key: string]: any }} event - Message payload.
 */
export function notifyParent(event) {
  if (isEmbedded()) {
    try {
      window.parent.postMessage({ source: 'inspectah-diagram', ...event }, '*');
    } catch {
      // Silently fail for cross-origin parents
    }
  }
}

/**
 * Build the complete standalone diagram page shell.
 * Creates all chrome (title, legend, hint bar, fullscreen button, back link),
 * injects theme CSS, adds SVG filters, and returns a rendering surface.
 *
 * ALL 6 diagram files MUST use this shell. Zero duplicate chrome markup.
 *
 * @param {Object}   config
 * @param {string}   config.title       - Diagram title (displayed as subtitle under "inspectah").
 * @param {string}   [config.subtitle]  - Additional subtitle text.
 * @param {string}   [config.backUrl='../'] - URL for the back-to-docs link.
 * @param {{ label: string, color: string }[]} [config.legendItems=[]]
 *   Legend entries. `color` is a CSS color string for the dot.
 * @returns {{ svg: d3.Selection, g: d3.Selection, width: number, height: number, zoom: d3.ZoomBehavior, cleanup: Function }}
 */
export function createDiagramShell(config) {
  const {
    title,
    subtitle,
    backUrl = '../',
    legendItems = [],
  } = config;

  // Inject shared styles
  injectCSS();

  // --- Title overlay ---
  const titleOverlay = document.createElement('div');
  titleOverlay.className = 'diagram-title-overlay';
  titleOverlay.innerHTML = `
    <h1>inspectah</h1>
    <div class="subtitle">${escapeHtml(title)}${subtitle ? ` &mdash; ${escapeHtml(subtitle)}` : ''}</div>
  `;
  document.body.appendChild(titleOverlay);

  // --- Back link ---
  const backLink = document.createElement('a');
  backLink.className = 'diagram-back-link';
  backLink.href = backUrl;
  backLink.textContent = '← Back to docs';
  document.body.appendChild(backLink);

  // --- Legend ---
  if (legendItems.length > 0) {
    const legend = document.createElement('div');
    legend.className = 'diagram-legend';
    legend.setAttribute('role', 'list');
    legend.setAttribute('aria-label', 'Diagram legend');

    for (const item of legendItems) {
      const entry = document.createElement('div');
      entry.className = 'diagram-legend-item';
      entry.setAttribute('role', 'listitem');

      const dot = document.createElement('span');
      dot.className = 'diagram-legend-dot';
      dot.style.backgroundColor = item.color;

      const label = document.createElement('span');
      label.textContent = item.label;

      entry.appendChild(dot);
      entry.appendChild(label);
      legend.appendChild(entry);
    }

    document.body.appendChild(legend);
  }

  // --- Fullscreen button ---
  const fsBtn = document.createElement('button');
  fsBtn.className = 'diagram-fullscreen-btn';
  fsBtn.id = 'diagram-fullscreen-btn';
  fsBtn.textContent = 'Fullscreen';
  fsBtn.setAttribute('aria-label', 'Toggle fullscreen mode');
  document.body.appendChild(fsBtn);

  // --- Hint bar ---
  const hint = document.createElement('div');
  hint.className = 'diagram-hint-bar';
  hint.setAttribute('aria-hidden', 'true');
  hint.textContent = 'Click a node to expand details. Scroll to zoom. Drag to pan.';
  document.body.appendChild(hint);

  // --- SVG + rendering surface ---
  const width = window.innerWidth;
  const height = window.innerHeight;

  const svg = d3.select(document.body)
    .append('svg')
    .attr('width', width)
    .attr('height', height)
    .attr('viewBox', `0 0 ${width} ${height}`)
    .attr('role', 'img')
    .attr('aria-label', `${title} diagram`);

  // Add filter defs (glow, arrowheads)
  createSVGFilters(svg);

  // Transformable inner group
  const g = svg.append('g').attr('class', 'diagram-canvas');

  // Zoom/pan
  const { zoom, cleanup: zoomCleanup } = setupZoom(svg, g);

  // Fullscreen
  const { cleanup: fsCleanup } = setupFullscreen('#diagram-fullscreen-btn');

  // Reduced-motion listener (updates CSS variable dynamically)
  const mqReducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)');
  function handleMotionChange(mq) {
    document.documentElement.style.setProperty(
      '--diagram-transition',
      mq.matches ? '0ms' : '200ms'
    );
  }
  mqReducedMotion.addEventListener('change', handleMotionChange);

  // Window resize handler
  function handleResize() {
    const w = window.innerWidth;
    const h = window.innerHeight;
    svg.attr('width', w).attr('height', h).attr('viewBox', `0 0 ${w} ${h}`);
  }
  window.addEventListener('resize', handleResize);

  // Re-center diagram on fullscreen enter/exit
  function handleFullscreenRecenter() {
    // Update SVG dimensions to match new viewport
    const w = window.innerWidth;
    const h = window.innerHeight;
    svg.attr('width', w).attr('height', h).attr('viewBox', `0 0 ${w} ${h}`);

    // Compute bounds from actual rendered content
    const gNode = g.node();
    if (!gNode) return;
    const bbox = gNode.getBBox();
    if (bbox.width === 0 && bbox.height === 0) return;

    // Re-center with a short delay to let the viewport settle
    requestAnimationFrame(() => {
      centerView(svg, zoom, {
        x: bbox.x,
        y: bbox.y,
        width: bbox.width,
        height: bbox.height,
      }, { padding: 60 });
    });
  }
  document.addEventListener('fullscreenchange', handleFullscreenRecenter);

  /** Tear down all event listeners and DOM elements created by the shell. */
  function cleanup() {
    zoomCleanup();
    fsCleanup();
    document.removeEventListener('fullscreenchange', handleFullscreenRecenter);
    mqReducedMotion.removeEventListener('change', handleMotionChange);
    window.removeEventListener('resize', handleResize);
    titleOverlay.remove();
    backLink.remove();
    fsBtn.remove();
    hint.remove();
  }

  return { svg, g, width, height, zoom, cleanup };
}

/**
 * Escape HTML special characters to prevent XSS in dynamic content.
 * @param {string} str
 * @returns {string}
 */
function escapeHtml(str) {
  const div = document.createElement('div');
  div.textContent = str;
  return div.innerHTML;
}
