/**
 * Dev-server only: decides the `Origin` the Vite proxy forwards to refine.
 *
 * Refine refuses a POST or DELETE whose `Origin` is not the origin it
 * serves, so a page loaded from the Vite dev server (another port) can
 * never change anything through the proxy. Rewriting every `Origin` to
 * refine's would fix that and also switch the check off: any page able to
 * reach the dev server would have its foreign origin laundered. So only
 * the dev page's own origin is rewritten, recognised by being the origin
 * of the `Host` it requested, on a loopback name. Anything else passes
 * through unchanged and refine refuses it as before.
 */

/** Where the dev proxy sends `/api`; the origin refine serves by default. */
export const REFINE_ORIGIN = "http://127.0.0.1:8642";

const LOOPBACK_HOST = /^(localhost|127\.0\.0\.1|\[::1\])(:\d+)?$/i;

export function proxiedOrigin(
  origin: string | undefined,
  host: string | undefined,
): string | undefined {
  if (
    origin !== undefined &&
    host !== undefined &&
    LOOPBACK_HOST.test(host) &&
    origin.toLowerCase() === `http://${host}`.toLowerCase()
  ) {
    return REFINE_ORIGIN;
  }
  return origin;
}
