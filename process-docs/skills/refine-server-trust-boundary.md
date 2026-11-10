# Refine server trust boundary

The refine server (`crates/web/src/lib.rs`, served by `inspectah refine`,
its only caller) binds `127.0.0.1` and holds the whole session: host
inventory, config contents, and with `--preserve password-hashes` the
hashes, readable through `GET /api/user-preview?reveal=true`. Every API
route is reachable by any client that can open a loopback TCP connection.
What stands between that and a hostile caller is two guards, and each one
covers a different caller.

## The guards, outermost first

1. **`host_guard`: the request must be addressed to a loopback name.**
   The `Host` header, and an absolute-form request target's authority when
   there is one, must name `127.0.0.1`, `localhost` or `[::1]`, with any
   port or none. More than one `Host` field is refused, even when every
   value is loopback. An HTTP/1.x request must carry exactly one `Host`,
   absolute-form target or not (RFC 9112 §3.2). An HTTP/2 request may
   carry its authority in `:authority` alone, which hyper maps into the
   request URI, and that authority is checked the same way (RFC 9113
   §8.3.1); when it also sends `Host`, both must pass. A request naming
   no authority at all is refused. This is the DNS-rebinding
   defense: once an attacker's domain resolves to 127.0.0.1, its page is
   same-origin with refine and the browser lets it read every GET; `Host`
   is the one thing that still carries the attacker's name. Any port is
   accepted on purpose, so an SSH forward to another local port still
   loads the page.
2. **`origin_guard`: a POST or DELETE with an `Origin` must carry the
   served origin**, port included, with `localhost` and `127.0.0.1` treated
   as the same. This is the CSRF defense against cross-site pages. A
   request with no `Origin` passes, because `curl` and other non-browser
   clients send none, and browsers send `Origin` on every non-GET request.
   `Origin: null` (sandboxed iframes, `data:` URLs, cross-origin redirects)
   is a present, foreign origin and is refused; `null_origin_post_rejected`
   pins that.

CORS is not a guard here. A rebinding page is same-origin, and a
`text/plain` form POST needs no preflight; `origin_guard` is what holds.

**Not covered: other local users.** Neither guard authenticates the
client. Any process on the host can `curl` the API with a loopback `Host`
and no `Origin`, reads and writes alike. That matters wherever refine runs
on a multi-user host.

The documented workflow is workstation-first (scan on the host, copy the
tarball, run `inspectah refine` locally), so the SSH tunnel is now a
caveated side note. The tunnel, with refine running on a shared host, is
the case the deferred per-session token would protect.

## The fallback-layer trap (axum 0.8)

`Router::layer` wraps only the routes and the fallback that exist when it
is called. A `.fallback()` added after `.layer()` is installed unlayered.
In `router()` the static-asset fallback comes after the API layers, so
`origin_guard`, CORS and the body limit do not reach it. That is harmless
for static files, but a guard meant to cover everything, like
`host_guard`, has to be applied **after** `.fallback(...)`. The Host tests
request `/`, `/assets/x.js` and an unknown `/api/` path to prove it.
Moving the fallback, or adding a guard, means re-checking that ordering.

## Tests: the router refuses a request with no Host

A `tower::ServiceExt::oneshot` request built with `Request::builder()`
carries no `Host`, so the raw router answers it with 403. The `app()`
helper in each `crates/web/tests/*.rs` file layers `with_loopback_host` on
top, which inserts `Host: localhost:8642` only when the test set none.
Use the helper for handler tests; use `inspectah_web::router(...)`
directly when the test controls the `Host` fields itself (none, two) or
the HTTP version. `Request::builder()` defaults to HTTP/1.1; an HTTP/2
request with only `:authority` is built with `.version(Version::HTTP_2)`
and an absolute URI, which is how hyper hands one to the router.
HTTP/1.1 clients over a real socket (`reqwest`, `curl`, browsers) always
send `Host`.

## The Vite dev proxy

`vite.config.ts` proxies `/api` to refine with `changeOrigin: true`, which
rewrites `Host` only. Without more, every change from the dev page fails
`origin_guard` (its `Origin` is the Vite port). The proxy rewrites
`Origin` to refine's served origin **only** when it is the dev page's own
origin: equal to `http://<Host>` on a loopback name
(`src/dev/proxyOrigin.ts`). Never rewrite every `Origin`: that launders
any foreign page that can reach the dev server past the CSRF check.
Vite's own `allowedHosts` check stops rebinding against the dev server.

## Related

- `docs/getting-started.md`, refine section: the documented workstation-first workflow and the caveated SSH note.
- [real-host-validation-runs](real-host-validation-runs.md): driving refine
  over HTTP with `curl` (no `Origin`, loopback `Host`).
