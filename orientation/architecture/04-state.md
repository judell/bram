# State and data: detail

Click-through detail for 04-state.mmd. Facts are as of commit `e8a38ac0` (2026-11-02). One arrow, sharing the claim refs, depends on someone pushing on purpose; the rest is what code does on its own.

Where the host resolves it, the files below live in the state folder (`.bram/` if it exists, else `resources/`; `bram_state_dir`). [`src-tauri/src/lib.rs:61523-61560`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L61523-L61560) No project has `.bram/` yet: at this commit the release plan is "compatibility and evidence. Nothing moves.", and the move itself is for a later release, so the paths below say `resources/`, which is what is on disk. [`docs/state-folder-migration.md:17-26`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/state-folder-migration.md?plain=1#L17-L26) [`docs/decisions/2026-11-02-bram-state-folder-is-dot-bram.md:1-9`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/decisions/2026-11-02-bram-state-folder-is-dot-bram.md?plain=1#L1-L9) There is one inventory of what Bram writes, `BRAM_OWNED_ENTRIES`: "Everything Bram writes into a project, in one table", with a test "that every literal state path in this file is on it". [`src-tauri/src/lib.rs:61562-61568`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L61562-L61568) A read-only dry run shows it per project: `GET /__state-dir/plan`, the Status tab's "State folder" section, and `bram --state-dir-plan <root>` with no window. [`docs/apis.md:127-127`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/apis.md?plain=1#L127-L127) [`app/tools/components/Status.xmlui:70-70`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/app/tools/components/Status.xmlui?plain=1#L70-L70) [`src-tauri/src/lib.rs:795-814`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L795-L814) Files not named here exist (the inventory is longer than the diagram).

## Bram host

<!-- read: docs/decisions/2026-11-02-bram-state-folder-is-dot-bram.md#Bram's per-project state folder becomes `.bram/`, moved in three releases -->

The Rust process owns the files below. At startup it clears leftovers from a previous run: a stale in-flight claim, stale turn context, stale Codex intent/result files and stale PTY intents. [`src-tauri/src/lib.rs:97223-97230`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L97223-L97230) [`src-tauri/src/lib.rs:97246-97248`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L97246-L97248) It refuses to scaffold the state folder (port files) or a trace directory unless the launch folder is a git repo root (issues #353, #448). [`src-tauri/src/lib.rs:97398-97411`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L97398-L97411) The repo doesn't say who may write to `resources/` besides the guard's rules; the authorization doc concedes authority files "are same-user writable artifacts". [`docs/authorization.md:278-280`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/authorization.md?plain=1#L278-L280)

## Worklist files

<!-- read: app/__shell/conventions.md#Schema and draft layout -->

`resources/worklist.json` holds metadata; prose lives in `resources/worklist-drafts/<id>.md`. [`app/__shell/conventions.md:169-204`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/app/__shell/conventions.md?plain=1#L169-L204) It needn't exist in advance: "Bram serves an empty default and the Worklist tab creates the file (and the state folder) on first use." [`app/__shell/conventions.md:48-53`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/app/__shell/conventions.md?plain=1#L48-L53) This repo ignores both: "Worklist proposals are per-developer scratch; keep them out of the repo." [`.gitignore:48-50`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/.gitignore?plain=1#L48-L50) The agent writes these files (through guarded edits); the host merges them and tracks versions.

## Authorization record

<!-- read: docs/authorization.md#Where the implementation lives -->

`resources/.worklist-authorization.json`: one slot, 30 minute TTL. [`docs/authorization.md:137-144`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/authorization.md?plain=1#L137-L144) [`docs/authorization.md:170-174`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/authorization.md?plain=1#L170-L174) Gitignored. [`.gitignore:64-64`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/.gitignore?plain=1#L64-L64) The record is also the guard's project-root marker: the guard "uses that file's existence as the project-root marker". [`docs/authorization.md:335-340`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/authorization.md?plain=1#L335-L340) Setup creates a stub with `kind: "none"` if none exists. [`src-tauri/src/lib.rs:59137-59153`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L59137-L59153)

## Execution claim

<!-- read: app/__shell/conventions.md#The claim ends with your turn -->

`resources/.inflight-claim.json`, single slot, retired when the turn ends. [`docs/authorization.md:145-149`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/authorization.md?plain=1#L145-L149) [`.gitignore:61-61`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/.gitignore?plain=1#L61-L61) Startup removes a stale one: "Remove any stale inflight sentinel from a prior session that didn't complete cleanly." [`src-tauri/src/lib.rs:97223-97226`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L97223-L97226)

## Claim refs

<!-- read: docs/git-as-infrastructure.md#Claim intervals: attribution as boundary trees -->
<!-- read: docs/git-as-infrastructure.md#Bram's private refs and object reachability -->

The ledger is `resources/.claim-intervals.json`, an ordered list. The trees are kept alive by `refs/bram/claims/<atMs>`. "Record order, not timestamp sorting, defines adjacency because two boundaries can share a millisecond." [`docs/git-as-infrastructure.md:122-125`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/git-as-infrastructure.md?plain=1#L122-L125) [`.gitignore:62-62`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/.gitignore?plain=1#L62-L62) Retired boundaries' refs are deleted lazily after item pruning, and "The newest boundary is always retained because it is the base of the open interval." [`docs/git-as-infrastructure.md:176-179`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/git-as-infrastructure.md?plain=1#L176-L179) A second namespace, `refs/bram/safety/*`, is for explicit operator snapshots and is "retained until explicitly deleted". [`docs/git-as-infrastructure.md:235-240`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/git-as-infrastructure.md?plain=1#L235-L240)

## Worklist history

<!-- read: docs/worklist-history.md#Flow -->

`resources/worklist-history/<unix_ms>.json` (the effective worklist after the change) and `.md` (a changelog of the transition). [`docs/worklist-history.md:49-55`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/worklist-history.md?plain=1#L49-L55) Trivial prose rewording is suppressed. [`docs/worklist-history.md:56-59`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/worklist-history.md?plain=1#L56-L59) The conventions call the directory host-written. [`app/__shell/conventions.md:206-207`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/app/__shell/conventions.md?plain=1#L206-L207) This repo's `.gitignore` opts out of tracking the snapshots "for the Codex-themed release; revisit whether to track them in a later release." [`.gitignore:44-46`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/.gitignore?plain=1#L44-L46) The doc argues for committing them: "that committed history is their context." [`docs/worklist-history.md:34-37`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/worklist-history.md?plain=1#L34-L37)

## Audit ledger

`resources/audit-ledger.jsonl`: "always-on, append-only JSONL record of authorization decisions and Bram-gated commits". [`src-tauri/src/lib.rs:49391-49393`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L49391-L49393) It is "NOT gated by bram_trace_enabled()"; "Metadata only — no request text, feedback, commit messages, diffs, or file contents"; and that "always-on" means "ungated, not fail-closed or tamper-evident". [`src-tauri/src/lib.rs:49394-49402`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L49394-L49402) Gitignored. [`.gitignore:80-83`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/.gitignore?plain=1#L80-L83)

## State mirror

A SQLite shadow of the authorization and claim files plus an append-only transitions ledger. "**Files remain the sole truth this phase.**" [`src-tauri/src/worklist_state.rs:1-12`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/worklist_state.rs?plain=1#L1-L12) Mirror failures are "trace-only — it must never block or alter the file-write path". [`src-tauri/src/worklist_state.rs:13-16`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/worklist_state.rs?plain=1#L13-L16) The database lives under the app cache directory, one per project: `worklist-state/<encoded project path>.db`. [`src-tauri/src/lib.rs:36703-36711`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L36703-L36711) Its "destiny is *authoritative*" in a planned migration, and a future grant book would move there, with the open question of how the separate guard process reads it. [`src-tauri/src/worklist_state.rs:21-25`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/worklist_state.rs?plain=1#L21-L25) [`docs/authorization.md:331-340`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/authorization.md?plain=1#L331-L340) `scripts/state-mirror-check.py` compares files and rows on demand. [`src-tauri/src/worklist_state.rs:37-39`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/worklist_state.rs?plain=1#L37-L39) The repo doesn't say whether the migration has started beyond the shadow.

## Search index

<!-- read: app/__shell/conventions.md#Search-first -->
<!-- read: docs/apis.md#14. Search index -->

An SQLite FTS5 database under the app's local data directory (outside the repo), one per project: `search-index/<encoded project path>.db`. [`src-tauri/src/lib.rs:36518-36526`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L36518-L36526) It indexes "Claude and Codex session transcripts, commits (with diffs), issues, worklist-history" and is served at `GET /__search`. [`app/__shell/conventions.md:732-734`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/app/__shell/conventions.md?plain=1#L732-L734) Changing the FTS table's shape "REBUILDS THE WHOLE INDEX — measured at ~50 minutes over 440 MB of session JSONL". [`src-tauri/src/search_index.rs:15-18`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/search_index.rs?plain=1#L15-L18) Convention: query it before diagnosing a past bug. [`app/__shell/conventions.md:736-740`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/app/__shell/conventions.md?plain=1#L736-L740)

## Trace log

<!-- read: app/__shell/reference/diagnostics.md#Debugging Bram itself -->
<!-- read: docs/trace-vocabulary.md#Trace subkind vocabulary -->

`resources/bram-traces/bram-trace.log`, with prior runs archived at startup. [`src-tauri/src/lib.rs:836-840`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L836-L840) Traces default on: an absent setting resolves to true; `.bram.json` or `BRAM_TRACE` can turn them off. [`src-tauri/src/lib.rs:97060-97071`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L97060-L97071) The trace is not written outside a project folder. [`src-tauri/src/lib.rs:2402-2406`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L2402-L2406) Gitignored. [`.gitignore:108-109`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/.gitignore?plain=1#L108-L109) The development doc's ritual after any pane edit is to grep it for `console-error|console-unhandledrejection`: "Zero matches is the pass condition". [`docs/developing-bram.md:170-176`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/developing-bram.md?plain=1#L170-L176) `docs/trace-vocabulary.md` catalogs the trace categories, subkinds and fields, and says to "Consult this on-demand" while reading the log; iframe events are `[iframe] subkind=<name>` lines and host events `[<category>] op=<name>` lines. [`docs/trace-vocabulary.md:1-6`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/trace-vocabulary.md?plain=1#L1-L6) [`docs/trace-vocabulary.md:85-90`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/trace-vocabulary.md?plain=1#L85-L90)

## Provider transcripts

<!-- read: docs/subagents.md#The provider seam -->

Claude's under `~/.claude/projects/<encoded path>`, Codex's under `~/.codex/sessions`. [`src-tauri/src/lib.rs:32844-32844`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L32844-L32844) [`src-tauri/src/lib.rs:26705-26705`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L26705-L26705) OpenCode keeps sessions in SQLite, not transcript files, so Bram exports each to a Claude-shaped JSONL in its cache and reads that. [`src-tauri/src/lib.rs:32513-32516`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L32513-L32516) [`src-tauri/src/lib.rs:33966-33973`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L33966-L33973) Antigravity's live under `~/.gemini/antigravity-cli`, and Bram exports them the same way: "a disposable Claude-shaped projection in Bram's cache". [`src-tauri/src/lib.rs:34427-34433`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L34427-L34433) The transcripts are the providers', and Bram only reads them.

## bram-guard

<!-- read: app/__shell/reference/diagnostics.md#Guards, retired hooks, and bundled skills -->

Besides the authorization file, the guard keeps one breadcrumb per invocation in `<state>/bram-traces/hook-events.log`, "written only when the root's state folder already exists". [`src-tauri/src/guard.rs:9-15`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/guard.rs?plain=1#L9-L15) In its session-hook modes it also reports the running session to the host (see Which session is current). [`src-tauri/src/guard.rs:344-355`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/guard.rs?plain=1#L344-L355) The link `~/.bram/bram-guard` is re-pointed to the guard beside the running binary every 60 seconds, so with two Bram builds on one machine "every project's hooks ran whichever guard wrote last" until per-project routing was added. [`src-tauri/src/guard.rs:108-113`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/guard.rs?plain=1#L108-L113)

## Agent pane

<!-- read: docs/session-context-architecture.md#The line -->

State placement is a stated rule: "Pages own nothing but pure view ephemera"; session-global context and expensive derived data sit in the shell. [`docs/session-context-architecture.md:44-47`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/session-context-architecture.md?plain=1#L44-L47) XMLUI unmounts a page on every tab switch. [`docs/session-context-architecture.md:32-34`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/session-context-architecture.md?plain=1#L32-L34) The doc notes unfinished items: the permission menu's full view leaks into the Transcript page, and a voice-recording wedge is "Open". [`docs/session-context-architecture.md:63-74`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/session-context-architecture.md?plain=1#L63-L74)

## Pane browser storage

<!-- read: docs/developing-bram.md#Client storage: decision vs. focus -->

Two classes. "**decision** (persists across sessions) → `localStorage`", "**focus** (dies with the session) → `sessionStorage`", and "Default to focus when unsure." [`docs/developing-bram.md:239-246`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/developing-bram.md?plain=1#L239-L246) `localStorage` is shared by every Bram window on the machine, so project-specific keys get a project suffix via `window.__bramProjectScopedKey(base)`. [`docs/developing-bram.md:288-298`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/developing-bram.md?plain=1#L288-L298) The doc records an audit of the existing unscoped keys on 2026-10-31: two project keys turned up, but nothing read them, "so they were deleted, not scoped." [`docs/developing-bram.md:317-321`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/developing-bram.md?plain=1#L317-L321) The repo doesn't list the keys.

## Git remote

<!-- read: docs/git-as-infrastructure.md#Bram's private refs and object reachability -->

A normal push configuration will "transfer branch refs under `refs/heads/*`" and doesn't publish `refs/bram/*`, so claim and safety stores "are therefore machine-local unless someone deliberately uses an explicit refspec." [`docs/git-as-infrastructure.md:253-256`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/git-as-infrastructure.md?plain=1#L253-L256)

## Which session is current

<!-- read: docs/decisions/2026-10-27-bram-chooses-the-session.md#Bram chooses the session; nothing else names it -->

A decision note, "Bram chooses the session; nothing else names it", governs it. [`docs/decisions/2026-10-27-bram-chooses-the-session.md:1-7`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/decisions/2026-10-27-bram-chooses-the-session.md?plain=1#L1-L7) After launch the rule is: "a session is current because something named it." First, the running CLI naming its own session in an event, from this Bram's terminal; then Bram naming it at launch; "Newest wins" only when neither has spoken. [`docs/decisions/2026-10-27-bram-chooses-the-session.md:9-15`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/decisions/2026-10-27-bram-chooses-the-session.md?plain=1#L9-L15) The table there says how each provider reports: Claude's and Codex's hooks, Antigravity's `PreInvocation` hook posting `conversationId` to `/__agent/session`, and OpenCode's plugin events on `/__opencode/event`, with Bram launching `opencode --standalone`. [`docs/decisions/2026-10-27-bram-chooses-the-session.md:17-22`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/decisions/2026-10-27-bram-chooses-the-session.md?plain=1#L17-L22) "From this Bram's terminal" is enforced by the environment: Bram exports `BRAM_PORT` and `BRAM_MENU_TOKEN` to its terminal, and the route refuses a wrong token. [`docs/decisions/2026-10-27-bram-chooses-the-session.md:26-26`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/decisions/2026-10-27-bram-chooses-the-session.md?plain=1#L26-L26)

Sessions across agents form groups, not pairs: "at most one session per agent, any number of agents, and a session is in at most one group." [`docs/decisions/2026-10-27-bram-chooses-the-session.md:50-50`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/decisions/2026-10-27-bram-chooses-the-session.md?plain=1#L50-L50) The named session is held in memory, not in a file: a named-session cell that "Only another explicit launch, or the running CLI naming its own session (handle_agent_session), replaces or clears". [`src-tauri/src/lib.rs:36212-36225`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L36212-L36225) Antigravity uses a live-session cell instead. [`src-tauri/src/lib.rs:36321-36330`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L36321-L36330) Groups are kept in the state-mirror database as `join` and `leave` records replayed into a snapshot. [`src-tauri/src/worklist_state.rs:267-274`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/worklist_state.rs?plain=1#L267-L274) [`src-tauri/src/worklist_state.rs:380-386`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/worklist_state.rs?plain=1#L380-L386) The README lists rename, delete and switch on the Sessions tab. [`README.md:41-41`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/README.md?plain=1#L41-L41)

## snapshot on change

<!-- read: docs/worklist-history.md#Flow -->

"A filesystem watcher in `src-tauri/src/lib.rs` notices writes to `resources/worklist.json` and draft files under `resources/worklist-drafts/`, then calls `maybe_snapshot_worklist`." [`docs/worklist-history.md:39-42`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/worklist-history.md?plain=1#L39-L42) [`src-tauri/src/lib.rs:78156-78162`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L78156-L78162) The snapshot function refuses to act on an unparseable read, after a past bug where a truncating writer made the board look empty (issue-426). [`src-tauri/src/lib.rs:78167-78172`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L78167-L78172)

## records approval

<!-- read: docs/authorization.md#Where the implementation lives -->

See diagram 3. The Tauri command `record_worklist_action_authorization` calls `write_worklist_authorization_record`. [`src-tauri/src/lib.rs:77041-77046`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L77041-L77046)

## writes claim

<!-- read: app/__shell/reference/worklist-mechanics.md#The claim and the authorization -->

The same command writes the claim sentinel for the record's ids. [`src-tauri/src/lib.rs:77047-77049`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L77047-L77049) Unless the `_no_snapshot` variant is used, writing the claim first calls `record_claim_interval`, which is how a claim change leads to a boundary tree (next arrow). [`src-tauri/src/lib.rs:67135-67149`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L67135-L67149) [`src-tauri/src/lib.rs:67157-67162`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L67157-L67162)

## captures boundary trees

<!-- read: docs/git-as-infrastructure.md#Claim intervals: attribution as boundary trees -->

`capture_claim_tree` creates a temporary index, seeds it from `HEAD`, runs path-scoped `git add -A`, and writes a tree: "without staging anything in the real index." [`docs/git-as-infrastructure.md:116-121`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/git-as-infrastructure.md?plain=1#L116-L121) [`src-tauri/src/lib.rs:62188-62197`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L62188-L62197) A successful ledger record needs a successful ref: "a bare tree ID written only to JSON is eventually eligible for garbage collection". [`docs/git-as-infrastructure.md:171-175`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/git-as-infrastructure.md?plain=1#L171-L175)

## appends decisions

See Audit ledger. The append is serialized through a process-local mutex. [`src-tauri/src/lib.rs:49400-49406`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L49400-L49406)

## mirrors writes

`lib.rs` calls the `mirror_*` functions "AFTER each successful file write" at the host's choke points: `write_worklist_authorization_record`, `retire_worklist_authorization`, `write_inflight_claim_sentinel`, `shrink_inflight_claim_sentinel`, `clear_inflight_claim_sentinel`. [`src-tauri/src/worklist_state.rs:8-13`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/worklist_state.rs?plain=1#L8-L13)

## appends trace lines

<!-- read: docs/trace-vocabulary.md#Trace subkind vocabulary -->

Every potential waypoint calls `append_bram_trace_line` after a single atomic check of `bram_trace_enabled()`, "so the cost when off is essentially zero". [`src-tauri/src/lib.rs:841-843`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L841-L843)

## indexed into FTS5

<!-- read: docs/apis.md#14. Search index -->

The host starts a background indexer at startup: "issue-230 unified search: background Claude-session indexer." [`src-tauri/src/lib.rs:97214-97215`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L97214-L97215) The module doc: the combined table covers a "common schema" for the unified search. [`src-tauri/src/search_index.rs:1-9`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/search_index.rs?plain=1#L1-L9) The indexer works in buckets: sessions, session intents, commits, worklist history and issues. [`src-tauri/src/lib.rs:39619-39625`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L39619-L39625) The Claude-shaped session pass lists Claude session files plus OpenCode's and Antigravity's exports, which are Claude-shaped so the same extraction serves all three. [`src-tauri/src/lib.rs:38104-38153`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L38104-L38153) A separate pass covers Codex sessions. [`src-tauri/src/lib.rs:38352-38374`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L38352-L38374)

## read by host

<!-- read: docs/subagents.md#The provider seam -->

The host discovers a project's Claude sessions directory from the encoded project path. [`src-tauri/src/lib.rs:32741-32750`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L32741-L32750) It scans session tails for turn end, see diagram 3. [`src-tauri/src/lib.rs:68857-68857`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L68857-L68857)

## read by guard

<!-- read: docs/authorization.md#Resolve, consumption, expiry, and interruption -->

The guard is a separate process; it "reads `resources/.worklist-authorization.json` directly for grant age (`guard_policy.rs`, `AUTH_REL`)". [`docs/authorization.md:335-339`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/authorization.md?plain=1#L335-L339) [`src-tauri/src/guard_policy.rs:362-362`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/guard_policy.rs?plain=1#L362-L362) The repo says the age check applies "only when `issuedAtMs > 0`". [`docs/authorization.md:172-174`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/authorization.md?plain=1#L172-L174)

## saves decisions and focus

<!-- read: docs/developing-bram.md#Client storage: decision vs. focus -->

See Pane browser storage. A commit moved one key from `localStorage` to `sessionStorage` after the realization that an X on the update banner meant "not now, this session". [`docs/developing-bram.md:263-285`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/developing-bram.md?plain=1#L263-L285)

## shared only by explicit refspec

<!-- read: docs/git-as-infrastructure.md#Bram's private refs and object reachability -->

Pushing is a person's choice. The refs are machine-local "unless someone deliberately uses an explicit refspec." [`docs/git-as-infrastructure.md:253-256`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/git-as-infrastructure.md?plain=1#L253-L256) The doc also says this "is a boundary, not secrecy: the referenced file content is ordinary Git object content in the local repository." [`docs/git-as-infrastructure.md:256-257`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/git-as-infrastructure.md?plain=1#L256-L257)

## session hook names it

<!-- read: docs/decisions/2026-10-27-bram-chooses-the-session.md#Bram chooses the session; nothing else names it -->
<!-- read: docs/apis.md#5. Sessions -->

The hook runs on its own. Three hooks call `session_report`: Claude's menu hook on `SessionStart` and `UserPromptSubmit`, Codex's worklist guard (so a change shows at the next tool call), and Antigravity's `antigravity-session` hook. It posts to `/__agent/session` using the port and token from the terminal's environment, "not resources/.bram-port, which names whichever instance wrote it last." [`src-tauri/src/guard.rs:1105-1111`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/guard.rs?plain=1#L1105-L1111) [`src-tauri/src/guard.rs:58-70`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/guard.rs?plain=1#L58-L70) [`src-tauri/src/guard.rs:94-97`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/guard.rs?plain=1#L94-L97) [`src-tauri/src/guard.rs:344-355`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/guard.rs?plain=1#L344-L355) The route, `handle_agent_session`, answers 403 to a bad token, accepts only Claude, Codex and Antigravity, and changes nothing unless the reporting provider is the current one. [`src-tauri/src/lib.rs:36296-36323`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L36296-L36323) The route catalog describes it as "The running CLI naming its own session". [`docs/apis.md:319-319`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/apis.md?plain=1#L319-L319) It "moves which session Bram shows as current and authorizes nothing." [`docs/authorization.md:57-59`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/authorization.md?plain=1#L57-L59) OpenCode reports through its plugin on `/__opencode/event` instead (diagram 5).
