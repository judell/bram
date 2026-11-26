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
            if s in ("<details>", "</details>") or s.startswith("<summary><b>"):
                continue
            if s.startswith("#### Prerequisite"):
                continue
        elif s in fixed_heads:
            continue
        c[s.lstrip("#").strip() if s.startswith("#") else s] += 1
    return c


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
        if signature(body, False) != signature(new, True):
            bad += 1
            print("FAIL\t%s\tlines differ" % f.stem)
            continue
        (dst / f.name).write_text(new)
        print("ok\t%s" % f.stem)
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main(*sys.argv[1:3]))
