---
observed: 2026-07-21
provider: claude
cli_version: 2.1.205-era
shape: allow-deny-dialog
source: screenshot
---

Captured from the pa11-campaign-app session (screenshot, 2026-07-21
~21:20 local): the Claude-in-Chrome extension's connection dialog,
rendered in the terminal while the pane displayed a DIFFERENT menu
(the hook-synthesized 3-option prompt for
`mcp__claude-in-chrome__tabs_context_mcp`).

```
  Claude in Chrome wants to create a browser
  window and read your tabs

  ❯ 1. Allow
    2. Deny (esc)
```

Notes — detection axes:

- `1./2.` pair: matches (two numbered options).
- cursor: matches (`❯` on option 1).
- header: pre-fix **no match** ("wants to create a browser window…"
  matched nothing in `headerRe`); post-fix `wants to` matches.
- footer: none; the `(esc)` hint on Deny satisfies the codex-style
  keystroke signal.
- first option: `Allow` — failed the pre-fix `Yes…` anti-prose gate,
  which dropped the whole run.

Consequence when undetected: no grid report → claim-display
reconciliation never re-ran → the pane stayed frozen on the hook's
3-option menu while the terminal asked a 2-option question; pane
answers 2/3 keystroked meanings the terminal never displayed (a
rejected tool call earlier in the session is the likely casualty).

Detection since grid-detect-allow-deny-dialog: the **Allow family** —
first option `Allow`, rendered cursor REQUIRED (prose lists lack `❯`),
usual header/footer/keystroke-hint signal. Surfaces via grid-rescue
with the terminal's own labels; not a picker.

Family C. Sibling shapes to watch: other extension/connector consent
dialogs with Allow/Deny or Grant/Deny phrasing.
