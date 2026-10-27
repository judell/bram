#!/usr/bin/env python3
"""Drive the session-switching matrix in a demo instance and check its trace.

docs/decisions/2026-10-27-bram-chooses-the-session.md states the rule (Bram
chooses the session, launches it by id, marks it current at launch, and pairs
only through an explicit act tied to that id) and the matrix that checks it.
This runs the matrix against a real Bram on the demo repo, with the real agent
CLIs, through the BRAM_TEST_DRIVE routes (switch-matrix-harness), and asserts on
the trace lines a person read by hand on 2026-10-27.

Cases run in order and build on each other's sessions. Exit status is the number
of failed checks. On failure the demo is left running for a look; stop it with
scripts/demo-instance.sh stop.

Cases that deliver a brief or catch-up start a real model turn. --no-turns runs
only the cases that don't. Needs a windowing session, so on demand, not in CI.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPTS))

import importlib.util  # noqa: E402

_spec = importlib.util.spec_from_file_location("msc", SCRIPTS / "membership-starter-check.py")
msc = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(msc)  # reuse run() and trace_lines()

def trace_lines(repo: Path) -> list[str]:
    """The trace the instance is writing. Observed 2026-10-27 22:37Z: after a
    launch the instance kept writing to a rotated name (bram-trace-<date>-N.log)
    while bram-trace.log stayed empty, so read the most recently written one."""
    files = [p for p in (repo / "resources/bram-traces").glob("bram-trace*.log") if p.is_file()]
    if not files:
        return []
    newest = max(files, key=lambda p: p.stat().st_mtime)
    return newest.read_text(errors="replace").splitlines()


msc.trace_lines = trace_lines


# Lines that must never appear during a run (each was a 2026-10-27 break).
NEVER = re.compile(
    r"\[agent-switch\] op=launch-timeout"
    r"|\[agent-switch\] op=launch-exited"
    r"|\[send-gate\] op=boot-held-restore"
    r"|\[session-new\] op=rebound"
)


class Run:
    def __init__(self, repo: Path, timeout: float):
        self.repo = repo
        self.timeout = timeout
        self.failures: list[str] = []
        self.results: list[tuple[str, str, str]] = []
        self.start = 0

    # --- transport -------------------------------------------------------
    def url(self, path: str) -> str:
        # Reread every call: right after a launch the file can still hold the
        # previous instance's port.
        port = (self.repo / "resources/.bram-port").read_text().strip()
        return f"http://127.0.0.1:{port}/{path}"

    def _send(self, make) -> tuple[int, str]:
        for _ in range(20):
            try:
                with urllib.request.urlopen(make(), timeout=30) as r:
                    return r.status, r.read().decode()
            except urllib.error.HTTPError as e:
                return e.code, e.read().decode()
            except urllib.error.URLError:
                time.sleep(0.5)  # not listening yet, or a stale port file
        return 0, "connection refused"

    def post(self, route: str, body: dict) -> tuple[int, str]:
        return self._send(lambda: urllib.request.Request(
            self.url(route), data=json.dumps(body).encode(), method="POST",
            headers={"Content-Type": "application/json"},
        ))

    def get(self, path: str) -> tuple[int, str]:
        return self._send(lambda: urllib.request.Request(self.url(path)))

    def sessions(self, provider: str) -> list[dict]:
        code, body = self.get(f"__sessions/list?provider={provider}")
        data = json.loads(body) if code == 200 else []
        rows = data if isinstance(data, list) else data.get("sessions", [])
        # A New session's placeholder row (pending, no file yet) is not a session
        # Bram can resume or delete; on 2026-10-27 it read as "newest" and "current".
        return [row for row in rows if not row.get("pending")]

    # --- trace -----------------------------------------------------------
    def mark(self) -> int:
        return len(msc.trace_lines(self.repo))

    def wait(self, since: int, pattern: str, timeout: float | None = None) -> re.Match | None:
        rx = re.compile(pattern)
        deadline = time.time() + (timeout or self.timeout)
        while time.time() < deadline:
            lines = msc.trace_lines(self.repo)
            if len(lines) < since:  # rotated to a new, shorter file
                since = 0
            for line in lines[since:]:
                m = rx.search(line)
                if m:
                    return m
            time.sleep(0.5)
        return None

    def act(self, route: str, body: dict, provider: str) -> int:
        """POST a test-drive action and wait until its launch settles, as a
        person would before the next click (a switch 0.3 s after a reload ran
        before the reload had launched, 2026-10-27 22:39Z). Returns the mark
        taken before the action."""
        m = self.mark()
        self.post(route, body)
        self.wait(m, rf"op=launch-(confirmed|exited|timeout) provider={provider}")
        return m

    # --- bookkeeping -----------------------------------------------------
    def case(self, name: str, checks: list[tuple[str, bool]]) -> bool:
        bad = [what for what, ok in checks if not ok]
        self.results.append((name, "FAIL" if bad else "ok", "; ".join(bad)))
        self.failures.extend(f"{name}: {b}" for b in bad)
        return not bad

    def skip(self, name: str, why: str) -> None:
        self.results.append((name, "skip", why))


def matrix(r: Run, turns: bool, agents: list[str]) -> None:
    stamp = time.strftime("%H%M%S")
    title = f"matrix {stamp}"

    # switch-to-claude: a plain switch launches and confirms.
    m0 = r.mark()
    code, _ = r.post("__test/switch", {"provider": "claude"})
    r.case("switch-to-claude", [
        (f"POST returned {code}", code == 200),
        ("claude launch-confirmed", bool(r.wait(m0, r"op=launch-confirmed provider=claude"))),
    ])

    a = b = None
    if turns:
        # new-session: named on its reserved id, brief released on the composer.
        m = r.mark()
        r.post("__test/new-session", {"provider": "claude", "title": title})
        # The dialog's brief comes from the pane; send a short one the same way.
        r.post("__test/turn", {"text": f"Switch-matrix brief for '{title}'. Reply with just: ok"})
        named = r.wait(m, rf"\[session-new\] op=named provider=claude sid=(\S+) title={re.escape(title)}$")
        a = named.group(1) if named else None
        r.case("new-session-claude", [
            ("named on its id", bool(named)),
            ("boot-settled rule=composer", bool(r.wait(m, r"op=boot-settled rule=composer"))),
            ("brief flushed", bool(r.wait(m, r"\[send-gate\] op=flush wrote=1"))),
        ])
        # Let New session's own launch settle before the next switch.
        r.wait(m, r"op=launch-(confirmed|exited|timeout|superseded) provider=claude source=new-session")

        # fresh partners: a New-session-born session starts a named, paired partner.
        for p, label in (("opencode", "OpenCode"), ("codex", "Codex")):
            if p not in agents:
                r.skip(f"fresh-partner-{p}", "agent not selected")
                continue
            if not a:
                r.skip(f"fresh-partner-{p}", "no session from new-session-claude")
                continue
            m = r.mark()
            r.post("__test/switch", {"provider": p})
            fresh = r.wait(m, rf"op=fresh-partner provider={p} .*from={a}")
            paired = r.wait(m, rf"op=pair-stamp claude={a} {p}=(\S+) source=switch-new-session-born", self_timeout(r))
            pid = paired.group(1) if paired else None
            named = r.wait(m, rf"op=named provider={p} sid={pid} title={re.escape(title)} \({label}\)") if pid else None
            r.case(f"fresh-partner-{p}", [
                ("fresh partner started", bool(fresh)),
                ("paired with the claude session", bool(paired)),
                ("named after it", bool(named)),
            ])
            if p == "opencode":
                b = pid
            # partner-resume back to claude.
            m = r.act("__test/switch", {"provider": "claude"}, "claude")
            r.case(f"partner-resume-from-{p}", [
                ("resumed the partner by id",
                 bool(a and r.wait(m, rf"op=start provider=claude .*resume_sid={a} resume_reason=partner"))),
            ])
    else:
        r.skip("new-session-claude", "--no-turns")
        r.skip("fresh-partners", "--no-turns")

    # opencode partner marked current at launch.
    if b:
        m = r.act("__test/switch", {"provider": "opencode"}, "opencode")
        r.case("opencode-partner-current", [
            ("resumed by id", bool(r.wait(m, rf"op=start provider=opencode .*resume_sid={b} resume_reason=partner"))),
            ("marked current at launch", bool(r.wait(m, rf"op=live-session-set provider=opencode session={b} .*via=switch"))),
        ])
    else:
        r.skip("opencode-partner-current", "no opencode partner")

    # fallback: from an unpaired OpenCode session, Claude resumes its newest by id
    # and the offer names that id.
    if "opencode" in agents:
        # A cross-agent reload pairs on purpose ("an explicit resume ... re-pairs
        # with the session being left", provider-session-counterparts), so the
        # unpaired session is reloaded from inside OpenCode and then unpaired.
        others = [s for s in r.sessions("opencode") if s.get("id") != b]
        if others:
            u = others[0]["id"]
            r.act("__test/switch", {"provider": "opencode"}, "opencode")
            m = r.act("__test/reload", {"provider": "opencode", "id": u}, "opencode")
            current = r.wait(m, rf"op=live-session-set provider=opencode session={re.escape(u)} .*via=reload")
            r.get(f"__sessions/unpair?provider=opencode&id={u}")
            newest = (r.sessions("claude") or [{}])[0].get("id")
            m2 = r.act("__test/switch", {"provider": "claude"}, "claude")
            offered = r.wait(m2, r"op=pair-offer active=true", 15)
            r.case("fallback-by-id", [
                ("reload marked current", bool(current)),
                ("claude resumed its newest by id",
                 bool(newest and r.wait(m2, rf"op=start provider=claude .*resume_sid={newest} resume_reason=latest"))),
                ("offer shown", bool(offered)),
            ])
            # reload clears the offer: only meaningful when one was up.
            if offered:
                m3 = r.mark()
                r.post("__test/reload", {"provider": "claude", "id": a or newest})
                cleared = r.wait(m3, r"op=reload provider=claude[\s\S]*")
                r.case("reload-clears-offer", [
                    ("offer cleared after reload", bool(cleared and r.wait(m3, r"op=pair-offer active=false"))),
                ])
                r.wait(m3, r"op=launch-(confirmed|exited|timeout) provider=claude")
            else:
                r.skip("reload-clears-offer", "no offer was shown")
        else:
            r.skip("fallback-by-id", "no OpenCode session in the demo repo")

    # delete: the running session is refused; a paired one ends its pairs.
    live = next((s for s in r.sessions("claude") if s.get("current")), None)
    if live:
        code, _ = r.get(f"__sessions/delete?id={live['id']}&provider=claude")
        r.case("delete-running-refused", [(f"HTTP {code}, expected 409", code == 409)])
    else:
        r.skip("delete-running-refused", "no current claude session")
    if b:
        m = r.mark()
        code, _ = r.get(f"__sessions/delete?id={b}&provider=opencode")
        r.case("delete-ends-pairs", [
            (f"HTTP {code}, expected 200", code == 200),
            ("opencode deleted it", bool(r.wait(m, rf"op=opencode-delete session={b} status=0"))),
            ("pair ended", bool(r.wait(m, rf"op=pair-stamp opencode={b} claude= source=delete"))),
        ])
    else:
        r.skip("delete-ends-pairs", "no opencode partner to delete")


def self_timeout(r: Run) -> float:
    # A fresh partner pairs only once its first message surfaces the session.
    return max(r.timeout, 120)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--repo", default=str(SCRIPTS.parent.parent / "bram-demo-instance"))
    ap.add_argument("--timeout", type=float, default=90)
    ap.add_argument("--no-turns", action="store_true", help="skip cases that start a model turn")
    ap.add_argument("--agents", default="claude,opencode,codex",
                    help="comma list; claude is always used (default: all three)")
    ap.add_argument("--keep", action="store_true", help="leave the demo running even on success")
    args = ap.parse_args()
    repo = Path(args.repo).resolve()
    agents = [a.strip() for a in args.agents.split(",") if a.strip()]

    os.environ["BRAM_TEST_DRIVE"] = "1"
    msc.run(repo, "stop", tolerate="no recorded demo process")
    r = Run(repo, args.timeout)
    launched_at = time.strftime("%Y-%m-%dT%H:%M:%S", time.gmtime(time.time() - 2))
    msc.run(repo, "launch")
    # Anchor on this instance's own session-start line. Lines from an earlier
    # run in the same file once satisfied every wait before this instance's
    # terminal existed ("pty not started", 2026-10-27 22:54Z).
    deadline = time.time() + 60
    r.start = -1
    while time.time() < deadline and r.start < 0:
        lines = msc.trace_lines(repo)
        starts = [i for i, l in enumerate(lines)
                  if "event=session-start" in l and l[1:20] >= launched_at]
        if starts:
            r.start = starts[-1]
        else:
            time.sleep(0.5)
    if r.start < 0 or not r.wait(r.start, r"\[test-drive\] op=enabled", 30):
        print("FAIL  test-drive never enabled (is the binary older than switch-matrix-harness?)")
        return 1
    # Let autostart settle before driving.
    r.wait(r.start, r"op=launch-(confirmed|exited|timeout) provider=\S+ source=autostart", 90)

    matrix(r, not args.no_turns, agents)

    never = sorted({line.split("] ", 2)[-1][:120] for line in msc.trace_lines(repo)[r.start:]
                    if NEVER.search(line)})
    r.case("no-failure-lines", [(f"saw: {n}", False) for n in never])

    width = max(len(n) for n, _, _ in r.results)
    for name, status, why in r.results:
        print(f"{status:<4}  {name.ljust(width)}  {why}")
    print(f"\n{len(r.failures)} failed check(s)")
    if r.failures or args.keep:
        print("demo left running; stop with scripts/demo-instance.sh stop")
    else:
        msc.run(repo, "stop")
    return len(r.failures)


if __name__ == "__main__":
    sys.exit(main())
