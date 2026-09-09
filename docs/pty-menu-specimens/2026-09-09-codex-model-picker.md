---
observed: 2026-09-09
provider: codex
cli_version: codex-cli 0.153.3
shape: model-picker
source: screenshot
screenshot: 2026-09-09-codex-model-picker.png
---

Codex's model selector, captured by hand (Jon) alongside the Claude sibling.
First Codex picker specimen of any kind in this corpus.

```
Select Model and Effort
Access legacy models by running codex -m <…>          [clipped at column edge]

    1. gpt-6-astra (default)    Our most capable model for complex,
                                demanding work.
    2. gpt-5.6-sol              Reliable agentic workhorse for everyday
                                tasks.
    3. gpt-5.6-terra            Balanced agentic coding model for everyday
                                work.
  › 4. gpt-5.6-luna (current)   Fast and affordable agentic coding model.
    5. gpt-5.5                  Proven previous generation model for coding
                                and general work.

Press enter to confirm or esc to go back
```

Notes — detection axes:

- `1./2.` pair: matches (five numbered options).
- cursor: matches (`›` on row 4, also rendered in cyan/bold).
- header: **no match** — "Select Model and Effort" is a plain title.
- footer: **fails the strict picker test**, on the opposite clause from the
  Claude sibling. `main.js:1267` needs BOTH `/Enter to confirm/i` AND
  `/Esc to cancel/i`. This footer satisfies the first (`Press enter to
  confirm`) but says **"esc to go back"**, not "Esc to cancel". `pickerSignal`
  is false; not admitted.

So both providers' model pickers are undetected, each failing a different half
of the same two-part footer test. That symmetry is the useful finding: the test
encodes one CLI's exact footer wording rather than the *shape* of a
confirm/cancel footer.

Differences from the Claude sibling that rule out one shared handling path:

- **Current model marked in the label text** (`(current)`), not by a separate
  `✓` glyph. A detector keying on `✓` sees nothing here.
- **Two commit semantics** (`enter` confirm, `esc` **go back**) against
  Claude's three. "Go back" rather than "cancel" says this list is one page of
  a multi-step flow — consistent with the title promising *Effort* that this
  screen never shows.
- **The right column clips** at the terminal edge rather than wrapping, where
  Claude's wraps. Different grid-reading hazard: clipping can truncate the very
  text a detector keys on, and it is not the wrap-corruption boundary named at
  `pty-menu-shapes.md:14`.

Not yet captured: **the effort screen** reached by pressing Enter here. The
title advertises it and no specimen exists.

Sibling: `2026-09-09-claude-model-picker.md`.
