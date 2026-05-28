/**
 * Keyboard navigation and ARIA support for inspectah D3 diagrams.
 * First-class accessibility — not bolted on.
 * @module accessibility
 */

/**
 * Enable keyboard navigation between focusable diagram nodes.
 *
 * Bindings:
 * - Tab / Shift+Tab — cycle through nodes in DOM order
 * - Enter / Space — toggle expand on the focused node
 * - Escape — collapse the currently expanded node
 * - Arrow keys — spatial navigation (moves to nearest neighbor in that direction)
 *
 * @param {d3.Selection} nodes — D3 selection of `.node` group elements. Each must have `tabindex="0"`.
 * @returns {{ cleanup: () => void }} — teardown function to remove listeners.
 */
export function setupKeyboardNav(nodes) {
  const elements = nodes.nodes();

  /**
   * Find the nearest node in the given direction from the currently focused element.
   * @param {SVGGElement} current
   * @param {'ArrowUp'|'ArrowDown'|'ArrowLeft'|'ArrowRight'} direction
   * @returns {SVGGElement|null}
   */
  function findSpatialNeighbor(current, direction) {
    const currentRect = current.getBoundingClientRect();
    const cx = currentRect.left + currentRect.width / 2;
    const cy = currentRect.top + currentRect.height / 2;

    let best = null;
    let bestDist = Infinity;

    for (const el of elements) {
      if (el === current) continue;
      const r = el.getBoundingClientRect();
      const ex = r.left + r.width / 2;
      const ey = r.top + r.height / 2;

      const dx = ex - cx;
      const dy = ey - cy;

      // Filter by direction — only consider candidates in the right quadrant
      let valid = false;
      switch (direction) {
        case 'ArrowUp':    valid = dy < -10; break;
        case 'ArrowDown':  valid = dy > 10;  break;
        case 'ArrowLeft':  valid = dx < -10; break;
        case 'ArrowRight': valid = dx > 10;  break;
      }
      if (!valid) continue;

      const dist = Math.sqrt(dx * dx + dy * dy);
      if (dist < bestDist) {
        bestDist = dist;
        best = el;
      }
    }

    return best;
  }

  /** @param {KeyboardEvent} event */
  function handleKeydown(event) {
    const target = event.currentTarget;

    switch (event.key) {
      case 'Enter':
      case ' ':
        event.preventDefault();
        target.click();
        break;

      case 'Escape':
        event.preventDefault();
        // Dispatch a custom event that diagram code can listen for
        target.dispatchEvent(new CustomEvent('diagram:collapse', { bubbles: true }));
        break;

      case 'ArrowUp':
      case 'ArrowDown':
      case 'ArrowLeft':
      case 'ArrowRight': {
        event.preventDefault();
        const neighbor = findSpatialNeighbor(target, event.key);
        if (neighbor) neighbor.focus();
        break;
      }
    }
  }

  nodes.attr('tabindex', '0');
  nodes.each(function () {
    this.addEventListener('keydown', handleKeydown);
  });

  function cleanup() {
    nodes.each(function () {
      this.removeEventListener('keydown', handleKeydown);
    });
  }

  return { cleanup };
}

/**
 * Manage focus trapping inside expanded detail panels and restore focus on collapse.
 *
 * @param {HTMLElement} container — the container element (usually the SVG parent div).
 * @returns {{ trapFocus: (panel: HTMLElement, returnTo: HTMLElement) => void, releaseFocus: () => void, cleanup: () => void }}
 */
export function setupFocusManagement(container) {
  let returnTarget = null;

  /** @param {KeyboardEvent} event */
  function trapHandler(event) {
    if (event.key !== 'Tab') return;

    const panel = container.querySelector('[data-focus-trap]');
    if (!panel) return;

    const focusable = panel.querySelectorAll(
      'a[href], button, [tabindex]:not([tabindex="-1"]), input, select, textarea'
    );
    if (focusable.length === 0) return;

    const first = focusable[0];
    const last = focusable[focusable.length - 1];

    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  }

  container.addEventListener('keydown', trapHandler);

  return {
    /**
     * Activate focus trap on a panel.
     * @param {HTMLElement} panel — the expanded detail panel.
     * @param {HTMLElement} returnTo — element to restore focus to on collapse.
     */
    trapFocus(panel, returnTo) {
      returnTarget = returnTo;
      panel.setAttribute('data-focus-trap', 'true');
      // Focus the first focusable child, or the panel itself
      const first = panel.querySelector(
        'a[href], button, [tabindex]:not([tabindex="-1"])'
      );
      if (first) {
        first.focus();
      } else {
        panel.setAttribute('tabindex', '-1');
        panel.focus();
      }
    },

    /** Release focus trap and restore focus to the triggering element. */
    releaseFocus() {
      const panel = container.querySelector('[data-focus-trap]');
      if (panel) panel.removeAttribute('data-focus-trap');
      if (returnTarget) {
        returnTarget.focus();
        returnTarget = null;
      }
    },

    /** Remove event listeners. */
    cleanup() {
      container.removeEventListener('keydown', trapHandler);
    },
  };
}

/**
 * Check whether the user prefers reduced motion.
 * All animation code should call this and skip/simplify transitions when true.
 * @returns {boolean}
 */
export function checkReducedMotion() {
  return window.matchMedia('(prefers-reduced-motion: reduce)').matches;
}

/**
 * Build an object of ARIA attributes for a diagram node.
 * Spread onto the node element via D3's `.attr()` or direct assignment.
 *
 * @param {{ id: string, label: string, description?: string }} node — node data.
 * @param {boolean} isExpandable — whether the node supports expand/collapse.
 * @param {boolean} isExpanded — current expanded state.
 * @returns {Record<string, string>} — attribute key-value pairs.
 */
export function ariaAttributes(node, isExpandable, isExpanded) {
  const attrs = {
    'role': isExpandable ? 'button' : 'img',
    'aria-label': node.label,
  };

  if (node.description) {
    attrs['aria-description'] = node.description;
  }

  if (isExpandable) {
    attrs['aria-expanded'] = String(isExpanded);
    attrs['aria-haspopup'] = 'true';
  }

  return attrs;
}
