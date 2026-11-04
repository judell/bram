"""The state-folder resolver: `.bram/` when it exists, else `resources/`."""

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

    def test_only_resources(self) -> None:
        (self.root / "resources").mkdir()
        self.assert_resolves("resources")

    def test_only_bram(self) -> None:
        (self.root / ".bram").mkdir()
        self.assert_resolves(".bram")

    def test_both_prefers_bram(self) -> None:
        (self.root / "resources").mkdir()
        (self.root / ".bram").mkdir()
        self.assert_resolves(".bram")

    def test_neither_falls_back_to_resources(self) -> None:
        self.assert_resolves("resources")

    def test_bram_file_is_not_a_folder(self) -> None:
        (self.root / ".bram").write_text("not a directory\n")
        self.assert_resolves("resources")

    def test_home_bram_is_ignored(self) -> None:
        (self.root / ".bram").mkdir()
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
