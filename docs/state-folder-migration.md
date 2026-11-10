# Moving Bram's state folder from `resources/` to `.bram/`

How the move lands, across three releases, under the decisions in
[`decisions/2026-11-02-bram-state-folder-is-dot-bram.md`](decisions/2026-11-02-bram-state-folder-is-dot-bram.md).
The checklist that tracks it is on [judell/bram#469](https://github.com/judell/bram/issues/469).
Nothing here changes what was decided; this is the plan of record moved out of the issue.

## What was in the way

- The host built the path in about 130 places in `lib.rs`, 38 in the guard's policy module, and a few each in the guard binary and the OpenCode plugin. Two helpers (`resource_relative_path`, `project_resource_path`) had 26 callers; everything else was literal. (Swept in `b54f19d8`; a test now fails on any new literal.)
- The shared `~/.bram/bram-guard` is one binary for every project on the machine. It finds the host by walking up to the port file. While projects run different Brams, it has to find either name, nearest instance first. (Landed in `b54f19d8`.)
- Bram's inventory of what it writes was split across `.gitignore`, `is_bram_owned_path` and the Files tab's state list, and #393 had already found two files missing from it. (One table, `BRAM_OWNED_ENTRIES`, since `2b31b07b`.)
- The conventions named `resources/` 17 times, the `AGENTS.md` block names it, the Codex transport is file paths under it, and `~/.codex/config.toml` names two of those files at user level. (The seeded docs define the state folder once and write `<state>/` since `b54f19d8`; the `AGENTS.md` block is deferred to #393.)

## Release sequence

### 0.8.3: compatibility and evidence. Nothing moves.

Every change here is behavior-preserving when `.bram/` doesn't exist, which is every project today. It is the rollback target for everything after.

- One resolver in the host, `bram_state_dir(root)`: `.bram/` if present (and the root is not the home folder, where `~/.bram/` holds the guard binary), else `resources/`. Every literal site goes through it; a test scans the source and fails on a new literal.
- The guard binary and its policy module resolve both names, nearest instance first, so a nested `xmlui/` instance is found before its parent.
- The OpenCode plugin reads either port file. The seeded conventions and reference docs say the folder is `.bram/` if it exists, else `resources/`, and write `<state>/` after that.
- One inventory of Bram-owned entries, `BRAM_OWNED_ENTRIES`, used by the owned-path predicate and the Files tab, with a test that every path the host writes is on it and that every machine-state entry is in this repo's `.gitignore`.
- A read-only dry run: `GET /__state-dir/plan`, the Status tab's "State folder" section, and `bram --state-dir-plan <root>` with no window. For a root: what would move, what's foreign, what's tracked, whether a claim is live, and nested instances.
- Setup writes `.opencode/.gitignore` and `.agents/.gitignore` naming its seeded entries (decision 7), after the loader check.

### 0.8.4: the move.

- Startup migration, per root, once, before any agent launches. Preconditions, all required: `.bram/` absent; `git ls-files resources` empty; every entry on the inventory; no live claim or authorization. Nested instances migrate themselves (decision 8).
- When a precondition fails, a dialog shows the inventory and offers: move only Bram-owned entries, or not now. It never guesses and never asks again once answered. This is the path for Bram's own repo (tracked assets), XMLUI (`audit-findings-2026-09-22.md`) and Studio (`upstream-drafts/`).
- The move is the rename, then a note file, then the port file. `.bram/.gitignore` is written first so there is no instant where the new folder is stageable. Then `.bram/.migrated-from`, one line with the old path and the time, so a person or a later Bram can tell the folder came from a migration and the migration can't run twice. The port file is written last, so a guard that wakes early finds the old one or nothing, never half of each. An empty `resources/` is removed; a non-empty one is left and reported.
- **As built** (branch `issue-469-mover`, `migrate_state_dir` in `lib.rs`): it runs first in the `.setup` closure, before `apply_launch_options`, the port read and `prepare_bram_trace_log`, and only in a project folder (issue-448's verdict). The preconditions come from the dry run's own core (`state_dir_plan_core`), so the mover and `/__state-dir/plan` can't disagree. The `*` ignore file is written into `resources/` before the rename and travels with it (a state-root dotfile is on the inventory), so `.bram/` is never stageable. A blocked rename is tried 10 times 200 ms apart; on giving up, the ignore file is removed (or its old bytes restored), so the old folder is left exactly as found. The resolver isn't cached, so the port read and everything after it find `.bram/` at once. The rename takes the folder whole, so no empty `resources/` is left behind. Trace category `state-migrate`, written once the log opens.
- `.bram.json` split (decision 2); `.bram.json~` named in the report.
- **As built, settings split** (`merged_project_config`, `settings_target_path` in `lib.rs`): every reader sees `.bram/settings.json` deep-merged over the root file (settings.json wins; root-only keys such as a team's `server.*` still apply). Once `.bram/` exists, settings writes go only to `.bram/settings.json` and the root `.bram.json` is never rewritten (#469 finding 4); before the move writes go to the root file and never create `.bram/`. No copy or backup at migration and no host-proposed worklist item: `/__state-dir/plan` reports `rootSettings: {exists, tracked, keys}` so an agent can propose the item at acceptance. **One exception, for an untracked root file** (`untracked-root-bram-json-folds-into-state`): an untracked root `.bram.json` that git doesn't ignore was stageable after the move (Windows acceptance: `?? .bram.json` in Studio and XMLUI). So at every startup once `.bram/` exists, `fold_untracked_root_settings` moves it into `.bram/settings.json`, deep-merging it under an existing one (settings.json wins), and moves a `.bram.json~` beside it to `.bram/.bram.json~`. A tracked root file (the settings note's case), an ignored one and an unparseable one stay where they are. Traced `op=settings-folded` / `op=settings-fold-skipped`.
- Instructions refreshed before the first agent turn: conventions, `AGENTS.md` block, provider files, and the Codex user-level instruction. Resumed sessions get a host-written note in their next turn that the folder moved. The host's own turn frame emits the new path; the guard already accepts either.
- Scripts read the state folder through `scripts/bram_state.py` and `scripts/bram-state.sh`, the host's rule in each language. That's 29 scripts as of 2026-11-04, not five. Where the demo fixture hydrates a scenario's board is pending the mover's answer on #474.
- `/__search` rescanned; `worklist-history` links by SHA still resolve in the Commits tab.
- `/__bram/` serving with the `/resources/` alias.
- **As built, after the move** (`issue-469-after-the-move`): a startup whose outcome was `moved` or `moved-selective` queues one host note (`queue_host_note`, kind `state-folder-moved`) for the next plain turn, so a resumed session learns its earlier `resources/` paths now live under `.bram/`. No rescan request: the indexer's first pass runs after the mover and reads `worklist-history` through the resolver, keyed `history:<id>`, and the Commits tab finds history by SHA through the resolver too. `/__bram/<rel>` serves the state folder (`state_url_rel` refuses `..`, `.`, empty segments, backslashes and drive colons); `/resources/<rel>` answers a file still in `resources/` first (a target app's asset, or one a selective move left), then the state folder. Nothing in Bram's pane fetched a state file by URL, so only the route changed. The seeded conventions and reference files already say `<state>/`; the `AGENTS.md` block's template (`app/shell/codex-startup-instructions.md`) still names `resources/` and is not yet folder-neutral.
- Conventions carry one line for history: "before 0.8.4 the folder was `resources/`; that path in older transcripts and issues means `.bram/`."
- Tracked content is never `git mv`'d by the host. Where a project tracks something Bram wrote, the host proposes a worklist item and the team commits it through the gate.
- If #393 has settled it: Setup stops editing `AGENTS.md` and proposes a worklist item instead.

- As built, the mixed-folder choice: when the move is refused only for foreign or tracked-foreign entries, a banner (and Status) offers **Move Bram's files** or **Not now**, stored in `resources/.state-move`. At the next startup `move` stages Bram's entries in `.bram-moving/` (its `*` `.gitignore` first) and renames it to `.bram/` in one step, so the resolver never sees a half-filled folder; any failure renames everything back and keeps the answer. `not-now` is quiet until Status's **Move at next launch**. A tracked Bram-owned entry still refuses (`tracked-owned`).

### 0.8.5: retirement.

- A found `resources/.bram-port` with no `.bram/` is reported ("an older Bram's folder; migrate from Status") and not silently used (decision 5).
- The `/resources/` alias is removed.
- The old-name branches in the resolver, guard and plugin are deleted, and the literal-path test tightens to `.bram/` only. `guard::GUARD_TARGET_REL` goes with them.

## Next release: recovery, retirement deferred (2026-11-10)

Reader retirement moves out again (Jon: "fine to move out again, nobody is
clamoring and better safe than sorry"). What ships instead is recovery for a
project whose state is still in `resources/` after the startup move didn't
happen (issue-469-leftover-folder-reported):

- Status, State folder, says Bram is still using `resources/`, and why, from
  the structured startup outcome in `/__state-dir/plan` (`startupMove`), never
  from trace text.
- **Migrate and relaunch** (`POST /__state-dir/migrate-relaunch`) is offered
  when a relaunch can help; for a mixed folder it records the `move` answer
  first. Otherwise Status names the obstacle: a live claim, unknown git status,
  Bram-owned tracked files, a leftover `.bram-moving/` or a failed rollback.
  Nothing is cleared, deleted or untracked to enable the button.
- Compatibility stays: the resolver, guards, plugin and scripts still read
  `resources/`, and nothing new is created in `.bram/`. The old-name branches
  stay until a migration-only startup path (or a separately specified runtime
  storage location) is designed. That is broader than reporting leftovers, and
  the call is Jon's. #469 stays open for parts 2 and 3.
- A blocked retry on Windows still needs confirming on Windows; a held handle
  on macOS is not evidence for its rename behaviour.

## Acceptance cases

Four checkouts, each with its own checks, run before and after each release:

- **XMLUI:** nested `xmlui/` instance finds its own port; tracked `AGENTS.md` untouched by migration; `audit-findings-2026-09-22.md` reported as foreign and left in place; an `iterate:` turn writes its draft under `.bram/`.
- **Bram Studio:** `upstream-drafts/` preserved; three reminders still in `worklist.json`; history count unchanged; OpenCode port discovery and the Codex request/reply files work from the new folder.
- **community-calendar:** tracked `.bram.json` keeps `server.*` at the root and `git status` is clean of personal keys; `xmlui/resources/` migrates independently; `.opencode/` and `.agents/` no longer untracked-and-unignored.
- **Bram (this repo):** the mixed-folder dialog offers the selective move; the six tracked files stay; the source-repo special cases in Setup still hold.

For all four: `git status --porcelain` differs only by the lines that named `resources/` disappearing; `worklist-history/` count unchanged; the Commits tab resolves its SHAs.

**A tracked root `.bram.json` that already holds personal keys** (community-calendar's has `shell.agent`, written by 0.8.3; Windows check, 2026-11-04). The settings split stops new writes to the root file but doesn't take existing personal keys out, and the host never edits a tracked file. So when the dry run reports `rootSettings.tracked: true` and the working copy differs from HEAD only in personal keys, Bram raises it itself: after the move it queues a host note (`tracked-settings-dirty`, once per state of the file) asking the agent to propose a worklist item, and Status says so (`tracked-bram-json-cleanup-offered`). The agent proposes the item. The item copies those keys into `.bram/settings.json` and restores the root file to HEAD. After it lands through the gate, `git diff .bram.json` is empty.

### The "before" readings (2026-11-02, macOS, after `2b31b07b`)

Taken with `bram --state-dir-plan <root>`. Nothing moved.

| checkout | state folder | would move | foreign (left in place by the mover) | tracked under the folder | nested instance |
|---|---|---|---|---|---|
| Bram (this repo) | `resources` | 27 entries | `code.svg`, `down.svg`, `up.svg`, `wrench.svg`, `xmlui-logo.svg`, `xmlui-org-repos.json` | 6 (the same six) | none |
| XMLUI | `resources` | 20 entries | `audit-findings-2026-09-22.md`, `sdetails-collapsed.png`, `sdetails-expanded.png` | 0 | `xmlui/` |
| Bram Studio | `resources` | 18 entries | `upstream-drafts/` | 0 | none |
| community-calendar | `resources` | 20 entries | `proposal.json` | 0 | `xmlui/` |

Every case the plan predicted shows up as predicted; no checkout has a `.bram/` folder yet. The same flag runs on Windows with no instance up, so that machine's readings can be added beside these.

## Judgment calls recorded for 0.8.4

From the dry run as landed: a consumed but unexpired authorization counts as a live claim (advance and commit still use it); the state-mirror database's claim is not consulted; `wouldMove` reads the folder's top level only. From the inventory: two classes (machine state, seeded content) plus "shown only" may be too few if a managed project commits its `worklist-history/`. From the self-ignore: `.agents/hooks.json` and `.opencode/opencode.json` are merged files, so a user's additions to them are hidden from git until the project tracks the file.
