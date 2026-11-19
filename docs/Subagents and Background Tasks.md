# Subagents and Background Tasks

Both mean "the work on this item continues outside the current turn", but
they differ in what Bram can see and what they can do:

| | subagent | background task |
|---|---|---|
| what it is | another agent, launched by the main one | a process the main agent started: a shell command, a watch, a test run |
| edits files? | often, so its lines need crediting to an item | rarely; it produces results the agent writes up afterwards |
| can Bram see it? | yes: hooks report its start and end, and Claude Code counts it (`pendingBackgroundAgentCount`) | no: Claude Code doesn't count it, and only the agent knows it's running and why |
| how it's tied to an item | inferred: bound to the whole live claim, which is #478's flaw | declared: the agent names the item ids (`/__worklist/wait`) |
| how it ends | the subagent stops, and Bram sees it | a notification wakes the main agent, which comes back to the item |
