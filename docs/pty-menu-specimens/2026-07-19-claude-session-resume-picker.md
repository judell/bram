---
observed: 2026-07-19
provider: claude
cli_version: unknown (Eric's machine)
shape: session-resume-picker
source: trace-excerpt
---

Captured by the `send-capture` at-send snapshot in Eric's 2026-07-19
session trace (`bram-trace.log`, 20:08:34) — the terminal rows at the
exact moment a pane send was injected into this undetected picker:

```
  This session is 1d 2h old and 383.2k tokens.

  Resuming the full session will consume a
  substantial portion of your usage limits. We
  recommend resuming from a summary.

  ❯ 1. Resume from summary (recommended)
    2. Resume full session as-is
    3. Don't ask me again

  Enter to confirm · Esc to cancel
```

Notes — detection axes:

- `1./2.` pair: matches (three numbered options).
- cursor: matches (`❯` on the selected option).
- header: **no match** — no "Do you want to…" / "requires approval" /
  "Would you like to run" phrasing; the banner is free prose ("This
  session is 1d 2h old…").
- footer: partial — "Esc to cancel" matches the permission footer, but
  option 1 is not "Yes…", so the pre-fix `__gridDetectMenu` Yes-gate
  dropped the run entirely (`menuPresent=false, optionCount=0` in the
  capture meta).

Consequence when undetected: the injected bracketed paste was swallowed
by the picker and the trailing CR confirmed option 1 ("Resume from
summary"), silently changing session state; the send stranded
(`strand-forensics.log` 20:08:34 → 20:11:45, `payload_in_tail=false`).

Detection since detect-session-resume-picker: admitted as the **picker
family** — non-"Yes" option 1 accepted only on the strict footer (BOTH
"Enter to confirm" AND "Esc to cancel" directly below the block) plus a
rendered cursor. Surfaces as `tool=Picker` (grid-rescue path and
`op=build-picker`), which also arms the send-gate hold.

Family C (select-from-list). Sibling shapes to watch for: the trust
dialog and `/resume` session list share the Enter-confirm footer
structure.
