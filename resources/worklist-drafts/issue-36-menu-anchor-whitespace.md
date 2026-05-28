# Before

`pty_menu_detect` (`src-tauri/src/lib.rs`) anchors the live permission
menu on the byte sequence `❯1.` — cursor arrow `U+276F` (`e2 9d af`)
**immediately** followed by `1.`, with no separator:

```rust
let needle1: &[u8] = b"\xe2\x9d\xaf1.";
let pos1_opt = tail.windows(needle1.len()).rposition(|w| w == needle1);
```

The comment explains the original reasoning: Claude Code used to render
the gap between the cursor and the option number via cursor-positioning
escapes, so after `strip_ansi` the bytes collapsed to `❯1.` with no
space.

**Claude Code has since changed its menu rendering.** Live captures from
the running binary (`/tmp/pty-menu-snapshot.txt`, written by the
detector's own miss-diagnostic) show the gap is now a literal space
and/or NBSP:

- `e29daf 20 312e59` = `❯` + space + `1.Yes` (the selected option-1 row)
- `e29daf c2a0 20` = `❯` + NBSP (`U+00A0`) + space (redraw rows)

The needle `❯1.` (`e29daf312e`) appears **only** in unrelated buffer text
(e.g. this conversation discussing the menu), never in an actual menu.
So detection misses every real prompt: `pty-menu-changed` never fires,
and the agent-tools drawer never shows the yellow "Agent wants to use…"
box. The user must approve from the terminal.

This is the live form of issue #36's first symptom ("yellow permission
menu doesn't fire"). It is **independent of terminal visibility** — the
"hide the terminal" framing was the v0.1.18 repro; the current cause is
menu-format drift. Confirmed live this session: a visible `mktemp`
prompt rewrote the miss-snapshot at the moment of the prompt, proving
the detector ran and failed to match.

Out of scope for this item (verified clean / no capture):

- Symptoms 2 & 3 (spinner stuck, stale agent state) are already
  JSONL-driven (`/__waiting-for-assistant`, `/__inflight`) and tested
  clean this session (`waiting:false`, `inflight:{}`).
- Codex's differently-shaped permission prompt — no live capture
  available; needs a separate verified pass before changing its
  matcher.

# After

Relax the anchor to tolerate optional whitespace between `❯` and `1.`,
while preserving the existing "newest menu wins" (`rposition`) semantics
and all downstream logic (the `2.`-within-512 window, the 200-byte
text capture, the `#77` pending-tool / eviction-grace / suppression
machinery, which all key off the returned arrow position).

Replace the fixed `needle1` lookup with a small helper that finds the
newest `❯` whose following bytes — after skipping any run of ASCII
spaces (`0x20`) and NBSPs (`c2 a0`) — begin with `1.`:

```rust
// Find the newest cursor-anchored first option: ❯ (U+276F) followed by
// an optional run of spaces / NBSP, then "1.". Claude Code's TUI once
// rendered the gap as cursor-positioning escapes (collapsing to "❯1."
// after strip_ansi); newer builds emit a literal space and/or NBSP
// (U+00A0 = c2 a0), giving "❯ 1." / "❯\u{a0} 1.". Tolerate all three so
// the anchor survives the format drift. Walk back to older arrows when
// the newest one is a redraw artifact rather than the option-1 row.
// Refs #36.
fn pty_menu_anchor_pos(tail: &[u8]) -> Option<usize> {
    let arrow: &[u8] = b"\xe2\x9d\xaf";
    let mut end = tail.len();
    while let Some(rel) = tail[..end].windows(arrow.len()).rposition(|w| w == arrow) {
        let mut k = rel + arrow.len();
        loop {
            if tail.get(k) == Some(&0x20) {
                k += 1;
            } else if tail.get(k) == Some(&0xc2) && tail.get(k + 1) == Some(&0xa0) {
                k += 2;
            } else {
                break;
            }
        }
        if tail[k..].starts_with(b"1.") {
            return Some(rel);
        }
        end = rel;
    }
    None
}
```

Then in `pty_menu_detect`: drop the `needle1` constant and use
`let pos1_opt = pty_menu_anchor_pos(tail);`. `pos1` stays the arrow
offset, so `needle2`, the 512-byte distance check, and the text window
are unchanged. Update the anchor comment to describe the new tolerance.

Alternatives considered:

- **[chosen]** Whitespace-tolerant cursor anchor. Smallest change that
  fixes the confirmed bug; leaves the delicate `#77` stabilization
  logic untouched.
- Header-anchored rewrite (anchor on `Do you want`, scan forward for
  `1.`/`2.`/`3.`) — rejected for now: a larger rewrite of a delicate
  function. The cursor anchor + `2.`-within-512 window is specific
  enough once whitespace tolerance is added. Revisit if the format
  drifts again.
- JSONL `latest-pending` as the menu source — rejected: proven this
  session that it over-fires (it reported our own auto-approved curl
  and Bash calls as "pending"), and `lib.rs` already documents JSONL
  flush-lag that makes it unusable for a live, on-screen menu.

Caveat: with the relaxed anchor, buffer text that literally contains
`❯ 1.` plus `2.` within 512 bytes plus the menu shape could in
principle false-positive. In practice this only happens when the
agent's own output discusses the menu (i.e. dogfooding Bram on Bram);
normal agent output does not emit the `❯` cursor glyph. Accepted as an
inherent self-hosting edge, not worth gating on the `Do you want`
header (which is also present in such discussion text).

Note: this is a Rust backend change — it needs `cargo build` + app
restart to take effect (per CLAUDE.md). The restart ends the current
in-app agent session.
