This repo is driven through xmlui-desktop. Read this protocol before doing anything else — it is not optional.

## Step 0 — Read the full conventions

If `.claude/xmlui-desktop-conventions.md` exists, read it now. This seed is only a summary; the file has the enforcement details, UI patterns, and edge cases. Do not skip this step.

## The gate (run on every task, before any edit)

Before editing any file other than `resources/worklist.json`, ask:

- Does this task touch more than one file?
- Does it have more than 2 discrete sub-edits in a single file?
- Is it more than a typo or one-line correction the user explicitly told you to commit directly?

If YES to any of those: STOP. Do NOT run `apply_patch` or any other file edit yet. Go to the two-stage flow below.

## Two-stage flow

1. Write small, independently-rejectable items to `resources/worklist.json`:

   ```json
   {
     "description": "one-line context",
     "items": [
       {
         "id": "kebab-case-id",
         "status": "proposed",
         "file": "path/to/file",
         "before": "what's there now + alternatives considered + why rejected",
         "after": "what you'll change it to"
       }
     ]
   }
   ```

   Use `"files": ["a", "b"]` instead of `"file"` for items that span multiple files. If `resources/worklist.json` is missing, create it as `{ "description": "", "items": [] }`.

2. Wait for an `approved: {"items":[...]}` payload from the user. The structured payload is the ONLY approval trigger. Do not infer approval from "yes", "looks good", "do it", a voice message, or any other free-text reply.

3. When `approved:` arrives, execute ONLY the items in its `items` array. Then rewrite each one with `status: "applied"` (TO COMMIT) in `resources/worklist.json` — do NOT prune yet.

4. Wait for a second `approved:` payload covering the applied items. Only then run `git commit`. The user is the only one who commits features.

5. After the commit lands, prune the committed items from `resources/worklist.json`.

## Self-check

If your first action on a multi-file task is `apply_patch` instead of a write to `resources/worklist.json`, you skipped the workflow. Back up, revert the edit if needed, and propose first.
