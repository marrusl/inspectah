/**
 * Accessibility utilities for inspectah diagrams.
 * Keyboard navigation, focus management, ARIA attributes, reduced-motion detection.
 * @module accessibility
 */

/**
 * Check whether the user prefers reduced motion.
 * @returns {boolean} True if reduced motion is preferred.
 */
export function checkReducedMotion() {
  return window.matchMedia('(prefers-reduced-motion: reduce)').matches;
}

/**
 * Set up keyboard navigation for diagram nodes.
 * - Tab / Shift+Tab: move between nodes in order
 * - Enter / Space: activate (expand/collapse) focused node
 * - Escape: collapse any expanded node, restore focus
 * - Arrow keys: spatial navigation (Up/Down/Left/Right)
 *
 * @param {d3.Selection} nodes     - D3 selection of focusable node groups.
 * @param {Object}       [opts]
 * @param {Function}     [opts.onActivate]  - Called with (nodeElement, nodeData) on Enter/Space.
 * @param {Function}     [opts.onEscape]    - Called when Escape is pressed.
 * @param {string}       [opts.nodeSelector='.node'] - CSS selector for nodes.
 * @returns {{ cleanup: Function }}
 */
export function setupKeyboardNav(nodes, opts = {}) {
  const { onActivate, onEscape, nodeSelector = '.node' } = opts;

  const nodeElements = nodes.nodes ? nodes.nodes() : Array.from(nodes);

  // Ensure all nodes are focusable
  nodeElements.forEach((el, i) => {
    if (!el.getAttribute('tabindex')) {
      el.setAttribute('tabindex', i === 0 ? '0' : '-1');
    }
  });

  function getNodeCenter(el) {
    const rect = el.getBoundingClientRect();
    return { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 };
  }

  function findSpatialNeighbor(current, direction) {
    const origin = getNodeCenter(current);
    let best = null;
    let bestDist = Infinity;

    for (const el of nodeElements) {
      if (el === current) continue;
      const pos = getNodeCenter(el);
      const dx = pos.x - origin.x;
      const dy = pos.y - origin.y;

      let valid = false;
      switch (direction) {
        case 'ArrowUp':    valid = dy < -10; break;
        case 'ArrowDown':  valid = dy > 10;  break;
        case 'ArrowLeft':  valid = dx < -10; break;
        case 'ArrowRight': valid = dx > 10;  break;
      }

      if (valid) {
        const dist = Math.sqrt(dx * dx + dy * dy);
        if (dist < bestDist) {
          bestDist = dist;
          best = el;
        }
      }
    }
    return best;
  }

  function handleKeydown(event) {
    const current = event.target.closest(nodeSelector);
    if (!current) return;

    const currentIdx = nodeElements.indexOf(current);

    switch (event.key) {
      case 'Tab':
        // Let browser handle Tab naturally across nodes
        return;

      case 'Enter':
      case ' ':
        event.preventDefault();
        if (onActivate) {
          const data = current.__data__;
          onActivate(current, data);
        }
        break;

      case 'Escape':
        event.preventDefault();
        if (onEscape) onEscape();
        break;

      case 'ArrowUp':
      case 'ArrowDown':
      case 'ArrowLeft':
      case 'ArrowRight': {
        event.preventDefault();
        const neighbor = findSpatialNeighbor(current, event.key);
        if (neighbor) {
          // Roving tabindex pattern
          current.setAttribute('tabindex', '-1');
          neighbor.setAttribute('tabindex', '0');
          neighbor.focus();
        }
        break;
      }

      default:
        return;
    }
  }

  // Attach at the container level (event delegation)
  const container = nodeElements[0]?.closest('svg') || document;
  container.addEventListener('keydown', handleKeydown);

  function cleanup() {
    container.removeEventListener('keydown', handleKeydown);
  }

  return { cleanup };
}

/**
 * Manage focus trapping within expanded content panels.
 * Traps Tab focus inside the panel and restores focus to the trigger on close.
 *
 * @param {HTMLElement} container - The container element (usually the SVG parent).
 * @returns {{ trap: Function, release: Function, cleanup: Function }}
 */
export function setupFocusManagement(container) {
  let triggerElement = null;
  let trapElement = null;
  let trapHandler = null;

  /**
   * Trap focus inside a panel element.
   * @param {HTMLElement} panel   - The expanded panel to trap focus in.
   * @param {HTMLElement} trigger - The element that opened the panel (restored on close).
   */
  function trap(panel, trigger) {
    triggerElement = trigger;
    trapElement = panel;

    // Find all focusable elements inside the panel
    const focusable = getFocusableElements(panel);
    if (focusable.length === 0) {
      // Make the panel itself focusable as fallback
      panel.setAttribute('tabindex', '-1');
      panel.focus();
      return;
    }

    focusable[0].focus();

    trapHandler = (event) => {
      if (event.key !== 'Tab') return;

      const elements = getFocusableElements(panel);
      if (elements.length === 0) return;

      const first = elements[0];
      const last = elements[elements.length - 1];

      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };

    panel.addEventListener('keydown', trapHandler);
  }

  /** Release the focus trap and restore focus to the original trigger. */
  function release() {
    if (trapHandler && trapElement) {
      trapElement.removeEventListener('keydown', trapHandler);
      trapHandler = null;
    }
    if (triggerElement) {
      triggerElement.focus();
      triggerElement = null;
    }
    trapElement = null;
  }

  function cleanup() {
    release();
  }

  return { trap, release, cleanup };
}

/**
 * Get all focusable elements inside a container.
 * @param {HTMLElement} container
 * @returns {HTMLElement[]}
 */
function getFocusableElements(container) {
  const selector = [
    'a[href]',
    'button:not([disabled])',
    'input:not([disabled])',
    'select:not([disabled])',
    'textarea:not([disabled])',
    '[tabindex]:not([tabindex="-1"])',
  ].join(', ');

  return Array.from(container.querySelectorAll(selector));
}

/**
 * Build an object of ARIA attributes for a diagram node.
 * @param {{ id: string, label: string, description?: string }} node
 *   Node data with at least an id and label.
 * @param {boolean} isExpandable - Whether this node can be expanded.
 * @param {boolean} isExpanded   - Whether this node is currently expanded.
 * @returns {Object<string, string>} Map of ARIA attribute names to values.
 */
export function ariaAttributes(node, isExpandable, isExpanded) {
  const attrs = {
    'role': isExpandable ? 'treeitem' : 'img',
    'aria-label': node.label || node.id,
    'tabindex': '0',
  };

  if (node.description) {
    attrs['aria-description'] = node.description;
  }

  if (isExpandable) {
    attrs['aria-expanded'] = String(isExpanded);
  }

  return attrs;
}
