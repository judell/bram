# xmlui navigation repro

A minimal check that a `DataSource` carrying a nested request payload does not
pin the router. Run it after every revendor of
`app/vendor/xmlui-standalone.umd.js`.

It exists because Bram documents a `/query` `DataSource` pattern
(`app/__shell/conventions.md` §*Live SQL views via `/query`*) that Bram itself
never runs — `app/tools/` has 54 `DataSource` elements and zero with any
`dataType`. An upstream regression in that pattern is therefore invisible here
and surfaces only in a downstream project, which is exactly how judell/bram#371
happened.

The server deliberately serves the **real** `app/vendor/xmlui-standalone.umd.js`
rather than a copy, so what you test is what we ship.

## Run

```sh
python3 scripts/xmlui-nav-repro/serve.py     # http://127.0.0.1:8791/one
```

Open `http://127.0.0.1:8791/one`, then: click **Two**, click **One**.

- **Pass** — the page follows the URL: `PAGE ONE` → `PAGE TWO` → `PAGE ONE`.
- **Fail** — the URL updates on every click (`#/one`, `#/two`, `#/one`) while
  the rendered body stays `PAGE TWO` forever. Silent: no console errors, no
  unhandled rejections, and no request loop.

`REPRO_PORT` overrides the port.

## The full matrix

`Main.xmlui` ships the failing shape (POST + nested `body` + `dataType="sql"`).
The other rows are what isolate the cause; edit `Main.xmlui` to reproduce them.

| DataSource on page two | pre-0.14.27 |
|---|---|
| none | NAV OK |
| `url="/rows.json"` (GET, no body) | NAV OK |
| `url="/query" method="POST"`, no body | NAV OK |
| `url="/query" method="POST" body="{{ sql: '…', params: [] }}"` | **WEDGED** |
| same, plus `dataType="sql"` | **WEDGED** |

Two conclusions worth keeping: `dataType="sql"` is incidental — a plain POST
with a nested body wedges identically — and it is not a fetch loop, which the
server log confirms (a handful of POSTs, not thousands).

## Mechanism

Fixed upstream in xmlui **0.14.27** by
[`cf138ea`](https://github.com/xmlui-org/xmlui/commit/cf138eaa964ecb5254387b433a56c381b6fb05e0)
(xmlui-org/xmlui#3889), which swaps `useShallowCompareMemoize` for
`useDeepCompareMemoize` on `body`, `queryParams`, `rawBody`, and `mockData` in
`DataLoader.tsx`.

A **nested** payload (`params: []`) is a fresh reference on every render, and a
shallow compare calls that a change. The `queryId` identity churns, which
re-runs the loader's `registerComponentApi` effect, which writes component-API
state, which re-renders — a loop that starves React's concurrent rendering, so
router transitions never commit.

So the trigger is *nesting*, not merely having a `body`, and `queryParams` and
`mockData` carry the same hazard.

## Why this is not in CI

It needs a real browser and a live server. Browser-per-worker suites launched
from an agent session have twice frozen every webview on this machine (see
`app/__shell/conventions.md` §*Resource-heavy test suites*). This has the same
standing as `scripts/setup-harness.sh`: a command a person or agent runs when it
matters — at revendor time.
