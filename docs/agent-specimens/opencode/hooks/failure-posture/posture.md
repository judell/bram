# OpenCode guard failure posture (2026-10-31)

CLI: opencode v2.1.4, macOS, model `anthropic/claude-haiku-4-5` (`opencode run --standalone`).

OpenCode's guard is a **plugin** (`api.tool.hook("execute.before")`), not a command hook: a thrown
`Error` denies, so exit codes and stdout do not exist. Cases were adapted (mapping below).

**Answer: a throw of anything denies; everything else fails open. A plugin that fails to load or set up
is silently skipped, so no guard runs. There is no host timeout: a hanging hook just blocks the call.**

| Case (adapted) | Maps to | write ran? | shell ran? | What the model saw |
|---|---|---|---|---|
| control, no plugin | control | yes | n/a | normal |
| `return` (allow) | allow | yes | yes | normal |
| `throw new Error(reason)` | deny + reason | **no** | **no** | `Error: <reason>` verbatim as the tool error |
| `throw "string"` | non-Error throw | **no** | **no** | the string, as the tool error |
| `throw {reason}` | non-Error throw | **no** | **no** | `[object Object]` (reason lost, still blocked) |
| `process.exit(1)` in the hook | exit 1 / crash | **no** | **no** | host died: `ECONNRESET`, CLI exit 1 (it kills the private server too, so not a clean guard crash) |
| `return {}` | `{}` | yes | yes | nothing |
| `return {permission:"deny",reason}` | JSON deny shape | yes | yes | nothing (return values are ignored) |
| `return` nothing | empty output | yes | yes | nothing |
| `console.error(msg); return` | older-guard shape | yes | yes | nothing |
| module throws at import (`load-throw`) | plugin load failure | yes | yes | nothing; plugin never reached `setup` |
| `setup()` throws | plugin load failure | yes | yes | nothing; hook never registered |
| syntax error in plugin file | plugin load failure | yes | yes | nothing |
| hang 40 s, then throw | timeout | **no** (denied after ~40 s) | **no** | late `HANG-LATE-DENY`, run took 44 s |
| hang 125 s, then throw | timeout | **no** (denied after ~125 s) | n/a | late deny, run took 128 s |

No host timeout was seen at 40 s or 125 s. Cases with no direct analogue: exit 3, invalid JSON, exit 2
(no process boundary), so they are not run. Shell rows: `s-*` in `summary.log`; hang run on both tools.

Fail-closed option: none needed or found for a thrown guard, but a failed plugin load is not
fail-closed: it silently drops the guard. A guard cannot detect that from inside; only an external check
that the plugin loaded would.

## Method

- `run.sh NAME MODE TOOL`: fresh repo with `.opencode/plugins/guard.js` and `opencode.json`
  `{"permission":{"*":"allow"}}`, so OpenCode's own permission system never decides. Mode and target tool
  come from `GUARD_MODE` / `GUARD_TOOL`; the plugin logs to `hook-fired.log` (`setup`, `ran`, `APPLY`).
- `--standalone` runs a private server; the user's background service and `~/.config/opencode` were not touched.
- Load-failure cases leave no `ran` line in the log (that is the proof the guard was absent).
- Paths redacted to `<scratch>`.

## Limits

- One run per cell. Tool names are `write` and `shell` (2.1.4). `run` mode only, not the TUI.
- `process.exit(1)` takes down the standalone server, so that row shows host death, not a guard crash.
- Hang beyond 125 s not tried.
