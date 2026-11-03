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
- `.bram.json` split (decision 2); `.bram.json~` named in the report.
- Instructions refreshed before the first agent turn: conventions, `AGENTS.md` block, provider files, and the Codex user-level instruction. Resumed sessions get a host-written note in their next turn that the folder moved. The host's own turn frame emits the new path; the guard already accepts either.
- The five scripts that hard-code `resources/.bram-port` (`xmlui-howto-gap-miner.py`, `send-ledger-settle-probe.py`, `switch-matrix.py`, `membership-starter-check.py`, `demo_instance.py`) read either.
- `/__search` rescanned; `worklist-history` links by SHA still resolve in the Commits tab.
- `/__bram/` serving with the `/resources/` alias.
- Conventions carry one line for history: "before 0.8.4 the folder was `resources/`; that path in older transcripts and issues means `.bram/`."
- Tracked content is never `git mv`'d by the host. Where a project tracks something Bram wrote, the host proposes a worklist item and the team commits it through the gate.
- If #393 has settled it: Setup stops editing `AGENTS.md` and proposes a worklist item instead.

### 0.8.5: retirement.

- A found `resources/.bram-port` with no `.bram/` is reported ("an older Bram's folder; migrate from Status") and not silently used (decision 5).
- The `/resources/` alias is removed.
- The old-name branches in the resolver, guard and plugin are deleted, and the literal-path test tightens to `.bram/` only. `guard::GUARD_TARGET_REL` goes with them.

## Acceptance cases

Four checkouts, each with its own checks, run before and after each release:

- **XMLUI:** nested `xmlui/` instance finds its own port; tracked `AGENTS.md` untouched by migration; `audit-findings-2026-09-22.md` reported as foreign and left in place; an `iterate:` turn writes its draft under `.bram/`.
- **Bram Studio:** `upstream-drafts/` preserved; three reminders still in `worklist.json`; history count unchanged; OpenCode port discovery and the Codex request/reply files work from the new folder.
- **community-calendar:** tracked `.bram.json` keeps `server.*` at the root and `git status` is clean of personal keys; `xmlui/resources/` migrates independently; `.opencode/` and `.agents/` no longer untracked-and-unignored.
- **Bram (this repo):** the mixed-folder dialog offers the selective move; the six tracked files stay; the source-repo special cases in Setup still hold.

For all four: `git status --porcelain` differs only by the lines that named `resources/` disappearing; `worklist-history/` count unchanged; the Commits tab resolves its SHAs.

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
