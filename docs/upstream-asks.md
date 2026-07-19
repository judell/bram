# Upstream asks: Claude Code hook-lifecycle completeness

Fully-formed proposals for the Claude Code hook system, recorded locally
(deliberately not filed externally). Each is evidence-backed from Bram's
own traces; see `docs/menu-interventions-catalog.md` for the failure
history that motivates them. If any is ever sent upstream by any channel,
note that here.

## 1. `PermissionRequest` should carry `tool_use_id`

**Gap.** The `PreToolUse` and `PostToolUse` hook payloads include
`tool_use_id`, but `PermissionRequest` — the event announcing that a
permission prompt is about to display — does not.

**Consequence.** External tooling that mirrors permission prompts cannot
bind a prompt to its tool call exactly. It must fall back to a
`tool_name` + `tool_input` signature, which is a guess whenever multiple
calls are pending. Under parallel tool calls this produces live
misbindings: in the 2026-07-18 subagent-fanout stress test, a `mkdir`
prompt was enriched with a concurrent agent's `dig` signature — both the
prompt's menu and the just-dismissed record carried the same wrong
binding, which routed a plainly different menu into staleness
suppression (see the catalog, bucket 3, and commit `f5c9342`).

**Proposal.** Add `tool_use_id` to the `PermissionRequest` payload,
matching its Pre/PostToolUse siblings. Additive and backward-compatible;
makes prompt↔call binding exact for any observer and removes the
signature-guess class entirely.

**Evidence.** `resources/bram-traces/` hook-menu ledger entries from
2026-07-18 19:36–19:48 (claims keyed `Bash|` colliding; `op=payload`
bodies showing no `tool_use_id` on PermissionRequest posts while clears
carry real `toolu_…` ids).
