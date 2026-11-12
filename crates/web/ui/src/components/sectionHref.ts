/**
 * The `href` a section entry carries. PatternFly renders a `NavItem` with
 * no `to` as an `<a>` with no `href`, which is neither a link to assistive
 * technology nor focusable, so Tab, number-key focus and the Ctrl+Shift
 * shortcuts could not reach it. The click is still handled in place
 * (`preventDefault`); the hash is never followed.
 */
export function sectionHref(sectionId: string): string {
  return `#${sectionId}`;
}
