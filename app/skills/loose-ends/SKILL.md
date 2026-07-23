---
name: loose-ends
description: Summarize loose ends — unpushed commits, uncommitted/untracked changes, stashes, and anything needing attention. Read-only.
argument-hint: optional path or area to focus on
---

<!-- bram-managed: installed by Bram Setup from app/skills/loose-ends/SKILL.md; local edits will be overwritten -->

# Loose ends

Survey this repository's git state and give the user a tight, scannable summary
of everything that is loose and may need attention. **Read-only** — inspect and
report; do not stage, commit, push, stash, or modify anything.

If an argument is provided, focus the summary on that path or area (e.g. only
loose ends touching `src-tauri/`); otherwise cover the whole repo.

## Gather

Run these (all read-only):

- `git status --short --branch` — uncommitted + untracked changes, plus the
  branch's ahead/behind vs its upstream.
- `git log --oneline @{u}..HEAD` — commits not yet pushed. If the branch has no
  upstream, say so (there is nothing to push against) and skip this.
- `git log --oneline -5` — recent history, for context.
- `git stash list` — stashed work.
- `git branch -vv` — local branches and their tracking / ahead-behind.

## Report

A short summary in Markdown, including only the non-empty groups:

- **Unpushed commits** — count, then one line each (short SHA + subject).
- **Uncommitted changes** — modified / added / deleted tracked files.
- **Untracked files** — new files not yet added; flag anything that looks like it
  should be committed versus scratch/output.
- **Stashes** — any stash entries.
- **Other** — detached HEAD, local branches ahead of their upstream, etc.

End with a one-line bottom line: either "clean and pushed" or the single most
important loose end to deal with next.

Do not propose or perform commits, pushes, or stashes unless the user asks —
just surface the state.
