#!/usr/bin/env python3
"""Refold a published release body into the shape of
.github/release-body-template.md: the notes first and visible, the fixed
parts (Prerequisite(s), Install, Audit-friendly manual install, Change log,
Troubleshooting) after them as folds.

It moves and wraps text only; it never rewrites it. Each output is checked
against its input: the same lines, ignoring blank lines, fold markup, the
fixed sections' "## " headings, and heading depth inside the Install fold.

    scripts/refold-release-bodies.py IN_DIR OUT_DIR

reads IN_DIR/<tag>.md and writes OUT_DIR/<tag>.md for each body with at
least one fixed section; others are reported and left out. Exit 1 if any
check fails. Publishing is a separate step (gh release edit --notes-file).
"""
import collections
import pathlib
import re
import sys

# Fixed section heading -> (fold key, fold summary). Order of FOLDS is the
# template's.
FIXED = {
    "Prerequisite": "install",
    "Prerequisites": "install",
    "Install": "install",
    "Audit-friendly manual install": "audit",
    "Change log": "changelog",
    "Troubleshooting": "troubleshooting",
}
FOLDS = [
    ("install", "Install"),
    ("audit", "Audit-friendly manual install"),
    ("changelog", "All commits since the previous release"),
    ("troubleshooting", "Troubleshooting"),
]
FENCE = re.compile(r"^\s*(```|~~~)")


def sections(body):
    """Split on top-level "## " headings outside code fences.
    Yields (heading or None for the preamble, [lines])."""
    out, head, cur, fenced = [], None, [], False
    for line in body.split("\n"):
        if FENCE.match(line):
            fenced = not fenced
        if not fenced and line.startswith("## "):
            out.append((head, cur))
            head, cur = line[3:].strip(), []
            continue
        if not fenced and line.startswith("# "):
            # A "# Bram vX" title starts the notes, wherever it sits; left
            # in the section before it, it would fold away with Install.
            out.append((head, cur))
            head, cur = None, [line]
            continue
        cur.append(line)
    out.append((head, cur))
    return out


def demote(lines):
    """Install's "### macOS / Linux" becomes "#### …" inside the fold, as in
    the template; code fences are left alone."""
    res, fenced = [], False
    for line in lines:
        if FENCE.match(line):
            fenced = not fenced
        if not fenced and line.startswith("### "):
            line = "#" + line
        res.append(line)
    return res


def trim(lines):
    while lines and not lines[0].strip():
        lines = lines[1:]
    while lines and not lines[-1].strip():
        lines = lines[:-1]
    return lines


CODE = re.compile(r"`([^`]+)`")


def fold_notes(lines):
    """Each section of the notes becomes a numbered fold, as in v0.9.1:
    the ### sections when the notes have any, else the ## sections when
    there are two or more. Text before the first section stays visible, and
    so does a shallower heading that ends a run of sections."""
    def level(line):
        # A "Release notes" heading is not a section; retitle() replaces it.
        if line.strip() in ("## Release notes", "## Release note"):
            return 0
        m = re.match(r"^(#{2,6}) ", line)
        return len(m.group(1)) if m else 0

    fenced, levels = False, []
    for line in lines:
        if FENCE.match(line):
            fenced = not fenced
        elif not fenced and level(line):
            levels.append(level(line))
    if 3 in levels:
        at = 3
    elif levels.count(2) >= 2:
        at = 2
    else:
        return lines
    out, n, open_, fenced = [], 0, False, False
    for line in lines:
        if FENCE.match(line):
            fenced = not fenced
        lv = 0 if fenced else level(line)
        if lv and lv <= at:
            if open_:
                out += ["", "</details>", ""]
                open_ = False
            # Credits stay visible, as in v0.9.1.
            if lv == at and line[lv + 1:].strip() not in ("Credits", "Thanks"):
                n += 1
                title = CODE.sub(r"<code>\1</code>", line[lv + 1:].strip())
                out += ["<details>", "<summary><b>%d. %s</b></summary>" % (n, title), ""]
                open_ = True
                continue
        out.append(line)
    if open_:
        out += ["", "</details>"]
    return out


def refold(body):
    secs = sections(body)
    notes, folds = [], collections.defaultdict(list)
    for head, lines in secs:
        key = FIXED.get(head) if head is not None else None
        if key is None:
            if head is not None:
                notes.append("## " + head)
            notes.extend(lines)
        elif head.startswith("Prerequisite"):
            folds[key] += ["#### " + head, ""] + lines + [""]
        elif key == "install":
            folds[key] += demote(lines)
        else:
            folds[key] += lines
    if not folds:
        return None
    notes = fold_notes(notes)
    parts = ["\n".join(trim(notes))] if trim(notes) else []
    for key, summary in FOLDS:
        if key in folds:
            parts.append(
                "<details>\n<summary><b>%s</b></summary>\n\n%s\n\n</details>"
                % (summary, "\n".join(trim(folds[key])))
            )
    return "\n\n".join(parts) + "\n"


def signature(body, refolded):
    fixed_heads = {"## " + h for h in FIXED}
    c = collections.Counter()
    for line in body.split("\n"):
        s = line.strip()
        if not s:
            continue
        if refolded:
            if s in ("<details>", "</details>"):
                continue
            m = re.match(r"^<summary><b>(?:\d+\. )?(.*)</b></summary>$", s)
            if m:
                if s.startswith("<summary><b>") and re.match(r"^<summary><b>\d+\. ", s):
                    # A notes section's heading, now the fold's summary.
                    c[re.sub(r"<code>(.*?)</code>", r"`\1`", m.group(1))] += 1
                continue
            if s.startswith("#### Prerequisite"):
                continue
        elif s in fixed_heads:
            continue
        c[s.lstrip("#").strip() if s.startswith("#") else s] += 1
    return c


def retitle(body, tag):
    """Every page opens with "# Bram <tag>", never a "Release notes"
    heading (Jon, 2026-11-26: "standardize on this")."""
    # Pages open with the title, "Release notes", both (in either order),
    # or neither: drop whichever lead them, then put the title first.
    title = "# Bram " + tag
    drop = {title, "## Release notes", "## Release note"}
    lines = body.split("\n")
    def leading(s):
        # An older title ("# Bram 2.3 release notes") is replaced too.
        return s in drop or (s.startswith("# Bram ") and title in drop)

    while lines and (not lines[0].strip() or leading(lines[0].strip())):
        s = lines[0].strip()
        drop.discard(title if s.startswith("# Bram ") else s)
        lines = lines[1:]
    return title + "\n\n" + "\n".join(lines)


def main(src, dst):
    src, dst = pathlib.Path(src), pathlib.Path(dst)
    dst.mkdir(parents=True, exist_ok=True)
    bad = 0
    for f in sorted(src.glob("*.md")):
        body = f.read_text()
        new = refold(body)
        if new is None:
            print("skip\t%s\tno fixed sections" % f.stem)
            continue
        new = retitle(new, f.stem)
        old_sig, new_sig = signature(body, False), signature(new, True)
        for s in ("Release notes", "Release note"):
            old_sig.pop(s, None)
        # The title appears exactly once, whether or not the page had it,
        # in place of any older one.
        for k in [k for k in old_sig if k.startswith("Bram ") and k in new_sig.keys() ^ old_sig.keys()]:
            old_sig.pop(k)
        old_sig["Bram " + f.stem] = 1
        if old_sig != new_sig:
            bad += 1
            print("FAIL\t%s\tlines differ" % f.stem)
            continue
        (dst / f.name).write_text(new)
        print("ok\t%s" % f.stem)
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main(*sys.argv[1:3]))
