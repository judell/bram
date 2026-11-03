# Worklist: detail

Click-through detail for 03-gate.mmd. Facts are as of commit `e8a38ac0` (2026-11-02). The picture is four stages: define an item, refine it until you click Start, build and test it until you click Commit, and land it. Each of the two middle stages is a loop driven by your feedback. Of the arrows inside the stages, one, the agent calling advance, depends on the agent remembering; the rest is what code does on its own. The gate has more states than one picture holds; the conventions file and `docs/authorization.md` are the full account.

## Issue

<!-- read: app/__shell/conventions.md#Authoring conventions -->

An item can start from a forge issue. Its id then takes the form `issue-<N>-<slug>`, "paired with `closesIssues` when both apply". [`app/__shell/conventions.md:472-474`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/app/__shell/conventions.md?plain=1#L472-L474) `closesIssues` "declares which issues the commit resolves and drives the close-on-commit dialog", and is set conservatively: "only when the commit truly closes the issue". [`app/__shell/conventions.md:232-235`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/app/__shell/conventions.md?plain=1#L232-L235)

![An issue opens in Bram's own issue view: here #466, which a contributor filed from their own Bram.](screenshots/worklist-00-issue.png "Issue #466")

## You or the agent

<!-- read: app/__shell/conventions.md#Lifecycle: propose → triage → mechanical transitions -->

Without an issue, the item gets "a bare descriptive slug". [`app/__shell/conventions.md:474-475`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/app/__shell/conventions.md?plain=1#L474-L475) Either way the agent writes it: "write the draft to `<state>/worklist-drafts/<id>.md`, then the metadata item to `<state>/worklist.json`. Each item small, discrete, independently rejectable." [`app/__shell/conventions.md:268-271`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/app/__shell/conventions.md?plain=1#L268-L271)

## Proposed item

<!-- read: app/__shell/conventions.md#When to route through the worklist -->
<!-- read: app/__shell/conventions.md#Schema and draft layout -->

The convention: "Default: every change request goes through `<state>/worklist.json`." The conventions define `<state>/` once: "Bram's state folder, written `<state>/` below, is `.bram/` if it exists, else `resources/`". [`app/__shell/conventions.md:45-46`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/app/__shell/conventions.md?plain=1#L45-L46) [`app/__shell/conventions.md:57-62`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/app/__shell/conventions.md?plain=1#L57-L62) A proposed item names files, with before/after prose in `<state>/worklist-drafts/<id>.md`. [`app/__shell/conventions.md:104-111`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/app/__shell/conventions.md?plain=1#L104-L111) Writing it is "*asking*, not approval". [`app/__shell/conventions.md:266-271`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/app/__shell/conventions.md?plain=1#L266-L271)

The guard checks writes to `worklist.json`: inline `before`/`after` is denied with `worklist-inline-prose`, and a write whose `version` isn't the on-disk version plus one is denied with `stale-worklist-version`. [`src-tauri/src/guard_policy.rs:4035-4044`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/guard_policy.rs?plain=1#L4035-L4044) [`src-tauri/src/guard_policy.rs:4054-4084`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/guard_policy.rs?plain=1#L4054-L4084) Item order, ids and the draft layout are conventions, see [`app/__shell/conventions.md:209-225`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/app/__shell/conventions.md?plain=1#L209-L225).

![The item from #466 as proposed: "for #466" beside its status, and the files it expects to change. "Shared with" counts the other items on the board that declare the same file.](screenshots/worklist-02a-files.png "Files expected to change")

![Its Before: what is there now, checked against the code, and what a search of the project's history found.](screenshots/worklist-02b-before.png "Before")

![Its After: what will change, and the design questions to settle before Start, each with a recommendation.](screenshots/worklist-02c-after.png "After")

## Worklist pane

<!-- read: docs/worklist-gate-design.md#The selection gate -->

The README's account of the user's side: tick items, then "*Start* accepts a proposal and authorizes the agent to alter files", "*Commit* commits an item's altered files to git", "*Drop* retires items you don't want". [`README.md:52-60`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/README.md?plain=1#L52-L60) Buttons "dim rather than disappear". [`README.md:54-54`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/README.md?plain=1#L54-L54) The pane is the `Worklist` and `WorklistGateBar` components. [`app/tools/Main.xmlui:1052-1054`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/app/tools/Main.xmlui?plain=1#L1052-L1054) The row offers one way to continue and one way to finish: on a begun item with a live approval and changes on disk, "Resume & commit" dims because it "would repeat Commit". [`docs/worklist-gate-design.md:148-150`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/worklist-gate-design.md?plain=1#L148-L150) In the markup, the Start & commit button is "dimmed whenever Commit can be lit". [`app/tools/components/WorklistGateBar.xmlui:371-373`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/app/tools/components/WorklistGateBar.xmlui?plain=1#L371-L373)

## Authorization record

<!-- read: docs/authorization.md#Where the implementation lives -->
<!-- read: docs/authorization.md#Resolve, consumption, expiry, and interruption -->

`resources/.worklist-authorization.json` holds `kind`, `ids`, recorded item bodies, `issuedAtMs`, `source`, `consumedAtMs`, `interruptedAtMs` and `commitToo`. It is "a **single slot**: a subsequent gate action replaces it." [`docs/authorization.md:137-144`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/authorization.md?plain=1#L137-L144) The TTL is `WORKLIST_AUTH_TTL_MS`, 30 minutes. [`src-tauri/src/lib.rs:153-153`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L153-L153) [`docs/authorization.md:170-174`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/authorization.md?plain=1#L170-L174) The writer is `write_worklist_authorization_record`. [`src-tauri/src/lib.rs:76392-76396`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L76392-L76396) The repo itself calls this record one of the "same-user writable artifacts, not cryptographically protected records." [`docs/authorization.md:278-282`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/authorization.md?plain=1#L278-L282) Both Start and Commit use kind `approved`; only apply-and-commit adds `commitToo`. [`docs/authorization.md:270-276`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/authorization.md?plain=1#L270-L276) A target design replaces this with per-(item, verb) grants; the doc says it is "**not shipped behavior**". [`docs/authorization.md:322-329`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/authorization.md?plain=1#L322-L329)

## Execution claim

<!-- read: app/__shell/conventions.md#The claim ends with your turn -->
<!-- read: app/__shell/reference/worklist-mechanics.md#The claim and the authorization -->

`resources/.inflight-claim.json` is "execution state: the host writes it while a turn runs, retires it when the turn ends, and re-arms it next turn while an authorization is live." [`app/__shell/conventions.md:357-363`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/app/__shell/conventions.md?plain=1#L357-L363) Turn-end clears the claim, while a still-live approval "may survive and rearm a later turn". [`docs/authorization.md:145-149`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/authorization.md?plain=1#L145-L149) Entering `working` with a live authorization re-establishes the claim. [`src-tauri/src/lib.rs:9299-9307`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L9299-L9307) The writer is `write_inflight_claim_sentinel`. [`src-tauri/src/lib.rs:67143-67149`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L67143-L67149)

## bram-guard

<!-- read: docs/authorization.md#Exceptions and enforcement gaps -->

The deciding hook binary: "Worklist hooks decide with real exit codes and fail CLOSED on guard faults (bram-guard-authority-fail-closed); menu hooks POST the pane's menu claims/clears and stay deliberately fail-open". [`src-tauri/src/guard.rs:5-9`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/guard.rs?plain=1#L5-L9) It dispatches `claude-worklist`, `codex-worklist`, `opencode-worklist` and `antigravity-worklist` to the same decider, `authority_worklist_hook`. [`src-tauri/src/guard.rs:48-93`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/guard.rs?plain=1#L48-L93) [`src-tauri/src/guard.rs:603-609`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/guard.rs?plain=1#L603-L609) Besides worklist coverage it denies commits and forge writes that carry session URLs (`no-session-url`) and worklist lifecycle calls made from subagents (`subagent-lifecycle-call`). [`src-tauri/src/guard_policy.rs:3717-3744`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/guard_policy.rs?plain=1#L3717-L3744) The subagent check has a documented limit: it "reads only inline command text", a substring test, so a route called from a script file, a shell function or an alias is not seen. [`docs/authorization.md:254-262`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/authorization.md?plain=1#L254-L262) For Antigravity the guard also fails closed when agy's payload has changed shape (commit `d8a7e925`): the matcher admits only the captured tools, "so anything else means agy's payload changed, and agy updates itself without a Bram release", and the hook refuses such a payload as a guard fault before translating it. [`src-tauri/src/guard.rs:527-533`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/guard.rs?plain=1#L527-L533) [`src-tauri/src/guard.rs:580-602`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/guard.rs?plain=1#L580-L602)

The header comment of `guard_policy.rs` says the module "began as a shadow port" of a Python guard, since retired, and that "The decision pipeline here is now the implementation." [`src-tauri/src/guard_policy.rs:1-6`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/guard_policy.rs?plain=1#L1-L6) `guard.rs` says the Rust guard is "the sole implementation since retire-python-hooks-rust-only". [`src-tauri/src/guard.rs:1-5`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/guard.rs?plain=1#L1-L5)

## Agent edits

<!-- read: app/__shell/conventions.md#When to route through the worklist -->
<!-- read: docs/authorization.md#Direct edits and provenance -->

The convention allows edits only inside an authorized item, with stated opt-outs (an explicit `just do it` in the same turn, the `skip-worklist:` prefix, correcting code written in the current iteration, an uncommitted draft, issue-only forge work). [`app/__shell/conventions.md:63-97`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/app/__shell/conventions.md?plain=1#L63-L97) Unauthorized edits "land in someone else's commit". [`app/__shell/conventions.md:156-158`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/app/__shell/conventions.md?plain=1#L156-L158) Direct-edit grants are a separate sidecar with a one-hour lifetime. [`docs/authorization.md:194-208`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/authorization.md?plain=1#L194-L208)

![The Start click arrives as a turn, and the agent begins by reading the code the change touches, narrating as it goes.](screenshots/worklist-04b-begins.png "Agent begins work")

![The agent hands part of the work to a subagent. Its transcript opens as its own tab, so you can follow its reasoning and tool calls as they happen.](screenshots/worklist-04c-subagent.png "Agent dispatches a subagent")

## Applied item

<!-- read: app/__shell/conventions.md#Schema and draft layout -->

A `status` of applied means changes are on disk and awaiting commit approval, and "a begun `proposed` item can be committable too". [`app/__shell/conventions.md:254-257`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/app/__shell/conventions.md?plain=1#L254-L257) The standard flow, in the conventions' words, is approved proposed, then advance to applied, then a separate commit approval, then prune. [`app/__shell/conventions.md:259-261`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/app/__shell/conventions.md?plain=1#L259-L261) `begunAtMs` is stamped by the host, never the agent. [`app/__shell/conventions.md:244-249`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/app/__shell/conventions.md?plain=1#L244-L249)

## You test the change

<!-- read: app/__shell/conventions.md#Lifecycle: propose → triage → mechanical transitions -->

The second loop. With items selected, a message you send "is **feedback addressed to those items**, and arrives as an `iterate:` turn." [`app/__shell/conventions.md:285-288`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/app/__shell/conventions.md?plain=1#L285-L288) On a begun or applied item the agent will "edit on-disk files per the feedback". [`app/__shell/conventions.md:334-335`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/app/__shell/conventions.md?plain=1#L334-L335) The loop ends when you click Commit.

![After the build, the new switch is under Settings, Worklist.](screenshots/worklist-05a-setting.png "The new Pause setting under Worklist")

![Its info note: what pausing does, what still applies, and what to do before turning enforcement back on.](screenshots/worklist-05b-description.png "Description of the setting")

![Switched on, a strip at the top of the pane says enforcement is paused, with a button to resume.](screenshots/worklist-05c-paused.png "Setting in action")

## Commit route

<!-- read: app/__shell/conventions.md#Commit and apply-and-commit gates -->
<!-- read: app/__shell/reference/worklist-mechanics.md#Commit timeouts: never re-POST -->

`handle_worklist_commit` is held under a lock for its whole body, so a re-POST "queues behind the in-flight commit". [`src-tauri/src/lib.rs:88712-88715`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L88712-L88715) Before it commits it rejects forge closing keywords, because the forge would auto-close the issue at push, "bypassing the close-on-commit dialog's explicit user consent". [`src-tauri/src/lib.rs:88732-88738`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L88732-L88738) It rejects session URLs. [`src-tauri/src/lib.rs:88748-88764`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L88748-L88764) In the Bram source repo it refuses unformatted Rust (`cargo fmt --check`, 409 `op=refuse-unformatted-rust`). [`src-tauri/src/lib.rs:88870-88883`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L88870-L88883) It refuses a commit that would split a jointly owned file (`op=refuse-joint-interval`). [`src-tauri/src/lib.rs:89005-89013`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L89005-L89013) The conventions add: "`worklist-commit` is single-shot per approval. On a timeout, never re-POST". [`app/__shell/conventions.md:405-407`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/app/__shell/conventions.md?plain=1#L405-L407)

## Git commit

<!-- read: docs/git-as-infrastructure.md#Dirty state and the ordinary Worklist commit -->
<!-- read: docs/git-as-infrastructure.md#Selective commits: Git as a transaction engine -->

Two paths. The ordinary one is conventional: path-scoped `git add -A -- <path>` then a path-scoped `git commit`, which runs normal commit hooks. [`docs/git-as-infrastructure.md:73-95`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/git-as-infrastructure.md?plain=1#L73-L95) When another begun item owns lines in the same file, `interval_stage_commit` builds the commit from interval patches in a temporary index, using `commit-tree` and `update-ref`. [`docs/git-as-infrastructure.md:181-231`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/git-as-infrastructure.md?plain=1#L181-L231) [`src-tauri/src/lib.rs:63099-63104`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L63099-L63104) [`src-tauri/src/lib.rs:63172-63172`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L63172-L63172) Because `commit-tree` runs no hooks, "it must enforce important repository gates before the plumbing commit". [`docs/git-as-infrastructure.md:93-95`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/git-as-infrastructure.md?plain=1#L93-L95) Pushing is separate: "Pushing is the user's". [`app/__shell/conventions.md:677-678`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/app/__shell/conventions.md?plain=1#L677-L678)

## Pruned, kept in history

<!-- read: docs/worklist-history.md#Flow -->

After a successful commit the route prunes the committed ids by calling the same mutate handler with `op: prune`. [`src-tauri/src/lib.rs:89631-89637`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L89631-L89637) A filesystem watcher calls `maybe_snapshot_worklist` and writes `resources/worklist-history/<unix_ms>.json` and `.md`. [`docs/worklist-history.md:41-52`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/worklist-history.md?plain=1#L41-L52) [`src-tauri/src/lib.rs:78156-78162`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L78156-L78162) This repo's `.gitignore` opts out of committing those snapshots. [`.gitignore:44-46`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/.gitignore?plain=1#L44-L46) The conventions call the directory host-written: "the guard denies agent edits". [`app/__shell/conventions.md:206-207`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/app/__shell/conventions.md?plain=1#L206-L207)

![The History tab lists each item's lifecycle: issue-466 went Proposed, Feedback, Applied, Feedback, Committed.](screenshots/worklist-08a-history.png "Worklist history shows item lifecycles")

![The entry for issue-466: when it ran, its lifecycle, its files, a link to the commit, and the item's Before and After as they stood. Searchable, like the rest of the history.](screenshots/worklist-08b-history-entry.png "The issue-466 item recorded in searchable History")

## shown on the board

<!-- read: app/__shell/conventions.md#Schema and draft layout -->

"Bram merges draft prose into" the served board payload, "so the pane sees one combined item". [`app/__shell/conventions.md:201-204`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/app/__shell/conventions.md?plain=1#L201-L204) Changes reach the pane by event; the record writer's caller emits `worklist-changed`. [`src-tauri/src/lib.rs:77050-77053`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L77050-L77053)

## Start records approval

<!-- read: docs/worklist-gate-design.md#The selection gate -->
<!-- read: docs/authorization.md#Where the implementation lives -->

The pane records the action first, through the `record_worklist_action_authorization` Tauri command, and sends the turn only if that succeeds. [`app/__shell/helpers.js:743-748`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/app/__shell/helpers.js?plain=1#L743-L748) [`src-tauri/src/lib.rs:77041-77046`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L77041-L77046) The doc's summary: "Start click → host records approved ids + execution claim → apply turn". [`docs/authorization.md:124-125`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/authorization.md?plain=1#L124-L125) An `approved:` string typed in chat is not the authority-creation path: "Merely supplying an `approved:` or `drop:` string is not the normal authority-creation path." [`docs/authorization.md:36-39`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/authorization.md?plain=1#L36-L39)

![With the item ticked, Start authorizes the work. "Applies to" names the items the click covers.](screenshots/worklist-04a-start.png "Click Start to authorize the work")

## starts claim

<!-- read: app/__shell/reference/worklist-mechanics.md#The claim and the authorization -->

The same Tauri command writes the claim for the record's ids. [`src-tauri/src/lib.rs:77047-77049`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L77047-L77049)

## coverage read by guard

<!-- read: docs/authorization.md#Exceptions and enforcement gaps -->

"File-aware tools can pass because a proposed/applied item declares the path." [`docs/authorization.md:245-246`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/authorization.md?plain=1#L245-L246) So a declared path is enough to edit even before Start; the stronger approval check is observed, not enforced: `trace_would_deny_unaddressed` "records stronger approval findings ... These are observations, not denials." [`docs/authorization.md:246-249`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/authorization.md?plain=1#L246-L249) The Bash branches are "coarser still". [`docs/authorization.md:249-250`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/authorization.md?plain=1#L249-L250) The no-coverage deny reads: "requires either a proposed/applied item in {sd}/worklist.json covering this path, or an explicit opt-out phrase", where `{sd}` is filled in with the state folder's name. [`src-tauri/src/guard_policy.rs:72-74`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/guard_policy.rs?plain=1#L72-L74)

## allow or deny

<!-- read: app/__shell/conventions.md#Enforcement -->

Deny is an exit code plus stderr that the model reads. [`src-tauri/src/guard_policy.rs:8-12`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/guard_policy.rs?plain=1#L8-L12) A tool allow "is not user consent. Hook denials and reverts are the convention enforcing itself". [`app/__shell/conventions.md:511-513`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/app/__shell/conventions.md?plain=1#L511-L513)

## boundary trees

<!-- read: docs/git-as-infrastructure.md#Claim intervals: attribution as boundary trees -->

"A claim transition captures the whole visible project state before the incoming claimant can change it". [`docs/git-as-infrastructure.md:99-101`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/git-as-infrastructure.md?plain=1#L99-L101) `capture_claim_tree` seeds a temporary index from `HEAD`, runs a path-scoped `git add -A`, and writes a tree, leaving the real index alone; the tree is kept alive by `refs/bram/claims/<atMs>`. [`docs/git-as-infrastructure.md:116-125`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/git-as-infrastructure.md?plain=1#L116-L125) [`src-tauri/src/lib.rs:62188-62196`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L62188-L62196) Capture "fails open: loss of attribution must not block the lifecycle operation". [`docs/git-as-infrastructure.md:171-173`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/git-as-infrastructure.md?plain=1#L171-L173)

## agent calls advance

<!-- read: app/__shell/conventions.md#Lifecycle: propose → triage → mechanical transitions -->

This step is the agent's to remember. After applying, the agent is told to call the mutate route with `op: advance`, and never to rewrite the applied status directly. [`app/__shell/conventions.md:279-283`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/app/__shell/conventions.md?plain=1#L279-L283) `validate_worklist_mutate_authorization` checks the stored record when the call arrives. [`src-tauri/src/lib.rs:85899-85907`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L85899-L85907) If the agent never advances, a begun `proposed` item still gets a Commit button, so the flow doesn't deadlock; Start & commit skips the advance by design. [`app/__shell/conventions.md:107-111`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/app/__shell/conventions.md?plain=1#L107-L111) [`app/__shell/conventions.md:382-387`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/app/__shell/conventions.md?plain=1#L382-L387)

Claude, OpenCode and Antigravity make this call with loopback curl: "Claude, OpenCode and Antigravity: loopback curl." [`app/__shell/conventions.md:416-416`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/app/__shell/conventions.md?plain=1#L416-L416) Codex writes intent and result files, and both reach the same handlers. [`app/__shell/reference/worklist-mechanics.md:268-275`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/app/__shell/reference/worklist-mechanics.md?plain=1#L268-L275)

![The subagent hands back; the agent checks its work, advances the item and says what it verified and what you need to do to try it.](screenshots/worklist-04d-ready.png "Agent announces v1 ready to test")

## derived from

<!-- read: app/__shell/conventions.md#Schema and draft layout -->

Before the item is committable, the issue link is "an *association* (the pane renders \"for #N\")"; "closes #N" and the gate's ticks appear "only once committable". [`app/__shell/conventions.md:236-238`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/app/__shell/conventions.md?plain=1#L236-L238) The agent must "re-verify the claim against what the work became" before asking for commit approval. [`app/__shell/conventions.md:238-240`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/app/__shell/conventions.md?plain=1#L238-L240)

![+ New item starts from an issue: here #466, with a note on why it's needed.](screenshots/worklist-01-new-item.png "New item from an issue")

## defined directly

<!-- read: app/__shell/conventions.md#Authoring conventions -->

Exploratory, cross-cutting or multi-issue items get "a bare descriptive slug". [`app/__shell/conventions.md:473-475`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/app/__shell/conventions.md?plain=1#L473-L475)

## feedback revises the draft

<!-- read: app/__shell/conventions.md#Lifecycle: propose → triage → mechanical transitions -->

The first loop. "Item feedback (`iterate:`) authorizes no state change." [`app/__shell/conventions.md:328-328`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/app/__shell/conventions.md?plain=1#L328-L328) On a "`proposed`, not yet begun" item the agent will "revise the draft's `before` / `after`; update `files` only if scope shifts. No project file edits." [`app/__shell/conventions.md:331-333`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/app/__shell/conventions.md?plain=1#L331-L333)

![With the item ticked, the message box sends to it: the bar reads "Selected in Worklist", and Send as chat instead is the way out.](screenshots/worklist-03a-feedback.png "Feedback on the proposal")

![The agent revised only the draft: the After now describes a narrower v1. No project files changed.](screenshots/worklist-03b-revised.png "The revised After")

![The loop runs as many times as it takes: a second round of feedback narrowed v1 to Claude only. Still only the draft changed.](screenshots/worklist-03c-revised-again.png "Revised again")

## you try it

<!-- read: app/__shell/conventions.md#Schema and draft layout -->

The changes are on disk and nothing is committed. See Applied item: a begun proposed item is committable too, so testing can start before the agent advances.

## feedback edits the files

<!-- read: app/__shell/conventions.md#Lifecycle: propose → triage → mechanical transitions -->

On a begun or applied item, feedback edits files on disk. "Either way the item keeps its status; iterate never advances or commits on its own." [`app/__shell/conventions.md:336-337`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/app/__shell/conventions.md?plain=1#L336-L337)

![After testing, feedback goes to the begun item: the strip was too heavy, so ask for a light indicator in the header. Commit is already lit, but the loop continues.](screenshots/worklist-06a-feedback.png "First round of feedback")

![The agent edited the files: the header now says Enforcement paused. Testing it prompts round two: use the standard pause icon instead.](screenshots/worklist-06b-round2.png "Testing round 1 and sending round 2")

![Round two in place: the pause icon before the provider selector, with a tooltip and a link to the setting.](screenshots/worklist-06c-tested.png "Testing round 2")

![Round three is a question, asked as chat with nothing selected, while enforcement is paused (the pause icon in the header). The item's files are all on the board, ready to commit.](screenshots/worklist-06d-round3.png "Round 3 feedback")

![The agent answers: selection still decides whether a message is feedback on an item or chat, but while paused, chat can edit too. It offers three options and leans to the smallest.](screenshots/worklist-06e-round3-response.png "Round 3 response")

## Commit click

<!-- read: app/__shell/conventions.md#Commit and apply-and-commit gates -->
<!-- read: docs/worklist-gate-design.md#The selection gate -->

The Commit button sends `approved:` with `gate: "to commit"` (or `"apply-and-commit"` on a begun proposed item), and the agent calls `worklist-commit { ids, message }`. [`app/__shell/conventions.md:305-307`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/app/__shell/conventions.md?plain=1#L305-L307) The authorization doc: "Start & commit uses one approval and skips the advance." [`docs/authorization.md:131-133`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/authorization.md?plain=1#L131-L133)

![The row reads "closes #466". At Commit, the close checkbox is ticked and a comment for the issue rides along with the approval.](screenshots/worklist-07a-commit.png "Click Commit and send the issue-close message")

## checked by mutate and commit

<!-- read: docs/authorization.md#Action matrix -->
<!-- read: docs/authorization.md#Resolve, consumption, expiry, and interruption -->

`ensure_worklist_commit_authorized` requires kind `approved`, matching ids and publish freshness. [`src-tauri/src/lib.rs:85981-85993`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L85981-L85993) The doc's matrix says the route checks "work, paths, membership and commit-message restrictions". [`docs/authorization.md:91-91`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/authorization.md?plain=1#L91-L91) The checks are coarser than the policy: "`ensure_worklist_commit_authorized` checks kind, ids and freshness, not a separate commit-only capability." [`docs/authorization.md:272-273`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/authorization.md?plain=1#L272-L273) A resolve read stamps `consumedAtMs`, but the mutate and commit validators "deliberately do not treat `consumedAtMs` as a blanket denial". [`docs/authorization.md:161-163`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/authorization.md?plain=1#L161-L163)

## stages and commits

<!-- read: app/__shell/reference/worklist-mechanics.md#Interval staging and entangled commits -->

See Git commit. Entangled means another begun item "**owns lines**" in a file the commit takes; entangled requests "stage only their own hunks", and "Committing them **together** is safe; **one of N** is risky." [`app/__shell/conventions.md:397-403`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/app/__shell/conventions.md?plain=1#L397-L403) Line ownership is computed from the boundary trees. [`docs/git-as-infrastructure.md:133-148`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/git-as-infrastructure.md?plain=1#L133-L148)

## prune and snapshot

<!-- read: docs/worklist-history.md#Flow -->

See Pruned, kept in history. The prune body is built in the commit route and handed to `handle_worklist_mutate`, which serializes concurrent mutates behind a lock. [`src-tauri/src/lib.rs:89631-89637`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L89631-L89637) [`src-tauri/src/lib.rs:87642-87649`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L87642-L87649) Queued issue closes are bound to the new SHA at commit time and fire on the user's next Push. [`src-tauri/src/lib.rs:89467-89467`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L89467-L89467) [`app/__shell/conventions.md:391-395`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/app/__shell/conventions.md?plain=1#L391-L395)

![The commit is tagged with its item, and the close waits: "Push will close #466", pending until you push, with Withdraw if you change your mind.](screenshots/worklist-07b-close-on-push.png "Will close on push")

## Drop

<!-- read: app/__shell/conventions.md#What worklist items represent (and when to drop) -->

A Drop with feedback goes to the agent, which calls resolve then prune; a Drop without feedback completes in the host. [`app/__shell/conventions.md:308-308`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/app/__shell/conventions.md?plain=1#L308-L308) `handle_worklist_drop_direct` has the comment "Record the drop authorization (the pane click IS the authorization)". [`src-tauri/src/lib.rs:86988-86989`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/src-tauri/src/lib.rs?plain=1#L86988-L86989) The repo flags the exposure: the feedback-less route "creates its own grant without establishing that a pane click happened." [`docs/authorization.md:236-241`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/authorization.md?plain=1#L236-L241) Drop "Removes the row, not the edits." [`docs/authorization.md:92-92`](https://github.com/judell/bram/blob/e8a38ac08d3ac3a6323b712bb00ebf23aa4a0f05/docs/authorization.md?plain=1#L92-L92)
