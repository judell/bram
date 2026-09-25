# Parallel prototyping notes

A running log of what we learn when parallel work is driven through chat
rather than through the gate buttons: an orchestrating agent, subagents in
worktrees, a shared preview, then branches and PRs. This is track 2 of
judell/bram#404. Track 1 keeps the gate bar serial and simple. After the #273
decision, separating hunks in a shared file (problem 2) is frozen, so
parallelism lives here until we know enough to design it.

These notes have three uses:

- **Feed the next run.** Better prompts and fewer mid-run interruptions.
- **Give each run something to compare against.** What changed, what recurred.
- **Supply the source material for codifying parallel work in the UI**, so the
  design comes from observed friction rather than invention.

Append a section per run, update the prompt when a run forces a correction,
and add to the requirements list when the pane or gate gets something wrong.
Detailed receipts belong in the run's own forge comments. Link them from here
rather than copying them.

## The prompt (current best version)

Revised after run 1. The user ticks the items, pastes this as the message,
and clicks **Start**. The message fans out to every ticked item, and the agent
answers once. Fill in the angle brackets.

```
Route A for <item-a> and <item-b>.

Don't edit this checkout. Leave it clean for the whole run. When we're done
I'll click Drop on both items, so don't advance, commit, or call the commit
gate here.

0. Starting state. Switch this checkout to <default branch> and fast-forward
   it from origin. Stop anything listening on <port range>, by PID only,
   never by name, and tell me what you stopped.

1. Worktrees. From origin/<default branch>, create one worktree per item
   under .claude/worktrees/<n>, on branch <prefix>-<n>.

2. Dev server: <one server on a preview worktree that mirrors the item
   worktrees | one server per worktree on distinct ports>. Share
   dependencies by symlinking node_modules instead of installing per
   worktree, and tell me which links were needed. Report the port each
   server actually bound, which may not be the one you asked for. Record the
   PID of every background process you start.

3. Subagents. Spawn one per item, in parallel. Each brief includes:
   - the item's draft (resources/worklist-drafts/<id>.md) as its spec;
   - its worktree, and the rule to edit nothing outside it and run no
     state-changing git;
   - the smallest possible edit to any file the items share;
   - lookup rules for the framework (for XMLUI: xmlui-mcp first, and cite a
     doc URL for each non-obvious choice);
   - "For anything a person reads or sees, don't render-check it or judge
     how it reads. Report where to look: the URL, what each part shows,
     what to hover or compare. Specs and measurements are fine.";
   - throwaway scripts go in the scratchpad, never in the worktree.

4. Report and stop. Check each subagent's claims yourself (files, line
   references, spec runs) before reporting. For each item, tell me what
   changed, the preview URL, where to look, and what the agent wasn't sure
   of. Then wait. I'll look and give feedback.

5. After I say go: in each worktree, commit with "Refs #N" (no closing
   keywords, no session URL), push, and open a PR against <default branch>.
   Each PR description opens with your signature line. If I want an issue
   closed on merge, put "Closes #N" in the PR description. Edit PR
   descriptions through the REST API, because gh pr edit can fail silently,
   and verify the result. Stop every background process by PID. Leave the
   worktrees in place until the PRs merge.

Then I'll click Drop on both items.
```

The `.claude/worktrees/<n>` location is a correction from run 1 and **hasn't
been tested yet**. Bram's guard and claim capture recognize that path (see
`docs/git-as-infrastructure.md`, "Linked agent worktrees and proposal
coverage"), so edits there are checked against the items' declared files
instead of going unseen. Two things to confirm on the next run:

- that the directory stays out of the main checkout's `git status`;
- whether the guard still allows a direct `git commit` there, as it did
  outside the repo.

## Run 1: route A, 2026-09-24

**Setup.** This ran in the xmlui session. It covered two how-to items that
both add a nav entry to `website/src/Main.xmlui`:

- `issue-3908-multi-line-tooltip-howto`
- `issue-3895-aligned-collapsible-summaries`

Each item was worked by its own subagent in its own worktree at
`../xmlui-wt/<n>`, on a branch cut from `origin/main`. The orchestrator's
receipts are on #404:
https://github.com/judell/bram/issues/404#issuecomment-5823184308.

**Where the user stepped in:**

- **Right after Start**, to switch the checkout to `main`, refresh it, clear
  the 51xx ports, and use a single dev server. The checkout had been on
  `fix/documentpage-dead-url-prop`, and a 16-day-old `xmlui start` held 5173.
- **Mid-run**, to stop a subagent from deciding whether tooltip lists "look
  broken", and then to relay the new rendered-output rule to both subagents.
- **At review**, with the page feedback that specs couldn't give: two
  playgrounds clipped, and an example whose intent the page didn't explain.
- **At the end**, to ask for `Closes #N` in the PR descriptions.

**Outcomes:**

- **PRs:** xmlui-org/xmlui#3909 and #3910, closing #3908 and #3895 on merge.
- **Follow-up issues:**
  - #3911: every `fontVariant-*` theme variable does nothing, across 9
    components;
  - #3912: the Markdown part of #3895 that #3910 doesn't cover.
- **Board:** both items dropped by hand.
- **Rule change:** `conventions-user-is-the-eyes` (7d5436c), written because
  of this run.

## Nuances by theme

### Starting state and environment

- **Say where to start.** The checkout was on a feature branch. Without
  "freshly pulled default branch" in the prompt, the worktrees could have
  carried that branch's commits into both PRs.
- **Clear stale processes first.** A dev server left running for 16 days held
  the default port. Clear the port range by PID before starting anything.
- **`xmlui start` sets no `strictPort`.** A busy port silently moves the
  server, so a URL reported before the server binds can be wrong. Have the
  agent report the port it actually got.

### Dev server topology

- **State the topology up front.** The first prompt chose one server per
  worktree, and the user wanted one server. Whether one server can span
  worktrees depends on how the site loads content. In xmlui, the runtime globs
  `/src/**`, markdown arrives through `appGlobals.prefetchedContent`, and each
  how-to route is an explicit `<Page>` in `Main.xmlui`.
- **A symlinked tree didn't work.** Vite returns `403 Restricted` for a
  symlink that resolves outside the served tree. `xmlui start` accepts only
  `--port`, `--withMock` and `--proxy`, and replaces Vite's `server` config
  wholesale, so the allow-list can't be widened.
- **What worked:** a throwaway preview worktree holding both routes, plus a
  one-way copy loop from each item's worktree. A save showed in the preview
  about a second later. The loop is one more background process to stop by
  PID.

### Dependencies and specs

- **Serving** needs only a symlink to the main checkout's `node_modules`,
  because the workspace link resolves to the main checkout's engine.
- **Running specs** also needs `xmlui/node_modules` linked, for the engine's
  own dependencies such as `@radix-ui`.
- **The links can't be committed.** The repo's `node_modules` ignore pattern
  has no trailing slash, so it matches symlinks too.
- **Specs start slowly.** The first test took 17–24s against a 30s limit, and
  one run lost two tests to it. That's a CI risk.
- **Playgrounds mount only near the viewport.** Any automation has to scroll
  to each one first.

### Steering running work

- **The orchestrator can message a subagent mid-run.** It uses `SendMessage`
  with the subagent's id, and the subagent picks the message up at its next
  step. **Confirmed:** after the #3908 subagent was told to keep both tooltip
  forms and give no verdict, its final report had both forms, neutral wording
  and no verdict. A correction needn't wait for the report or cost a restart.
- **The user can't reach a subagent directly.** The path is the user's chat
  with the orchestrator, then `SendMessage` to the subagent.
- **Rules travel the same way.** A convention written in the bram session was
  pasted into the orchestrator's chat, and the orchestrator relayed it to both
  running subagents as overrides. That reached running work before the rule
  was committed. Setup's seeding reaches future sessions; relaying reaches the
  current one.
- **A relayed rule needs its boundaries.** The short version carried no
  exceptions, so the orchestrator over-applied it and dropped measurements
  and even a liveness check that the full rule allows. That's why the core
  bullet now carries its exceptions itself.
- **"Asks, then proceeds anyway."** At review, the orchestrator raised a
  question and then made the edit without waiting for an answer. It was
  harmless here, a small wording change in a worktree, but worth watching.

### Who judges rendered output

- **The old rule had agents render-check and judge.** The seeded conventions
  used to say that "verify" means render it. The orchestrator's briefs had
  subagents check pages with headless Playwright, and one brief told a
  subagent to decide whether tooltip lists "look broken" and change the page's
  recommendation on that basis.
- **Automated measurements mislead.** `getByRole("tooltip")` on a Radix
  tooltip is the visually hidden accessible copy (about 1px wide, `nowrap`).
  Measuring it gave 276px against a visible 123px, and a recommendation had
  been built on that number. Measure the popper content.
- **The user's look found what specs couldn't.** #3895's specs passed 8/8,
  yet the user read a contrast example ("these two 7s are meant *not* to line
  up") as a failed alignment. The page never said what it was showing. Two
  clipped playgrounds were the same kind of find.
- **The resulting rule** (`conventions-user-is-the-eyes`): the agent serves
  the page and says where to look, and the person judges. Specs, measurements
  and a liveness check remain allowed.

### Closing issues and forge writes

- **Closing moves to the PR.** Route A commits never pass the commit gate, so
  Bram's close-on-push never sees them. Commits keep `Refs #N`, and
  `Closes #N` in the PR description closes the issue on merge; merging is the
  consent. Bram's rule against closing keywords is enforced only on commit
  messages at the gate (`commit_message_closing_keyword`).
- **Partial fixes link without closing.** A PR that only partly resolves an
  issue should link follow-up issues without closing them: #3910 closes #3895
  and links #3911 and #3912.
- **`gh pr edit` can fail silently.** A Projects (classic) deprecation error
  meant nothing was saved. The REST API worked, and the closing links were
  confirmed through GraphQL `closingIssuesReferences`. The orchestrator saved
  this to its own xmlui memory. Because it affects every project and Codex
  too, it's a candidate for the seeded forge-CLI section of
  `app/__shell/reference/environment.md`.
- **Checking pays.** The orchestrator checked its subagents' claims itself
  (line references, file counts, closing links) before reporting or filing.
  That's how it found #3911 was broader than one component. Keep it in the
  prompt.

### What Bram doesn't see

Route A ran almost entirely outside Bram's record:

- **Board:** no removal. The items end in a manual Drop.
- **Issue closing:** no queued close.
- **History:** no worklist-history entry.
- **Gate:** no visibility of the worktree commits.
- **Guard:** it allowed `git commit` and `git push` in a worktree outside the
  project root, and never checked the edits there.

The pane also got things wrong while the run was live:

- **The status line.** Both items read "Green-lit under an hour ago, nothing
  came of it · the reason is in the draft — Start again, or Drop". The work
  was real but lived in worktrees, and no reason was in the draft.
- **The Shared with column.** It said "done: issue-3895…" for an item that
  wasn't done.
- **The gate note.** It read "…'s edits are entangled in a shared file;
  neither has exclusive changes, so Commit is withheld. Ask the agent to
  separate their edits". Neither item had any change in the checkout: a
  *planned* shared file was being counted as a change. User: "Way too
  complicated." This is being fixed in
  `gate-note-planned-overlap-is-not-a-change`.

### Observing a run

- **Watch everything, not just the agent's actions.** A watcher on the
  orchestrator's actions missed the user's interjections and permission
  prompts. Watch user turns too.
- **Observers converge.** The bram session, watching the xmlui transcript,
  reached the same reading of the user's feedback as the orchestrator did.
  The user had drafted a clarifying reply and deleted it unsent.
- **Leave the Setup banner alone mid-run.** The xmlui pane offered Run Setup
  because the seeded conventions changed during the run. We deferred it, for
  three reasons:
  - Setup doesn't reach running sessions; relaying the rule does.
  - It would have seeded an uncommitted copy.
  - It may touch the tracked `AGENTS.md` in a checkout the run keeps clean.

  Run it after the conventions commit lands and the run is over.

## Requirements for codifying parallel work in the UI

These are gathered from the runs. None is designed yet.

1. **An item knows where its work lives.** A declared field (a worktree, then
   a branch, then a PR URL), written by the agent when it creates the
   worktree. Inferring it from changed files fails when two worktrees share a
   file.
2. **Status reads from there.** The Changes column and status line diff that
   worktree for the item's files ("In progress in worktree 3908"), instead of
   reporting "nothing came of it" from a clean main checkout.
3. **The gate follows the landing path.** Commit doesn't apply to work that
   lands through a PR. Drop has to say that work exists elsewhere. Closing
   happens through the PR, and the board should show that.
4. **The record catches up.** A route A item that ends in merged PRs should
   leave a worklist-history entry that links them, rather than vanishing in a
   Drop.
5. **Start & commit stays single-item** (the #272 rule), kept deliberately
   conservative while this is learned.

## Open questions

- Does `.claude/worktrees/<n>` work in practice? Specifically: guard
  coverage, `git status` cleanliness, and whether direct commits are still
  allowed there.
- Should run-ending (PRs up, items dropped) be one step the agent can take,
  or stay the user's click?
- What does the user need to see while subagents run? Run 1 relied on a
  preview URL plus an observing session. The pane showed nothing.
