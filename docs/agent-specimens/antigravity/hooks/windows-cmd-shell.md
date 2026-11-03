# agy runs hook commands through cmd.exe on Windows

Captured 2026-11-02 on Windows 11, agy 1.4.12, Bram 0.8.2 at `2b31b07b`,
from `~/.gemini/antigravity-cli/log/cli-20261102_165648.log` at the first
user message of a fresh conversation. The seeded `.agents/hooks.json`
carried the quoted form Bram used everywhere else:

```json
"command": "\"C:\\Users\\jon\\.bram\\bram-guard.exe\" guard antigravity-session"
```

agy's log:

```
I1102 16:58:12.884518     894 command_hook_executor.go:75] JSON hook command stderr: '\"C:\Users\jon\.bram\bram-guard.exe\"' is not recognized as an internal or external command,
E1102 16:58:12.885870     894 provider.go:35] pre-invocation hook "jsonhook__bram-worklist-guard_PreInvocation_0_0" failed: JSON hook "jsonhook__bram-worklist-guard_PreInvocation_0_0" failed: command failed: exit status 1, stderr: '\"C:\Users\jon\.bram\bram-guard.exe\"' is not recognized as an i
```

"is not recognized as an internal or external command" is cmd.exe's
wording (PowerShell says "is not recognized as the name of a cmdlet"), so
agy runs hook commands through `cmd /c`. A Go `exec.Command("cmd", "/c",
command)` backslash-escapes every `"` in the argument (`syscall.EscapeArg`),
which is right for programs that parse their command line with
`CommandLineToArgvW` and wrong for cmd.exe, which receives `\"C:\…\"`
verbatim and cannot resolve it.

Reproduced with Python's `subprocess`, whose `list2cmdline` escapes the same
way, feeding `{}` on stdin:

| command form | result |
|---|---|
| `"C:\Users\jon\.bram\bram-guard.exe" guard antigravity-session` | exit 1, the message above |
| `C:\Users\jon\.bram\bram-guard.exe guard antigravity-session` | exit 0 |
| `""C:\…\bram-guard.exe" guard antigravity-session"` | exit 1 |
| `call "C:\…\bram-guard.exe" guard antigravity-session` | exit 1 |

Consequences, per `failure-posture/posture.md`: agy fails closed on a hook
that exits non-zero, so with the quoted form every `write_file`,
`edit_file`, `run_command` and `call_mcp_tool` is blocked, and the
PreInvocation session hook never names the conversation. The PreInvocation
failure did not stop the model call (a quota 429 followed it in the same
log), so chat-only turns still worked, which is how this hid behind the
quota error at first.

What Bram does about it (`antigravity_guard_command`,
`antigravity_session_hook_command` in `src-tauri/src/lib.rs`): on Windows
both commands are seeded without quotes, and a guard path containing a space
is replaced by its 8.3 short name (cmd's `%~s` expansion at seed time). The
Unix form, with its no-decision wrapper, is unchanged.
