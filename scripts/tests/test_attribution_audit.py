import contextlib
import gzip
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "attribution_audit.py"
SPEC = importlib.util.spec_from_file_location("attribution_audit", SCRIPT)
assert SPEC and SPEC.loader
audit = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(audit)


def git(repo: Path, *args: str, env: dict | None = None) -> str:
    return subprocess.run(
        ["git", "-C", str(repo), *args],
        check=True,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env=env,
    ).stdout.strip()


def run_cli(*argv: str) -> dict:
    out = io.StringIO()
    with contextlib.redirect_stdout(out):
        audit.main([*argv, "--json"])
    return json.loads(out.getvalue())


def ts(minute: int, second: int = 0) -> str:
    return f"2026-09-26T10:{minute:02d}:{second:02d}.000Z"


class GateParsingTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory(prefix="attribution-audit-test-")
        self.dir = Path(self.temp.name)

    def tearDown(self) -> None:
        self.temp.cleanup()

    def test_overlapping_rotations_and_truncated_gzip(self) -> None:
        g1 = [
            f"[{ts(0)}] [claim-interval] op=capture ref=refs/bram/claims/1 tree=aaaaaaaaaaaa kind=approved ids=1",
            f"[{ts(0, 1)}] [inflight-sentinel] op=write kind=approved ids=[\"A\"]",
            f"[{ts(1)}] [claim-interval] op=gate-membership-diverges ids=A path=p1 replay=A,B membership=A",
            f"[{ts(1)}] [claim-interval] op=gate-membership-diverges ids=A path=p2 replay=A membership=-",
            f"[{ts(1)}] [claim-interval] op=gate-membership-diverges ids=A path=p3 replay=- membership=A",
            f"[{ts(1)}] [claim-interval] op=gate-membership ids=1 paths=3 ms=40 fresh=true agrees=false",
            f"[{ts(1, 2)}] [worklist-commit] op=interval-staged sha=abc1234 files=3 ids=1",
            f"[{ts(1, 3)}] [terminal-attention] op=candidate preview=\"op=gate-membership ms=1 agrees=true\"",
        ]
        g2 = [
            f"[{ts(2)}] [claim-interval] op=gate-membership ids=1 paths=1 ms=7 fresh=true agrees=true",
            f"[{ts(2, 1)}] [worklist-commit] op=refuse-joint-interval path=p1",
        ]
        g3 = [
            f"[{ts(3)}] [claim-interval] op=gate-membership-unavailable ids=1 ms=3",
            "[2026-09-26T10:09:00.000Z] [noise] " + "x" * 400,
        ]
        (self.dir / "bram-trace-1.log").write_text("\n".join(g1 + g2[:1]) + "\n")
        # The second rotation overlaps the first (g1 again, plus g2) and
        # lists lines out of order.
        (self.dir / "bram-trace-2.log").write_text("\n".join(g2 + g1[2:]) + "\n")
        blob = gzip.compress(("\n".join(g3) + "\n").encode())
        (self.dir / "bram-trace-3.log.gz").write_bytes(blob[:-10])

        result = run_cli("gate", "--trace-dir", str(self.dir))
        rows = result["rows"]
        self.assertEqual(len(rows), 3)
        first, second, third = rows
        self.assertEqual(first["ids"], ["A"])
        self.assertFalse(first["agrees"])
        self.assertEqual(first["outcome"], "abc1234")
        self.assertEqual(
            {d["path"]: d["classes"] for d in first["divergences"]},
            {
                "p1": ["outside-named"],
                "p2": ["committer-uncredited"],
                "p3": ["membership-extra"],
            },
        )
        self.assertTrue(second["agrees"])
        self.assertEqual(second["ids"], ["A"])  # holder of the last claim capture
        self.assertEqual(second["outcome"], "refused:refuse-joint-interval")
        self.assertIsNone(third["agrees"])
        self.assertEqual(third["outcome"], "unknown")
        summary = result["summary"]
        self.assertEqual(summary["attempts"], 3)
        self.assertEqual(
            summary["agreement"], {"agreed": 1, "disagreed": 1, "unavailable": 1}
        )
        self.assertEqual(
            summary["classes"],
            {"outside-named": 1, "committer-uncredited": 1, "membership-extra": 1},
        )
        self.assertEqual(
            summary["outcomes"], {"committed": 1, "refused": 1, "unknown": 1}
        )

    def test_since_and_until_filter_rows_not_history(self) -> None:
        lines = [
            f"[{ts(0)}] [claim-interval] op=capture ref=r tree=aaaaaaaaaaaa kind=approved ids=1",
            f"[{ts(0, 1)}] [inflight-sentinel] op=write kind=approved ids=[\"A\"]",
            f"[{ts(5)}] [claim-interval] op=gate-membership ids=1 paths=1 ms=1 fresh=true agrees=true",
        ]
        (self.dir / "bram-trace.log").write_text("\n".join(lines) + "\n")
        got = run_cli("gate", "--trace-dir", str(self.dir), "--since", "2026-09-26T10:03")
        self.assertEqual([r["ids"] for r in got["rows"]], [["A"]])
        got = run_cli("gate", "--trace-dir", str(self.dir), "--until", "2026-09-26T10:03")
        self.assertEqual(got["rows"], [])


class LineRuleTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory(prefix="attribution-audit-test-")
        base = Path(self.temp.name)
        self.repo = base / "repo"
        self.trace_dir = base / "traces"
        self.scratch = base / "scratch"
        for d in (self.repo, self.trace_dir, self.scratch):
            d.mkdir()
        git(self.repo, "init", "-q")
        git(self.repo, "config", "user.name", "t")
        git(self.repo, "config", "user.email", "t@example.com")
        self.file = self.repo / "f.txt"
        self.base_lines = [f"l{i:02d}" for i in range(1, 21)]
        self.write(self.base_lines)
        git(self.repo, "add", "-A")
        git(self.repo, "commit", "-q", "-m", "base")
        self.build()

    def tearDown(self) -> None:
        self.temp.cleanup()

    def write(self, lines: list[str]) -> None:
        self.file.write_text("\n".join(lines) + "\n")

    def capture(self) -> str:
        env = dict(os.environ, GIT_INDEX_FILE=str(self.scratch / "idx"))
        git(self.repo, "read-tree", "HEAD", env=env)
        git(self.repo, "add", "-A", env=env)
        tree = git(self.repo, "write-tree", env=env)
        (self.scratch / "idx").unlink()
        return tree[:12]

    def build(self) -> None:
        lines = list(self.base_lines)
        lines[19] = "pre20"  # work before the committer's first claim
        self.write(lines)
        t0 = self.capture()  # X claims
        lines[1] = "x02"  # X's first window
        self.write(lines)
        t1 = self.capture()  # cleared
        lines[1] = "u02"  # unclaimed edit to the same line, plus an insertion
        lines.insert(10, "u-new")
        self.write(lines)
        t2 = self.capture()  # Y claims
        lines[15] = "y15"  # Y's window (l15 shifted down one by u-new)
        self.write(lines)
        t3 = self.capture()  # X claims again
        lines[18] = "x18"  # X's second window
        self.write(lines)
        t4 = self.capture()  # cleared
        git(self.repo, "add", "-A")
        git(self.repo, "commit", "-q", "-m", "gate commit")
        self.sha = git(self.repo, "rev-parse", "--short=7", "HEAD")
        self.trees = [t0, t1, t2, t3, t4]
        rows = [
            f"[{ts(0)}] [claim-interval] op=capture ref=refs/bram/claims/1 tree={t0} kind=approved ids=1",
            f"[{ts(0, 1)}] [inflight-sentinel] op=write kind=approved ids=[\"X\"]",
            f"[{ts(1)}] [claim-interval] op=capture ref=refs/bram/claims/2 tree={t1} kind=cleared ids=0",
            f"[{ts(2)}] [claim-interval] op=capture ref=refs/bram/claims/3 tree={t2} kind=approved ids=1",
            f"[{ts(2, 1)}] [inflight-sentinel] op=write kind=approved ids=[\"Y\"]",
            f"[{ts(3)}] [claim-interval] op=capture ref=refs/bram/claims/4 tree={t3} kind=iterate ids=1",
            f"[{ts(3, 1)}] [inflight-sentinel] op=write kind=iterate ids=[\"X\"]",
            f"[{ts(4)}] [claim-interval] op=capture ref=refs/bram/claims/5 tree={t4} kind=cleared ids=0",
            f"[{ts(5)}] [worklist-commit] op=interval-staged sha={self.sha} files=1 ids=1",
        ]
        (self.trace_dir / "bram-trace.log").write_text("\n".join(rows) + "\n")

    def cli(self, *argv: str) -> dict:
        return run_cli(*argv, "--trace-dir", str(self.trace_dir), "--repo", str(self.repo))

    def test_windows_counts_per_window_churn(self) -> None:
        result = self.cli("windows", self.sha)
        self.assertEqual(result["committer"], ["X"])
        self.assertEqual(result["missing_trees"], [])
        (row,) = result["paths"]
        self.assertEqual(
            (row["path"], row["own"], row["other"], row["unclaimed"]),
            ("f.txt", 4, 2, 3),
        )

    def test_windows_reports_missing_trees(self) -> None:
        text = (self.trace_dir / "bram-trace.log").read_text()
        text = text.replace(self.trees[2], "deadbeefdead")
        (self.trace_dir / "bram-trace.log").write_text(text)
        result = self.cli("windows", self.sha)
        self.assertEqual(result["missing_trees"], ["deadbeefdead"])

    def test_lines_buckets_and_conservation(self) -> None:
        result = self.cli("lines")
        self.assertEqual(result["skipped"], [])
        (row,) = result["rows"]
        self.assertEqual(
            row["added"],
            {"own": 1, "other": 1, "unclaimed": 2, "before": 1, "unexplained": 0},
        )
        self.assertEqual(
            row["removed"],
            {"own": 2, "other": 1, "unclaimed": 0, "before": 1, "unexplained": 0},
        )
        self.assertEqual(row["numstat"], [5, 4])
        self.assertTrue(row["conserved"])
        self.assertEqual(result["summary"]["not_conserved"], 0)
        self.assertEqual(result["summary"]["added"]["own"], 1)
        one = self.cli("lines", "--sha", self.sha)
        self.assertEqual(one["rows"], result["rows"])

    def test_lines_skips_commits_with_missing_trees(self) -> None:
        text = (self.trace_dir / "bram-trace.log").read_text()
        (self.trace_dir / "bram-trace.log").write_text(
            text.replace(self.trees[1], "deadbeefdead", 1)
        )
        result = self.cli("lines")
        self.assertEqual(result["rows"], [])
        self.assertEqual(result["skipped"][0]["reason"], "trees-missing")

    def test_probe_reports_whole_patch_and_first_failing_section(self) -> None:
        result = self.cli("probe", self.sha)
        (row,) = result["paths"]
        self.assertEqual(row["sections"], 2)
        self.assertIs(row["whole_patch_reverses"], False)
        self.assertEqual(row["sections_reversed_before_failure"], 1)
        failing = row["first_failing_section"]
        self.assertEqual(failing["newest_first_index"], 2)
        self.assertEqual((failing["from"], failing["to"]), (self.trees[0], self.trees[1]))

    def snapshot(self) -> dict:
        index = self.repo / ".git" / "index"
        return {
            "status": git(self.repo, "status", "--porcelain"),
            "refs": git(self.repo, "for-each-ref"),
            "head": git(self.repo, "rev-parse", "HEAD"),
            "objects": git(self.repo, "count-objects", "-v"),
            "index": hashlib.sha256(index.read_bytes()).hexdigest(),
            "worktree": self.file.read_bytes(),
        }

    def test_every_subcommand_is_read_only(self) -> None:
        before = self.snapshot()
        self.cli("gate")
        self.cli("windows", self.sha)
        self.cli("lines")
        self.cli("probe", self.sha)
        self.assertEqual(self.snapshot(), before)


if __name__ == "__main__":
    unittest.main()
