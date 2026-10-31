# State and data: detail

Click-through detail for 04-state.mmd. Facts are as of commit `b2eb8e0b` (2026-10-31). Solid arrows are what code does on its own; the dashed arrow depends on someone pushing on purpose. The repo has no single inventory of every file under `resources/`; the closest is the `.gitignore`, which this page leans on, and it doesn't say what each file is for. Files not named here exist (the ignore list is longer than the diagram).

## Bram host

The Rust process owns the files below. At startup it clears leftovers from a previous run: a stale in-flight claim, stale turn context, stale Codex intent/result files and stale PTY intents. [`src-tauri/src/lib.rs:88765-88772`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/src-tauri/src/lib.rs?plain=1#L88765-L88772) [`src-tauri/src/lib.rs:88787-88789`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/src-tauri/src/lib.rs?plain=1#L88787-L88789) It refuses to scaffold `resources/` or a trace directory unless the launch folder is a git repo root (issues #353, #448). [`src-tauri/src/lib.rs:88938-88951`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/src-tauri/src/lib.rs?plain=1#L88938-L88951) The repo doesn't say who may write to `resources/` besides the guard's rules; the authorization doc concedes authority files "are same-user writable artifacts". [`docs/authorization.md:254-256`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/docs/authorization.md?plain=1#L254-L256)

## Worklist files

`resources/worklist.json` holds metadata; prose lives in `resources/worklist-drafts/<id>.md`. [`app/__shell/conventions.md:156-201`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/app/__shell/conventions.md?plain=1#L156-L201) It needn't exist in advance: "Bram serves an empty default and the Worklist tab creates the file (and `resources/`) on first use." [`app/__shell/conventions.md:45-50`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/app/__shell/conventions.md?plain=1#L45-L50) This repo ignores both: "Worklist proposals are per-developer scratch; keep them out of the repo." [`.gitignore:46-48`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/.gitignore?plain=1#L46-L48) The agent writes these files (through guarded edits); the host merges them and tracks versions.

## Authorization record

`resources/.worklist-authorization.json`: one slot, 30 minute TTL. [`docs/authorization.md:129-136`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/docs/authorization.md?plain=1#L129-L136) [`docs/authorization.md:162-166`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/docs/authorization.md?plain=1#L162-L166) Gitignored. [`.gitignore:62-62`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/.gitignore?plain=1#L62-L62) The record is also the guard's project-root marker: the guard "uses that file's existence as the project-root marker". [`docs/authorization.md:311-316`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/docs/authorization.md?plain=1#L311-L316) Setup creates a stub with `kind: "none"` if none exists. [`src-tauri/src/lib.rs:55675-55691`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/src-tauri/src/lib.rs?plain=1#L55675-L55691)

## Execution claim

`resources/.inflight-claim.json`, single slot, retired when the turn ends. [`docs/authorization.md:137-141`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/docs/authorization.md?plain=1#L137-L141) [`.gitignore:59-59`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/.gitignore?plain=1#L59-L59) Startup removes a stale one: "Remove any stale inflight sentinel from a prior session that didn't complete cleanly." [`src-tauri/src/lib.rs:88765-88768`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/src-tauri/src/lib.rs?plain=1#L88765-L88768)

## Claim refs

The ledger is `resources/.claim-intervals.json`, an ordered list. The trees are kept alive by `refs/bram/claims/<atMs>`. "Record order, not timestamp sorting, defines adjacency because two boundaries can share a millisecond." [`docs/git-as-infrastructure.md:122-125`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/docs/git-as-infrastructure.md?plain=1#L122-L125) [`.gitignore:60-60`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/.gitignore?plain=1#L60-L60) Retired boundaries' refs are deleted lazily after item pruning, and "The newest boundary is always retained because it is the base of the open interval." [`docs/git-as-infrastructure.md:176-179`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/docs/git-as-infrastructure.md?plain=1#L176-L179) A second namespace, `refs/bram/safety/*`, is for explicit operator snapshots and is "retained until explicitly deleted". [`docs/git-as-infrastructure.md:235-240`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/docs/git-as-infrastructure.md?plain=1#L235-L240)

## Worklist history

`resources/worklist-history/<unix_ms>.json` (the effective worklist after the change) and `.md` (a changelog of the transition). [`docs/worklist-history.md:49-55`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/docs/worklist-history.md?plain=1#L49-L55) Trivial prose rewording is suppressed. [`docs/worklist-history.md:56-59`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/docs/worklist-history.md?plain=1#L56-L59) The conventions call the directory host-written. [`app/__shell/conventions.md:203-204`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/app/__shell/conventions.md?plain=1#L203-L204) This repo's `.gitignore` opts out of tracking the snapshots "for the Codex-themed release; revisit whether to track them in a later release." [`.gitignore:42-44`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/.gitignore?plain=1#L42-L44) The doc argues for committing them: "that committed history is their context." [`docs/worklist-history.md:34-37`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/docs/worklist-history.md?plain=1#L34-L37)

## Audit ledger

`resources/audit-ledger.jsonl`: "always-on, append-only JSONL record of authorization decisions and Bram-gated commits". [`src-tauri/src/lib.rs:46216-46218`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/src-tauri/src/lib.rs?plain=1#L46216-L46218) It is "NOT gated by bram_trace_enabled()"; "Metadata only — no request text, feedback, commit messages, diffs, or file contents"; and that "always-on" means "ungated, not fail-closed or tamper-evident". [`src-tauri/src/lib.rs:46219-46227`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/src-tauri/src/lib.rs?plain=1#L46219-L46227) Gitignored. [`.gitignore:74-77`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/.gitignore?plain=1#L74-L77)

## State mirror

A SQLite shadow of the authorization and claim files plus an append-only transitions ledger. "**Files remain the sole truth this phase.**" [`src-tauri/src/worklist_state.rs:1-12`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/src-tauri/src/worklist_state.rs?plain=1#L1-L12) Mirror failures are "trace-only — it must never block or alter the file-write path". [`src-tauri/src/worklist_state.rs:13-16`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/src-tauri/src/worklist_state.rs?plain=1#L13-L16) The database lives under the app cache directory, one per project: `worklist-state/<encoded project path>.db`. [`src-tauri/src/lib.rs:33686-33694`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/src-tauri/src/lib.rs?plain=1#L33686-L33694) Its "destiny is *authoritative*" in a planned migration, and a future grant book would move there, with the open question of how the separate guard process reads it. [`src-tauri/src/worklist_state.rs:21-25`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/src-tauri/src/worklist_state.rs?plain=1#L21-L25) [`docs/authorization.md:307-316`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/docs/authorization.md?plain=1#L307-L316) `scripts/state-mirror-check.py` compares files and rows on demand. [`src-tauri/src/worklist_state.rs:37-39`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/src-tauri/src/worklist_state.rs?plain=1#L37-L39) The repo doesn't say whether the migration has started beyond the shadow.

## Search index

An SQLite FTS5 database under the app's local data directory (outside the repo), one per project: `search-index/<encoded project path>.db`. [`src-tauri/src/lib.rs:33501-33509`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/src-tauri/src/lib.rs?plain=1#L33501-L33509) It indexes "Claude and Codex session transcripts, commits (with diffs), issues, worklist-history" and is served at `GET /__search`. [`app/__shell/conventions.md:728-730`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/app/__shell/conventions.md?plain=1#L728-L730) Changing the FTS table's shape "REBUILDS THE WHOLE INDEX — measured at ~50 minutes over 440 MB of session JSONL". [`src-tauri/src/search_index.rs:15-18`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/src-tauri/src/search_index.rs?plain=1#L15-L18) Convention: query it before diagnosing a past bug. [`app/__shell/conventions.md:732-736`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/app/__shell/conventions.md?plain=1#L732-L736)

## Trace log

`resources/bram-traces/bram-trace.log`, with prior runs archived at startup. [`src-tauri/src/lib.rs:816-820`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/src-tauri/src/lib.rs?plain=1#L816-L820) Traces default on: an absent setting resolves to true; `.bram.json` or `BRAM_TRACE` can turn them off. [`src-tauri/src/lib.rs:88601-88612`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/src-tauri/src/lib.rs?plain=1#L88601-L88612) The trace is not written outside a project folder. [`src-tauri/src/lib.rs:2308-2312`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/src-tauri/src/lib.rs?plain=1#L2308-L2312) Gitignored. [`.gitignore:102-103`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/.gitignore?plain=1#L102-L103) The development doc's ritual after any pane edit is to grep it for `console-error|console-unhandledrejection`: "Zero matches is the pass condition". [`docs/developing-bram.md:170-176`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/docs/developing-bram.md?plain=1#L170-L176) `docs/trace-vocabulary.md` catalogs the event kinds; I did not read it.

## Provider transcripts

Claude's under `~/.claude/projects/<encoded path>`, Codex's under `~/.codex/sessions`. [`src-tauri/src/lib.rs:31471-31471`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/src-tauri/src/lib.rs?plain=1#L31471-L31471) [`src-tauri/src/lib.rs:25734-25734`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/src-tauri/src/lib.rs?plain=1#L25734-L25734) OpenCode keeps sessions in SQLite, not transcript files, so Bram exports each to a Claude-shaped JSONL in its cache and reads that. [`src-tauri/src/lib.rs:31157-31160`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/src-tauri/src/lib.rs?plain=1#L31157-L31160) [`src-tauri/src/lib.rs:32593-32600`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/src-tauri/src/lib.rs?plain=1#L32593-L32600) Claude's and Codex's are the provider's, and Bram only reads them.

## bram-guard

Besides the authorization file, the guard keeps one breadcrumb per invocation in `resources/bram-traces/hook-events.log`, "written only when <root>/resources already exists". [`src-tauri/src/guard.rs:9-13`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/src-tauri/src/guard.rs?plain=1#L9-L13) The link `~/.bram/bram-guard` is re-pointed to the guard beside the running binary every 60 seconds, so with two Bram builds on one machine "every project's hooks ran whichever guard wrote last" until per-project routing was added. [`src-tauri/src/guard.rs:85-90`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/src-tauri/src/guard.rs?plain=1#L85-L90)

## Agent pane

State placement is a stated rule: "Pages own nothing but pure view ephemera"; session-global context and expensive derived data sit in the shell. [`docs/session-context-architecture.md:44-47`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/docs/session-context-architecture.md?plain=1#L44-L47) XMLUI unmounts a page on every tab switch. [`docs/session-context-architecture.md:32-34`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/docs/session-context-architecture.md?plain=1#L32-L34) The doc notes unfinished items: the permission menu's full view leaks into the Transcript page, and a voice-recording wedge is "Open". [`docs/session-context-architecture.md:63-74`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/docs/session-context-architecture.md?plain=1#L63-L74)

## Pane browser storage

Two classes. "**decision** (persists across sessions) → `localStorage`", "**focus** (dies with the session) → `sessionStorage`", and "Default to focus when unsure." [`docs/developing-bram.md:239-246`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/docs/developing-bram.md?plain=1#L239-L246) `localStorage` is shared by every Bram window on the machine, so project-specific keys get a project suffix via `window.__bramProjectScopedKey(base)`. [`docs/developing-bram.md:288-298`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/docs/developing-bram.md?plain=1#L288-L298) The doc records an audit of the existing unscoped keys on 2026-10-31: two project keys turned up, but nothing read them, "so they were deleted, not scoped." [`docs/developing-bram.md:317-321`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/docs/developing-bram.md?plain=1#L317-L321) The repo doesn't list the keys.

## Git remote

A normal push configuration will "transfer branch refs under `refs/heads/*`" and doesn't publish `refs/bram/*`, so claim and safety stores "are therefore machine-local unless someone deliberately uses an explicit refspec." [`docs/git-as-infrastructure.md:253-256`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/docs/git-as-infrastructure.md?plain=1#L253-L256)

## snapshot on change

"A filesystem watcher in `src-tauri/src/lib.rs` notices writes to `resources/worklist.json` and draft files under `resources/worklist-drafts/`, then calls `maybe_snapshot_worklist`." [`docs/worklist-history.md:39-42`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/docs/worklist-history.md?plain=1#L39-L42) [`src-tauri/src/lib.rs:72863-72869`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/src-tauri/src/lib.rs?plain=1#L72863-L72869) The snapshot function refuses to act on an unparseable read, after a past bug where a truncating writer made the board look empty (issue-426). [`src-tauri/src/lib.rs:72874-72879`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/src-tauri/src/lib.rs?plain=1#L72874-L72879)

## records approval

See diagram 3. The Tauri command `record_worklist_action_authorization` calls `write_worklist_authorization_record`. [`src-tauri/src/lib.rs:71747-71752`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/src-tauri/src/lib.rs?plain=1#L71747-L71752)

## writes claim

The same command writes the claim sentinel for the record's ids. [`src-tauri/src/lib.rs:71753-71755`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/src-tauri/src/lib.rs?plain=1#L71753-L71755) Unless the `_no_snapshot` variant is used, writing the claim first calls `record_claim_interval`, which is how a claim change leads to a boundary tree (next arrow). [`src-tauri/src/lib.rs:62024-62038`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/src-tauri/src/lib.rs?plain=1#L62024-L62038) [`src-tauri/src/lib.rs:62046-62051`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/src-tauri/src/lib.rs?plain=1#L62046-L62051)

## captures boundary trees

`capture_claim_tree` creates a temporary index, seeds it from `HEAD`, runs path-scoped `git add -A`, and writes a tree: "without staging anything in the real index." [`docs/git-as-infrastructure.md:116-121`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/docs/git-as-infrastructure.md?plain=1#L116-L121) [`src-tauri/src/lib.rs:57081-57090`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/src-tauri/src/lib.rs?plain=1#L57081-L57090) A successful ledger record needs a successful ref: "a bare tree ID written only to JSON is eventually eligible for garbage collection". [`docs/git-as-infrastructure.md:171-175`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/docs/git-as-infrastructure.md?plain=1#L171-L175)

## appends decisions

See Audit ledger. The append is serialized through a process-local mutex. [`src-tauri/src/lib.rs:46225-46231`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/src-tauri/src/lib.rs?plain=1#L46225-L46231)

## mirrors writes

`lib.rs` calls the `mirror_*` functions "AFTER each successful file write" at the host's choke points: `write_worklist_authorization_record`, `retire_worklist_authorization`, `write_inflight_claim_sentinel`, `shrink_inflight_claim_sentinel`, `clear_inflight_claim_sentinel`. [`src-tauri/src/worklist_state.rs:6-12`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/src-tauri/src/worklist_state.rs?plain=1#L6-L12)

## appends trace lines

Every potential waypoint calls `append_bram_trace_line` after a single atomic check of `bram_trace_enabled()`, "so the cost when off is essentially zero". [`src-tauri/src/lib.rs:821-823`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/src-tauri/src/lib.rs?plain=1#L821-L823)

## indexed into FTS5

The host starts a background indexer at startup: "issue-230 unified search: background Claude-session indexer." [`src-tauri/src/lib.rs:88756-88757`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/src-tauri/src/lib.rs?plain=1#L88756-L88757) The module doc: the combined table covers a "common schema" for the unified search. [`src-tauri/src/search_index.rs:1-9`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/src-tauri/src/search_index.rs?plain=1#L1-L9) I did not trace the indexer past its entry point, so what exactly is indexed per source is taken from the conventions line above.

## read by host

The host discovers a project's Claude sessions directory from the encoded project path. [`src-tauri/src/lib.rs:31368-31377`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/src-tauri/src/lib.rs?plain=1#L31368-L31377) It scans session tails for turn end, see diagram 2. [`src-tauri/src/lib.rs:63746-63746`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/src-tauri/src/lib.rs?plain=1#L63746-L63746)

## read by guard

The guard is a separate process; it "reads `resources/.worklist-authorization.json` directly for grant age (`guard_policy.rs`, `AUTH_REL`)". [`docs/authorization.md:311-315`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/docs/authorization.md?plain=1#L311-L315) [`src-tauri/src/guard_policy.rs:231-231`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/src-tauri/src/guard_policy.rs?plain=1#L231-L231) The repo says the age check applies "only when `issuedAtMs > 0`". [`docs/authorization.md:164-166`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/docs/authorization.md?plain=1#L164-L166)

## saves decisions and focus

See Pane browser storage. A commit moved one key from `localStorage` to `sessionStorage` after the realization that an X on the update banner meant "not now, this session". [`docs/developing-bram.md:263-285`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/docs/developing-bram.md?plain=1#L263-L285)

## shared only by explicit refspec

Dashed. The refs are machine-local "unless someone deliberately uses an explicit refspec." [`docs/git-as-infrastructure.md:253-256`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/docs/git-as-infrastructure.md?plain=1#L253-L256) The doc also says this "is a boundary, not secrecy: the referenced file content is ordinary Git object content in the local repository." [`docs/git-as-infrastructure.md:256-257`](https://github.com/judell/bram/blob/b2eb8e0b702970b3cfb160702268c51395cbe319/docs/git-as-infrastructure.md?plain=1#L256-L257)
