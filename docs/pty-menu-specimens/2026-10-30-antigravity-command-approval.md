---
observed: 2026-10-31
provider: antigravity
cli_version: 1.4.12
shape: antigravity-permission
source: terminal-capture
raw: ../agent-specimens/antigravity/command-approval.raw
---

Command approval prompt from Antigravity CLI, captured at 120×40 in a
throwaway workspace. A `PreToolUse` hook returned `force_ask` with the reason
“Specimen command requires approval.” The prompt shows the requested `echo`
command and four numbered choices: run once, allow for matching commands in
this conversation, persist that allow rule to `settings.json`, or cancel.

The canceled case is captured separately in
[`../agent-specimens/antigravity/permission-declined.txt`](../agent-specimens/antigravity/permission-declined.txt).
Escape interrupted the turn; selecting option 4 produced the explicit
“User declined the tool call” result. The approval-screen family is classified
as `antigravity-permission`.

The screen is now rendered in
[`../agent-specimens/antigravity/command-approval.txt`](../agent-specimens/antigravity/command-approval.txt).
