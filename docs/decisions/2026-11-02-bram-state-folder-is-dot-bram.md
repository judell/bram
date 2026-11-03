# Bram's per-project state folder becomes `.bram/`, moved in three releases

Status: decided 2026-11-02 · revisit when 0.8.5 retires the old name, or when a project needs `resources/` kept

**Question:** Bram keeps its per-project state (worklist, drafts, feedback, traces, port file, sidecars) in `resources/`, a name that collides with project folders of the same name (judell/bram#469, raised from XMLUI). What should the folder be, how should personal settings in the tracked `.bram.json` be handled, and how does the move land without losing anyone's state?

**Decision:** eight points, taken together on 2026-11-02 from the three acceptance-case reports (XMLUI, Bram Studio, community-calendar) and Antigravity's synthesis in #469's comments. The sequencing lives in [`../state-folder-migration.md`](../state-folder-migration.md); #469 keeps the checklist.

1. **The folder is `.bram/`, and it ignores itself.** Setup writes `.bram/.gitignore` containing `*` before anything else lands there. No project `.gitignore` or `.git/info/exclude` is edited. A project that wants to commit something from inside adds a `!` exception.
2. **`.bram.json` splits into team settings and personal settings.** Team settings (community-calendar's `server` command is the example) stay in the root `.bram.json`, which stays tracked. Personal settings (`shell`, `ai`, `traces`, `worklist`, `guards`, `menus`, `ui`) move to `.bram/settings.json`, ignored with the rest. Relative paths in either stay anchored to the project root. Migration moves the personal keys and names any `.bram.json~` backup it finds.
3. **No folder-override setting in the first two releases.** The selective move (Bram-owned entries only) covers every mixed folder seen, including Bram's own repo. If a project turns up that needs `resources/` kept, the setting is added then, at user level.
4. **The loopback URL moves to `/__bram/`,** with `/resources/` kept as an alias until retirement. Today `/resources/worklist.json` answers on Bram's port; only the different port keeps that from colliding with XMLUI's website assets.
5. **The old reader is removed only when a leftover folder can be migrated later.** A project nobody has opened since the move still keeps its worklist and history in `resources/`; if Bram simply stopped looking, opening it would show an empty worklist. So before the old reader goes, Bram must notice a leftover `resources/` folder, say it is from an older Bram, and offer to migrate it from the Status tab. A release count can't prove every project was opened; a working migrate-later path can.
6. **`AGENTS.md` is deferred to #393, deliberately.** Setup appends a block to a tracked team file, and every contributor without Bram carries that diff. The folder rename never touches `AGENTS.md` (no ignore rule covers a tracked file), the fix is to change what Setup writes into a team's file, which needs the team's consent and its own design, and its blast radius is every managed project's contributors. The cost: the diff persists through 0.8.3 and 0.8.4. If #393 settles in time, one piece may ride on 0.8.4: Setup stops editing `AGENTS.md` and proposes a worklist item instead.
7. **`.opencode/` and `.agents/` ignore what Setup seeds into them.** The rename alone would trade one staging hazard for two. Verified first (2026-11-02, demo instance) that neither OpenCode 2.1.4 nor Antigravity 1.4.12 consults `.gitignore` when loading. As landed in `8e015b16`, the ignore file names Bram's seeded entries rather than `*`, because unlike `.bram/` these folders are shared with the project and `*` would hide a user's own uncommitted rules or plugins. An existing `.gitignore` is never overwritten; a folder the project tracks anything in is skipped.
8. **Nested instances migrate themselves.** A Bram launched on a subfolder keeps its own state folder there (two exist today, both live: `community-calendar/xmlui/` and `xmlui/xmlui/`). Migration is each Bram's job for its own root only; otherwise the root Bram would move state out from under a running instance it doesn't own.

**Evidence:**

- The three acceptance reports and the synthesis, #469 comments of 2026-11-02 (Codex from XMLUI, OpenCode from Bram Studio, Antigravity from community-calendar).
- The host built the path by hand in about 200 lines of `lib.rs`, 91 of `guard_policy.rs` and 26 of `guard.rs`, with only 26 callers of the two path helpers; the rest were literals (counted 2026-11-02 before `b54f19d8`).
- Bram's own repo tracks six asset files under `resources/`, which makes it the mixed-folder case rather than the XMLUI report's `worklist-history/` guess.
- Dry-run readings of the four acceptance checkouts, taken with `bram --state-dir-plan <root>` on 2026-11-02: in the migration doc.
- The 0.8.3 landings under these decisions: `b54f19d8` (resolver, guard, plugin, conventions), `2b31b07b` (inventory), `9219d7c9` (dry run), `8e015b16` (self-ignore).

**Alternatives:**

- A user-level setting naming the folder, no rename: rejected as the first step; the collision is the default's, and a setting leaves every new project with it.
- `.gitignore` or `.git/info/exclude` edits by Setup: rejected; a tracked file is a team file, and the exclude file is per clone.
- Renaming now, with a shim, in one release: rejected; 0.8.3 lands nothing that moves state, so it is the rollback target for everything after.
- Leaving a project's tracked files in `resources/` and moving only Bram's own entries: not an alternative but decision 3's selective move, accepted; the global override setting is what was rejected.
- `*` in the `.opencode/` and `.agents/` ignore files: rejected at landing (decision 7) for hiding the user's own files.

**Revisit when:** a project needs `resources/` kept (add the user-level setting, decision 3); #393 settles the `AGENTS.md` design (decision 6); 0.8.5 removes the old name (decision 5's migrate-later path must exist first).
