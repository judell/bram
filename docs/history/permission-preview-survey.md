# Agent-pane interaction & display survey (Claude)

*History: moved here 2026-11-19. A June 2026 survey; superseded by the shipped permission-menu and Transcript surfaces, documented in `docs/menu.md` and the `pty-menu-*` docs.*

How Bram presents what the agent does — and how it could be a richer surface
than the terminal for both **interaction** (approving gated actions in the
permission menu) and **display** (reviewing what the agent ran and produced in
the Transcript). All tools; Claude-only for now, Codex a later pass.

North Star (from live review): **Bram should be the superior surface.** Render
what the agent does well and visible **by default**, most of all where the
terminal shows little or nothing — the proposed change before you approve it,
and the command and its output after.

Two moments, one principle:

- **Interaction (pre-run)** — the permission menu: preview *what you're
  authorizing*.
- **Display (post-run)** — the Transcript: render *what the agent ran and
  produced*.

## What already works well

Cases that already beat or match the terminal — the baseline to preserve:

- **Write — preview shows the document (superior to terminal).** When shown, it
  renders the file about to be written; the terminal shows only the path.
- **Bash — the command rides in the menu (parity, readable).** In a 3-option
  menu the command appears in option 2 ("Yes, and don't ask again :
  `<command>`"), visible without any preview affordance. (The narrow-window
  spacing gap before a trailing path is a wrap artifact, not fundamental.)

## What the agent does, and what gates

Tool-call frequency across the session JSONLs: Bash 11192, Read 3485, Edit
3359, Write 992, then Task/MCP/Agent/Web tools. The tools that actually present
**permission menus** (from rotated real-session logs):

```
Bash 6018 | Write 763 | Agent 732 | Edit 594 | Update 321 | Read 210 | WebFetch 127
```

`Update` is Claude Code's terminal verb for Edit, so `Edit`+`Update` = 915 edit
menus (out-gating Write). The surface to design for is broader than edits:
**Bash, Edit/Update, Write, Agent, Read, WebFetch.**

## By nature

The surface sorts into a few natures, each with a natural rendering.

### Structured edits — Edit/Update, Write

Render the **change** — a diff (Edit) or the file content (Write), **by type**,
**by default**.

- **Edit is the model.** Bram builds `tool_call_diff` and renders `DiffView`,
  cleaner than the terminal. Caveats: collapsed by default; absent for
  signature-less menus.
- **Post-run edit results should also be diffs.** In the observed Codex
  `apply_patch app/__shell/helpers.js patch` result, Bram showed only
  `Success. Updated the following files: M app/__shell/helpers.js`. That
  acknowledgement is less useful than the actual patch. For edit/apply-patch
  tool results, render the diff by default and keep the success line as
  secondary metadata.
- **Write is the biggest gap.** Bram *has* the content (`tool_call_content`,
  lib.rs:4376) but hides it (collapsed) and renders it as `Markdown`
  (AgentMenuView.xmlui:50), which mangles non-markdown files. It should be a
  **code block, syntax-by-extension** (`.md` → Markdown is the one case
  Markdown is right).

### Command actions — Bash

- **Pre-run**: render the **command** (parity, more readable). Signature when
  recoverable; the 2-option restricted menu is the bare case, tracked as
  `grid-menu-command-preview`.
- **Post-run**: render the **output line-mode** — monospace, no wrap,
  horizontal scroll. Most Bash is read-only inspection (grep/cat/ls/jq), where
  wrapping breaks alignment and makes Bram a *downgrade* from the line-oriented
  terminal. This is the Transcript's tool-result path (`kind==='tool'`,
  `$item.result`), a separate `Transcript.xmlui` change.

### Other gated actions — Agent, Read, WebFetch

Each gates and shows a menu; each has an obvious natural preview, minimally
rendered today:

- **Agent** (732) → the **subagent prompt** — what task you're launching and
  which agent. The most content-rich and the least previewed.
- **WebFetch** (127) → the **URL** (and prompt).
- **Read** (210) → the **path** — notable that reads gate at all (likely reads
  outside the project).

### Structured tool results — MCP

MCP calls can return structured JSON that Bram currently shows as a raw
serialized payload. In the observed `xmlui.xmlui_component_docs APICall` result,
the visible output was a JSON array containing text content, escaped newlines,
and markdown. That should render as structured JSON with readable text/markdown
extracted when available, not as one long wrapped JSON string.

## Patterns from the logs

- **Edit/Write targets are markdown-heavy with a source tail** (`.md` 525,
  `.txt` 218, `.json` 140, `.xmlui` 48, `.js` 22, `.py` 13, …) — render by
  extension, not always-Markdown. (Counts are inflated by docs/scratch files;
  read as a type catalog, not a workload profile.)
- **Bash is ~90% read-only** (grep/curl/git/gh/cat/ls/…); the mutating subset
  (touch/sed/rm/cp/redirects) is small and command-shaped.
- **The gating mix is broad and balanced** (above) — Write/Edit/Agent are all
  first-class, not edge cases.
- **Edit surfaces under two names** (Edit/Update) by build path — preview
  routing and the menu label must treat both as "show a diff."

## Cross-cutting findings

1. **Visibility is backwards.** Previews sit behind "Show preview"
   (`menuPreviewOpen = false`). For a consent decision the content should be
   default-visible. Highest-impact, lowest-effort lever; makes blind approval
   structural rather than per-case.
2. **Renderer correctness.** Write content as Markdown is wrong → code block by
   extension. Read-only Bash output wrapped is wrong → line-mode. Edit's
   `DiffView` is the model.
3. **Consistency.** One model across natures: every action shows *what it will
   do / what it produced*, rendered well, by default.
4. **Coverage / fidelity.** Signature-less menus lack diff/content because the
   JSONL `tool_use` isn't flushed while the menu is up — the same gap the
   in-flight-prose feature hit, and the same bridge applies (best-effort now,
   enrich from JSONL when it lands).

## Recommended sequence

1. **Default-open previews** when a diff/content/command exists — the
   structural "approve blind" fix.
2. **Write content as a code block** (syntax-by-extension stretch).
3. **Post-run edit diffs** in the Transcript for Edit/Update/apply-patch
   results, instead of foregrounding generic "file edited" acknowledgements.
4. **Line-mode tool output** in the Transcript (read-only Bash especially).
5. **Signature-less coverage** — grid command for Bash; JSONL bridge for
   diff/content.
6. **Structured MCP result rendering** — JSON tree by default, with extracted
   text/markdown rendered readably when the payload shape supports it.
7. **Previews for Agent / WebFetch / Read** — prompt / URL / path.

Out of scope here: Codex (claude-only for now). Menu-stack parity is the
separately-filed `grid-menu-command-preview`.
