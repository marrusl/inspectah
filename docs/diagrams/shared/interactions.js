/**
 * Interaction utilities for inspectah diagrams.
 * Zoom/pan, tooltips, expand/collapse with animated transitions.
 * @module interactions
 */

import * as d3 from 'https://cdn.jsdelivr.net/npm/d3@7/+esm';
import { checkReducedMotion } from './accessibility.js';

/**
 * Configure D3 zoom behavior on an SVG with a transformable inner group.
 * @param {d3.Selection} svg  - Root SVG selection.
 * @param {d3.Selection} g    - Inner `<g>` that receives the transform.
 * @param {Object}       [opts]
 * @param {number[]}     [opts.scaleExtent=[0.3, 3]] - Min/max zoom scale.
 * @param {number}       [opts.transitionMs=300]      - Duration for programmatic zooms.
 * @returns {{ zoom: d3.ZoomBehavior, cleanup: Function }}
 */
export function setupZoom(svg, g, opts = {}) {
  const { scaleExtent = [0.3, 3], transitionMs = 300 } = opts;
  const reducedMotion = checkReducedMotion();
  const duration = reducedMotion ? 0 : transitionMs;

  const zoom = d3.zoom()
    .scaleExtent(scaleExtent)
    .on('zoom', (event) => {
      g.attr('transform', event.transform);
    });

  svg.call(zoom);

  // Prevent scroll from propagating when embedded
  svg.on('wheel.passthrough', null);

  function cleanup() {
    svg.on('.zoom', null);
  }

  return { zoom, cleanup };
}

/**
 * Create a tooltip attached to a container element.
 * @param {HTMLElement|d3.Selection} container - Parent element for the tooltip div.
 * @returns {{ show: Function, hide: Function, move: Function, el: HTMLElement, cleanup: Function }}
 */
export function setupTooltip(container) {
  const parent = container instanceof HTMLElement
    ? container
    : container.node();

  const el = document.createElement('div');
  el.className = 'diagram-tooltip';
  el.setAttribute('role', 'tooltip');
  el.id = `tooltip-${Date.now()}`;
  parent.appendChild(el);

  /**
   * Show the tooltip with HTML content near the cursor.
   * @param {string} content - HTML string to display.
   * @param {MouseEvent} event - Mouse event for positioning.
   */
  function show(content, event) {
    el.innerHTML = content;
    el.classList.add('visible');
    position(event);
  }

  /** Hide the tooltip. */
  function hide() {
    el.classList.remove('visible');
  }

  /**
   * Reposition the tooltip near the cursor.
   * @param {MouseEvent} event
   */
  function move(event) {
    position(event);
  }

  function position(event) {
    const pad = 12;
    const rect = el.getBoundingClientRect();
    const vw = window.innerWidth;
    const vh = window.innerHeight;

    let x = event.clientX + pad;
    let y = event.clientY + pad;

    // Flip horizontally if overflowing right
    if (x + rect.width > vw - pad) {
      x = event.clientX - rect.width - pad;
    }
    // Flip vertically if overflowing bottom
    if (y + rect.height > vh - pad) {
      y = event.clientY - rect.height - pad;
    }

    el.style.left = `${Math.max(pad, x)}px`;
    el.style.top = `${Math.max(pad, y)}px`;
  }

  function cleanup() {
    el.remove();
  }

  return { show, hide, move, el, cleanup };
}

/**
 * Manage expand/collapse state for a diagram node.
 * Tracks which nodes are expanded and calls the render function on toggle.
 * @param {string}   nodeId    - Unique node identifier.
 * @param {Map}      expanded  - Map<string, boolean> tracking expand states.
 * @param {Function} renderFn  - Called with (nodeId, isExpanded) after state change.
 */
export function toggleExpand(nodeId, expanded, renderFn) {
  const isExpanded = expanded.get(nodeId) || false;
  expanded.set(nodeId, !isExpanded);
  renderFn(nodeId, !isExpanded);
}

/**
 * Auto-center and fit content within the SVG viewport.
 * @param {d3.Selection}    svg    - Root SVG selection.
 * @param {d3.ZoomBehavior} zoom   - The zoom behavior attached to the SVG.
 * @param {{ x: number, y: number, width: number, height: number }} bounds
 *   Bounding box of the content to center.
 * @param {Object}  [opts]
 * @param {number}  [opts.padding=40]        - Padding around content in px.
 * @param {number}  [opts.transitionMs=500]  - Animation duration.
 */
export function centerView(svg, zoom, bounds, opts = {}) {
  const { padding = 40, transitionMs = 500 } = opts;
  const reducedMotion = checkReducedMotion();
  const duration = reducedMotion ? 0 : transitionMs;

  const svgNode = svg.node();
  const { width: vw, height: vh } = svgNode.getBoundingClientRect();

  const contentW = bounds.width || 1;
  const contentH = bounds.height || 1;

  const scale = Math.min(
    (vw - padding * 2) / contentW,
    (vh - padding * 2) / contentH,
    1.5 // Don't zoom in too far
  );

  const tx = (vw / 2) - (bounds.x + contentW / 2) * scale;
  const ty = (vh / 2) - (bounds.y + contentH / 2) * scale;

  const transform = d3.zoomIdentity.translate(tx, ty).scale(scale);

  if (duration > 0) {
    svg.transition()
      .duration(duration)
      .call(zoom.transform, transform);
  } else {
    svg.call(zoom.transform, transform);
  }
}
