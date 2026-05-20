# `bram enhance` subcommand — design notes

A planned subcommand of the `bram` binary (not the `xmlui` CLI —
that has its own `add-inspector`). Run from any XMLUI project root,
idempotent, makes the project work well inside Bram.

## v0 scope

When we get to it, after the outside-the-surface layout work lands:

- Inject `<script src="/__shell/helpers.js"></script>` into the project's
  `index.html`.
- Add or update an `xmluiDesktop` block in `config.json` declaring the
  toolbar layout (which buttons appear top vs. bottom).

## v1 scope (followup, deliberately deferred)

CLAUDE.md handling — pattern (3) of three discussed: sidecar file plus a
marker-block in CLAUDE.md.

- Write `.claude/xmlui-desktop-conventions.md` containing the portable
  subset of xmlui-desktop's `app/__shell/conventions.md` (right-pane
  purpose, `toShell`/`toTurn` helper table, `worklist.json` schema +
  lifecycle, charting via `<EChart>`, drawer hosts Worklist / Sessions).
  Skip repo-internal bits (files-to-edit lists, architecture pointer to
  `~/.agents/scout/`).
- Add a single line to project `CLAUDE.md` inside markers:
  `<!-- xmlui-desktop:start -->` ... `<!-- xmlui-desktop:end -->`
  containing `<!-- @.claude/xmlui-desktop-conventions.md -->`
  (Claude Code's `@`-import directive).
- Re-runs replace what's between the markers; everything else in
  `CLAUDE.md` is preserved.

## Why

Without conventions guidance, an agent in a guest project doesn't know
`toShell`/`toTurn` exist, doesn't know `worklist.json` conventions,
doesn't know about the drawer. The sidecar pattern keeps the user's
`CLAUDE.md` uncluttered while still giving every Claude Code session in
that project the context.

## How to apply

- Print every path touched on each run.
- Offer `--dry-run`.
- A wrong write to `CLAUDE.md` is recoverable but annoying — be
  conservative about edits to existing files.

Reference: Claude Code `@`-import —
https://docs.claude.com/en/docs/claude-code/memory#claude-md-imports
