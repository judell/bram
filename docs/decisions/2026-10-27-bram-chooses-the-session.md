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

**Revisit when:** a CLI changes how it names, resumes or rewrites sessions (Claude's 2.1.311 boot change is the precedent), or Bram adds a fourth agent.
