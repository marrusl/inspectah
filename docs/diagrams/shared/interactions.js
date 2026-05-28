/**
 * Shared interaction behaviors for inspectah D3 diagrams.
 * Zoom/pan, tooltips, expand/collapse, viewport centering.
 * @module interactions
 */

import * as d3 from 'https://cdn.jsdelivr.net/npm/d3@7/+esm';
import { checkReducedMotion } from './accessibility.js';

/**
 * Set up D3 zoom/pan behavior on an SVG element.
 * @param {d3.Selection} svg — root SVG selection.
 * @param {d3.Selection} g — inner <g> group that receives the transform.
 * @returns {{ zoom: d3.ZoomBehavior, cleanup: () => void }} — zoom instance and teardown function.
 */
export function setupZoom(svg, g) {
  const zoom = d3.zoom()
    .scaleExtent([0.3, 3])
    .on('zoom', (event) => {
      g.attr('transform', event.transform);
    });

  svg.call(zoom);

  // Prevent scroll-hijacking when cursor isn't over the diagram
  svg.on('wheel.zoom', function (event) {
    event.preventDefault();
    zoom.scaleBy(
      checkReducedMotion() ? svg : svg.transition().duration(150),
      event.deltaY < 0 ? 1.1 : 0.9,
    );
  }, { passive: false });

  /** Remove zoom listeners. */
  function cleanup() {
    svg.on('.zoom', null);
  }

  return { zoom, cleanup };
}

/**
 * Create a tooltip element and return show/hide/move helpers.
 * @param {HTMLElement|d3.Selection} container — element to append the tooltip div to.
 * @returns {{ show: (content: string, event: MouseEvent) => void, hide: () => void, move: (event: MouseEvent) => void, el: HTMLDivElement }}
 */
export function setupTooltip(container) {
  const parent = container instanceof HTMLElement
    ? container
    : container.node();

  const el = document.createElement('div');
  el.className = 'diagram-tooltip';
  el.setAttribute('role', 'tooltip');
  parent.appendChild(el);

  /**
   * Position the tooltip near the cursor, clamped to viewport edges.
   * @param {MouseEvent} event
   */
  function position(event) {
    const pad = 12;
    let x = event.clientX + pad;
    let y = event.clientY + pad;

    const rect = el.getBoundingClientRect();
    if (x + rect.width > window.innerWidth - pad) {
      x = event.clientX - rect.width - pad;
    }
    if (y + rect.height > window.innerHeight - pad) {
      y = event.clientY - rect.height - pad;
    }

    el.style.left = `${x}px`;
    el.style.top = `${y}px`;
  }

  return {
    /**
     * Show the tooltip with HTML content.
     * @param {string} content — HTML string for the tooltip body.
     * @param {MouseEvent} event — mouse event for initial positioning.
     */
    show(content, event) {
      el.innerHTML = content;
      el.classList.add('visible');
      position(event);
    },

    /** Hide the tooltip. */
    hide() {
      el.classList.remove('visible');
    },

    /**
     * Update tooltip position (call on mousemove).
     * @param {MouseEvent} event
     */
    move(event) {
      position(event);
    },

    /** The tooltip DOM element, for direct manipulation if needed. */
    el,
  };
}

/**
 * Manage expand/collapse state for diagram nodes.
 * @param {string} nodeId — unique node identifier.
 * @param {Map<string, boolean>} expanded — mutable state map of expanded nodes.
 * @param {(expanded: Map<string, boolean>) => void} renderFn — callback to re-render the diagram.
 */
export function toggleExpand(nodeId, expanded, renderFn) {
  if (expanded.has(nodeId) && expanded.get(nodeId)) {
    expanded.set(nodeId, false);
  } else {
    expanded.set(nodeId, true);
  }
  renderFn(expanded);
}

/**
 * Auto-center and fit content within the SVG viewport.
 * Smoothly transitions unless reduced motion is preferred.
 * @param {d3.Selection} svg — root SVG selection.
 * @param {d3.ZoomBehavior} zoom — the zoom behavior instance.
 * @param {{ x: number, y: number, width: number, height: number }} bounds — content bounding box.
 * @param {{ padding?: number }} [opts] — optional padding around the content.
 */
export function centerView(svg, zoom, bounds, opts = {}) {
  const padding = opts.padding ?? 60;
  const svgNode = svg.node();
  const { width: vw, height: vh } = svgNode.getBoundingClientRect();

  const contentW = bounds.width + padding * 2;
  const contentH = bounds.height + padding * 2;
  const scale = Math.min(vw / contentW, vh / contentH, 1);

  const tx = (vw - bounds.width * scale) / 2 - bounds.x * scale;
  const ty = (vh - bounds.height * scale) / 2 - bounds.y * scale;

  const transform = d3.zoomIdentity.translate(tx, ty).scale(scale);

  if (checkReducedMotion()) {
    svg.call(zoom.transform, transform);
  } else {
    svg.transition().duration(500).call(zoom.transform, transform);
  }
}
