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
    name: "PROJECT",
    commit: "abc1234",
    date: "YYYY-MM-DD",
    signature: "",
    // The forge the citations point at, for reading docs at the pin:
    // `repo` as owner/repo, `commitFull` the full SHA of `commit`, and
    // `forgeHost` only when it isn't github.com (gitlab.com is understood).
    repo: "OWNER/REPO",
    commitFull: "",  // the agent's signature line, forge form; shown above the caption
  };
  const ARCHITECTURE = [
    {file: "01-system.mmd", title: "System at a glance",
      note: "The parts, who uses them, and where state lives. Click a box or arrow for detail."},
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
    const shotted = new Set();
    const sized = sizedSvg(decorate(svg, parseTips(source), details, labels, shotted));
    const height = +(sized.match(/<svg[^>]*\sheight="(\d+)"/) || [0, 200])[1];
    return {srcdoc: diagramDocument(sized, diagram), height: height + 24,
      mismatches: mismatches(labels, details), shotCount: shotted.size};
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

  // ---- Reading the real doc -------------------------------------------------
  // A section may open with directive lines `<!-- read: <path>#<heading> -->`
  // (heading optional). The dialog shows a link line per directive and
  // nothing is fetched up front; "Read here" opens the doc's section in a
  // reader dialog, fetched from the forge at the pinned commit (Main.xmlui's
  // DataSource on Arch.rawUrl, rendered through Arch.section), and the link
  // opens the rendered page in a new tab. The site never copies a doc's
  // text. The forge forbids framing, so the reader renders the Markdown
  // itself. Rule: when a human-readable doc explains the thing, link it
  // here and keep the code citations as the "where it lives" footnote.
  const READ_RE = /^<!--\s*read:\s*([^#\s]+)(?:#(.+?))?\s*-->\s*$/;
  const forgeBase = () => "https://" + (SITE.forgeHost || "github.com") + "/" + SITE.repo;
  const rawUrl = (path) => (SITE.forgeHost || "github.com") === "github.com"
    ? "https://raw.githubusercontent.com/" + SITE.repo + "/" + SITE.commitFull + "/" + path
    : forgeBase() + "/-/raw/" + SITE.commitFull + "/" + path;
  const blobUrl = (path) => (SITE.forgeHost || "github.com") === "github.com"
    ? forgeBase() + "/blob/" + SITE.commitFull + "/" + path
    : forgeBase() + "/-/blob/" + SITE.commitFull + "/" + path;
  const anchorOf = (heading) => heading.toLowerCase().replace(/[`*_]/g, "").replace(/[^\w\s-]/g, "").trim().replace(/\s+/g, "-");
  // The lines under `heading` (any level) up to the next heading of the same
  // or a higher level; the whole file when no heading is given.
  function sectionOf(text, heading) {
    const lines = text.split("\n");
    if (!heading) return lines;
    const want = norm(heading);
    let level = 0, start = -1;
    for (let i = 0; i < lines.length; i++) {
      const m = /^(#{1,6})\s+(.*?)\s*#*\s*$/.exec(lines[i]);
      if (!m) continue;
      if (start < 0) { if (norm(m[2]) === want) { level = m[1].length; start = i + 1; } continue; }
      if (m[1].length <= level) return lines.slice(start, i);
    }
    return start < 0 ? null : lines.slice(start);
  }
  // Rewrite the doc's own links so they still work from here: relative paths
  // become rendered URLs at the pin; bare anchors point into the rendered doc.
  function rewriteLinks(md, path) {
    const dir = path.includes("/") ? path.slice(0, path.lastIndexOf("/") + 1) : "";
    return md.replace(/\]\(([^)\s]+)\)/g, (whole, target) => {
      if (/^(https?:|mailto:)/.test(target)) return whole;
      if (target.startsWith("#")) return "](" + blobUrl(path) + target + ")";
      const resolved = new URL(target, "https://x/" + dir).pathname.replace(/^\//, "");
      return "](" + blobUrl(resolved) + ")";
    });
  }
  // For the reader dialog: the Markdown of one doc section, from the fetched
  // text. A missing heading says so rather than showing nothing.
  function section(text, heading, path) {
    if (text == null) return "";
    const raw = typeof text === "string" ? text : String(text);
    const lines = sectionOf(raw, heading);
    if (lines == null) return "*No heading \"" + heading + "\" in " + path + " at this site's pin.*";
    const head = heading ? "## " + heading + "\n\n" : "";
    return head + rewriteLinks(lines.join("\n").trim(), path);
  }
  // Split a section body into its directives and the prose that follows.
  function splitReads(body) {
    const lines = body.split("\n");
    const reads = [];
    while (lines.length && READ_RE.test(lines[0].trim())) {
      const m = READ_RE.exec(lines.shift().trim());
      const path = m[1], heading = m[2] ? m[2].trim() : "";
      reads.push({path, heading, label: path + (heading ? " § " + heading : ""),
        link: blobUrl(path) + (heading ? "#" + anchorOf(heading) : "")});
    }
    return {reads, prose: lines.join("\n").trim()};
  }

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
    if (body == null) return null;
    const {reads, prose} = splitReads(body);
    // A line that is only a Markdown image, ![caption](src "Title"), becomes a shot: the dialog shows it
    // as a thumbnail that expands in place, not as inline full-width Markdown.
    const shots = [];
    const text = prose.split("\n").filter((line) => {
      const m = /^!\[([^\]]*)\]\(([^)\s]+)(?:\s+"([^"]*)")?\)\s*$/.exec(line.trim());
      if (m) shots.push({alt: m[1], src: m[2], title: m[3] || ""});
      return !m;
    }).join("\n").trim();
    return {title: key, body: pageLinks(text), reads, shots};
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
  // Citations open in the site, not on the forge: a forge blob link with a
  // line anchor (GitHub #L10-L20, GitLab #L10-20) becomes a relative
  // ?cite=<path>:<from>-<to> link, which the
  // Markdown component routes in-app; Main.xmlui's citation dialog follows
  // $queryParams.cite and shows the cited lines, fetched raw at the pin. (The
  // forge won't render very large files such as lib.rs at all.)
  const CITE_LINK = /\]\(https?:\/\/[^)\s]*?\/blob\/[0-9a-f]{40}\/([^?#)\s]+)(?:\?plain=1)?#L(\d+)(?:-L?(\d+))?\)/g;
  function citeLinks(md) {
    return md.replace(CITE_LINK, (m, path, from, to) =>
      "](?cite=" + encodeURIComponent(path + ":" + from + "-" + (to || from)) + ")");
  }
  // For a page or a detail section: citations as above, and every other
  // external link opens in a new tab. That needs Markdown's per-link
  // "label|target=_blank" form, which works only on a plain-text label, so a
  // code-styled label (a commit SHA) is unwrapped to plain text.
  const EXTERNAL_LINK = /\[`?([^\]`|]+)`?\]\((https?:\/\/[^)\s]+)\)/g;
  function pageLinks(md) {
    if (md == null) return "";
    return citeLinks(String(md)).replace(EXTERNAL_LINK, (m, label, url) => "[" + label + "|target=_blank](" + url + ")");
  }
  function cite(spec) {
    const m = /^(.+):(\d+)-(\d+)$/.exec(spec || "");
    if (!m) return null;
    return {path: m[1], from: +m[2], to: +m[3], label: m[1] + ":" + m[2] + (m[3] !== m[2] ? "-" + m[3] : ""),
      link: blobUrl(m[1]) + "?plain=1#L" + m[2] + "-L" + m[3]};
  }
  const FENCE_LANG = {rs: "rust", js: "js", ts: "ts", md: "markdown", json: "json", toml: "toml",
    xmlui: "xml", xml: "xml", html: "html", sh: "bash", py: "python", ps1: "powershell", yml: "yaml", yaml: "yaml"};
  // The cited lines, numbered, as a fenced block. A range past the end of the
  // file says so rather than showing nothing.
  function citeExcerpt(text, spec) {
    const c = cite(spec);
    if (!c || text == null) return "";
    const lines = (typeof text === "string" ? text : String(text)).split("\n");
    if (c.from > lines.length) return "*Line " + c.from + " is past the end of " + c.path + " (" + lines.length + " lines) at this site's pin.*";
    const last = Math.min(c.to, lines.length);
    const width = String(last).length;
    const body = lines.slice(c.from - 1, last).map((l, i) => String(c.from + i).padStart(width) + "  " + l).join("\n");
    const ext = (c.path.split(".").pop() || "").toLowerCase();
    return "```" + (FENCE_LANG[ext] || "") + "\n" + body + "\n```";
  }

  // How many screenshots a detail section holds: its image-only lines, the
  // same lines detail() turns into thumbnails.
  const SHOT_LINE = /^!\[([^\]]*)\]\(([^)\s]+)(?:\s+"([^"]*)")?\)\s*$/;
  function shotsIn(body) {
    return body == null ? 0 : body.split("\n").filter((l) => SHOT_LINE.test(l.trim())).length;
  }
  const SHOT_MARK = "\u25A3"; // ▣ a framed square: "has screenshots"
  const SHOT_GAP = 6;         // px between an arrow label's right edge and its mark
  const SHOT_PAD = 6;         // px between a box's top-right corner and its mark
  const SHOT_ROOM = 20;       // px added at the right of the drawing for marks

  function decorate(svg, tips, details, labels, shotted) {
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
      const shots = shotsIn(details[text]);
      if (tips[text]) {
        const title = doc.createElementNS("http://www.w3.org/2000/svg", "title");
        title.textContent = tips[text] + (details[text] != null ? "  (click for more)" : "");
        el.insertBefore(title, el.firstChild);
        el.setAttribute("class", el.getAttribute("class") + " has-tip");
      }
      // The screenshot mark ▣, drawn in the rect's own coordinates. Boxes: inside
      // the top-right corner, padded. Arrow labels: to the right of the label,
      // centred on it, with a gap. Its own hover tip says only "N screenshots";
      // a click on it falls through to the label, which opens the detail.
      const rect = shots && el.querySelector("rect");
      if (rect) {
        const x = +rect.getAttribute("x"), y = +rect.getAttribute("y");
        const w = +rect.getAttribute("width"), h = +rect.getAttribute("height");
        if ([x, y, w, h].every(Number.isFinite)) {
          const arrow = (el.getAttribute("class") || "").includes("edgeLabel");
          const mark = doc.createElementNS("http://www.w3.org/2000/svg", "text");
          if (arrow) {
            mark.setAttribute("x", String(x + w + SHOT_GAP));
            mark.setAttribute("y", String(y + h / 2));
            mark.setAttribute("text-anchor", "start");
          } else {
            mark.setAttribute("x", String(x + w - SHOT_PAD));
            mark.setAttribute("y", String(y + SHOT_PAD + 7));
            mark.setAttribute("text-anchor", "end");
          }
          mark.setAttribute("dominant-baseline", "central");
          mark.setAttribute("fill", "#2f5d8a");
          mark.setAttribute("class", "shot-mark");
          mark.textContent = SHOT_MARK;
          const tip = doc.createElementNS("http://www.w3.org/2000/svg", "title");
          tip.textContent = shots + (shots === 1 ? " screenshot" : " screenshots");
          mark.appendChild(tip);
          rect.parentNode.appendChild(mark);
          shotted.add(text);
        }
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
        [vb[0], vb[1] - LABEL_HEADROOM, vb[2] + (shotted.size ? SHOT_ROOM : 0), vb[3] + LABEL_HEADROOM].join(" "));
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
    + "text.shot-mark{font-size:14px;fill:#2f5d8a !important;cursor:pointer}"
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

    // "GitHub" or the forge host, for the "Read on …" line under a detail.
  const forgeLabel = (SITE.forgeHost || "github.com") === "github.com" ? "GitHub" : SITE.forgeHost;

  root.Arch = {site: SITE, caption, sections: VISIBLE, diagrams, detail, page, rawUrl, section, forgeLabel, shotMark: SHOT_MARK, cite, citeExcerpt, pageLinks};
})(globalThis);
