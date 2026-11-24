# Runbook: the freestyle survey fixture

This runbook is adapted from Elle O'Brien's demo runbook and dataset notes for
the lab "Analyzing Data with Coding Agents" (UMSI AI Clinic, University of
Michigan School of Information):

- [`docs/demo-runbook.md`](https://github.com/elleobrien/analyzing-data-with-coding-agents/blob/main/docs/demo-runbook.md)
- [`docs/demo-dataset.md`](https://github.com/elleobrien/analyzing-data-with-coding-agents/blob/main/docs/demo-dataset.md)
- Both live in
  [github.com/elleobrien/analyzing-data-with-coding-agents](https://github.com/elleobrien/analyzing-data-with-coding-agents),
  licensed [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/). This
  adaptation is shared under the same license; see `ATTRIBUTION.md`.
- The folder the agent opens is
  [github.com/elleobrien/course-survey-demo](https://github.com/elleobrien/course-survey-demo)
  (CC0), vendored under `folder/`.

**Keep this file out of the folder the agent reads.** It holds the answer key.
`demo_instance.py new ... --bare` materializes only `folder/`, so the
fixture carries no answers.

The goal of the demo: show a confident analysis that runs without errors and
gives the wrong answer, then show how to catch it without reading the code.
Bram's additions: watch the same stretch of work from the agent pane while it
happens, and use Bram's history to ask what an earlier session decided.

## What you need

- The fixture, created and checked by the script (default location
  `../bram-freestyle-survey`, no `.git`):

  ```sh
  python3 scripts/demo_instance.py --repo ~/bram-freestyle-survey new survey --from freestyle-survey --bare
  ```

  Reusing a fixture folder name inherits its recorded mode (the folder's
  entry in `~/.bram/folders.json`), its `.bram/` state and Claude's sessions
  for that path. `new` on an existing folder refuses and says what it found;
  `new --replace` deletes the folder and Claude's sessions for it. The mode
  record is yours to clear: remove the folder's entry from `folders.json`
  (the script does not edit it).

- `python3` in that folder with pandas and matplotlib. `launch` checks this
  and refuses with `python3 -m pip install pandas matplotlib` if they are
  missing. Otherwise the agent may spend the demo trying to install packages.
- A coding agent. Her notes assume Claude Code; the prompts work in any agent.
- **Model choice decides whether you get the miss.** A smaller model (Haiku)
  usually reports the naive answer. A larger one (Opus) usually catches the
  repeat submissions by itself. Decide in advance which story you are telling
  and test it once.
- Permission mode set so every action asks first, so the room (and the Log)
  sees what the agent is about to do.
- Reset between practice runs with
  `python3 scripts/demo_instance.py --repo ~/bram-freestyle-survey reset`.
  It deletes anything the agent created (scripts, figures, `CLAUDE.md`) and
  puts `folder/` back byte for byte, keeping Bram's own `.bram/` state.

On first wake Bram asks how you want to work in the folder (the two cards at
the top of the agent pane) and starts the agent without running Setup
(`[auto-setup] op=skip reason=mode-undecided` in the fixture's
`.bram/bram-traces/bram-trace.log`). Choose **Freestyle** for the steps below.
The choice is recorded per folder on this machine; the fixture launcher sets
`BRAM_USER_STATE_DIR` so a demo's choice stays out of your real `~/.bram/`
only when you set it yourself. To see the Workflow card without the
"creates a repo" line, make a second fixture with `--git`, which materializes
the same folder with a `git init` and one commit.

Before 2026-11-22 (Bram 0.8.11) a folder with no `.git` was refused outright
(`op=autostart-held reason=not-git`); that refusal was the baseline the
freestyle work was measured against.

## The four scenarios

Two folder states times two choices (Jon, 2026-11-22). Make one fixture per
scenario, named for what it tests, and click the matching card in each:

```sh
D="python3 scripts/demo_instance.py"
$D --repo ~/bram-workflow-git     new workflow-git     --from freestyle-survey --bare --git   # existing repo; choose Workflow
$D --repo ~/bram-workflow-no-git  new workflow-no-git  --from freestyle-survey --bare         # no repo; Workflow creates one
$D --repo ~/bram-freestyle-git    new freestyle-git    --from freestyle-survey --bare --git   # existing repo; Freestyle, upgrade later
$D --repo ~/bram-freestyle-no-git new freestyle-no-git --from freestyle-survey --bare         # Elle's scenario (the steps below)
```

What each one is for:

- **workflow-git:** Bram as it was, reached through the chooser. Setup on the
  click, hooks, the gate, and Orient me (the repo predates Bram).
- **workflow-no-git:** `git init` then Setup; Orient me hidden. The repo has
  no commits, so the Transcript offers the first commit through the Worklist
  (**First commit**): the agent proposes it as an item and you click
  **Start & commit**. This is the path to watch.
- **freestyle-git:** no Setup, no hooks; Commits and Files have a repo to
  show. Later, **Switch to workflow** on the Worklist tab must add Bram's
  import to an existing `CLAUDE.md` rather than replace it, and leave tracked
  files alone.
- **freestyle-no-git:** the runbook below, then optionally Switch to
  workflow.

## Demo

**1. Tour the folder (about 3 min).** Show `README.md`, `data/survey.csv`, and
the empty `scripts/` folder. Mention it is not real data and the XYZ courses
are made up.

> Say: it can see this folder and the output of commands it runs. Not your
> browser, not your other folders, not anything you haven't put here.
> Everything it reads goes to a server you don't own.

Bram: open the Status tab and the Context tab. Note what Bram says about a
folder with no repository, and that Context lists no `CLAUDE.md` yet.

**2. Look before analyzing (about 5 min).**

> *Tell me about the data in data/survey.csv. Don't analyze it yet.*

Click open a step ("Read..." or "Ran python3...") to show that it actually
opened the file. Ask the room: what did it look at?

Bram: the Transcript tab shows the same tool calls in order. The Log should
show a short stretch with reads and no edits; compare that with step 3.

**3. The question (about 7 min).**

> *Calculate the average rating for each course and make a figure. Save the code as a script.*

Open the script and the figure.

Pause and ask: **"What did it decide that I didn't ask for?"** Expect it to
have left out the N/A ratings. That is fine, since the README says N/A means
skipped, but it is a choice. If it reports Applied Knot Theory as lowest at
3.39, that is the miss.

> *Explain in plain language what this script does. Did any student submit more than once?*

Models vary here. It may not have mentioned the repeats at all, it may say "no
duplicate response IDs" (it checked IDs, not emails), or it may have noticed
them and kept them anyway. All three make the point.

Bram: in the Log, find the entries for the new script and the figure. In
Transcript, find the turn where the agent chose to drop N/A rows and see
whether it said so. In Sessions, note this session's model.

**4. Fix it and record it (about 5 min).**

> *Keep only each student's latest submission and rerun.*

Expect Lighthouse Management to come out lowest (3.42), with Knot Theory up
to 3.61. The answer flips.

> *Add that to CLAUDE.md so you do it next time.*

Open `CLAUDE.md` to show the note. That file is read at the start of every
session in this folder.

Bram: Context should now show the new `CLAUDE.md`. The Log should show the
rerun and the edit to `CLAUDE.md`. Status shows whether anything about the
folder changed.

**5. A second session with a different model (Bram's addition).** Start a new
session in the same folder with a different model, and repeat step 3 with the
same prompt. A larger model usually catches the repeats on its own, though it
may miscount them (for example "6 of 7" when it is 5). Then, from the second
session, ask:

> what did the earlier session decide about repeat submissions?

This is the Search case: the answer lives in the folder's own session
history, not in any file the agent was given. Look at whether the second agent
finds the first session's decision (through Bram's history search or the
`CLAUDE.md` note) or answers from its own re-analysis. In Sessions, both
sessions should be listed with their models; in Transcript, compare how each
one handled the repeats.

**Optional: a notebook.**

> *Turn this script into a Jupyter notebook I can explore.*

Start `jupyter lab` in the folder, open the notebook, and use Restart and Run
All. The agent edits the file on disk; if the notebook is open while it works,
reload before running anything.

## Expected numbers

| | Beekeeping (XYZ 110) | Pastry (XYZ 120) | Knot Theory (XYZ 230) | Lighthouses (XYZ 240) | Overall |
|---|---|---|---|---|---|
| Naive (repeats kept) | 3.95 | 3.92 | **3.39** | 3.41 | 3.71 (n=204) |
| One per student (latest) | 3.95 | 3.92 | 3.61 | **3.42** | 3.77 (n=197) |
| N/A as 0 (wrong) | 3.72 | 3.64 | 3.19 | 3.09 | 3.45 |

There are 15 N/A ratings. 7 students submitted twice (14 rows): 5 in Knot
Theory, 1 in Beekeeping, and 1 in Lighthouses. The repeats share an `email`
but differ in `response_id` and `submitted_at`, so `drop_duplicates()` on the
whole row and a check of `response_id` both miss them. Keeping each student's
first submission instead gives Knot Theory 3.57, and the ranking stays the
same.

The two traps: pandas reads `N/A` as missing by default, so an agent can drop
those rows without saying so (correct here, but a silent choice); and the
repeat submissions flip the lowest-rated course.

## If...

- **It catches and removes the repeats by itself:** point out that it told
  you, and ask the room whether they would have read that far. Then ask which
  submission it kept, and whether keeping the first would change anything.
- **It asks to install a package:** deny it, and tell it to use `python3` in
  this folder, which already has pandas and matplotlib.
- **It stalls or loops:** press Esc, then restate the request in one sentence.
- **The live run goes sideways:** use the expected numbers above to tell the
  story.
- **Bram refuses to open the folder:** a binary older than 2026-11-22. Its
  `bram.log` reads `op=autostart-held reason=not-git`; rebuild and relaunch.
- **The chooser doesn't appear:** the folder already has a mode recorded for
  this machine (`~/.bram/folders.json`, or `$BRAM_USER_STATE_DIR/folders.json`).
  Remove its row, or launch with a fresh `BRAM_USER_STATE_DIR`.
