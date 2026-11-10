# Bram reference: orientation

Seeded by Bram Setup as `.claude/bram-reference/orientation.md`. Read this
when the user clicks **Orient me** on Bram's startup greeting, or
asks you to orient them in, survey, or map a repo. It is how to build an
**orientation site**: a small static XMLUI app of three pages, each a real
URL: Start (the folder root) for the owner's own words, `diagrams/` of Mermaid
diagrams, and `overview/`. Every box and arrow in a diagram opens
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
   orientation` (on Windows, `Copy-Item -Recurse`). It is three pages with no
   router: the folder root is Start, and `diagrams/index.html` and
   `overview/index.html` are copies of the root `index.html` with
   `<base href="../">`, so they share every file in the folder. A new page is
   a new folder with the same copy.
2. Edit only the `SITE` block at the top of `orientation/arch.js`: the
   project name, the pinned commit and its date, your signature line (the older
   one-line forge form, `<owner>'s <Agent> (Bram <version>, <thread>, <model>,
   <os>, <machine>) speaking from the <Project> project (<locator>):`, shown
   above the diagrams; on a published site the `<thread>` and `<machine>`
   segments may be left out). Forge issues and comments now sign with the
   expander form in `conventions.md`; a site's credit line may stay one line),
   and the diagram list. Leave `orientation/START.md` alone:
   the Start page is the owner's, in their own words, and you never write or
   edit it.
   **The engine is Bram's.** `arch.js` and `Main.xmlui` carry a
   `bram-managed: orientation engine` marker, and Bram's Setup refreshes
   them from the template wherever the site lives, keeping your `SITE` /
   `ARCHITECTURE` block and your `PageMetaTitle`. Don't change anything else
   in those two files: the next Setup overwrites it. An engine change (how
   dialogs, citations or marks work) is made in Bram's template, and reaches
   every site from there. Everything else in the folder, `index.html`
   included, is the site's own and is never touched.
   **Adopting** (a site built before the marker existed): copy the
   template's `arch.js` and `Main.xmlui` over the site's, then put the
   site's own `SITE` / `ARCHITECTURE` block and title back. Setup keeps it
   current from then on. A site that doesn't follow the template's layout
   is rebuilt on it rather than patched.
3. Write `orientation/OVERVIEW.md`: what the project is, how it runs, where
   to start reading, and open questions. Keep the template's headings unless
   the repo calls for others.
4. For each diagram, write `orientation/architecture/0N-<slug>.mmd` and its
   companion `0N-<slug>.md`, replacing the example `01-system` pair:
   - a Mermaid `flowchart`, top-down (`flowchart TD`) by default, with
     labels short enough to read at a glance (long ones wrap, which is fine);
   - `%% tip: <label> :: <text>` lines for hover text;
   - in the `.md`, one `## <label>` section per box **and** per arrow
     label, headed by the label as drawn. Every box and arrow opens its
     detail, so none is left without a section.
   - Each diagram has an address, `diagrams/#<slug>`, where the slug is its
     title lowercased with punctuation dropped and spaces as hyphens
     ("Item, agent, session" is `#item-agent-session`); hovering the heading
     shows the link. Use it when an issue or a doc points at one diagram.
5. Screenshots, if the owner wants them: put each image under
   `orientation/screenshots/` (shrunk to at most 1600 px on the long side, for
   example with `sips -Z 1600` on macOS) and add it to the section for the box
   or arrow it shows, on a line by itself:
   `![<caption>](screenshots/<file>.png "<title>")`. The title shows above the
   thumbnail, the caption below, and a click expands it in place. A box or
   arrow with screenshots gets a small ▣ mark. Take them from a real run of
   the thing the diagram describes; anything in the frame is published, so
   crop what isn't meant for the public.

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

  On the site, a GitHub or GitLab citation opens in place: a dialog shows the
  cited lines, fetched raw at the pin, so a citation into a file too large for
  the forge to render (a big `lib.rs`) still works. Write the link in the
  forge form above; the site rewrites it. Other external links open in a new
  tab.
- **Quote, don't paraphrase,** where a short phrase from the repo carries
  the point.
- **Read the doc, don't retell it.** When a human-readable document explains
  the thing a box or arrow names (a conventions file, a decision note, a
  design doc), open the section's body with a directive line,
  `<!-- read: <path>#<heading> -->`, naming that doc and its most specific
  heading at the pin. The dialog shows one "Read <doc> § <heading>" link per
  directive, which opens that section in a reader dialog, fetched from the
  forge at the pinned commit (the forge forbids framing, so the reader
  renders the Markdown itself).
  Nothing is copied, and nothing is fetched until asked. Keep the code
  citations below as the "where it lives" footnote. Verify every heading exists at
  the pin (`git show <sha>:<path> | grep '^#* <heading>$'`). Set `repo` and
  `commitFull` in the `SITE` block so the fetch has a forge and a commit.
- **Draw what happens; don't encode it in the line style.** Where a step
  depends on a person or a model doing what they're asked (a convention the
  agent must follow, a click, a push), say so in that arrow's tip and detail
  and in the diagram's note. A dashed-versus-solid convention was tried and
  read as a quirk: one dashed arrow per diagram, each explained differently.
- **Say where the repo is silent.** "The repo doesn't say" beats a guess.

### 5. Done means checked

All three, before you call it done:

- **Structure.** Serve the site, open the diagrams page in a real
  browser, and look at what the page draws. Under each diagram, the template
  lists every label with no section and every section matching nothing
  drawn. There must be none. Comparing the source files doesn't count: that
  check once passed while the page was wrong.
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
