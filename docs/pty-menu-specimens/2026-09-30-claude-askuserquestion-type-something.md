---
observed: 2026-09-30
provider: claude
cli_version: 2.1.286
shape: askuserquestion
source: bram-trace (resources/bram-traces/bram-trace.log, [pty-out])
---

Claude Code *AskUserQuestion* (Family B): how free text reaches it.
Captured in two runs while building
`askuserquestion-card-type-something-and-chat`, each asking a
throwaway two-option question ("Which fruit?", Apple / Pear). Claude
rendered its usual extra rows below the agent's options:
**3. Type something.** and **4. Chat about this**.

## 1. A pane send while the question is open: the text is lost

Jon typed "ok here is random free text" in the pane's message box, with
no Worklist row ticked, and sent it. At the time, the send gate
exempted AskUserQuestion. The bytes that went out:

```
21:17:21.785Z [pty-out] bytes=176 "\x1b\x15\x1b[200~Read and follow this Bram turn: @resources/outbound-turns/1790803041784-turn.json …\x1b[201~"
21:17:21.880Z [pty-out] bytes=1   "\r"
```

The answer was recorded as **"Apple"**, option 1: the paste was ignored
and the Enter chose the highlighted option. The message was lost. This
falsifies the 2026-07-19 note ("typing over it is a legitimate answer
path"), and the gate now holds pane sends while any menu is open,
AskUserQuestion included.

## 2. Typing in the terminal: key N+1, then text, then Enter

Jon clicked into the terminal, pressed **3**, typed "random input" (with
some corrections), and pressed Enter:

```
21:18:18.242Z [pty-out] "3"          <- Claude switches to its input line at once; no Enter
21:18:18.242Z [pty-menu] state=dismissed tool=AskUserQuestion reason=user-input
21:18:21.495Z [pty-out] "r" "a" "n" "d" "o" "m" " " …   (one byte per key, backspaces as \x7f)
             [pty-out] "\r"
```

The answer was recorded as **"random input"**.

## Notes

- A digit acts immediately in this menu (no Enter), both for the
  agent's options and for Claude's own rows.
- Bram marked the menu dismissed at the digit, before the text was
  typed. That's why the card now sends the digit only when the typed
  answer is complete.
- Not yet observed: whether the free text survives as a **single write**
  (the card sends the digit, then the text as one write, then `\r`, with
  short pauses between), or as a bracketed paste. The card avoids both
  paste and a combined digit+text write for that reason. The first live
  use of the card's Send is the check.
- "Chat about this" (key N+2) wasn't captured here.
