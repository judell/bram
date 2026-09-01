# Trace gestures

A scripted UI gesture is a sequence of clicks and keystrokes a human
operator performs in Bram, paired with a precondition setup and an
expected outcome. Each gesture has a stable name and a documented
recipe. `scripts/record-trace.sh <gesture-name>` consults this file to
remind the operator of the steps before capturing.

The point of fixing the recipes here is reproducibility: the same
person running the same gesture against unchanged code should produce
byte-identical normalized output via `scripts/normalize-trace.py`.

Each gesture entry below uses the header form `## \`gesture-name\``.
The recorder grep matches that exact form, so don't drop the
backticks.

---

## `worklist-approve-top`

**Precondition.** The worklist has exactly one item, in `TO APPLY` (proposed) status, with `id=harness-target`. Use the Worklist tab's `+ New item` button to set this up if needed, or write `resources/worklist.json` directly outside the recorder window.

**Steps.**

1. In the Worklist tab, click the radio dot next to `harness-target` to select it.
2. Click the **Approve** button (no feedback typed).
3. Wait until the item appears as `TO COMMIT` (status applied) and the inflight spinner clears.

**Expected outcome.** Item moves from TO APPLY to TO COMMIT. No feedback. No close-issue dialog (the item must have empty `closesIssues`).

---

## `worklist-approve-with-feedback`

**Precondition.** Same as `worklist-approve-top`.

**Steps.**

1. Select `harness-target`.
2. Type the literal string `gesture-feedback-line` into the feedback box.
3. Click **Approve**.
4. Wait until the spinner clears.

**Expected outcome.** Item moves to TO COMMIT, the feedback line appears in the conversation.

---

## `worklist-drop-top`

**Precondition.** Same as `worklist-approve-top`.

**Steps.**

1. Select `harness-target`.
2. Click the **Drop** button (no feedback typed).
3. Wait until the item disappears from the Worklist tab.

**Expected outcome.** Worklist becomes empty. Spinner clears.

---

## `worklist-iterate-with-feedback`

**Precondition.** Same as `worklist-approve-top`.

**Steps.**

1. Select `harness-target`.
2. Type the literal string `gesture-iterate-feedback` into the feedback box.
3. Click **Refine**.
4. Wait until the spinner clears.

**Expected outcome.** Item remains TO APPLY. Feedback is delivered to the agent.

---

## `bash-menu-shown-and-dismissed`

**Precondition.** Bram is running with an agent session that will surface a Bash permission menu on the next command. No active permission menu, no active inflight sentinel.

**Steps.**

1. Have the agent run a one-shot Bash command on an unfamiliar path that will trigger a permission menu (e.g., `awk '/test/' /Users/<name>/some-path-not-yet-allowed`).
2. Wait until the menu renders in the AgentMenu surface in the agent pane.
3. Press `1` in the terminal to dismiss (deny / answer No is acceptable; what matters is dismissal via a keystroke).

**Expected outcome.** AgentMenu disappears from the agent pane. Terminal returns to the agent's spinner / next-action state.

---

## `bash-menu-shown-and-allowed`

**Precondition.** Same as `bash-menu-shown-and-dismissed`.

**Steps.**

1. Have the agent run a one-shot Bash command on an unfamiliar path that will trigger a permission menu.
2. Wait until the menu renders.
3. Press `2` in the terminal to choose "Yes, allow and don't ask again for similar commands" (session-allow rule will land).

**Expected outcome.** AgentMenu disappears. The session-allow rule is now in effect for similar commands.

---

## Capture modes

The recorder has two modes that differ in what they preserve.

**Structural (default).** `scripts/record-trace.sh <gesture-name>`. Strips all per-run noise: timestamps, correlation IDs, `at_host_ms`, `last_host_ms`, `delta_to_emit_ms`, `elapsed_ms`, pids, nonces. Output is deterministic for any given input. Use for behavioral-equivalence checks across a refactor — record before, change, re-record after, diff with `scripts/diff-trace.sh`. Exit 0 from the diff means the refactor preserved behavior.

**Timing-preserved.** `scripts/record-trace.sh --with-timing <gesture-name>`. Captures both the structural snapshot (same as default) and a second `<gesture>__<unix-ms>__timing.jsonl` that preserves leading ISO8601 timestamps and the timing fields. Use for lag / perf investigations — record N runs of the same gesture, compute median / p95 of `delta_to_emit_ms` (or any other timing field) before and after a perf-targeted change. Output is no longer deterministic between runs; that's the point — you can subtract timestamps to compute per-event latency.

The timing snapshot is not for `scripts/diff-trace.sh`; the diff would always report changes because timestamps drift run-to-run. The structural snapshot remains the equivalence-check baseline.

## Adding a new gesture

1. Pick a kebab-case name that names the *user-observable outcome* of the gesture, not its mechanics. `worklist-drop-top`, not `click-drop-button`.
2. Add a `## \`<name>\`` heading and the three subsections: Precondition, Steps, Expected outcome.
3. Run `scripts/record-trace.sh <name>` against an unchanged tree, twice, and confirm the two snapshots diff cleanly via `scripts/diff-trace.sh`. If they don't, the gesture is not yet reproducible — investigate before committing.
