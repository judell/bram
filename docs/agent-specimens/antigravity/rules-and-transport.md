# How agy 1.4.12 loads instructions, and whether it reaches loopback

Captured 2026-10-31 for `antigravity-signature-and-conventions` (#462), with
`agy` 1.4.12 on macOS, signed in, Gemini 3.8 Flash. Every run was one-shot
print mode in a throwaway git repo:

```sh
agy -p '<prompt>' --output-format json \
  --settings '{"trustedWorkspaces":["<scratch>"]}' --log-file <scratch>/agy.log
```

The settings are session-only and never saved. No run used
`--dangerously-skip-permissions`, and nothing under `~/.gemini` was changed.
`capture/rule-probes.py` lays out each probe's files. Below, conversation ids,
the scratch path and the server port are redacted, and each response is
quoted from the JSON's `response` field.

## 1. Which files load

Layout: `AGENTS.md`, `GEMINI.md`, `.agents/rules/test.md` with `trigger:
always_on` frontmatter, and `.agents/rules/bare.md` with no frontmatter, each
holding a different made-up secret. Prompt: "Do not use any tools. List every
SECRET_* name and value that appears anywhere in your instructions or
context, one per line."

```text
SECRET_AGENTS_MD = "alpha-quokka"
SECRET_GEMINI_MD = "bravo-lynx"
SECRET_RULE_FRONTMATTER = "charlie-otter"
```

agy's log for the rule with no frontmatter:

```text
rules.go:482] Invalid rule trigger: CORTEX_MEMORY_TRIGGER_UNSPECIFIED
```

So `AGENTS.md` and `GEMINI.md` load always-on. A rule loads only with
`trigger: always_on` frontmatter; without it the rule is rejected. That
matches OpenCode's earlier partial run of this item, which also found
`.agents/AGENTS.md` loaded and nested `sub/AGENTS.md` not loaded in print mode.

## 2. The 24,000-byte truncation

Layout: one always-on rule of 29,528 bytes (28 of frontmatter), with markers
at body bytes 23,800 (A), 23,965–23,990 (B), 24,100 (C) and 29,000 (D).
Prompt: list every `SECRET_MARK_*` name.

```text
SECRET_MARK_A
SECRET_MARK_B
```

```text
rules.go:571] Rule file <scratch>/.agents/rules/big.md truncated by 5511 bytes (original 29501 bytes, limit 24000 bytes)
```

The limit applies to what follows the frontmatter: B ends past byte 24,000 of
the file and survived. Truncation is silent apart from that log warning.
Bram's check is stricter: each whole slice file, frontmatter included, stays
under 24,000 bytes.

## 3. The real channel: three questions only the conventions answer

Layout: the two slices Setup's split produces, `.agents/rules/bram-conventions-1.md`
(22,827 bytes) and `-2.md` (22,813), plus Bram's `AGENTS.md` block (5,635),
from the conventions as they stood before this item's edits. Prompt:

```text
Do not use any tools. Answer from your instructions only, quoting exact wording; write NOT PRESENT if an answer is not there.
1. What exact phrase, ending a user message, opts that turn out of the Worklist?
2. What route advances a worklist item to applied, and with what request body?
3. Write the exact first line of the signature a forge artifact you author must open with, as a template with its placeholders.
Be terse.
```

```text
1. `"just do it"`
2. Route: `POST /__worklist/mutate` (or `worklist-mutate` via filesystem intent)
   Body: `{"op":"advance","ids":[...],"status":"applied"}`
3. `<owner>'s <Agent> (Bram <version>, <thread>, <model>, <os>, <machine>) speaking from the <Project> project (<forge-host>/<owner-or-group>/<repo>):`
```

The filesystem-intent aside comes from the `AGENTS.md` block, which is
written for Codex. A rerun against real Setup output is in section 7.

## 4. The shared always-on budget

Layout: probe 3's set (51,275 bytes), plus one always-on "user rule" of
prose-like filler with a secret at its top. Prompt: YES or NO for each of:
the heading "Coordinate via worklist.json" (slice 1), the heading
"Search-first" (slice 2), the user rule's secret, and the `AGENTS.md`
block's first sentence.

| User rule | Total always-on bytes | Slice 1 | Slice 2 | User rule | AGENTS.md |
|---|---|---|---|---|---|
| `zz-user.md`, 8,000 | 59,275 | YES | YES | YES | YES |
| `aa-user.md`, 10,000 | 61,275 | **NO** | YES | YES | YES |
| `aa-user.md`, 12,000 | 63,275 | **NO** | YES | YES | YES |
| `zz-user.md`, 16,000 | 67,275 | **NO** | YES | YES | YES |
| `aa-user.md`, 16,000 | 67,275 | **NO** | YES | YES | YES |
| `aa-user.md`, 23,500 | 74,775 | YES | YES | **NO** | YES |

- **The budget is about 60,000 bytes** of always-on text. That fits "20,000
  tokens" only as an estimate of about 3 bytes per token: the filler and the
  conventions would be far fewer real tokens. OpenCode's earlier dot-filler
  runs gave the same threshold.
- **agy drops the largest rule first,** whatever its name sorts as. Slice 1
  (22,827 bytes) went before slice 2 (22,813). A user rule bigger than both
  went instead of them. Nothing is logged when a rule is dropped.
- **Headroom:** Bram's set leaves about 8 KB for everything else always-on,
  such as the user's own rules, a `GEMINI.md` and global rules. Past that, a
  Bram slice is usually what goes.

## 5. Transport: can `run_command` reach 127.0.0.1?

A throwaway `python3 -m http.server` on 127.0.0.1 served one file
(`LOOPBACK_OK`) at a neutral path. Prompt: run exactly the given curl, in
Bram's shape (`curl -4 -sS --retry-connrefused --retry 3 --retry-delay 1
"http://127.0.0.1:<port>/ping.txt"`), and reply with its output.

| Run | Session-only `permissions.allow` | Result |
|---|---|---|
| 5a | none | auto-denied: `denied_actions: [{"action":"command"}]` |
| 5b | `command(curl)` | `LOOPBACK_OK`; the server logged the request |
| 5c | `command(curl -4 -sS --retry-connrefused)` | `LOOPBACK_OK` |
| 5d | `command(curl)`, with `curl … && echo CHAINED-7731` | auto-denied |

Print mode's message for 5a and 5d:

```text
jetski: no output produced — a tool required the "command" permission that headless mode cannot prompt for, so it was auto-denied. Add an allow-rule under permissions.allow in settings.json (e.g. command(<target>)). [...]
```

- agy's shell isn't network-sandboxed as Bram launches it (no `--sandbox`),
  so loopback curl works, as it does for Claude and OpenCode. The only gate
  is agy's own "command" permission. An interactive session asks, offering
  persistent and conversation-only allow choices (`permission-prompt.txt`);
  print mode denies.
- A multi-word prefix works as an allow rule, and a prefix grant doesn't
  cover a chained command: agy judges each part separately.
- agy's log shows where grants live: `CLI settings initialized:
  permissions=&{Allow:[command(curl)] …}` and `ApplyProjectPermissionGrants:
  no grants for project "CLI Project"`. Project grants belong to agy's own
  project store, not to a file in the workspace.

## 6. `@` mentions inside a rule

agy's rules docs describe `@`-included files counting toward the 24,000
bytes. Two runs tried five forms in one always-on rule: a relative
`@inc-rel.txt`, an absolute path inside a curl line (`--data @/<scratch>/inc-abs.txt`,
as the conventions' POST example has), one in backticks, a bare line
`@inc-ruledir.txt` naming a file in the rules folder, `@./inc-dotslash.md`,
and `@../../inc-rel.txt`. Each named file held its own secret.

```text
SECRET_RULE_ITSELF = "rule-text"
```

None was expanded, in either run. So in 1.4.12 a rule's size on disk is the
size agy loads, and the conventions' `--data @/tmp/body.json` can't pull a
stale body file into a slice.

## 7. After the change: Setup's own output

`scripts/setup-harness.sh pristine_git`, run with this item's build, seeded
`.agents/rules/bram-conventions-1.md` (22,818 bytes) and `-2.md` (22,787),
the second starting at `### Transports`, beside the `AGENTS.md` block
(5,635). Those three files, copied unchanged into a fresh scratch repo, make
51,240 bytes of always-on text. Probe 3's prompt, with a fourth question
("Which single word do you, the agent running in this session, put in its
<Agent> slot?"):

```text
1. `"just do it"`
2. Route: `POST /__worklist/mutate`
   Request body: `{"op":"advance","ids":[...],"status":"applied"}`
3. `<owner>'s <Agent> (Bram <version>, <thread>, <model>, <os>, <machine>) speaking from the <Project> project (<forge-host>/<owner-or-group>/<repo>):`
4. `Antigravity`
```

Then: "As the agent running in this session, which transport do you use for
Bram lifecycle calls such as advancing an item: loopback curl to 127.0.0.1,
or the filesystem intent file? Name it, and quote the one line of your
instructions that decides it for you."

```text
**Loopback curl to 127.0.0.1**

> "**Claude, OpenCode and Antigravity: loopback curl.**"
```

The `AGENTS.md` block still tells Codex to use the filesystem channel, and
agy loads it always-on as well. Here the conventions' line won. Either
channel reaches the same host handlers.
