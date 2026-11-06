# The Transcript session-menu experiment is dropped; switching stays on the Sessions tab for now

Status: decided 2026-11-06 · revisit when "where does my next message go" has a settled design

**Question:** whether to keep, merge or drop `sessions-menu-experiment`, a local branch that moved session switching, and then the whole Sessions tab, into a menu on the Transcript's session line.

**Decision:** drop it, branch included. Session switching stays on the Sessions tab until one design fits the three things that decide where a message goes: the agent, the session, and what the message is about (Worklist ticks). Jon, 2026-11-06: "this is getting too complicated", then "We want all this stuff, but it's got to fit together," and "You need to be able to see all your worklist items all the time."

**Evidence:** the two commits, which stay findable by SHA in the reflog (about 90 days) after the branch is deleted:

- `7e3ed08bf9ede2b8297123c95ea44687fb4bcc5e`, "Switch sessions from the Transcript, and see that you have" (2026-11-05). It answered Peggy's two findings: the switcher was an unlabeled icon on a Sessions row, and a switch landed with nothing saying you had moved. The Transcript's session line became a menu of the project's sessions; a "Switched to X (from Y)" banner and a "you arrived here" divider marked the arrival; the Sessions tab lost its switch button; a `session-switch` trace line let the Log tell it.
- `9d4b749257296170a46b7ca04b1d7b6143cb94eb`, "Experiment: the Transcript's session menu does what the Sessions tab did" (2026-11-05). Every session became a submenu (Switch or Reload, Rename, Delete, Add to group or Leave group), the menu ended with New session, and the plan was to retire the Sessions tab. Its own open questions: groups (one session per agent made Add to group look arbitrary), and whether the tab or the menu is the better home for rename, delete and groups.

`main` had been rewound to `4ae71aeb` with the branch kept. The branch was never pushed and had no PR.

**Alternatives:**

- Keep the branch parked. Rejected: the work is better carried by this note than by a stale branch, and the branch was already misleading the Push button, whose tooltip said `main`'s commits reach `main` "through its PR" (there was none) and not to push `main`.
- Merge `7e3ed08b` alone, since Peggy's two findings are real. Rejected for now: its menu and banner are part of the same question the design has to answer, and landing them first would set that answer by default.

**Revisit when:** a "where does my next message go" design is settled. `7e3ed08b`'s banner and divider are the first candidates to bring back, since they answer "did I move?" whatever the switcher looks like.
