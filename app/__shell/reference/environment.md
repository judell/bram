# Bram reference: environment

Seeded by Bram Setup as `.claude/bram-reference/environment.md`. Read
the relevant section when your project's markup talks to the agent,
binds project SQLite, uses the forge CLI, runs heavy test suites, hits a
Windows block, or coordinates with a session across a project boundary.

## Target app helpers

Bram's own Worklist and Sessions tabs already use these helpers
internally — the worklist Approve/Drop flow works with no extra setup.
You only need them if **your own** project markup wants to talk back to
the agent (custom Approve buttons, in-page forms that submit a fresh
user turn).

Include `<script src="/__shell/helpers.js"></script>` in your project's
`index.html` to expose:

| helper | usage |
|---|---|
| `toShell(text)` | inject text into stdin; user must press Enter |
| `toTurn(text)` | submit text as a complete user turn (auto-Enter) |
| `openExternal(url)` | open URL in the system browser |
| `logToHost(payload)` | log to Bram stderr without bothering you |

Use `toTurn` for one-shot form submissions (Approve, Confirm). Use
`toShell` to inject text the user can edit before sending.

> **Target-pane origin isolation.** The target pane is served at a
> distinct `bramapp://localhost` origin, so `getTauriInvoke()` returns
> `null` there and `toShell` / `toTurn` / `sendKeys` / `openExternal`
> **no-op** inside an embedded target app — the pane is display-only.
> `helpers.js` is still served (so XMLUI apps boot) but its host-driving
> functions are inert; only Bram's own agent pane (Worklist/Sessions),
> which stays same-origin, drives them. If an embedded app needs to talk
> back to the agent, render the control in the agent pane instead. The
> target scheme (`handle_target_scheme` in Bram's `lib.rs`) refuses the dynamic host routes (`__file`,
> `__worklist/*`, `__settings`, …) and serves only project content plus
> the static `__vendor/*` / `__shell/*` namespaces.

Device APIs that Bram's app bundle can't grant — geolocation is the first
one (judell/bram#386) — are made **absent** in the preview pane on
purpose, not left present-but-failing: `navigator.geolocation` doesn't
exist there, so `"geolocation" in navigator` and
`if (navigator.geolocation)` both fail honestly and a well-written app
takes its own no-location path instead of hitting a denial after its
feature detection passes. Test those features in a real browser at the
same URL, not in this pane.

## Live SQL views via `/query`

When a managed project keeps data in SQLite, its target app (or the
agent pane) can bind live views directly instead of consuming derived
files: set `"db": "<relative path>"` in `.bram.json`, and Bram's
loopback serves `POST /query` — the receiving end of XMLUI's
`DataSource dataType="sql"`. Request body is `{ sql, params }`; the
response is a JSON array of column-keyed row objects:

```xml
<DataSource id="txns" url="/query" method="POST" dataType="sql"
  body="{{ sql: 'SELECT * FROM transactions ORDER BY txn_date', params: [] }}" />
```

- **Read-only, enforced at the engine** (read-only open plus `PRAGMA
  query_only`): the UI can never mutate project data through this
  route. The agent writes the database through ordinary gate-governed
  file access (`sqlite3` CLI etc.) — that division of authority is the
  design.
- **Scratch-pad pattern**: the agent can materialize an analysis (a
  normalized import, an aggregation, a cross-source join) as tables in a
  project-local scratch database — e.g. a gitignored
  `resources/scratch.db` pointed at by `db` — and hand the user a live
  table or chart in three lines of markup. Analyses that prove out get
  promoted to project-owned data and, if the app ships, a project-owned
  endpoint.
- **Scope**: only the explicitly designated project file is reachable —
  never Bram's own databases — and the path must resolve inside the
  project root. No `db` configured → the route refuses with guidance.
- **Limitation**: when the project runs its own dev server, relative
  `/query` reaches that server, not Bram; such projects implement their
  own endpoint against the same wire contract (the xmlui test server is
  one existing implementation), and the markup carries over unchanged.

## Updating forge issues via gh / glab

Use the project's forge CLI directly — the Issues tab refetches on the
indexer's `issues-changed` signal (no polling), so updates surface
without a restart. There is no manual refresh; `/__issues?fresh=1`
remains a curl-only diagnostic that live-builds and rewrites the cached
list. The forge is detected from the `origin` remote (`.bram.json`
`"forge"` override for ambiguous self-hosted remotes; `GET /__app-info`
reports the detection). On GitHub projects:

- `gh issue edit <n> --title "…" --body "…"`
- `gh issue comment <n> --body "…"`
- `gh issue close <n>` / `gh issue reopen <n>`

On GitLab projects the parallel `glab` commands apply (`glab issue note
<n> -m "…"` for comments). The worklist contract around issues
(`closesIssues`, `issue-<N>-` ids, automatic close-on-push) is
forge-agnostic and identical on both. Every body you post needs the
signature (see the core conventions); build it in a body file first.

Bram internals, for diagnosing Bram itself — the forge adapter:
https://github.com/judell/bram/blob/main/docs/forge-adapter.md

**When filing or commenting on an issue against Bram itself
(judell/bram), cite the Bram version.** Triage of a version-less report
starts with "which build is this?", which is unanswerable after the
fact. Two cheap lookups: the `<!-- bram vX.Y.Z -->` marker on the first
line of `.claude/bram-conventions.md` (stamped by Setup at seed time), or
the version field of `GET /__app-info` on the loopback port. If neither
is available — an old seed, no running instance — say so in the report
rather than omitting the version silently.

## Resource-heavy test suites

When running a multi-worker browser test suite (Playwright, or anything
else that spawns a browser per worker) from an agent session, cap
parallelism explicitly — e.g. `npx playwright test --workers=2` —
instead of accepting the default worker-per-core. Machine-wide memory
pressure from default parallelism can freeze **every** webview on the
machine, including Bram's panes, for tens of seconds or more.

The worst multiplier is the fail-retry loop: a failing test that retries
with tracing enabled spawns extra browsers *and* records traces.
Iterating on a failing spec is exactly when the cap matters most — and
exactly when an agent is most likely to be re-running the suite over and
over.

## Windows: Smart App Control

Bram ships unsigned binaries, and on some Windows 11 machines Smart App
Control (SAC) blocks them. Most users see no problem; when a block does
hit, this is the advice protocol (user-facing twin: the Bram README's
*Smart App Control* section):

- **Recognize both symptom shapes.** (1) Bram itself refused at launch —
  a Windows Security dialog naming Smart App Control. (2) The reentrant
  `bram-guard` hook binary (`~/.bram`, spawned on every hook event):
  hook invocations failing while the app runs fine — the signature is
  missing `claude-rs` / `codex-rs` breadcrumbs in
  `resources/bram-traces/hook-events.log`, or hook timeouts/errors, with
  no visible launch failure.
- **Verify before advising.** Confirm the block dialog actually names
  Smart App Control — Defender and SmartScreen blocks read differently,
  and their remedies differ.
- **State the trade-off honestly, then let the user decide.** Disabling
  SAC is currently the only workaround short of a signed executable
  (signing is not currently planned). Windows Defender remains fully
  active with SAC off — if the user deems Defender sufficient
  protection, disabling is the supported path. Name the UI path:
  **Windows Security → App & browser control → Smart App Control
  settings**, and point at Microsoft's Smart App Control FAQ
  (https://support.microsoft.com/en-us/windows/smart-app-control-frequently-asked-questions-285ea03d-fa88-4d56-882e-6698afdb7003) —
  including that the switch is effectively one-way on current Windows
  builds (re-enabling has required a Windows reset).
- **Never flip the setting on the user's behalf.** It is a machine-level
  security decision; the agent names the control and the consequences,
  the user clicks.

## Working across project boundaries

Some of the best work happens between two sessions that each hold
evidence the other cannot reach, coordinating through issue threads.
Three shapes recur, and the practices are the same in all three — only
the asymmetry differs:

- **Downstream ↔ upstream.** Your project consumes a library; a bug or
  gap here is a change there.
- **Machine ↔ machine.** The same repo running on two platforms, where a
  failure reproduces on only one of them.
- **Agent ↔ agent.** The same repo, two agents, each able to reach
  things the other can't.

(Pivoting *this* session's attention to another project is covered by
*Cross-project pivots* in the core conventions; this section is for two
sessions that both stay put and coordinate.)

### Name the boundary

Say which side of the boundary you are on, and scope every claim to it.
The signature that carries this is not boundary-scoped — it is required
on every agent-authored forge artifact, in every repo.

### Scope claims to what your side can observe

Say "from this side" and mean it. State what you verified and how; name
what you *cannot* check from here rather than letting silence imply it's
fine. *Local absence is not disproof* (in `diagnostics.md` §Log-first
development) is the special case of this rule for machine-specific
artifacts; this is the general one.

### The thread is the design document

Chat dies with the session; the thread is what the other side reads,
and months later it's the only record of why. So file at mechanism
depth — symptom, the mechanism cited at the *other* side's `file:line`,
blast radius, and the trace or measurement receipts — not "this seems
broken."

One issue, one mechanism. When a second mechanism surfaces mid-thread,
split it into its own issue and say in both places what moved and what
remains.

### Make the ask specific, and report gates by name

Ask for something answerable: a litmus test to run, a build to
validate, a specific gate to clear. When work is gated, enumerate the
gates and report their status per side ("gate 2 is green; from this
side, remaining: ..."), so neither session has to guess what the other
is waiting on.

### Green-light before irreversible or expensive steps

Vendoring a candidate build, merging, restarting something shared —
request the go-ahead across the boundary explicitly, and grant it
explicitly. An assumed green light is how two sessions end up half-way
through incompatible states.

### Reproduce before fixing; correct the record in public

On the other side of a boundary the currency is a reproduction — a
failing test pins the decision so prose doesn't have to. Build it before
proposing a fix, because it frequently contradicts the filing.

When it does, post a **correction comment** carrying the measurements.
Do not quietly edit the original body: the other side may have already
acted on it, and the correction is the most useful thing in the thread.

### Recompute rather than defer

Being upstream, or being the side where the bug reproduces, is not
authority over arithmetic. When the other session's claim conflicts
with yours, re-run the numbers and read the source before conceding —
then concede once, to the evidence, and move on. Deference and digging
in are the same failure.

### Verify the artifact you run, not the source diff

A merged fix, a green CI run, and a source diff are not the build in
your hands. Verify the artifact you actually execute (checksum, marker
string, behavioral probe), and when you report results, label which of
your instruments are authoritative and which are only corroborating.

### The user is the eyes for rendered output

For changes whose deliverable is something a human reads or sees — a
docs page, a pane surface, a rendered table — the person looks and you
show. A passing spec verifies behavior, not communication, and judging
communication is the user's call, not something to decide for them.

Automated rendering can fail where a person's browser works fine — a
headless run can come back blank on a page that renders correctly for
a real user — so a self-check can report a phantom defect that has
nothing to do with the actual change.

Checks that can't be done by eye are still fine: computed widths, DOM
assertions, and existing headless spec suites (xmlui's
`tests-e2e/how-to-examples/*.spec.ts`, for example). They verify
behavior and stay in place; they just don't stand in for the look. A
liveness check before handing over a URL — the server responds, no
console errors — is fine too; that confirms the thing exists, it
doesn't judge how it reads.

Serve the artifact, hand over the exact URL (or say where to click),
say what changed, and wait for the verdict. When the user is away, say
the rendered review is pending on them rather than substituting your
own judgment in the meantime.

When delegating such work, tell the subagent to report where to look,
not to render-check it: "here's the URL, here's what changed" is the
right shape of instruction, not "verify this renders correctly."

### Close every hard stretch with two questions

This is the engine that turns local pain into shared improvement. When
a struggle ends, ask:

- *What documentation would have short-circuited this?* → name the
  question you couldn't answer and where you looked.
- *What feature would have obviated this workaround?* → carry the
  workaround itself as the evidence.

Then file each one on the other side. If that boundary doesn't take
issues from you, write the ask up locally anyway, fully formed and
evidence-backed, so it exists the day a channel opens.

Any workaround you land carries the issue number it's waiting on, and
its retirement is its own worklist item. A workaround with no filed
issue is a decision to keep the pain.

Where the other side publishes a searchable doc corpus, the
documentation half has a stricter form — validate the gap against the
current corpus before filing, since the fastest way to lose standing is
to report a gap that closed last week. See `diagnostics.md`
§Coordinating MCP demand with search.

### Carry gated follow-ups, and don't edit across the boundary

Actions gated on the other side (a merge, a release, a verdict) become
**reminder items**, `reminder-` prefix and all (see *Reminder items* in
the core conventions).

Act only in the repo whose session you're in. The thread is the
transport, not a shortcut for reaching across and editing the other
project directly.
