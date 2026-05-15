This repository is being used through xmlui-desktop with agent coordination enabled.

Follow these repo-local rules for multi-step work:

- Treat `resources/worklist.json` as the canonical pending-work surface.
- When a change spans multiple files or several discrete edits, propose small items in `resources/worklist.json` before making the change.
- New items are `proposed` / TO APPLY. After the user approves and you apply the edits, rewrite the same items with `status: "applied"` / TO COMMIT instead of pruning them immediately.
- Do not commit `applied` items until the user explicitly approves the TO COMMIT step.
- After a commit or an explicit drop, prune only the affected items from `resources/worklist.json`.
- If `resources/worklist.json` is missing, create it as:
  `{ "description": "", "items": [] }`

If `.claude/xmlui-desktop-conventions.md` exists, treat it as additional repo instructions and read it early in the session.
