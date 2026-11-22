# Bram asks how you want to work when it wakes up somewhere new: workflow or freestyle

Status: decided 2026-11-22 · revisit after the first freestyle user reports, or when #466's pause switch is folded in

**Question:** What Bram does at launch in a folder it hasn't seen. Today it assumes a managed git repo: Setup runs with no prompt (f6e797ac) and a non-repo folder is refused (337291d0). Three reports (#513, #466, the Elle session) show users who pointed Bram at a folder and did not want that.

**Decision:** The unit of Bram's attention is a folder, not a repo, and "managed" is a state the user enters, not one Bram infers from the launch. On first wake in a folder, Bram offers two personas and remembers the answer:

- **Workflow.** What Bram is today: Setup seeds the conventions and guard hooks, the Worklist gate is on, commits go through the gate. Offered as "git workflow" in a folder with no repo, where choosing it includes `git init`.
- **Freestyle.** Bram seeds nothing into the folder and installs no hooks. The terminal, Sessions, Transcript, Log, Context and Status work as now; the Worklist tab shows one affordance, **Switch to workflow**, which runs the workflow path. Freestyle is a product, not a refusal, which is what retires f6e797ac's argument that a "no" branch is broken.

The choice is per folder and per machine (Walt's case is "not managed on this Mac"), so it lives under `~/.bram/`, keyed by the folder, in a file the host owns and the guards deny agents from writing. Switching from freestyle to workflow is the Setup path; switching back is the uninstall path from #466. Launching at home stays refused (#353, #448).

**Evidence:**

- `lib.rs:44122-44124`, f6e797ac: the no-dialog rationale this note supersedes in spirit. The consent-from-launch premise failed on #513's first report.
- #513: `[auto-setup] op=ran firstRun=true … wrote=22` after an undo; no persisted opt-out exists anywhere in `src-tauri/src` or `app/__shell`.
- #466: the v1 pause (`1a4f2f98`, `8d4c67dc`) is Claude-only and agent-writable (`docs/authorization.md`, "The enforcement pause has no human-only path"); it is a one-provider freestyle already in the field.
- Session fb876140 (2026-11-22): a talk by a researcher who studies computer-supported statistical analysis; repo secondary, Worklist overkill, Log and provenance the fit.
- 337291d0: `pristine_nogit` in `scripts/setup-harness.sh` asserts the refusal that freestyle replaces.

**Alternatives:**

- Keep auto-setup and add only a persisted "not managed" marker. Rejected: answers #513's undo but leaves the first wake unasked, and leaves no-repo folders refused.
- A yes/no dialog on Setup. Rejected for the reason f6e797ac gave: with nothing on the "no" side it is a speed bump. The persona choice differs in that both answers are products.
- Infer the persona (repo with remote → workflow, bare folder → freestyle). Rejected: #513 is a repo with a remote whose owner wanted freestyle on one machine.

**Revisit when:** a freestyle user reports what they missed, #466's pause is reconciled with the per-machine choice, or the harness scenarios for first wake (`setup-harness.sh`) are rewritten for the two personas.

Left for the items that follow, not decided here: the exact wording of the two prompts, which pane the choice is made in, what `.bram/` state (port, traces) freestyle keeps in the folder versus under `~/.bram/`, and whether Codex's home config write is gated on the persona or on the CLI being present.
