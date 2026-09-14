#!/usr/bin/env python3
"""Measure how long after injection the send ledger's payloadInTail flips true.

Context (judell/bram#372 ask 2): the send ledger's strand verdict currently
keys on ELAPSED TIME (SEND_LEDGER_STRAND_GRACE_MS in src-tauri/src/lib.rs).
Ask 2 wants it to prefer `payload_in_tail` evidence instead -- is the sent
text actually visible in the terminal tail? -- because that is a direct
observation of delivery rather than a timeout guess. That change was
deferred over a suspected race window right after injection: maybe the
predicate is unreliable for the first moment or two, and switching to it
too eagerly would trade false strands-by-timeout for false strands-by-race.

The rotated trace archives (`resources/bram-traces/bram-trace-*.log.gz`)
can NEVER settle this, no matter how many are collected. `payload_in_tail`
was, until this change, only ever computed at strand-VERDICT time -- 60s or
120s after injection -- so nothing in the archives records what the
predicate would have returned at T+200ms. Waiting longer only produces more
LATE samples; it can never produce an EARLY one. This is the same trap
named in docs/developing-bram.md's "Logs cannot prove absence": an
event-shaped log proves presence only, and here the event was never even
emitted at the time in question.

But the question is answerable, because it is deterministic and forward
observable: after injecting a known payload, how long until
`payload_in_tail` first returns true? `src-tauri/src/lib.rs`'s
`/__send-ledger` route now computes `payloadInTail` /
`payloadInTailBasis` live on every serve (previously computed only inside
the strand-verdict branch), which is what makes the predicate samplable
from outside at all. This script polls that route at a fixed short
interval and, for each ledger entry, measures the elapsed time between
`injectedAtMs` (host-recorded, ms) and the first poll where `payloadInTail`
is observed true.

WHAT THIS SCRIPT DOES NOT DO: it does not inject turns. Driving the PTY
from a script is out of scope here and risky (see the reentrant-hook and
sandbox cautions elsewhere in this repo) -- an operator sends turns by
hand in the Bram terminal (or a `scripts/demo-instance.sh` instance is
driven by hand) while this script watches passively from the side, over
the loopback HTTP route only.

Reading a good vs. a bad result:

  - FAST AND DETERMINISTIC (e.g. settle times tightly clustered within one
    or two poll intervals, no long tail, no left-censored samples where the
    predicate was already true the first time an entry was seen) supports
    promoting `payload_in_tail` to the PRIMARY strand test, with elapsed
    time demoted to a backstop for the case the predicate never resolves.
  - SLOW OR JITTERY (settle times spread over seconds, a heavy tail, or a
    basis that flaps between true/false/echo-only across consecutive
    polls) means the deferral was the right call: the race window is real,
    and shipping ask 2 as originally scoped would trade timeout-shaped
    false strands for race-shaped ones instead of removing the class.

Degrades honestly against an old binary: `/__send-ledger` entries served
by a Bram build that predates this change simply have no `payloadInTail`
key. This script detects that on the first entry it sees and says so
plainly -- it never reports a silent zero as if it were a real
measurement.

Standard library only. Read-only: this script writes nothing anywhere,
including no requests to the server beyond plain GETs.
"""

from __future__ import annotations

import argparse
import json
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

DEFAULT_PORT_FILE = Path("resources/.bram-port")
DEFAULT_INTERVAL_S = 0.1


class ProbeFailure(Exception):
    pass


def ledger_endpoint(port: int | None, host: str, port_file: Path) -> str:
    if port is None:
        try:
            text = port_file.read_text(encoding="utf-8").strip()
        except OSError as error:
            raise ProbeFailure(
                f"cannot read Bram port file {port_file}: {error}"
            ) from error
        if not text.isdigit():
            raise ProbeFailure(f"invalid Bram port in {port_file}: {text!r}")
        port = int(text)
    return f"http://{host}:{port}/__send-ledger"


def fetch_ledger(url: str, opener=urllib.request.urlopen, timeout: float = 2.0) -> dict:
    try:
        with opener(url, timeout=timeout) as response:
            payload = json.loads(response.read().decode("utf-8"))
    except urllib.error.URLError as error:
        raise ProbeFailure(f"request to {url} failed: {error}") from error
    except json.JSONDecodeError as error:
        raise ProbeFailure(f"non-JSON response from {url}: {error}") from error
    if not isinstance(payload, dict) or "entries" not in payload:
        raise ProbeFailure(f"unexpected /__send-ledger shape from {url}")
    return payload


def percentile(sorted_values: list[float], pct: float) -> float:
    """Nearest-rank percentile over an already-sorted list."""
    if not sorted_values:
        return 0.0
    if len(sorted_values) == 1:
        return sorted_values[0]
    rank = pct / 100.0 * (len(sorted_values) - 1)
    lo = int(rank)
    hi = min(lo + 1, len(sorted_values) - 1)
    frac = rank - lo
    return sorted_values[lo] + (sorted_values[hi] - sorted_values[lo]) * frac


def distribution(values: list[float]) -> dict:
    if not values:
        return {"count": 0}
    s = sorted(values)
    return {
        "count": len(s),
        "min_ms": round(s[0], 1),
        "p50_ms": round(percentile(s, 50), 1),
        "p90_ms": round(percentile(s, 90), 1),
        "p95_ms": round(percentile(s, 95), 1),
        "max_ms": round(s[-1], 1),
    }


class Watcher:
    """Tracks each send-ledger entry (by id) across successive polls."""

    def __init__(self):
        # id -> {"injected_at_ms": int, "settled": bool}
        self.tracked: dict[str, dict] = {}
        self.measured_ms: list[float] = []
        self.left_censored = 0  # payloadInTail already true the first time we saw it
        self.basis_counts: dict[str, int] = {}
        self.field_supported: bool | None = None  # None = not yet known
        self.polls = 0
        self.entries_seen: set[str] = set()

    def observe(self, payload: dict) -> None:
        self.polls += 1
        now_ms = payload.get("nowMs")
        for entry in payload.get("entries", []):
            entry_id = entry.get("id")
            if not entry_id:
                continue
            self.entries_seen.add(entry_id)
            has_field = "payloadInTail" in entry
            if self.field_supported is None:
                self.field_supported = has_field
            if not has_field:
                continue
            in_tail = bool(entry.get("payloadInTail"))
            basis = entry.get("payloadInTailBasis", "?")
            state = self.tracked.get(entry_id)
            if state is None:
                state = {
                    "injected_at_ms": entry.get("injectedAtMs"),
                    "settled": False,
                }
                self.tracked[entry_id] = state
                if in_tail:
                    # First time we've ever seen this id, and it is already
                    # true -- we cannot know when it actually flipped, only
                    # that it was at-or-before our first observation.
                    self.left_censored += 1
                    state["settled"] = True
            if in_tail:
                self.basis_counts[basis] = self.basis_counts.get(basis, 0) + 1
            if in_tail and not state["settled"]:
                state["settled"] = True
                injected = state.get("injected_at_ms")
                if isinstance(injected, (int, float)) and isinstance(
                    now_ms, (int, float)
                ):
                    self.measured_ms.append(float(now_ms - injected))

    def report(self) -> dict:
        unsettled = sum(
            1 for s in self.tracked.values() if not s["settled"]
        )
        return {
            "field_supported": self.field_supported,
            "polls": self.polls,
            "entries_seen": len(self.entries_seen),
            "measured": distribution(self.measured_ms),
            "left_censored": self.left_censored,
            "unsettled_at_exit": unsettled,
            "basis_seen": dict(sorted(self.basis_counts.items())),
        }


def run(
    url: str,
    interval_s: float,
    watch_s: float | None,
    opener=urllib.request.urlopen,
    sleeper=time.sleep,
    clock=time.monotonic,
) -> Watcher:
    watcher = Watcher()
    start = clock()
    try:
        while True:
            try:
                payload = fetch_ledger(url, opener=opener)
            except ProbeFailure as error:
                print(f"    poll failed: {error}", file=sys.stderr)
            else:
                watcher.observe(payload)
            if watch_s is not None and clock() - start >= watch_s:
                break
            sleeper(interval_s)
    except KeyboardInterrupt:
        # Passive (unbounded) mode is stopped this way by design -- report
        # whatever was accumulated rather than discarding it.
        print(file=sys.stderr)  # move off the ^C line
    return watcher


def print_report(result: dict, url: str, interval_s: float, watch_s: float | None) -> None:
    print(f"endpoint       {url}")
    print(f"poll interval  {interval_s * 1000:.0f}ms")
    print(f"watch window   {watch_s}s" if watch_s is not None else "watch window   unbounded (Ctrl-C to stop)")
    print(f"polls made     {result['polls']}")
    print(f"entries seen   {result['entries_seen']}")
    print()
    if result["field_supported"] is False:
        print(
            "payloadInTail NOT PRESENT on any entry served -- this Bram binary"
        )
        print(
            "predates the field (issue-372-quantile-grace-and-probe). No settle"
        )
        print(
            "measurement is possible against this instance; rebuild and relaunch"
        )
        print("./bram, then re-run this probe.")
        return
    if result["field_supported"] is None:
        print("payloadInTail: unknown -- no ledger entries were served during")
        print("this run, so nothing could be observed either way. Send a turn")
        print("in Bram (or widen --watch) and try again.")
        return
    dist = result["measured"]
    if dist["count"] == 0:
        print("payloadInTail is present on served entries, but none of the")
        print("entries observed transitioned from false to true during this")
        print("run -- no settle time could be measured.")
    else:
        print(f"settle time (injectedAtMs -> first payloadInTail=true), ms:")
        print(
            f"    count {dist['count']}  min {dist['min_ms']}  p50 {dist['p50_ms']}"
            f"  p90 {dist['p90_ms']}  p95 {dist['p95_ms']}  max {dist['max_ms']}"
        )
    if result["left_censored"]:
        print(
            f"left-censored  {result['left_censored']}  (payloadInTail was already"
        )
        print(
            "               true the first poll that saw the entry -- true onset"
        )
        print("               is unknown, only that it was at-or-before that poll)")
    if result["unsettled_at_exit"]:
        print(
            f"unsettled      {result['unsettled_at_exit']}  (never observed"
            " payloadInTail=true before the run ended)"
        )
    print(f"basis values seen (on true observations): {result['basis_seen']}")


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument(
        "--project-root",
        type=Path,
        default=Path.cwd(),
        help="project whose resources/.bram-port to read (default: cwd)",
    )
    ap.add_argument("--port-file", type=Path, help="override the port file path")
    ap.add_argument("--port", type=int, help="explicit port, skips reading the port file")
    ap.add_argument("--host", default="127.0.0.1", help="Bram binds IPv4 only (default: 127.0.0.1)")
    ap.add_argument(
        "--interval",
        type=float,
        default=DEFAULT_INTERVAL_S,
        help=f"poll interval in seconds (default: {DEFAULT_INTERVAL_S})",
    )
    ap.add_argument(
        "--watch",
        type=float,
        default=None,
        help="observe for this many seconds, then report and exit "
        "(default: run until Ctrl-C)",
    )
    ap.add_argument("--json", action="store_true", help="emit JSON instead of text")
    args = ap.parse_args(argv)

    root = args.project_root.expanduser().resolve()
    port_file = (args.port_file or (root / DEFAULT_PORT_FILE)).expanduser()
    try:
        url = ledger_endpoint(args.port, args.host, port_file)
    except ProbeFailure as error:
        print(str(error), file=sys.stderr)
        return 2

    watcher = run(url, args.interval, args.watch)
    result = watcher.report()
    if args.json:
        print(json.dumps(result, indent=2))
    else:
        print_report(result, url, args.interval, args.watch)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
