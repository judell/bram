# Bram reference: orientation

Seeded by Bram Setup as `.claude/bram-reference/orientation.md`. Read this
when the user clicks **Orient me** on Bram's startup greeting, or
asks you to orient them in, survey, or map a repo. It is how to build an
**orientation site**: a small static XMLUI app with an Overview tab and an
Architecture tab of Mermaid diagrams. Every box and arrow in a diagram opens
detail that cites the code at a pinned commit. The pattern was built by hand
three times (tau-hub, together-groups-beta, pwkm) before it became this
playbook. The template you copy is beside this file, in
`.claude/bram-reference/orientation-template/`.

Budget a long turn. This is a reading job first and a writing job second.

### 1. Decide placement before writing anything

- **Default: local-only.** The site lives in `orientation/` at the repo
  root, and you append `/orientation/` to `.git/info/exclude`, not to
  `.gitignore`, which is tracked. Nothing is committed or pushed.
- **Commit it only if the user says so.** Ask once, after the item is
  proposed, not before.
- **Check the license first.** Read `LICENSE`, `NOTICE*` and the README's
  license section. If they restrict derivative works or publication, say so
  in the item's draft. Local-only is still fine for reading, but publishing
  needs the owner's permission.
- **Never carry content across repos.** Facts about another repo don't
  belong in this one, and nothing private belongs in a public repo.
- If `orientation/` already exists and isn't a previous orientation site,
  pick another name and say which.

### 2. Propose one Worklist item

The site is new files, so it goes through the Worklist like any change,
whether or not they're excluded. Use one item (for example
`orientation-site`) whose `files` is `["orientation/"]`, plus
`.git/info/exclude` only if the repo tracks it (it normally doesn't). In the
draft, record:

- the placement, and the license finding;
- the commit you will pin (`git rev-parse HEAD`, short and full);
- your planned diagrams: 3 to 6, each a different view (system at a glance,
  a main flow or lifecycle, data and state, configuration and environment,
  build and deploy).

Then stop and wait for **Start**.

### 3. Build

1. Copy the template: `cp -R .claude/bram-reference/orientation-template
   orientation` (on Windows, `Copy-Item -Recurse`).
2. Edit only the `SITE` block at the top of `orientation/arch.js`: the
   project name, the pinned commit and its date, and the diagram list.
3. Write `orientation/OVERVIEW.md`: what the project is, how it runs, where
   to start reading, and open questions. Keep the template's headings unless
   the repo calls for others.
4. For each diagram, write `orientation/architecture/0N-<slug>.mmd` and its
   companion `0N-<slug>.md`, replacing the example `01-system` pair:
   - a Mermaid `flowchart`, with labels short enough to read at a glance;
   - `%% tip: <label> :: <text>` lines for hover text;
   - in the `.md`, one `## <label>` section per box **and** per arrow
     label, headed by the label exactly as drawn.

### 4. Evidence rules

- **Pin one commit** and say so in the caption (the `SITE` block) and at the
  top of each detail file. Every fact is "as of" that commit.
- **Cite line ranges.** Every claim links to the lines it rests on, at the
  pinned full SHA. Build links for the forge in `git remote get-url origin`:
  - GitHub: `https://github.com/O/R/blob/<sha>/<path>?plain=1#L10-L20`
    (`?plain=1` makes line anchors work on Markdown files);
  - GitLab: `https://gitlab.com/G/R/-/blob/<sha>/<path>#L10-20`;
  - other forges or no remote: `` `<path>:10-20` `` as plain text, and say
    once in the overview that links can't be built.
- **Quote, don't paraphrase,** where a short phrase from the repo carries
  the point.
- **Solid arrows** (`-->`) are what code does on its own. **Dashed arrows**
  (`-.->`) are what happens only if a person or agent remembers to do it.
  Say which is which in the diagram's note.
- **Say where the repo is silent.** "The repo doesn't say" beats a guess.

### 5. Done means checked

All three, before you call it done:

- **Structure.** Serve the site and open the Architecture tab. Under each
  diagram, the template lists every label with no section and every section
  matching nothing drawn. There must be none.
- **Citations.** For each cited range, `git show <sha>:<path> | wc -l` is at
  least the range's last line, and the lines say what you claim.
- **Rendering.** Every diagram renders. A red error under a diagram is
  Mermaid syntax to fix.

### 6. Show, don't judge

Serve the folder (`python3 -m http.server 8766` from `orientation/`, or any
static server; on Windows, `py -m http.server 8766`) and give the user the
URL. If they use Bram's target pane, `.bram.json`'s `server` setting can
point at it. How it looks is theirs to judge; how it checks is yours. Then
advance the item, and commit only if placement was "commit".
