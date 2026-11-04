# The two published diagrams are D3 data, not artwork

`docs/diagrams/software-architecture.html` and `docs/diagrams/user-flow.html`
are self-contained pages: inline `<script type="module">` D3 code reading an
inline JavaScript data array (`crates` / stage arrays), plus shared helpers
in `docs/diagrams/shared/` (`embed.js`, `theme.js`, `interactions.js`,
`accessibility.js`). There is no external source (no Graphviz, no Mermaid
source file, no exported SVG) to regenerate from. **Editing the diagram means
editing the data array in the HTML file itself.**

Confirmed by reading the layout code in `software-architecture.html`: tier
width, node centering, and dependency arrows are all computed at render time
from the data.

- `depEdges` is built once, by iterating every crate's `deps` array — there
  is no separately maintained edge list to keep in sync.
- A tier's width and node spacing come from `tier.crateIds.length`; removing
  or adding a crate re-centers its tier automatically.

So adding, removing, or renaming a crate is three data edits with no
coordinate math: delete/add the node object in `crates`, update any other
node's `deps` array that references it, and update the owning tier's
`crateIds`. Same pattern for `user-flow.html`'s stage nodes — a node's
`children` array drives both its on-canvas detail count and its expanded
content; there's no separate "N details" label to edit.

## Verifying a redraw

The pages import D3 as an ES module from a CDN and `shared/*.js` via
relative imports, so opening the file directly (`file://`) fails with a CORS
error on the module imports and the diagram never renders — this looks like
a build problem but is a browser security restriction, not a code defect.
Serve `docs/` over HTTP first:

```bash
cd docs && python3 -I -m http.server 8911
```

Then load `http://localhost:8911/diagrams/<file>.html` in a real browser, or
headlessly with Playwright (`npx playwright install chromium` the first
time; `chromium.launch()` + `page.goto(url)` + `page.screenshot()`). Check
the browser/page console for errors — a stale reference to a removed node id
throws at render time rather than silently drawing nothing.
