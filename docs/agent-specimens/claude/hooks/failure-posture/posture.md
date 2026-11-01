# Claude Code guard failure posture (2026-10-31)

CLI: Claude Code 2.1.316, macOS, model `haiku` (`-p`, one tiny prompt per case).
Question: when the `PreToolUse` guard itself breaks, does the action still run?

**Answer: fail open on everything except an explicit deny (JSON) or exit 2.** Crash (exit 1, exit 3),
invalid JSON, a hang past the hook `timeout`, empty output, `{}` and the older-guard shape
(stderr message, exit 0, no stdout) all let the call run, and the model is told nothing.

| Case (hook behaviour) | Write ran? | Bash ran? | What the model saw |
|---|---|---|---|
| control, no hook | yes | n/a | normal tool result |
| allow (`permissionDecision:"allow"`) | yes | yes | normal result |
| deny + reason (JSON, exit 0) | **no** | **no** | `PreToolUse:Write hook error: <reason>` (Bash likewise) |
| exit 1, stderr | yes | yes | nothing (stderr not shown) |
| exit 3, stderr | yes | yes | nothing |
| invalid JSON on stdout, exit 0 | yes | yes | nothing |
| hang (30 s sleep, `timeout: 5`) | yes (run took ~12 s) | yes (~9 s) | nothing |
| exit 2, stderr | **no** | **no** | `PreToolUse:Write hook error: [<hook command>]: <stderr>` |
| exit 0, empty stdout | yes | yes | nothing |
| exit 0, `{}` | yes | yes | nothing |
| older-guard shape (stderr msg, exit 0, empty) | yes | yes | nothing |

The shell command run (`echo SHELLMARK > file`) covers the same failure cases; the raw rows are
in `summary.log` (`c-*` = file write, `s-*` = Bash). The shell allow/deny/exit1/exit3/badjson/hang/
exit2/empty/`{}`/oldguard rows match the write rows. The Bash "control" is the write control plus
the `s-allow` row (no separate unhooked shell run).

Fail-closed option: none found. No hook setting turns a crash, timeout or empty output into a
block; exit 2 is the only non-JSON way to block. A guard must therefore catch its own failures
and exit 2 (or print a deny) itself.

## Method

- `run.sh NAME MODE TOOL [timeout]`: fresh throwaway git repo per case; project
  `.claude/settings.json` registers `hook.sh <mode> <tool>` on `PreToolUse` with a matcher for the
  target tool. `claude -p --model haiku --setting-sources project,local --permission-mode bypassPermissions`.
- **Isolation:** `--setting-sources project,local` leaves out user settings, which carry real
  hooks, while keeping the login. Nothing under `~/.claude` was edited.
- **Permissions:** `bypassPermissions` so Claude's own permission system never decides the
  result; PreToolUse hooks still run in that mode.
- **Proof the hook fired:** `hook-fired.log` has a `ran` and an `APPLY` line per invocation.
  The file existence check in `summary.log` records whether the action happened.
- Deny output: `{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":...}}`.
- Paths in the logs are redacted to `<scratch>`.

## Limits

- One run per cell; a model run is nondeterministic, though every allow/deny result matched the mechanism.
- Only `PreToolUse` was exercised, not `PermissionRequest`.
- The hang case used `timeout: 5` in the hook entry; Claude's default timeout was not measured.
- Run in `-p` mode only; interactive behaviour not captured.
- `ANTHROPIC_API_KEY` was set in the environment, so the run used API auth, not the subscription login.
