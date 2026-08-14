"""Provider hooks must read files as UTF-8, not the platform ANSI codepage.

Python's default text encoding is locale-dependent: UTF-8 on macOS/Linux,
cp1252 on Windows. A bare open() therefore works everywhere the developers
run it and crashes on Windows the moment a file holds a non-ASCII byte.

That is not hypothetical. The Claude worklist guard read the session
transcript with a bare open(); transcripts are reliably non-ASCII, so on
Windows the guard raised UnicodeDecodeError and exited 1 on every
invocation -- before reaching any trace call or decision. Exit 1 is not the
documented "2 = block", so Claude Code treated it as a failed hook and
proceeded. The guard was registered, reported healthy, and never once
enforced. See judell/bram#249.

The subprocess tests below cover the specific paths that bit us; the
source-scan test is what keeps the whole class from returning.
"""

import ast
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


HOOKS_DIR = Path(__file__).parents[2] / "app" / "provider-hooks"
GUARD = HOOKS_DIR / "claude-worklist-guard.py"

# Non-ASCII that a real session or a real worklist draft would contain. The
# em-dash and arrow are what our own conventions ask agents to write in draft
# prose; 0x90 is the byte from the original Windows traceback.
SPICY = "prose with an em-dash — an arrow → a curly quote ’ and emoji \U0001f600"


def run_guard(payload, cwd):
    """Run the guard on a payload; return (exit_code, stdout, stderr)."""
    proc = subprocess.run(
        [sys.executable, str(GUARD)],
        input=json.dumps(payload),
        capture_output=True,
        text=True,
        cwd=cwd,
    )
    return proc.returncode, proc.stdout, proc.stderr


class SourceScan(unittest.TestCase):
    """Every open() in the provider hooks names an encoding.

    This is the durable half of the fix. The individual call sites are
    one-line changes that are just as easy to reintroduce as they were to
    make, and the failure is silent on the platforms most of us develop on.
    """

    def test_no_bare_open_in_provider_hooks(self):
        # Parsed rather than grepped: a regex over lines both misses calls
        # split across lines and fires on "open(" inside string literals
        # (the guard carries sample command text that contains one).
        offenders = []
        for path in sorted(HOOKS_DIR.glob("*.py")):
            tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
            for node in ast.walk(tree):
                if not isinstance(node, ast.Call):
                    continue
                func = node.func
                if isinstance(func, ast.Name):
                    name = func.id
                elif isinstance(func, ast.Attribute):
                    name = func.attr
                else:
                    continue
                if name not in ("open", "fdopen"):
                    continue
                if any(kw.arg == "encoding" for kw in node.keywords):
                    continue
                offenders.append("%s:%d: %s(...)" % (path.name, node.lineno, name))
        self.assertEqual(
            offenders,
            [],
            "bare open() in provider hooks (locale-dependent decoding):\n"
            + "\n".join(offenders),
        )


class GuardSurvivesNonAscii(unittest.TestCase):
    """The guard must reach a decision rather than dying on a decode."""

    def setUp(self):
        self.tmp = tempfile.mkdtemp(prefix="bram-hook-encoding-")
        os.makedirs(os.path.join(self.tmp, "resources"), exist_ok=True)
        # Managed-repo marker: without it the guard exits 0 unconditionally
        # and every assertion below would pass vacuously.
        with open(
            os.path.join(self.tmp, "resources", ".worklist-authorization.json"),
            "w",
            encoding="utf-8",
        ) as f:
            json.dump({"kind": "none", "ids": [], "items": []}, f)

    def write_worklist(self, text):
        with open(
            os.path.join(self.tmp, "resources", "worklist.json"), "w", encoding="utf-8"
        ) as f:
            json.dump(
                {"description": text, "items": [], "version": 1}, f, ensure_ascii=False
            )

    def transcript(self, name, lines):
        path = os.path.join(self.tmp, name)
        with open(path, "w", encoding="utf-8") as f:
            for line in lines:
                f.write(json.dumps(line, ensure_ascii=False) + "\n")
        return path

    def payload(self, transcript_path):
        return {
            "hook_event_name": "PreToolUse",
            "tool_name": "Write",
            "tool_input": {
                "file_path": os.path.join(self.tmp, "unauthorized.txt"),
                "content": "x",
            },
            "cwd": self.tmp,
            "transcript_path": transcript_path,
            "session_id": "test",
        }

    def assert_decided(self, code, stderr):
        # 0 = allow, 2 = block. Anything else means the guard failed rather
        # than decided -- which is the bug, because a failed hook is
        # non-blocking and the write goes through.
        self.assertIn(
            code,
            (0, 2),
            "guard did not reach a decision (exit %s):\n%s" % (code, stderr),
        )
        self.assertNotIn("UnicodeDecodeError", stderr)
        self.assertNotIn("Traceback", stderr)

    def test_non_ascii_transcript(self):
        self.write_worklist("")
        tp = self.transcript(
            "t.jsonl",
            [{"type": "user", "message": {"role": "user", "content": SPICY}}],
        )
        code, _, stderr = run_guard(self.payload(tp), self.tmp)
        self.assert_decided(code, stderr)

    def test_undecodable_transcript_bytes(self):
        """Even a corrupt transcript must not take the guard down.

        It is untrusted input read only for opt-out phrase matching, so it
        is opened with errors="replace".
        """
        self.write_worklist("")
        path = os.path.join(self.tmp, "bad.jsonl")
        with open(path, "wb") as f:
            f.write(b'{"type":"user","message":{"role":"user","content":"')
            f.write(b"\x90\xff\xfe")
            f.write(b'"}}\n')
        code, _, stderr = run_guard(self.payload(path), self.tmp)
        self.assert_decided(code, stderr)

    def test_non_ascii_worklist_json(self):
        """worklist.json is our own file, and drafts are full of em-dashes."""
        self.write_worklist(SPICY)
        tp = self.transcript(
            "t.jsonl", [{"type": "user", "message": {"role": "user", "content": "hi"}}]
        )
        code, _, stderr = run_guard(self.payload(tp), self.tmp)
        self.assert_decided(code, stderr)


class GuardFailsClosed(unittest.TestCase):
    """A crashing guard must block, not allow.

    The encoding fix removes one crash; it does not remove the class. Any
    unhandled exception in a PreToolUse guard exits non-zero-but-not-2,
    which both providers read as "hook failed, proceed" -- an enforcement
    bypass that leaves no receipt, because the crash lands upstream of the
    guard's own instrumentation. #249 is the existence proof that such a
    hole goes unnoticed indefinitely.
    """

    def test_claude_guard_denies_on_unhandled_exception(self):
        # Unparseable stdin makes json.load raise inside main(); the Claude
        # guard has no internal handler for it, so it exercises the
        # top-level fail-closed path.
        proc = subprocess.run(
            [sys.executable, str(GUARD)],
            input="not json",
            capture_output=True,
            text=True,
        )
        self.assertEqual(proc.returncode, 2, proc.stderr)
        self.assertIn("decision=error", proc.stderr)
        self.assertIn("denied by default", proc.stderr)
        # The message must say this is a guard bug, so a user hitting it
        # reports it instead of assuming they tripped a policy.
        self.assertIn("guard bug", proc.stderr)


if __name__ == "__main__":
    unittest.main()
