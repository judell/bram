# Reading the terminal screen from xterm.js instead of re-parsing the PTY stream

Status: partially implemented. The spike validated the approach on
2026-06-23; current code now uses the xterm.js grid for live permission-menu
shape/options, in-flight menu prose, status rows, and finished banners, while
raw PTY bytes remain in use for transport, activity timing, fallbacks, and
debugging.

## Summary

Bram's Rust host keeps a rolling buffer of recent PTY output bytes
(`PTY_TAIL` / `pty_tail_cell`). Historically it ran `strip_ansi` over that
byte stream and hand-parsed the result to detect the permission menu, the
agent status line, the end-of-turn banner, the `⏺ Tool(...)` signature, and
more. That parsing is fragile — and the fragility is **structural**, not
incidental.

Meanwhile the frontend already renders the *same bytes* through **xterm.js**,
a full terminal emulator. **The grid** means xterm.js's rendered 2-D terminal
buffer: rows and columns of cells after xterm has interpreted PTY bytes,
ANSI/control sequences, cursor motion, overwrites, wrapping, and redraws. It
is not the DOM and it is not the raw PTY stream. Bram reads that buffer with
`term.buffer.active.getLine(n).translateToString(true)` in `app/main.js`.
A spike proved the grid yields clean, complete screen text exactly where the
strip_ansi parse yields mangled fragments.

The conclusion remains: **stop re-deriving rendered screen text in Rust from
a de-positioned byte stream; read it from the emulator we already run.** The
host keeps everything that *isn't* screen text: transport, timing, emission,
JSONL correlation, suppression windows, and operational fallbacks.

## Current split

There is still substantial terminal-derived parsing. The important change is
the source of the user-visible screen facts.

- **Raw PTY bytes/control characters remain for transport and liveness.**
  User turns still go through `queue_pty_intent` / `pty_write`; PTY chunks
  still drive activity/silence detection, spinner glyph detection, Codex
  fallback "Working (`esc to interrupt`)" detection, menu dismissal
  suppression, stale-sentinel cleanup, PTY-tail debug endpoints, and some
  fallback banner/status paths.
- **xterm grid reads now own the clean screen-text path.** `app/main.js`
  reads live rows near the cursor for permission-menu shape/options and
  in-flight prose, and reads recent rows for working status and finished
  banners. Those reports reach Rust through `report_grid_menu`,
  `report_grid_status`, and `report_grid_banner`.
- **JSONL/state remains the structured authority.** Provider JSONL supplies
  completion boundaries, tool-call signatures, diffs/content, and turn
  correlation. The grid tells Bram what is visibly on screen; JSONL tells
  Bram what durable provider event that screen state belongs to.

So this migration did not remove parsing. It moved the most failure-prone
screen parsing from "decode terminal protocol from bytes" to "match text rows
from the already-rendered terminal buffer." That is cleaner, but still a
heuristic screen read and still coordinated with PTY timing and JSONL state.

## Why strip_ansi mangles

`pty_tail` is a **time-ordered byte stream of the TUI's repaints**. The
terminal turns that stream into a 2-D grid using cursor-positioning escape
codes ("move to row 12 col 3, write `o`, move back, overwrite with a spinner
frame") hundreds of times a second. `strip_ansi` (`lib.rs:12409`) **deletes
exactly those positioning codes**. The visible glyphs then collapse together
in *stream* order rather than *screen* order, interleaved with spinner-
animation frames. Two consequences:

- Fragments out of order, jumbled with `✢ ✳ ✶ ✻ ✽` spinner glyphs and
  repeated `thinking` — e.g. the word "Flowing" appears as `F\nl\no\nw…`.
- Hard-wrap boundary spaces are lost (`menu box, not` → `menu box,not`).

`strip_ansi` itself has Windows-gated arms that try to *reconstruct* a
fraction of the grid (re-emit `\n` for CSI cursor-row moves, a space for
column moves) — i.e. it is hand-reimplementing a sliver of a terminal
emulator, and its own comments admit it is "platform-specific and
incomplete." The end-of-turn banner parse is even **irrecoverably lossy**:
`1m 22s` reconstructs as `1m 2s` because the missing digit "lived in the
prior screen state we can't recover."

## The unused asset

`app/main.js` creates `const term = new Terminal({... allowProposedApi:
true ...})`; PTY chunks are written in via `term.write`. xterm.js resolves
every repaint/overwrite/animation into a clean grid. Bram now uses that
content API for live screen-derived facts. The useful primitives are:

- `buffer.active.getLine(n).translateToString(trimRight)` — clean rendered
  text of grid row *n*.
- `line.isWrapped` — soft-wrap continuation flag (see caveat below).
- `buffer.active.cursorY / baseY / length` — geometry.
- `term.registerMarker(offset)` — pin a row.
- `term.onWriteParsed` — buffer-current notification (fires after a write
  chunk is parsed; renderer-independent, so it works even when the terminal
  pane is `display:none`).

Outbound channels are already wired: `invoke(...)` / `fetch("/__...")` to
the host, `postMessage` to the agent-pane iframe.

## What the host re-derives today (inventory)

Three buckets:

- **(A) Screen-derived — now grid-first.** Menu shape/options/prose, working
  status rows, and finished banners are read from xterm's rendered rows when
  fresh. Rust still owns the state machine around those reports, and raw
  PTY fallbacks/debug paths remain, but fresh grid data is the preferred
  source for visible screen text.
- **(B) Stream/timing-derived — stays host-side.** The spinner-cadence
  end-of-turn machine (`pty_agent_turn_update`, glyph/silence detectors).
  Depends on byte timing, not rendered text. xterm.js does not replace it.
- **(C) JSONL/state-derived — stays host-side.** `lookup_pending_tool_call`,
  the JSONL completion deciders, turn stats. The authoritative truth for
  completion and tool signatures.

## Spike validation (2026-06-23)

`main.js` was instrumented to read the grid above the menu and log it on
menu appearance (`subkind=xterm-grid-spike`). Two captures, both clean:

1. **Prose.** The same content that came out of strip_ansi as hundreds of
   lone spinner glyphs and `F\nl\no\nw` fragments read back as fully
   readable wrapped prose, plus a pristine menu box.

2. **Menu structure (the hardest case).** A live three-option permission
   menu — including the **multi-line wrapped option 2** ("…always allow
   access to Desktop/ from this project") — read back complete and in
   order:

   ```
   Do you want to proceed?
   ❯ 1. Yes
     2. Yes, and always allow access to Desktop/
       from this project
     3. No
   Esc to cancel · Tab to amend · ctrl+e to explain
   ```

   This is exactly the input that `split_collapsed_option_lines` and
   `codex_tail_has_third_option_marker` exist to fight (#187
   "options collapsed onto one line", "missing 3rd option"). The grid got
   it right with zero heuristics.

## Residuals (cosmetic / filtering — not blockers)

- **Hard-wrap joins persist** (`box,not`, `2.Yes`). Claude **hard-wraps its
  own output** rather than letting the terminal soft-wrap, so `isWrapped` is
  usually `false` and can't rejoin perfectly. Cosmetic; for a transient
  preview, display wrapped as the terminal does. Far milder than strip_ansi
  mangling.
- **Occasional status rows leak in** (`✽ Flowing… (1m 17s …)` glued to a
  prose row's tail). Filter with the recognizers we already have.
- **Region targeting.** The spike's anchor was a loose substring scan that
  also matched scrollback menus and its own source shown on screen. The real
  extractor must target the **live** menu near the cursor with a proper
  shape (header + `❯ 1.` rows), not a substring.

## Risk

Not capability — **event timing + visibility.** Read on `onWriteParsed`
(buffer-current, renderer-independent), never `onRender` (frame-lagged, can
go quiet when the terminal pane is `display:none` and WebGL drops its
context). The buffer model stays readable while hidden; the event choice is
the thing to keep correct.

## Architecture & migration path

The screen text lives in the **frontend** (`main.js`, the only place with
`term`); menu detection + event emission live in the **host**; the
transcript lives in the **iframe**. So a migrated signal flows: frontend
reads the grid → reports clean text to host/iframe → host keeps owning
timing, emission, and JSONL correlation. That cross-layer hop is the real
work, not the text extraction.

The migration sequence is now partly complete:

1. **Done:** read in-flight assistant prose above a pending menu from the
   grid and show it transiently until JSONL catches up.
2. **Done:** use grid menu reports to build or override permission-menu
   options when fresh, including Codex menus.
3. **Done:** use grid status/banner reports for full-fidelity working and
   finished text when fresh.
4. **Still to do:** retire old raw-PTY menu/status fallback code only after
   traces show no coverage gaps for real Claude/Codex menu shapes.
5. Leave buckets B and C (timing, transport, JSONL) host-side throughout.
