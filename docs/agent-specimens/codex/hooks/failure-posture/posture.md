# Codex guard failure posture (2026-10-31)

CLI: codex-cli 0.166.0, macOS, `gpt-6-astra` at low effort (`codex exec`, one tiny prompt per case).

**Answer: fail open on everything except an explicit deny (JSON) or exit 2.** Exit 1, exit 3, a
hang past `timeout`, empty output, `{}`, invalid JSON and the older-guard shape all run the
action; the model is told nothing. Surprise: an explicit **allow** JSON is itself rejected
(`hook: PreToolUse Failed`) yet the call runs, so "allow" is spelled by printing nothing.

| Case (hook behaviour) | apply_patch ran? | Bash ran? | Host status line / what the model saw |
|---|---|---|---|
| control, no hook | yes | n/a | normal |
| allow (`permissionDecision:"allow"`) | yes | yes | `PreToolUse Failed` (allow unsupported); model saw nothing |
| deny + reason (JSON, exit 0) | **no** | **no** | `Command blocked by PreToolUse hook: <reason>. Command: <patch or cmd>` |
| exit 1, stderr | yes | yes | `PreToolUse Failed`; nothing to the model |
| exit 3, stderr | yes | yes | `PreToolUse Failed`; nothing |
| invalid JSON on stdout, exit 0 | yes | yes | `PreToolUse Completed`; nothing |
| hang (30 s sleep, `timeout = 5`) | yes (~15 s run) | yes (~19 s) | `PreToolUse Failed`; nothing |
| exit 2, stderr | **no** | **no** | `Command blocked by PreToolUse hook: <stderr>. Command: ...` |
| exit 0, empty stdout | yes | yes | `Completed`; nothing |
| exit 0, `{}` | yes | yes | `Completed`; nothing |
| older-guard shape (stderr msg, exit 0, empty) | yes | yes | `Completed`; nothing |

Rows are in `summary.log` (`c-*` = file write via `apply_patch`, `s-*` = Bash). The Bash rows match
except that no unhooked shell control was run (the write control is `c0-nohook`).

Fail-closed option: none found in `codex exec --help` or the hook config used here (not exhaustively
searched in docs). Same advice as Claude: the guard must exit 2 or print a deny itself.

## Method

- `run.sh NAME MODE TOOL [timeout]`: fresh throwaway repo and a fresh **isolated `CODEX_HOME`** per
  case containing only a copy of `auth.json` and a minimal `config.toml` (model, folder trust, one
  `[[hooks.PreToolUse]]` with matcher `^(apply_patch|Bash)$` for the case). The home is deleted after
  each case. `~/.codex/config.toml` (which has Bram's global hooks) was neither read by the CLI nor edited.
- `codex exec --dangerously-bypass-hook-trust -s workspace-write`: bypasses the hook trust prompt for
  the isolated home; `exec` never asks for approval, so Codex's own permission system did not decide.
- `OPENAI_API_KEY` unset for the run; credentials not printed or saved.
- Proof the hook fired: `hook-fired.log` (`ran`/`APPLY`) plus the CLI's `hook: PreToolUse` line.
- Paths redacted to `<scratch>`.

## Limits

- One run per cell; hook entry `timeout = 5` for the hang row; Codex's default timeout not measured.
- Only `PreToolUse`; `PermissionRequest` not exercised.
- `exec` mode only, not the TUI. `codex` printed a "Reading additional input from stdin" note; stdin was `/dev/null`.
