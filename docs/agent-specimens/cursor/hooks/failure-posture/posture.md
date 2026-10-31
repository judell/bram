# Cursor guard failure posture (2026-10-31)

CLI: Cursor agent 2026.10.22-27dcf5f, macOS (`cursor-agent -p --force --trust`). First posted on #462;
the raw logs were copied here from a session scratch folder. `fc` rows set `"failClosed": true` on the
hook entry in `.cursor/hooks.json`.

| Case (preToolUse hook) | ran, no fc | ran, failClosed | What the model saw |
|---|---|---|---|
| control, no hook | yes | n/a | normal |
| allow | yes | n/a | normal |
| deny (`permission:"deny"`) | **no** | **no** | `Error: <user_message>` (the `user_message`, not `agent_message`, reached the model in this run) |
| exit 1, stderr | yes | **no** | with fc: `Tool blocked because this hook is configured to fail closed ... failed with exit code 1: <stderr>` |
| exit 3, stderr | yes | **no** | likewise |
| invalid JSON | **no** | **no** | `returned invalid JSON. The command was blocked for safety.` (blocks even without fc) |
| exit 2, stderr | **no** | **no** | the stderr text |
| exit 0, empty stdout | yes | **no** | with fc: `returned no output` |
| exit 0, `{}` | yes | **yes** | nothing: `{}` means allow even under failClosed |
| hang (`timeout: 5`) | yes | **no** | with fc: `timed out after 5000ms` |
| older-guard shape (stderr, exit 0, empty) | yes | **no** | nothing without fc |

Shell rows (`s0`, `s1`, `s2`, `s3`, `o2`) match the write rows. `failClosed: true` is Cursor's
fail-closed option; without it a crash, non-2 exit, timeout or empty output fails open.

Run script `run.sh`, hook `hook.sh`, raw output `logs/`, `summary.log`, `hook-fired.log`. Paths redacted.
Limits: one run per cell; not every case was run for Bash; only `preToolUse`.
