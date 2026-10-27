# Bram chooses the session; nothing else names it

Status: decided 2026-10-27 · revisit when a CLI changes how it names or resumes sessions, or a fourth agent arrives

**Question:** With three agents (Claude, Codex, OpenCode) and sessions paired across them, who decides which session a switch, reload or New session is about?

**Decision:** Bram chooses the session, launches it by id, marks it current at launch, and pairs only through an explicit act tied to that id. Nothing in session switching takes a CLI's own "most recent", a cached guess, or a "file created inside a time window" as the identity of a session.

**Evidence:** one day of driving switches across the three agents broke this rule in a different place each time. Each break, its trace receipt and the commit that closed it:

| Break | Receipt (2026-10-27 UTC) | Fix |
|---|---|---|
| A replaced launch reported "didn't start" | `op=launch-timeout source=switch` 19:47:23, after a newer click launched Claude | `387913a` |
| A fresh OpenCode partner kept OpenCode's own name | `op=named provider=opencode` absent; Match name clicked 19:46:25 | `39683e4` |
| New session's brief never arrived | `boot-held-restore … reason=expired` 07:18, 19:45, 20:30, 20:56; Claude 2.1.311+ no longer bounces focus reporting | `dcb7678` |
| A reserved Claude title moved to another session | `op=rebound from=e026049d… sid=a222e9f8…` 21:05:47 | `aeef82f` |
| Deleting the running session left a stub its partner resumed | delete 20:49:12, then `launch-exited status=1` "No conversation found" | `556023c` |
| The fallback and the offer named different sessions | `opencode --continue` ran `ses_e80e…`; the offer named `ses_e83a…` 21:27 | `0854ac2` |
| An OpenCode resume was labelled with another session | `--session ses_e83a…` 21:39:24, then `remember-last-active … ses_e80e…` | `0854ac2` |
| A pending pair attached to a reloaded old session | `pair-stamp … source=switch-new-session-born` 21:24:11, from the reload | `c03a5c1` |
| The offer hid what Keep replaces, and outlived a reload | banner still naming 'pending pair test' after `op=reload` 22:08:27 | `c14d2df` |
| OpenCode sessions couldn't be deleted from Bram | ✕ refused for every OpenCode row | `4c75b9a` |

The consolidated live run at 22:21–22:23 exercised partner resume, unpair, reload, no-partner fallback, the replace warning and "Start a Claude session for …" in sequence. Each step did what it should.

**Alternatives:**

- *Let each CLI choose (`--continue`, `--last`).* Rejected: the CLI's "most recent" and Bram's diverge (OpenCode orders by its own update time, Claude by file mtime with Bram's hysteresis), and the user acts on what Bram shows.
- *Infer identity from timing* (a session file created within 2 s of a request; the first session change after a launch). Rejected as the deciding signal: CLIs rewrite session files at exit, and reloads produce session changes. Both defeated it on the day. Timing may still narrow a choice Bram has already made; it never makes one.
- *Make pairing automatic on every switch.* Already rejected on 2026-10-04 (`e311c09`): an automatic pair undid the user's Unpairs. This decision keeps pairing explicit and adds that the explicit act must be tied to a known id.

**Known gaps (recorded, not filed):**

- No way to pair with an arbitrary existing session; resume it first, then Keep.
- `opencode session delete` is untested on Windows, where an npm `opencode.cmd` shim may not resolve from `Command::new("opencode")`.

**Switch matrix, to run once and record here:** Claude, Codex and OpenCode × partner resume, no-partner fallback, fresh partner (New session, then switch), Sessions-tab reload, delete (running refused, other allowed). A new failure is recorded in this note rather than filed as an item, unless it blocks a release.

**Matrix runs** (`scripts/switch-matrix.py`):

- *2026-10-27 22:45–23:04Z, Claude + OpenCode.* Plain switch, fallback by id with the offer up, reload clearing the offer, delete-running refused, and the New session brief released on the composer and flushed: all pass. **One break found:** after New session with a short first turn, Claude's new session `6920c90a` was named and became current only 2 minutes later, at the next switch. Its file existed from the brief onward (watcher `change=create` 23:02:05.4), but `latest_claude_session_path`'s hysteresis kept the outgoing session, whose exit write made it seconds old, and nothing re-evaluated once the short turn ended. This breaks (c), "marks it current at launch": New session knows the exact Claude id before launch and should mark it current when its file appears, as an explicit resume already does, rather than wait for file times to agree. **Fixed in the harness item** (`mark_reserved_claude_session_current`): once the reserved id's file exists it is current (`live-session-set … source=reserved-id`); the next run named the session 2.4 s after Create.
- *2026-10-27 23:12Z.* **Second break, fixed:** a fresh OpenCode partner's catch-up was always held (`no-composer`). The logged tail (`0c37baf`) showed why: in a narrow pane OpenCode 2.1.4 drops its `ctrl+p commands` footer for the cwd and its "/update to install v2.2.0" notice. The idle input box's placeholder "Ask anything…" now counts as ready; the captured tail is pinned as a test.
- *2026-10-27 23:19Z, all three agents.* **Claude + OpenCode: 10 cases, 0 failures.** **Codex: open, recorded rather than filed.** A fresh Codex partner's catch-up was held `no-composer`: Codex 0.163.0 drew its "Update available! 0.163.0 -> 0.164.0" box, its header and status line, then stopped mid-line (`… high · ~/bram-dem`) without drawing its composer within 30 s. The same hold as 10-20 and 10-21. Not a blocker (the catch-up offer appears; the user sends it). Next step when someone takes it: capture Codex 0.163.0's narrow-pane startup with `scripts/capture-terminal-specimen.py`. The three failures after it in that run (partner resumes falling back) follow from the missing pair.
- Seen while building the harness, not a session-switching break: a demo launch kept writing its trace to a rotated name (`bram-trace-<date>-N.log`) while `bram-trace.log` stayed empty. The script reads the most recently written trace file.

**Revisit when:** a CLI changes how it names, resumes or rewrites sessions (Claude's 2.1.311 boot change is the precedent), or Bram adds a fourth agent.
