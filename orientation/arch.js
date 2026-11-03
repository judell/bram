// Helpers for the Architecture tab of a Bram orientation site (Main.xmlui).
// Template from Bram (.claude/bram-reference/orientation-template/), first
// built for tau-hub, together-groups-beta and pwkm. The playbook is
// .claude/bram-reference/orientation.md.
//
// Each diagram is a Mermaid file in architecture/*.mmd. Two optional layers
// ride along with it:
//   - hover text, as Mermaid comments keyed by a label as drawn (a wrapped
//     or <br/> label is one line of text, single-spaced):
//       %% tip: <label> :: <short text>
//   - click-through detail, in a companion .md file (01-system.md beside
//     01-system.mmd), one "## <label>" section per box or arrow.
(function (root) {
  // ---- Edit this block, and nothing else in this file. -------------------
  // `commit` is the short SHA every citation is pinned to; `date` is that
  // commit's date. Sections render in order; `hidden: true` keeps a draft off
  // the page.
  const SITE = {
    name: "Bram",
    commit: "e8a38ac0",
    date: "2026-11-02",
    signature: "Jon's Claude (Bram 0.8.2, Fable 5.1, macOS) speaking from the Bram project (github.com/judell/bram):",
  };
  const ARCHITECTURE = [
    {file: "01-system.mmd", title: "System at a glance",
      note: "What you are looking at when Bram is open. A Rust shell runs the agent CLI in a terminal and provides a UI over it; the pane and the agent both talk to a small HTTP server on your machine; a target-app pane is optional. The one dashed arrow is the agent reading the conventions Bram seeds into the repo: everything else is wiring that works on its own. Click a box or arrow for detail."},
    {file: "02-turn.mmd", title: "A turn's lifecycle",
      note: "From a pane action to the PTY, the agent and its transcript, and back to the pane as turn state. The dashed arrow depends on the agent opening a referenced draft file."},
    {file: "03-gate.mmd", title: "The Worklist gate",
      note: "How a change gets into the repo. An agent proposes an item; you click Start, which records your approval and opens a claim; the guard lets the agent edit only what the item covers; you click Commit, and the host stages, commits and keeps the record. The one dashed arrow is the agent telling the board it has finished editing: everything else happens whether or not anyone remembers."},
    {file: "04-state.mmd", title: "State and data",
      note: "Where Bram keeps what it knows. Everything per project lives in one state folder (resources/ today, .bram/ once it exists): the worklist and its drafts, the record of what you approved, the claim marking which lines an item wrote, traces, a mirror of the board's history, and the search index over all of it. Git holds the claim refs. The one dashed arrow is a person pushing those refs on purpose."},
    {file: "05-providers.mmd", title: "Providers",
      note: "A provider wraps an agentic CLI. Each wrapped CLI is influenced by the same set of soft conventions and hard enforcement."},
    {file: "06-build.mmd", title: "Build, run and release",
      note: "How Bram is built, started and shipped. A debug build is the shipping build; launched through the ./bram symlink, the pane is served from the source tree so edits show on reload, while the Rust host needs a rebuild and relaunch. Setup seeds each managed repo with the conventions and the hooks. A release is one script that bumps, tags, pushes and starts the build. Dashed arrows need a person to rebuild, relaunch or click Setup."},
  ];
  // ------------------------------------------------------------------------

  const DIR = "architecture/";
  const VISIBLE = ARCHITECTURE.filter((s) => !s.hidden);

  // Render a Mermaid file to a small HTML document holding the live SVG, for an
  // <IFrame srcdoc>. htmlLabels: false keeps labels as plain SVG <text> inside
  // <g class="edgeLabel">, which decorate() relies on.
  let mermaidReady = false;
  let mermaidSeq = 0;
  async function mermaidDiagram(path) {
    const m = globalThis.mermaid;
    if (!m) throw new Error("Mermaid is not loaded (see index.html).");
    if (!mermaidReady) {
      m.initialize({startOnLoad: false, securityLevel: "strict", theme: "default",
        flowchart: {htmlLabels: false}});
      mermaidReady = true;
    }
    const response = await fetch(path, {cache: "no-store"});
    if (!response.ok) throw new Error("Cannot load " + path + " (HTTP " + response.status + ").");
    const source = await response.text();
    const details = await loadDetails(path.replace(/\.mmd$/, ".md"));
    const diagram = path.split("/").pop().replace(/\.mmd$/, "");
    DETAILS[diagram] = details;
    const {svg} = await m.render("mermaid-" + (++mermaidSeq), source);
    const labels = new Set();
    const sized = sizedSvg(decorate(svg, parseTips(source), details, labels));
    const height = +(sized.match(/<svg[^>]*\sheight="(\d+)"/) || [0, 200])[1];
    return {srcdoc: diagramDocument(sized, diagram), height: height + 24,
      mismatches: mismatches(labels, details)};
  }

  // The structural check the playbook asks for: every box and arrow label has
  // a "## " section, and every section matches a label. Empty when they agree.
  function mismatches(labels, details) {
    const out = [];
    for (const l of labels) if (details[l] == null) out.push("no section for: " + l);
    for (const k of Object.keys(details)) if (!labels.has(k)) out.push("section matches nothing drawn: " + k);
    return out;
  }

  // Labels are compared as whitespace-collapsed text, so line breaks, whether
  // wrapped by Mermaid or written as <br/>, can't change a key.
  const norm = (s) => (s || "").replace(/\s+/g, " ").trim();

  // Click-through detail: {label: markdown} from the companion .md file.
  // Missing file = no clickable items; the mismatch check reports every
  // label. Text above the first "## " heading is ignored.
  const DETAILS = {};
  async function loadDetails(path) {
    const response = await fetch(path, {cache: "no-store"});
    if (!response.ok) return {};
    const out = {};
    const parts = (await response.text()).split(/^##\s+/m).slice(1);
    for (const part of parts) {
      const nl = part.indexOf("\n");
      const key = norm(nl < 0 ? part : part.slice(0, nl));
      out[key] = nl < 0 ? "" : part.slice(nl + 1).trim();
    }
    return out;
  }

  // For the modal: {title, body} for a clicked box or arrow, or null.
  function detail(diagram, key) {
    const body = (DETAILS[diagram] || {})[key];
    return body == null ? null : {title: key, body};
  }

  function parseTips(source) {
    const tips = {};
    for (const m of source.matchAll(/^\s*%%\s*tip:\s*(.+?)\s*::\s*(.+?)\s*$/gm)) tips[norm(m[1])] = m[2];
    return tips;
  }

  const LABEL_GAP = 3;       // px between an arrow and the bottom of its label
  const LABEL_HEADROOM = 16; // px added above the drawing for raised labels

  // Post-process Mermaid's SVG: raise each edge label just above its arrow,
  // attach tips as SVG <title> (native tooltip), mark items that have detail,
  // collect every label drawn, and add headroom so a raised label can't be
  // clipped.
  function decorate(svg, tips, details, labels) {
    // Mermaid draws a box with line breaks as HTML in a foreignObject, and
    // HTML's <br> is not closed; the XML parser below rejects it ("Unexpected
    // closing tag"). Close it before parsing. Other void tags don't occur in
    // labels.
    const doc = new DOMParser().parseFromString(svg.replace(/<br\s*>/gi, "<br/>"), "image/svg+xml");
    const svgRoot = doc.documentElement;
    const num = "(-?[\\d.]+)";
    const translate = new RegExp("translate\\(\\s*" + num + "[ ,]+" + num + "\\s*\\)");
    // Mermaid puts each wrapped or <br/> line of an SVG label in its own tspan
    // with no space between lines, so textContent alone would glue "in" and
    // "~/.codex". Boxes may be HTML in a foreignObject, where <br> adds none.
    const labelText = (el) => {
      const lines = [...el.querySelectorAll("tspan.text-outer-tspan")].map((l) => l.textContent);
      if (lines.length) return norm(lines.join(" "));
      const copy = el.cloneNode(true);
      for (const br of copy.querySelectorAll("br")) br.replaceWith(" ");
      return norm(copy.textContent);
    };
    const annotate = (el, text) => {
      labels.add(text);
      if (tips[text]) {
        const title = doc.createElementNS("http://www.w3.org/2000/svg", "title");
        title.textContent = tips[text] + (details[text] != null ? "  (click for more)" : "");
        el.insertBefore(title, el.firstChild);
        el.setAttribute("class", el.getAttribute("class") + " has-tip");
      }
      if (details[text] != null) {
        el.setAttribute("data-key", text);
        // Keyboard reach: a focusable button that Enter and Space activate
        // (see diagramDocument). aria-label names the item for screen readers.
        el.setAttribute("tabindex", "0");
        el.setAttribute("role", "button");
        el.setAttribute("aria-label", text);
        el.setAttribute("class", el.getAttribute("class") + " has-detail");
      }
    };
    for (const label of doc.querySelectorAll("g.edgeLabel")) {
      const text = labelText(label);
      if (!text) continue;
      const outer = (label.getAttribute("transform") || "").match(translate);
      const inner = label.querySelector("g.label");
      const innerT = inner && (inner.getAttribute("transform") || "").match(translate);
      if (outer && innerT) {
        const half = Math.abs(+innerT[2]);
        label.setAttribute("transform",
          "translate(" + outer[1] + ", " + (+outer[2] - half - LABEL_GAP) + ")");
      }
      annotate(label, text);
    }
    for (const node of doc.querySelectorAll("g.node")) {
      const text = labelText(node);
      if (text) annotate(node, text);
    }
    const vb = (svgRoot.getAttribute("viewBox") || "").trim().split(/[\s,]+/).map(Number);
    if (vb.length === 4 && vb.every(Number.isFinite)) {
      svgRoot.setAttribute("viewBox",
        [vb[0], vb[1] - LABEL_HEADROOM, vb[2], vb[3] + LABEL_HEADROOM].join(" "));
    }
    return new XMLSerializer().serializeToString(svgRoot);
  }

  // The iframe's whole document. A click, or Enter or Space on a focused item
  // with detail, posts {type: 'arch-detail', diagram, key} to the app's
  // MessageListener. Key events reach this document when the frame has focus.
  const diagramDocument = (svg, diagram) =>
    "<!doctype html><html><head><meta charset='utf-8'><style>"
    + "html,body{margin:0;background:transparent;font-family:system-ui,sans-serif}"
    + "svg{max-width:100%;height:auto;display:block}"
    + "g.has-tip{cursor:help}"
    + "g.edgeLabel.has-tip rect{stroke:#8a8a8a;stroke-dasharray:2 2;stroke-width:1}"
    + "g.has-detail{cursor:pointer}"
    + "g.node.has-detail:hover rect,g.node.has-detail:hover path,g.node.has-detail:hover polygon{stroke-width:2.5px}"
    + "g.edgeLabel.has-detail:hover text{text-decoration:underline}"
    + "g.has-detail:focus{outline:none}"
    + "g.has-detail:focus-visible{outline:2px solid #2f5d8a;outline-offset:2px}"
    + "g.node.has-detail:focus-visible rect,g.node.has-detail:focus-visible path,g.node.has-detail:focus-visible polygon{stroke:#2f5d8a;stroke-width:3px}"
    + "g.edgeLabel.has-detail:focus-visible text{text-decoration:underline}"
    + "g.edgeLabel.has-detail:focus-visible rect{stroke:#2f5d8a;stroke-width:2px}"
    + "</style></head><body>" + svg
    + "<script>function openDetail(g){parent.postMessage({type:'arch-detail',diagram:" + JSON.stringify(diagram)
    + ",key:g.getAttribute('data-key')},'*');}"
    + "document.addEventListener('click',function(e){"
    + "var g=e.target.closest&&e.target.closest('[data-key]');if(g)openDetail(g);});"
    + "document.addEventListener('keydown',function(e){"
    + "var g=e.target.closest&&e.target.closest('[data-key]');"
    + "if(g&&(e.key==='Enter'||e.key===' ')){e.preventDefault();openDetail(g);}});</script>"
    + "</body></html>";

  // Mermaid emits width="100%" and no height; give the SVG its real size from
  // the viewBox so the frame can be sized to it.
  function sizedSvg(svg) {
    const vb = svg.match(/viewBox="\s*[-\d.]+\s+[-\d.]+\s+([\d.]+)\s+([\d.]+)\s*"/);
    if (!vb) return svg;
    return svg.replace(/<svg([^>]*)>/, (whole, attrs) => {
      const cleaned = attrs.replace(/\swidth="[^"]*"/, "").replace(/\sheight="[^"]*"/, "")
        .replace(/max-width:\s*[^;"]+;?/, "");
      return "<svg" + cleaned + ' width="' + Math.ceil(+vb[1]) + '" height="' + Math.ceil(+vb[2]) + '">';
    });
  }

  // Render every visible section; {file: {srcdoc, height, mismatches} | {error}}.
  async function diagrams() {
    const out = {};
    for (const s of VISIBLE) {
      try { out[s.file] = await mermaidDiagram(DIR + s.file); }
      catch (e) { out[s.file] = {error: e.message}; }
    }
    return out;
  }

  const caption = "The big picture of " + SITE.name + " as of commit " + SITE.commit
    + " (" + SITE.date + "). Each diagram is a file in architecture/. Hover a box or"
    + " arrow for a tip; click it, or Tab to it and press Enter, for detail that cites the code.";

  // The site is three plain pages, the web's own mechanism, no router: the
  // folder root is Start, and diagrams/ and overview/ hold a copied
  // index.html with <base href="../">. Each index.html names its page in
  // window.ORIENTATION_PAGE and carries the nav as plain HTML; Main.xmlui
  // renders the section for Arch.page.
  const page = root.ORIENTATION_PAGE || "start";

  root.Arch = {site: SITE, caption, sections: VISIBLE, diagrams, detail, page};
})(globalThis);
