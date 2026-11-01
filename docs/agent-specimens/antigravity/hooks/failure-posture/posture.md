# Antigravity guard failure posture (2026-10-31)

CLI: `agy` 1.4.12, macOS, one-shot print mode, a project `.agents/hooks.json` with a `PreToolUse` hook on `write_file` and `timeout: 3`.
Question: when the `PreToolUse` guard itself breaks, does the action still run?

**Answer: fail closed on everything except exit 0 with empty output.** A crash (exit 1, exit 3), exit 2, invalid JSON and a hang past the hook `timeout` all block the call. A hook that exits 0 and prints nothing lets it run, and so does an explicit allow.

| Case (hook behaviour) | Write ran? | What the model saw |
|---|---|---|
| `{"decision":"allow"}` | yes | normal tool result |
| `{"decision":"deny","reason":…}` | **no** | the reason |
| exit 1, or exit 3 with stderr | **no** | the exit status and stderr |
| invalid JSON on stdout | **no** | |
| hang past `timeout` | **no** (the hook is killed) | |
| exit 2 | **no** | |
| **exit 0, empty stdout** | **yes** | nothing |

Two related findings from the same spike:

- **`allow` doesn't skip agy's own permission check.** A `run_command` hook answering `allow` still left the command waiting on agy's "command" permission. Bram's guard answers `allow`, never `auto_approve`.
- **The older-guard shape is the fail-open row.** A guard that predates Antigravity answers an unknown hook with a message on stderr, exit 0 and no stdout. Bram 0.8.1's `bram-guard` does exactly that.

## What Bram does about it

- **Empty output becomes a deny.** On macOS and Linux, Setup registers agy's hook through a shell wrapper, `antigravity_guard_command` (`src-tauri/src/lib.rs`). It captures the guard's output and prints a deny decision, naming the older-Bram cause, when the output is empty. By the same path, a dangling `~/.bram/bram-guard` gets that deny too: the exec fails, so nothing reaches stdout. That follows from the code; it hasn't been run against a dangling link. On Windows the hook is registered bare, and agy blocks a failed exec through the exit-status rows above.
- **Forwarding across builds.** `~/.bram/bram-guard` is one link per machine, and every running Bram re-points it about once a minute. In the live checks of 2026-10-31, an installed 0.8.1 and a demo on this branch's build held it in turn. agy's real hooks still reached the right guard, because 0.8.1 forwards to the project's recorded guard (`resources/.bram-guard-target`) before it looks at the hook name. The guard self-test calls the link directly with a synthetic project, so it reads the Antigravity rows red while 0.8.1 holds the link.

## Method and limits

- Each case swapped the hook command in a scratch workspace's `.agents/hooks.json` and asked agy, in print mode, to create a file.
- **The spike's scripts were scratch and weren't kept.** This page records the results as reported on judell/bram#462 ([comment 6548388674](https://github.com/judell/bram/issues/462#issuecomment-6548388674)). Re-running needs a small `run.sh`/`hook.sh` pair like the other packs'.
- **Limits:** one version, print mode only, `write_file` only (`edit_file` and `run_command` weren't run through the failure cases), and `{}` wasn't tried.
