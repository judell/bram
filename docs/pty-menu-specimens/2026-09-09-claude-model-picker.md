---
observed: 2026-09-09
provider: claude
cli_version: 2.1.236
shape: model-picker
source: screenshot
screenshot: 2026-09-09-claude-model-picker.png
---

`/model` in Claude Code. Captured by hand (Jon) while gathering evidence for
the model-reselection work; the corpus had **no** model-picker specimen for
either provider before this.

```
Select model
Switch between Claude models. Your pick becomes the default for new
sessions. For other/previous model names, specify with --model.

    1. Default (recommended)   Opus 5 with 1M context · Best for everyday,
                               complex tasks
  › 2. Opus (1M context) ✓     Opus 5 with 1M context · Best for everyday,
                               complex tasks
    3. Fable                   Fable 5 · Most capable for your hardest and
                               longest-running tasks
    4. Sonnet                  Sonnet 5 · Efficient for routine tasks
    5. Haiku                   Haiku 4.5 · Fastest for quick answers
    6. Fable 5.1 (disabled)    Update to 2.1.255+ to use Fable 5.1

  ● High effort (default)  ←/→ to adjust

Use /fast to turn on Fast mode (Opus 5).

Enter to set as default · s to use this session only · Esc to cancel
```

Notes — detection axes:

- `1./2.` pair: matches (six numbered options).
- cursor: matches (`›` on the selected row). Note a **second** marker, `✓`,
  denotes the *current* model independently of the cursor. In this capture both
  sit on row 2, so it does not discriminate them — a capture with the cursor
  moved off the current model is still wanted.
- header: **no match** — no "Do you want to…" / "wants to" phrasing. "Select
  model" is a plain title, like the session-resume picker's free prose.
- footer: **fails the strict picker test.** `main.js:1267` requires BOTH
  `/Enter to confirm/i` AND `/Esc to cancel/i` below the block. This footer
  supplies `Esc to cancel` but says **"Enter to set as default"**, not "Enter to
  confirm". So `pickerSignal` is false and the shape is not admitted.

Consequently this picker is **invisible to Bram today** — same consequence
class as the 2026-07-19 session-resume picker before
detect-session-resume-picker: an injected bracketed paste can be swallowed and
the trailing CR can act on the highlighted row. Here that would mean silently
setting a default model.

Two structural differences from every picker already cataloged, which is why
this is not just another Family C row:

- **Three commit semantics, not two**: `Enter` = set default, `s` = this
  session only, `Esc` = cancel. Every cataloged picker is confirm/cancel.
- **A second, non-list axis on the same screen**: `● High effort (default)`
  adjusted with `←/→`. Selecting a row is not the whole interaction, so
  "send a number and Enter" does not express what this menu does.

Sibling: `2026-09-09-codex-model-picker.md`, which fails the *other* half of
the same footer test.
