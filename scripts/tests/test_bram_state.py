"""The state-folder resolver: `.bram/` (issue-469-retire-resources)."""

import importlib.util
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


SCRIPTS = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("bram_state", SCRIPTS / "bram_state.py")
assert SPEC and SPEC.loader
bram_state = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(bram_state)


def _bash() -> str | None:
    # On Windows, PATH can put WSL's bash.exe ahead of Git Bash, which the
    # scripts actually run under; use the one beside git.
    if os.name == "nt":
        git = shutil.which("git")
        if git:
            for rel in ("../bin/bash.exe", "../usr/bin/bash.exe", "../../bin/bash.exe"):
                candidate = (Path(git).parent / rel).resolve()
                if candidate.is_file():
                    return str(candidate)
        return None
    return shutil.which("bash")


BASH = _bash()


class StateDirTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory(prefix="bram-state-test-")
        self.root = Path(self.temp.name)

    def tearDown(self) -> None:
        self.temp.cleanup()

    def sh_state_dir(self) -> str:
        script = f'. "{(SCRIPTS / "bram-state.sh").as_posix()}"; bram_state_dir "$1"'
        out = subprocess.run(
            [BASH, "-c", script, "bram-state-test", self.root.as_posix()],
            check=True,
            text=True,
            stdout=subprocess.PIPE,
        ).stdout.strip()
        return out.rsplit("/", 1)[-1]

    def assert_resolves(self, name: str) -> None:
        self.assertEqual(bram_state.state_dir(self.root), self.root / name)
        self.assertEqual(bram_state.port_file(self.root), self.root / name / ".bram-port")
        self.assertEqual(bram_state.trace_dir(self.root), self.root / name / "bram-traces")
        self.assertEqual(
            bram_state.trace_log(self.root), self.root / name / "bram-traces" / "bram-trace.log"
        )
        if BASH:
            self.assertEqual(self.sh_state_dir(), name)

    def test_only_resources_still_names_bram(self) -> None:
        # The pre-0.8.4 folder is never read: the host moves it or holds.
        (self.root / "resources").mkdir()
        (self.root / "resources" / ".bram-port").write_text("1\n")
        self.assert_resolves(".bram")

    def test_only_bram(self) -> None:
        (self.root / ".bram").mkdir()
        (self.root / ".bram" / ".gitignore").write_text("*\n")
        self.assert_resolves(".bram")
        self.assertTrue(bram_state.is_bram_state_dir(self.root))

    def test_both_names_bram(self) -> None:
        (self.root / "resources").mkdir()
        (self.root / ".bram").mkdir()
        (self.root / ".bram" / ".gitignore").write_text("*\n")
        self.assert_resolves(".bram")

    def test_migrated_from_marker_counts(self) -> None:
        (self.root / ".bram").mkdir()
        (self.root / ".bram" / ".migrated-from").write_text("x 2026-11-11T00:00:00Z\n")
        self.assertTrue(bram_state.is_bram_state_dir(self.root))
        self.assert_resolves(".bram")

    def test_foreign_bram_is_not_bram_state(self) -> None:
        # #469: another tool's .bram/ (no marker) is not Bram's; the host
        # holds that start, and no script falls back to resources/.
        (self.root / "resources").mkdir()
        (self.root / ".bram").mkdir()
        (self.root / ".bram" / "index.json").write_text("{}\n")
        self.assertFalse(bram_state.is_bram_state_dir(self.root))
        (self.root / ".bram" / ".gitignore").write_text("node_modules\n")
        self.assertFalse(bram_state.is_bram_state_dir(self.root))
        self.assert_resolves(".bram")

    def test_neither_names_bram(self) -> None:
        self.assert_resolves(".bram")

    def test_home_bram_is_ignored(self) -> None:
        # ~/.bram is the machine folder: the host answers resources there.
        (self.root / ".bram").mkdir()
        (self.root / ".bram" / ".gitignore").write_text("*\n")
        saved = {k: os.environ.get(k) for k in ("HOME", "USERPROFILE")}
        os.environ["HOME"] = os.environ["USERPROFILE"] = str(self.root)
        try:
            self.assert_resolves("resources")
        finally:
            for k, v in saved.items():
                if v is None:
                    os.environ.pop(k, None)
                else:
                    os.environ[k] = v


if __name__ == "__main__":
    unittest.main()
