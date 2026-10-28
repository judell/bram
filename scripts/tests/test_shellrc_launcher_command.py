"""The PTY shell's rcfile and a launcher-supplied first command (judell/bram#442).

A launcher (e.g. bram_herdr.py, to run `herdr agent attach <pane>`) hands Bram's
terminal one command to run before the first prompt, in
BRAM_LAUNCHER_PROMPT_COMMAND. An ordinary exported PROMPT_COMMAND must not be
picked up that way: running it reports a started command while autostart may be
typing a launch, which launch confirmation reads as "the agent exited".

These drive the real rcfile the way Jon checked it by hand: pipe commands into
`bash --noprofile --rcfile <shellrc> -i` and read the OSC 7779 markers it prints.
"""

import os
import re
import shutil
import subprocess
import unittest
from pathlib import Path

SHELLRC = Path(__file__).resolve().parents[2] / "app" / "shell" / "claude-code-shellrc"
BASH = shutil.which("bash")
MARKER = re.compile(r"\x1b\]7779;bram;([^\x07]*)\x07")

# Two commands: one that succeeds and prints, one that fails.
COMMANDS = "echo hi\nfalse\n"
# What `main` reports for COMMANDS with nothing inherited: the first prompt,
# then each command starting and the prompt it returns to with its status.
MAIN_MARKERS = ["prompt;0", "exec", "prompt;0", "exec", "prompt;1"]


def run_shell(extra_env, commands=COMMANDS):
    env = {"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "HOME": os.environ.get("HOME", "/tmp"), "TERM": "dumb"}
    env.update(extra_env)
    proc = subprocess.run(
        [BASH, "--noprofile", "--rcfile", str(SHELLRC), "-i"],
        input=commands,
        capture_output=True,
        text=True,
        env=env,
        timeout=20,
    )
    return proc.stdout


@unittest.skipIf(BASH is None or os.name == "nt", "needs a Unix bash")
class LauncherPromptCommandTests(unittest.TestCase):
    def test_unset_markers_match_main(self):
        out = run_shell({})
        self.assertEqual(MARKER.findall(out), MAIN_MARKERS)

    def test_set_runs_once_as_a_started_command(self):
        # The command unsets PROMPT_COMMAND, as a launcher's may; the later
        # prompts still being reported proves the shellrc restores its own.
        out = run_shell({"BRAM_LAUNCHER_PROMPT_COMMAND": "unset PROMPT_COMMAND; echo LAUNCHED"})
        markers = MARKER.findall(out)
        # exec, its output, then the first prompt with its status; after that,
        # the same as main. No prompt is reported before the command returns.
        self.assertEqual(markers, ["exec"] + MAIN_MARKERS)
        launched = out.index("LAUNCHED")
        self.assertLess(out.index("\x1b]7779;bram;exec\x07"), launched)
        self.assertLess(launched, out.index("\x1b]7779;bram;prompt;0\x07"))
        self.assertEqual(out.count("LAUNCHED"), 1)

    def test_set_is_not_visible_to_a_child_process(self):
        # The child that matters most is the launcher's own command (e.g.
        # `herdr agent attach`), so check from inside it, not only afterwards.
        probe = (
            f"{BASH} -c '[ -n \"${{BRAM_LAUNCHER_PROMPT_COMMAND+x}}\" ]"
            " && echo CHILD-SEES-IT || echo CHILD-SEES-NOTHING'"
        )
        commands = 'echo "SHELL-VAR=[${BRAM_LAUNCHER_PROMPT_COMMAND-unset}]"\n'
        out = run_shell({"BRAM_LAUNCHER_PROMPT_COMMAND": probe}, commands)
        self.assertIn("CHILD-SEES-NOTHING", out)
        self.assertNotIn("CHILD-SEES-IT", out)
        self.assertIn("SHELL-VAR=[unset]", out)

    def test_exported_prompt_command_never_runs(self):
        out = run_shell({"PROMPT_COMMAND": "echo EXPORTED-RAN"})
        self.assertNotIn("EXPORTED-RAN", out)
        self.assertEqual(MARKER.findall(out), MAIN_MARKERS)


if __name__ == "__main__":
    unittest.main()
