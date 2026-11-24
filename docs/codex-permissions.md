# Codex approval prompts — quick string list

**What this is:** the human-visible strings in OpenAI Codex CLI's approval
prompts. Fast orientation, like CodeExam's "LLM Prompts" list.

**Read first:**
- `<angle-bracket>` = runtime-filled value (command prefix, host, path, keybinding).
  Unlike Claude Code, **most Codex option labels ARE literal constants** — only
  the `<…>` slots are templated, so these strings are largely greppable as-is.
- For detection, the durable anchor is the **enums**, not the labels — see
  `Codex_approval_prompt_detection_results.md`. This list is "what the user sees."
- From `.codex_from_gh_zip` (= `openai/codex` `codex-rs` source). Re-pin per commit.

---

## Headers / framing (all prompts)
```
Thread: <label>
Reason: <reason>
Permission rule: <rule>
$ <command>                         (exec prompts; bash-highlighted)
<key> to confirm   ·   <key> to cancel   ·   <key> to open thread
```

## Exec / command approval — the numbered choices
```
Yes, proceed
Yes, just this once                                              (network request)
Yes, and don't ask again for commands that start with `<prefix>`
Yes, and don't ask again for this command in this session
Yes, and allow this host for this conversation                  (network)
Yes, and allow these permissions for this session               (with extra perms)
Yes, and allow this host in the future                          (network policy)
No, and block this host in the future                           (network policy)
No, continue without running it
No, and tell Codex what to do differently
```

## Apply-patch / file change approval
```
Yes, proceed
Yes, and don't ask again for these files
No, and tell Codex what to do differently
```

## Permissions (request_permissions tool)
```
Yes, grant these permissions for this turn
Yes, grant for this turn with strict auto review
Yes, grant these permissions for this session
No, continue without permissions
```
Post-decision transcript lines:
```
You granted additional permissions
You granted additional permissions for this session
You granted additional permissions with strict auto review
You did not grant additional permissions
```

## MCP elicitation
```
Yes, provide the requested info
No, but continue without it
Cancel this request                 (Esc is always Cancel)
```
(server-supplied <message> shown above the options)

## MCP tool approval (codex-cli 0.172.0; an elicitation form, not the exec prompt)
```
Field 1/1
Allow the <server> MCP server to run tool "<tool>"?      (or the generic: Allow this request?)
1. Allow                   Run the tool and continue
2. Allow for this session  Run the tool and remember this choice for this session   (only when the server allows persisting)
3. Always allow            Run the tool and remember this choice for future tool calls   (same)
4. Cancel                  Cancel this tool call                                   (last row, whatever the count)
enter to submit | esc to cancel
```
Keys: a **digit selects its row and submits in one stroke**; Enter submits
the highlighted row; **Esc cancels**; `y` is not a key. Bram answers Allow
with `1` and Cancel with Esc and offers no session/always rows, since they
move Cancel's number (`codex_permission_request_to_menu`, the `mcp__` arm).
Source: `codex-rs/tui/src/bottom_pane/mcp_server_elicitation.rs` at
`rust-v0.172.0`; specimen `docs/agent-specimens/codex/turn-with-mcp-tool-approval.txt`.

---

## The decisions behind the labels (typed enums — the real spine)
```
ReviewDecision:           Approved · ApprovedExecpolicyAmendment · ApprovedForSession ·
                          NetworkPolicyAmendment · Denied · TimedOut · Abort
CommandExecutionApprovalDecision: Accept · AcceptForSession · AcceptWithExecpolicyAmendment ·
                          ApplyNetworkPolicyAmendment · Decline · Cancel
FileChangeApprovalDecision:       Accept · AcceptForSession · Decline · Cancel
PermissionsDecision:      GrantForTurn · GrantForTurnWithStrictAutoReview · GrantForSession · Deny
Wire response tokens:     approve · deny · abort
```

## Modes & sandbox (the policy context — not prompt options)
```
Approval policy (AskForApproval): untrusted · on-failure · on-request · granular · never
Sandbox (SandboxPolicy):          read-only · workspace-write · external-sandbox · danger-full-access
```

> Note the lineage: Codex's "No, and tell **Codex** what to do differently"
> mirrors Claude Code's "No, and tell **Claude** what to do differently" — same
> UX pattern, different agent.