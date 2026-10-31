#!/usr/bin/env python3
"""Lay out the instruction-file probes of ../rules-and-transport.md.

Captured against agy 1.4.12 on 2026-10-31. Each probe writes files into a
scratch git repository; run agy there yourself with the prompt and the
session-only --settings given in rules-and-transport.md. Nothing here touches
~/.gemini.

    python3 -I rule-probes.py <scratch> files        # probe 1
    python3 -I rule-probes.py <scratch> truncation   # probe 2
    python3 -I rule-probes.py <scratch> budget <n>   # probe 4, an n-byte user rule
    python3 -I rule-probes.py <scratch> includes     # probe 6

Probes 3 and 4 also need Bram's own always-on set in the scratch repo: copy
.agents/rules/bram-conventions-*.md and AGENTS.md from a project Bram's
Setup has run in.
"""
import os
import random
import sys

FRONTMATTER = "---\ntrigger: always_on\n---\n\n"


def write(root, rel, text):
    path = os.path.join(root, rel)
    os.makedirs(os.path.dirname(path) or root, exist_ok=True)
    with open(path, "w") as f:
        f.write(text)
    print("%-32s %6d bytes" % (rel, len(text.encode())))


def files(root):
    write(root, "AGENTS.md", 'SECRET_AGENTS_MD = "alpha-quokka"\n')
    write(root, "GEMINI.md", 'SECRET_GEMINI_MD = "bravo-lynx"\n')
    write(root, ".agents/rules/test.md",
          FRONTMATTER + 'SECRET_RULE_FRONTMATTER = "charlie-otter"\n')
    write(root, ".agents/rules/bare.md",
          '# a rule with no frontmatter\n\nSECRET_RULE_BARE = "delta-marmot"\n')


def truncation(root):
    # Markers placed by offset into the body (what follows the frontmatter).
    # MARK_B ends past byte 24,000 of the file but within 24,000 of the body.
    body = ""
    n = 0

    def pad_to(off):
        nonlocal body, n
        while len(body) < off:
            line = "padding line %05d %s\n" % (n, "x" * 50)
            if len(body) + len(line) > off:
                line = "p" * (off - len(body) - 1) + "\n"
            body += line
            n += 1

    def mark(name, start=None, end=None):
        nonlocal body
        line = 'SECRET_%s = "%s"\n' % (name, name.lower())
        pad_to(start if start is not None else end - len(line))
        print("%s at body bytes %d-%d" % (name, len(body), len(body) + len(line)))
        body += line

    mark("MARK_A", start=23800)
    mark("MARK_B", end=23990)
    mark("MARK_C", start=24100)
    mark("MARK_D", start=29000)
    pad_to(29500)
    write(root, ".agents/rules/big.md", FRONTMATTER + body)


def budget(root, size):
    # Prose-like filler of varied words, so a tokenizer can't compress runs.
    random.seed(7)
    words = ("the user prefers small commits with tests and clear names before "
             "review keep functions short document public interfaces avoid "
             "global state prefer pure helpers log errors with context never "
             "swallow exceptions").split()
    text = FRONTMATTER + 'SECRET_USER_RULE = "echo-pika"\n\n'
    while len(text) < size:
        text += " ".join(random.choice(words) for _ in range(14)) + ".\n"
    write(root, ".agents/rules/aa-user.md", text[:size])


def includes(root):
    write(root, "inc-rel.txt", 'SECRET_INC_REL = "rel-included"\n')
    write(root, "inc-abs.txt", 'SECRET_INC_ABS = "abs-included"\n')
    write(root, "inc-tick.txt", 'SECRET_INC_TICK = "tick-included"\n')
    write(root, ".agents/rules/inc-ruledir.txt",
          'SECRET_INC_RULEDIR = "ruledir-included"\n')
    write(root, ".agents/rules/inc-dotslash.md",
          'SECRET_INC_DOTSLASH = "dotslash-included"\n')
    abs_path = os.path.join(os.path.abspath(root), "inc-abs.txt")
    write(root, ".agents/rules/inc-test.md", FRONTMATTER +
          'SECRET_RULE_ITSELF = "rule-text"\n\n'
          "Relative mention: @inc-rel.txt\n\n"
          'Inside a curl line: curl -X POST --data @%s "http://127.0.0.1:1/x"\n\n'
          "In backticks: `@inc-tick.txt`\n\n"
          "@inc-ruledir.txt\n\n@./inc-dotslash.md\n\n@../../inc-rel.txt\n" % abs_path)


def main(argv):
    if len(argv) < 3:
        sys.exit(__doc__)
    root, probe = argv[1], argv[2]
    if probe == "files":
        files(root)
    elif probe == "truncation":
        truncation(root)
    elif probe == "budget" and len(argv) == 4:
        budget(root, int(argv[3]))
    elif probe == "includes":
        includes(root)
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main(sys.argv)
